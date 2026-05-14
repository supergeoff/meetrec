//! Downmix (stereo → mono) + resample (44.1/48 kHz → 16 kHz) + LAME encode.
//!
//! Runs on its own thread, pulling f32 samples from the rtrb ring buffer
//! written by the cpal callback. Output is streamed to disk through a
//! `BufWriter<File>`; no full-recording buffer is held in memory.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use audioadapter_buffers::direct::InterleavedSlice;
use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};
use rtrb::Consumer;
use rubato::{Fft, FixedSync, Resampler};

use crate::audio::AudioState;
use crate::transcription::samples_to_b64_wav;

const TARGET_RATE: usize = 16_000;
const RESAMPLER_CHUNK_IN: usize = 1024;

/// Tees 16 kHz mono f32 samples into base64-encoded WAV chunks sent to the
/// transcription worker.  Dropped after the encoder thread exits, which
/// closes the channel and signals the worker to drain and stop.
pub(crate) struct TranscriptionTee {
    tx: std::sync::mpsc::Sender<String>,
    accumulator: Vec<f32>,
    chunk_samples: usize,
}

impl TranscriptionTee {
    pub(crate) fn new(chunk_seconds: u32, tx: std::sync::mpsc::Sender<String>) -> Self {
        let chunk_samples = chunk_seconds as usize * TARGET_RATE;
        Self {
            tx,
            accumulator: Vec::with_capacity(chunk_samples),
            chunk_samples,
        }
    }

    /// Accumulate samples; flush complete chunks to the channel.
    pub(crate) fn push(&mut self, samples: &[f32]) {
        self.accumulator.extend_from_slice(samples);
        while self.accumulator.len() >= self.chunk_samples {
            let chunk: Vec<f32> = self.accumulator.drain(..self.chunk_samples).collect();
            let _ = self.tx.send(samples_to_b64_wav(&chunk));
        }
    }

    /// Send any remaining samples as a partial (shorter-than-chunk) WAV.
    pub(crate) fn flush_partial(&mut self) {
        if !self.accumulator.is_empty() {
            let chunk = std::mem::take(&mut self.accumulator);
            let _ = self.tx.send(samples_to_b64_wav(&chunk));
        }
    }
}

pub struct EncoderHandle {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<()>>>,
}

impl EncoderHandle {
    pub fn finish(mut self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        match self.handle.take() {
            Some(h) => h.join().map_err(|_| anyhow!("encoder thread panicked"))?,
            None => Ok(()),
        }
    }
}

pub fn spawn_encoder(
    consumer: Consumer<f32>,
    in_channels: usize,
    in_sample_rate: u32,
    output_path: PathBuf,
    state: Arc<AudioState>,
    tee: Option<TranscriptionTee>,
) -> Result<EncoderHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let file = File::create(&output_path)
        .with_context(|| format!("creating {}", output_path.display()))?;
    let writer = BufWriter::with_capacity(64 * 1024, file);

    let handle = thread::Builder::new()
        .name("encoder".into())
        .spawn(move || {
            let result = run_encoder(
                consumer,
                in_channels,
                in_sample_rate,
                writer,
                stop_thread,
                tee,
            );
            if let Err(e) = &result {
                log::error!("encoder thread error: {e:#}");
                *state.last_error.lock() = Some(format!("encoder: {e:#}"));
            }
            result
        })
        .context("spawning encoder thread")?;

    Ok(EncoderHandle {
        stop,
        handle: Some(handle),
    })
}

