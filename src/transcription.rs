use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine};

use crate::audio::AudioState;
use crate::config::TranscriptionConfig;

/// Handle for the transcription worker thread.
/// The worker drains its channel and exits when the Sender side is dropped.
pub struct TranscriptionHandle {
    handle: Option<JoinHandle<()>>,
}

impl TranscriptionHandle {
    pub fn finish(mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Spawn the transcription worker thread.
///
/// The caller owns the `Sender<String>` side of the chunk channel (held by the
/// encoder's `TranscriptionTee`). When that Sender is dropped the channel
/// closes, the worker drains any remaining buffered chunks, then exits.
pub fn spawn_transcription_worker(
    config: TranscriptionConfig,
    txt_path: PathBuf,
    rx: Receiver<String>,
    state: Arc<AudioState>,
) -> Result<TranscriptionHandle> {
    let handle = thread::Builder::new()
        .name("transcription".into())
        .spawn(move || {
            run_worker(config, txt_path, rx, state);
        })
        .map_err(|e| anyhow!("spawning transcription thread: {e}"))?;

    Ok(TranscriptionHandle {
        handle: Some(handle),
    })
}

fn run_worker(
    config: TranscriptionConfig,
    txt_path: PathBuf,
    rx: Receiver<String>,
    state: Arc<AudioState>,
) {
    let mut txt_file: Option<BufWriter<File>> = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&txt_path)
    {
        Ok(f) => Some(BufWriter::new(f)),
        Err(e) => {
            log::error!("failed to open transcript file {}: {e}", txt_path.display());
            None
        }
    };

    let url = format!(
        "{}/audio/transcriptions",
        config.base_url.trim_end_matches('/')
    );
    let mut chunk_n: u32 = 0;

    // Iterate until the Sender side is dropped (channel closed).
    for chunk_b64 in &rx {
        chunk_n += 1;
        state.transcript_waiting.store(true, std::sync::atomic::Ordering::Release);

        let text = match call_api(&config, &url, &chunk_b64) {
            Ok(t) => t,
            Err(e) => {
                log::error!("transcription chunk {chunk_n} failed: {e:#}");
                format!("[transcription échec — chunk {chunk_n}]")
            }
        };

        state.transcript_waiting.store(false, std::sync::atomic::Ordering::Release);

        {
            let mut buf = state.transcript.lock();
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&text);
        }
        state
            .transcript_version
            .fetch_add(1, std::sync::atomic::Ordering::Release);

        if let Some(ref mut f) = txt_file {
            if let Err(e) = writeln!(f, "{}", text) {
                log::error!("writing transcript: {e}");
            }
            let _ = f.flush();
        }
    }
}

fn call_api(config: &TranscriptionConfig, url: &str, chunk_b64: &str) -> Result<String> {
    let mut body = serde_json::json!({
        "model": config.model,
        "input_audio": {
            "data": chunk_b64,
            "format": "wav"
        }
    });
    if let Some(ref lang) = config.language {
        body["language"] = serde_json::Value::String(lang.clone());
    }

    let mut response = ureq::post(url)
        .header("Authorization", &format!("Bearer {}", config.api_key))
        .send_json(&body)
        .map_err(|e| anyhow!("HTTP: {e}"))?;

    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow!("reading response: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| anyhow!("parsing JSON: {e}"))?;

    Ok(json["text"].as_str().unwrap_or("").to_string())
}

/// Build a 44-byte WAV header + PCM-16 data for the given mono 16 kHz f32 samples.
/// No external WAV library — the header is written by hand.
pub(crate) fn build_wav_bytes(samples: &[f32]) -> Vec<u8> {
    let num_samples = samples.len();
    let data_size = num_samples * 2; // 16-bit mono → 2 bytes per sample
    let riff_size = 36u32 + data_size as u32; // total file size − 8

    let mut buf = Vec::with_capacity(44 + data_size);

    // RIFF chunk descriptor
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt sub-chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // sub-chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM = 1
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&16_000u32.to_le_bytes()); // sample rate
    buf.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate = 16000 * 1 * 2
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align = 1 * 2
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data sub-chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_size as u32).to_le_bytes());

    // PCM samples: f32 → i16 (saturating cast via Rust 1.45+ semantics)
    for &s in samples {
        let pcm = (s * 32_768.0f32) as i16;
        buf.extend_from_slice(&pcm.to_le_bytes());
    }

    buf
}

// ── stub for use by TranscriptionTee in encoder.rs ──────────────────────────

/// Encode `samples` as a base64-encoded WAV string ready to send to the API.
pub(crate) fn samples_to_b64_wav(samples: &[f32]) -> String {
    STANDARD.encode(build_wav_bytes(samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_wav_bytes ──────────────────────────────────────────────────────

    #[test]
    fn wav_starts_with_riff_magic_and_wave_marker() {
        let wav = build_wav_bytes(&[0.0f32]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
    }

    #[test]
    fn wav_fmt_chunk_specifies_pcm_mono_16khz_16bit() {
        let wav = build_wav_bytes(&[0.0f32]);
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(wav[16..20].try_into().unwrap()), 16); // chunk size
        assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1); // PCM
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000); // sample rate
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 32_000); // byte rate
        assert_eq!(u16::from_le_bytes(wav[32..34].try_into().unwrap()), 2); // block align
        assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16); // bits/sample
    }

    #[test]
    fn wav_data_chunk_size_matches_sample_count() {
        let samples = vec![0.0f32; 100];
        let wav = build_wav_bytes(&samples);
        assert_eq!(&wav[36..40], b"data");
        let data_size = u32::from_le_bytes(wav[40..44].try_into().unwrap());
        assert_eq!(data_size as usize, 100 * 2);
    }

    #[test]
    fn wav_riff_size_field_equals_total_size_minus_8() {
        let samples = vec![0.0f32; 100];
        let wav = build_wav_bytes(&samples);
        let riff_size = u32::from_le_bytes(wav[4..8].try_into().unwrap());
        assert_eq!(riff_size as usize + 8, wav.len());
    }

    #[test]
    fn wav_total_length_is_44_bytes_plus_data() {
        let n = 256;
        let wav = build_wav_bytes(&vec![0.0f32; n]);
        assert_eq!(wav.len(), 44 + n * 2);
    }

    #[test]
    fn wav_positive_full_scale_maps_to_i16_max() {
        let wav = build_wav_bytes(&[1.0f32]);
        let sample = i16::from_le_bytes(wav[44..46].try_into().unwrap());
        assert_eq!(sample, i16::MAX); // 32767
    }

    #[test]
    fn wav_negative_full_scale_maps_to_i16_min() {
        let wav = build_wav_bytes(&[-1.0f32]);
        let sample = i16::from_le_bytes(wav[44..46].try_into().unwrap());
        assert_eq!(sample, i16::MIN); // -32768
    }

    #[test]
    fn wav_zero_maps_to_zero() {
        let wav = build_wav_bytes(&[0.0f32]);
        let sample = i16::from_le_bytes(wav[44..46].try_into().unwrap());
        assert_eq!(sample, 0);
    }

    #[test]
    fn wav_8_seconds_at_16khz_has_correct_size() {
        let n = 8 * 16_000;
        let wav = build_wav_bytes(&vec![0.0f32; n]);
        assert_eq!(wav.len(), 44 + n * 2);
        let data_size = u32::from_le_bytes(wav[40..44].try_into().unwrap());
        assert_eq!(data_size, (n * 2) as u32);
    }
}
