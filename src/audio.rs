//! cpal capture + lock-free ring buffer + native pause.
//!
//! The audio thread owns the `cpal::Stream` (which is `!Send` on some
//! backends) and reacts to commands sent from the UI. The capture callback
//! pushes interleaved f32 samples into an `rtrb::Producer`; the encoder
//! thread pops them from the matching `Consumer`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use parking_lot::Mutex;

use crate::config::TranscriptionConfig;
use crate::devices;
use crate::encoder::{spawn_encoder, EncoderHandle, TranscriptionTee};
use crate::transcription::{spawn_transcription_worker, TranscriptionHandle};

/// Approx. 5 seconds of stereo 48 kHz f32 audio.
const RING_CAPACITY: usize = 48_000 * 2 * 5;

pub enum AudioCommand {
    Start {
        device_id: String,
        output_path: PathBuf,
        transcription: Option<TranscriptionConfig>,
    },
    Pause,
    Resume,
    Stop,
}

#[derive(Default)]
pub struct AudioState {
    pub recording: AtomicBool,
    pub paused: AtomicBool,
    /// Peak dBFS over the last callback, encoded as fixed-point (value * 100).
    /// `-inf` is represented as `i32::MIN`.
    pub peak_dbfs_x100: AtomicI32,
    /// Total recorded duration in milliseconds (excluding pause time).
    pub elapsed_ms: AtomicU64,
    /// Last fatal error message (UI surfaces this).
    pub last_error: Mutex<Option<String>>,
    /// Accumulated transcript text for the current session.
    pub transcript: Mutex<String>,
    /// Monotonically incremented each time a new chunk is appended; the UI
    /// polls this to detect changes without holding the mutex.
    pub transcript_version: AtomicU64,
    /// True while the transcription worker is waiting for an HTTP response.
    pub transcript_waiting: AtomicBool,
}

impl AudioState {
    pub fn peak_dbfs(&self) -> f32 {
        let v = self.peak_dbfs_x100.load(Ordering::Relaxed);
        if v == i32::MIN {
            f32::NEG_INFINITY
        } else {
            v as f32 / 100.0
        }
    }

    fn set_peak_dbfs(&self, db: f32) {
        let v = if db.is_finite() {
            (db * 100.0).clamp(i32::MIN as f32 + 1.0, i32::MAX as f32) as i32
        } else {
            i32::MIN
        };
        self.peak_dbfs_x100.store(v, Ordering::Relaxed);
    }

    fn record_error(&self, msg: impl Into<String>) {
        *self.last_error.lock() = Some(msg.into());
    }

    pub fn take_error(&self) -> Option<String> {
        self.last_error.lock().take()
    }
}

pub struct AudioController {
    tx: Sender<AudioCommand>,
    pub state: Arc<AudioState>,
}

impl AudioController {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(AudioState::default());
        let state_thread = Arc::clone(&state);
        thread::spawn(move || audio_thread(rx, state_thread));
        Self { tx, state }
    }

    pub fn start(
        &self,
        device_id: String,
        output_path: PathBuf,
        transcription: Option<TranscriptionConfig>,
    ) {
        let _ = self.tx.send(AudioCommand::Start {
            device_id,
            output_path,
            transcription,
        });
    }
    pub fn pause(&self) {
        let _ = self.tx.send(AudioCommand::Pause);
    }
    pub fn resume(&self) {
        let _ = self.tx.send(AudioCommand::Resume);
    }
    pub fn stop(&self) {
        let _ = self.tx.send(AudioCommand::Stop);
    }
}

struct ActiveSession {
    stream: cpal::Stream,
    encoder: EncoderHandle,
    transcription: Option<TranscriptionHandle>,
    paused_flag: Arc<AtomicBool>,
    timer_stop: Arc<AtomicBool>,
    timer_handle: Option<JoinHandle<()>>,
}