fn run_encoder(
    mut consumer: Consumer<f32>,
    in_channels: usize,
    in_sample_rate: u32,
    mut writer: BufWriter<File>,
    stop: Arc<AtomicBool>,
    mut tee: Option<TranscriptionTee>,
) -> Result<()> {
    // LAME encoder ----------------------------------------------------------
    let mut builder = Builder::new().ok_or_else(|| anyhow!("mp3lame Builder::new failed"))?;
    builder
        .set_num_channels(1)
        .map_err(|e| anyhow!("lame set_num_channels: {e:?}"))?;
    builder
        .set_sample_rate(TARGET_RATE as u32)
        .map_err(|e| anyhow!("lame set_sample_rate: {e:?}"))?;
    builder
        .set_brate(Bitrate::Kbps32)
        .map_err(|e| anyhow!("lame set_brate: {e:?}"))?;
    builder
        .set_quality(Quality::Best)
        .map_err(|e| anyhow!("lame set_quality: {e:?}"))?;
    let mut encoder = builder
        .build()
        .map_err(|e| anyhow!("lame build: {e:?}"))?;

    // Resampler -------------------------------------------------------------
    // rubato 1.0+ unified the old Fft/FastFixedIn types. We use the
    // synchronous FFT resampler with a fixed input chunk size — well-suited
    // for the fixed-ratio downsample (48 kHz/44.1 kHz → 16 kHz).
    let mut resampler: Box<dyn Resampler<f32> + Send> = Box::new(
        Fft::<f32>::new(
            in_sample_rate as usize,
            TARGET_RATE,
            RESAMPLER_CHUNK_IN,
            2,
            1,
            FixedSync::Input,
        )
        .map_err(|e| anyhow!("rubato init: {e}"))?,
    );

    // Working buffers -------------------------------------------------------
    let mut interleaved: Vec<f32> = Vec::with_capacity(in_channels * RESAMPLER_CHUNK_IN * 4);
    let mut mono_pending: Vec<f32> = Vec::with_capacity(RESAMPLER_CHUNK_IN * 4);
    let mut mono_chunk: Vec<f32> = Vec::with_capacity(RESAMPLER_CHUNK_IN);

    loop {
        // Drain whatever is available from the ring buffer.
        let mut got_any = false;
        if let Ok(chunk) = consumer.read_chunk(consumer.slots()) {
            let (a, b) = chunk.as_slices();
            interleaved.extend_from_slice(a);
            interleaved.extend_from_slice(b);
            got_any = !a.is_empty() || !b.is_empty();
            chunk.commit_all();
        }

        // Downmix interleaved → mono.
        if !interleaved.is_empty() {
            if in_channels == 1 {
                mono_pending.extend_from_slice(&interleaved);
            } else {
                for frame in interleaved.chunks_exact(in_channels) {
                    let mut sum = 0.0f32;
                    for &s in frame {
                        sum += s;
                    }
                    mono_pending.push(sum / in_channels as f32);
                }
            }
            interleaved.clear();
        }

        // Resample + encode complete chunks.
        while mono_pending.len() >= RESAMPLER_CHUNK_IN {
            mono_chunk.clear();
            mono_chunk.extend_from_slice(&mono_pending[..RESAMPLER_CHUNK_IN]);
            mono_pending.drain(..RESAMPLER_CHUNK_IN);
            let resampled = resample_mono(&mut *resampler, &mono_chunk)?;
            // Branch B: tee 16 kHz samples to transcription accumulator.
            if let Some(ref mut t) = tee {
                t.push(&resampled);
            }
            encode_and_write(&mut encoder, &resampled, &mut writer)?;
        }

        // Stop condition: capture has stopped AND the consumer is empty AND
        // we've drained all pending mono samples.
        let want_stop = stop.load(Ordering::Acquire);
        let is_idle = !got_any && consumer.is_empty();
        if want_stop && is_idle {
            break;
        }
        if !got_any {
            thread::sleep(Duration::from_millis(5));
        }
    }

    // Flush the transcription accumulator with real audio BEFORE zero-padding.
    // Dropping `tee` here closes the channel → worker drains then exits.
    if let Some(mut t) = tee.take() {
        t.flush_partial();
    }

    // Flush remaining sub-chunk pending samples by zero-padding to a chunk (LAME only).
    if !mono_pending.is_empty() {
        mono_pending.resize(RESAMPLER_CHUNK_IN, 0.0);
        let resampled = resample_mono(&mut *resampler, &mono_pending)?;
        encode_and_write(&mut encoder, &resampled, &mut writer)?;
    }

    // LAME flush — writes the final MP3 frames and the LAME tag.
    let mut tail: Vec<u8> = Vec::with_capacity(7200);
    let n = encoder
        .flush::<FlushNoGap>(tail.spare_capacity_mut())
        .map_err(|e| anyhow!("lame flush: {e:?}"))?;
    unsafe {
        tail.set_len(n);
    }
    writer.write_all(&tail)?;
    writer.flush()?;

    // Ensure data is on disk before returning success.
    let file = writer
        .into_inner()
        .map_err(|e| anyhow!("BufWriter flush: {e}"))?;
    file.sync_all()?;

    Ok(())
}

