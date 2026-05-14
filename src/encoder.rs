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
use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use rtrb::Consumer;

use crate::audio::AudioState;

const TARGET_RATE: usize = 16_000;
const RESAMPLER_CHUNK_IN: usize = 1024;

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
    // rubato's FastFixedIn takes fixed-size *input* chunks of mono f32 and
    // uses polynomial interpolation — plenty for 32 kbps voice.
    let mut resampler: Box<dyn Resampler<f32> + Send> = Box::new(
        FastFixedIn::<f32>::new(
            TARGET_RATE as f64 / in_sample_rate as f64,
            1.0,
            PolynomialDegree::Septic,
            RESAMPLER_CHUNK_IN,
            1,
        )
        .map_err(|e| anyhow!("rubato init: {e}"))?,
    );

    // Working buffers -------------------------------------------------------
    let mut interleaved: Vec<f32> = Vec::with_capacity(in_channels * RESAMPLER_CHUNK_IN * 4);
    let mut mono_pending: Vec<f32> = Vec::with_capacity(RESAMPLER_CHUNK_IN * 4);
    let mut resample_in: Vec<Vec<f32>> = vec![Vec::with_capacity(RESAMPLER_CHUNK_IN)];

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
            resample_in[0].clear();
            resample_in[0].extend_from_slice(&mono_pending[..RESAMPLER_CHUNK_IN]);
            mono_pending.drain(..RESAMPLER_CHUNK_IN);

            let out = resampler
                .process(&resample_in, None)
                .map_err(|e| anyhow!("rubato process: {e}"))?;
            let resampled = &out[0];
            encode_and_write(&mut encoder, resampled, &mut writer)?;
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

    // Flush remaining sub-chunk pending samples by zero-padding to a chunk.
    if !mono_pending.is_empty() {
        let padded = {
            let mut v = std::mem::take(&mut mono_pending);
            v.resize(RESAMPLER_CHUNK_IN, 0.0);
            v
        };
        resample_in[0].clear();
        resample_in[0].extend_from_slice(&padded);
        let out = resampler
            .process(&resample_in, None)
            .map_err(|e| anyhow!("rubato final process: {e}"))?;
        encode_and_write(&mut encoder, &out[0], &mut writer)?;
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