fn audio_thread(rx: Receiver<AudioCommand>, state: Arc<AudioState>) {
    let mut session: Option<ActiveSession> = None;
    let mut accumulated_paused = std::time::Duration::ZERO;
    let mut pause_started_at: Option<Instant> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            AudioCommand::Start {
                device_id,
                output_path,
                transcription,
            } => {
                if session.is_some() {
                    continue;
                }
                accumulated_paused = std::time::Duration::ZERO;
                pause_started_at = None;
                // Clear any transcript from a previous session.
                *state.transcript.lock() = String::new();
                state.transcript_version.store(0, Ordering::Release);
                state.transcript_waiting.store(false, Ordering::Release);
                match start_session(&device_id, output_path, transcription, Arc::clone(&state)) {
                    Ok(s) => {
                        state.recording.store(true, Ordering::Release);
                        state.paused.store(false, Ordering::Release);
                        state.elapsed_ms.store(0, Ordering::Release);
                        state.peak_dbfs_x100.store(i32::MIN, Ordering::Release);
                        session = Some(s);
                    }
                    Err(e) => {
                        log::error!("start_session failed: {e:#}");
                        state.record_error(format!("start: {e:#}"));
                    }
                }
            }
            AudioCommand::Pause => {
                if let Some(s) = session.as_mut() {
                    s.paused_flag.store(true, Ordering::Release);
                    let _ = s.stream.pause();
                    pause_started_at = Some(Instant::now());
                    state.paused.store(true, Ordering::Release);
                }
            }
            AudioCommand::Resume => {
                if let Some(s) = session.as_mut() {
                    s.paused_flag.store(false, Ordering::Release);
                    let _ = s.stream.play();
                    if let Some(t) = pause_started_at.take() {
                        accumulated_paused += t.elapsed();
                    }
                    state.paused.store(false, Ordering::Release);
                }
            }
            AudioCommand::Stop => {
                if let Some(mut s) = session.take() {
                    s.timer_stop.store(true, Ordering::Release);
                    if let Some(h) = s.timer_handle.take() {
                        let _ = h.join();
                    }
                    drop(s.stream);
                    // Encoder finish flushes the tee and drops the Sender,
                    // which closes the channel and lets the worker drain.
                    if let Err(e) = s.encoder.finish() {
                        log::error!("encoder finish failed: {e:#}");
                        state.record_error(format!("encode: {e:#}"));
                    }
                    // Wait for the transcription worker to drain and exit.
                    if let Some(t) = s.transcription {
                        t.finish();
                    }
                    state.transcript_waiting.store(false, Ordering::Release);
                    state.recording.store(false, Ordering::Release);
                    state.paused.store(false, Ordering::Release);
                    state.peak_dbfs_x100.store(i32::MIN, Ordering::Release);
                }
            }
        }
    }
}

/// Computes the peak amplitude of an interleaved chunk (mono-mix for stereo).
fn peak_amplitude(samples: &[f32], channels: usize) -> f32 {
    let mut peak: f32 = 0.0;
    if channels >= 2 {
        for frame in samples.chunks_exact(channels) {
            let mut m = 0.0;
            for &s in frame {
                m += s;
            }
            m /= channels as f32;
            let a = m.abs();
            if a > peak {
                peak = a;
            }
        }
    } else {
        for &s in samples {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
        }
    }
    peak
}

fn push_samples(samples: &[f32], state: &AudioState, producer: &mut rtrb::Producer<f32>, channels: usize) {
    let peak = peak_amplitude(samples, channels);
    let db = if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        f32::NEG_INFINITY
    };
    state.set_peak_dbfs(db);

    if let Ok(chunk) = producer.write_chunk_uninit(samples.len()) {
        chunk.fill_from_iter(samples.iter().copied());
    }
    // If the ring buffer is full, samples are dropped silently — the encoder
    // is behind. This shouldn't happen at 48 kHz stereo with 5s of capacity
    // unless the disk stalls.
}