fn resample_mono(
    resampler: &mut dyn Resampler<f32>,
    samples: &[f32],
) -> Result<Vec<f32>> {
    let input = InterleavedSlice::new(samples, 1, samples.len())
        .map_err(|e| anyhow!("rubato input adapter: {e:?}"))?;
    let output = resampler
        .process(&input, 0, None)
        .map_err(|e| anyhow!("rubato process: {e}"))?;
    Ok(output.take_data())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn tee_does_not_send_before_threshold() {
        let (tx, rx) = mpsc::channel();
        let mut tee = TranscriptionTee::new(1, tx); // 1 s = 16 000 samples
        tee.push(&vec![0.0f32; 100]);
        assert!(rx.try_recv().is_err(), "should not send before threshold");
    }

    #[test]
    fn tee_sends_one_chunk_when_exactly_at_threshold() {
        let (tx, rx) = mpsc::channel();
        let mut tee = TranscriptionTee::new(1, tx);
        tee.push(&vec![0.5f32; 16_000]);
        let chunk = rx.try_recv().expect("should have sent exactly one chunk");
        assert!(!chunk.is_empty());
        assert!(rx.try_recv().is_err(), "no second chunk");
    }

    #[test]
    fn tee_sends_two_chunks_for_two_full_seconds() {
        let (tx, rx) = mpsc::channel();
        let mut tee = TranscriptionTee::new(1, tx);
        tee.push(&vec![0.0f32; 32_000]);
        rx.try_recv().expect("first chunk");
        rx.try_recv().expect("second chunk");
        assert!(rx.try_recv().is_err(), "no third chunk");
    }

    #[test]
    fn tee_flush_partial_sends_remaining_samples() {
        let (tx, rx) = mpsc::channel();
        let mut tee = TranscriptionTee::new(1, tx);
        tee.push(&vec![0.0f32; 500]); // below threshold
        assert!(rx.try_recv().is_err());
        tee.flush_partial();
        let chunk = rx.try_recv().expect("partial chunk");
        assert!(!chunk.is_empty());
    }

    #[test]
    fn tee_flush_partial_on_empty_sends_nothing() {
        let (tx, rx) = mpsc::channel();
        let mut tee = TranscriptionTee::new(1, tx);
        tee.flush_partial(); // nothing accumulated
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn tee_chunk_is_valid_base64() {
        let (tx, rx) = mpsc::channel();
        let mut tee = TranscriptionTee::new(1, tx);
        tee.push(&vec![0.25f32; 16_000]);
        let chunk = rx.try_recv().unwrap();
        use base64::{engine::general_purpose::STANDARD, Engine};
        let decoded = STANDARD.decode(&chunk).expect("must be valid base64");
        assert_eq!(&decoded[0..4], b"RIFF", "decoded chunk must be a WAV");
    }
}

fn encode_and_write(
    encoder: &mut mp3lame_encoder::Encoder,
    samples: &[f32],
    writer: &mut BufWriter<File>,
) -> Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let cap = mp3lame_encoder::max_required_buffer_size(samples.len());
    let mut out: Vec<u8> = Vec::with_capacity(cap);
    let n = encoder
        .encode(MonoPcm(samples), out.spare_capacity_mut())
        .map_err(|e| anyhow!("lame encode: {e:?}"))?;
    if n > 0 {
        unsafe {
            out.set_len(n);
        }
        writer.write_all(&out)?;
    }
    Ok(())
}