fn start_session(
    device_id: &str,
    output_path: PathBuf,
    transcription_cfg: Option<TranscriptionConfig>,
    state: Arc<AudioState>,
) -> Result<ActiveSession> {
    let device = devices::resolve_device(device_id)?;
    let supported = device
        .default_input_config()
        .with_context(|| format!("no default input config for '{}'", device_id))?;

    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate;

    log::info!(
        "starting capture: device='{device_id}' fmt={sample_format:?} rate={sample_rate} ch={channels}"
    );

    let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);

    let paused_flag = Arc::new(AtomicBool::new(false));

    let err_state = Arc::clone(&state);
    let err_fn = move |e| {
        log::error!("cpal stream error: {e}");
        err_state.record_error(format!("stream: {e}"));
    };

    let stream = build_stream(
        &device,
        &config,
        sample_format,
        producer,
        Arc::clone(&paused_flag),
        Arc::clone(&state),
        channels,
        err_fn,
    )?;

    stream.play()?;

    // Build the optional transcription tee + worker.
    let (tee, transcription_handle) =
        if let Some(cfg) = transcription_cfg.filter(|c| c.enabled) {
            let txt_path = output_path
                .parent()
                .map(|d| d.join("transcript.txt"))
                .unwrap_or_else(|| output_path.with_extension("txt"));
            let (tx_chunks, rx_chunks) = std::sync::mpsc::channel::<String>();
            let tee = TranscriptionTee::new(cfg.chunk_seconds, tx_chunks);
            let handle =
                spawn_transcription_worker(cfg, txt_path, rx_chunks, Arc::clone(&state))?;
            (Some(tee), Some(handle))
        } else {
            (None, None)
        };

    let encoder = spawn_encoder(
        consumer,
        channels,
        sample_rate,
        output_path,
        Arc::clone(&state),
        tee,
    )?;

    let timer_stop = Arc::new(AtomicBool::new(false));
    let state_for_timer = Arc::clone(&state);
    let stop_for_timer = Arc::clone(&timer_stop);
    let start_instant = Instant::now();
    let paused_for_timer = Arc::clone(&paused_flag);
    let timer_handle = thread::spawn(move || {
        let mut accumulated_paused = std::time::Duration::ZERO;
        let mut pause_began: Option<Instant> = None;
        while !stop_for_timer.load(Ordering::Acquire) {
            let is_paused = paused_for_timer.load(Ordering::Acquire);
            match (is_paused, pause_began) {
                (true, None) => pause_began = Some(Instant::now()),
                (false, Some(t)) => {
                    accumulated_paused += t.elapsed();
                    pause_began = None;
                }
                _ => {}
            }
            let mut paused = accumulated_paused;
            if let Some(t) = pause_began {
                paused += t.elapsed();
            }
            let elapsed = start_instant.elapsed().saturating_sub(paused);
            state_for_timer
                .elapsed_ms
                .store(elapsed.as_millis() as u64, Ordering::Release);
            thread::sleep(std::time::Duration::from_millis(33));
        }
    });

    Ok(ActiveSession {
        stream,
        encoder,
        transcription: transcription_handle,
        paused_flag,
        timer_stop,
        timer_handle: Some(timer_handle),
    })
}

#[allow(clippy::too_many_arguments)]
fn build_stream<E>(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    mut producer: rtrb::Producer<f32>,
    paused_flag: Arc<AtomicBool>,
    state: Arc<AudioState>,
    channels: usize,
    err_fn: E,
) -> Result<cpal::Stream>
where
    E: FnMut(cpal::StreamError) + Send + 'static,
{
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                if paused_flag.load(Ordering::Acquire) {
                    return;
                }
                push_samples(data, &state, &mut producer, channels);
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => {
            let mut scratch: Vec<f32> = Vec::new();
            device.build_input_stream(
                config,
                move |data: &[i16], _| {
                    if paused_flag.load(Ordering::Acquire) {
                        return;
                    }
                    scratch.clear();
                    scratch.reserve(data.len());
                    for &s in data {
                        scratch.push(s as f32 / i16::MAX as f32);
                    }
                    push_samples(&scratch, &state, &mut producer, channels);
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::U16 => {
            let mut scratch: Vec<f32> = Vec::new();
            device.build_input_stream(
                config,
                move |data: &[u16], _| {
                    if paused_flag.load(Ordering::Acquire) {
                        return;
                    }
                    scratch.clear();
                    scratch.reserve(data.len());
                    for &s in data {
                        scratch.push((s as f32 - i16::MAX as f32) / i16::MAX as f32);
                    }
                    push_samples(&scratch, &state, &mut producer, channels);
                },
                err_fn,
                None,
            )?
        }
        other => return Err(anyhow!("unsupported sample format: {other:?}")),
    };
    Ok(stream)
}
