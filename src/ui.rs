use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Local;
use dioxus::prelude::*;

use dioxus_desktop::{use_window, LogicalSize};

use crate::audio::AudioController;
use crate::config::{Config, SummaryConfig, TranscriptionConfig, UiConfig};
use crate::devices::{list_input_devices, SYSTEM_DEFAULT_ID};
use crate::summary::{call_summary_api, ParsedSummary};

#[derive(Clone, PartialEq)]
enum Toast {
    Simple(String),
    Success {
        mp3: PathBuf,
        txt: PathBuf,
        md: PathBuf,
    },
    Degraded {
        mp3: PathBuf,
        txt: PathBuf,
        md: PathBuf,
    },
}

fn open_folder_cmd(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

const STYLE: &str = r#"
@import url("https://cdn.jsdelivr.net/fontsource/fonts/red-hat-text:vf@latest/latin-wght-normal.css");
@import url("https://fonts.googleapis.com/css2?family=Geist+Mono:wght@400;500;700&display=swap");

:root {
    --ink:        #000000;
    --paper:      #FFFFFF;
    --ink-80:     #333333;
    --ink-70:     #4D4D4D;
    --ink-50:     #808080;
    --ink-25:     #BFBFBF;
    --ink-10:     #E6E6E6;
    --ink-05:     #F2F2F2;
    --fg:         var(--ink);
    --fg-muted:   var(--ink-70);
    --fg-faint:   var(--ink-25);
    --hairline:   var(--ink-10);
    --r-sm:       8px;
    --r-md:       12px;
    --r-pill:     999px;
    --r-circle:   50%;
    --ease-out:   cubic-bezier(0.2, 0.0, 0, 1);
    --dur-fast:   120ms;
    --font-sans:  "Red Hat Text", -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    --font-mono:  "Geist Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace;
}

* { box-sizing: border-box; }
html, body {
    margin: 0; padding: 0; height: 100%;
    background: var(--paper);
    color: var(--fg);
    font-family: var(--font-sans);
    font-size: 13px;
    -webkit-font-smoothing: antialiased;
    user-select: none;
}

.app {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: 18px;
    padding: 16px 18px;
    height: 100%;
    align-items: stretch;
}

/* ── left column: controls + timer + vumeter ── */
.left {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 10px;
}

.controls {
    display: flex;
    gap: 10px;
    justify-content: center;
}

.ctl {
    width: 44px; height: 44px;
    border: none; padding: 0;
    border-radius: var(--r-circle);
    background: var(--ink-05);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out),
                transform var(--dur-fast) var(--ease-out);
}
.ctl:hover:not(:disabled) { background: var(--ink-10); }
.ctl:active:not(:disabled) { transform: scale(0.97); }
.ctl:disabled { cursor: default; opacity: 0.35; }
.ctl:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }

.ctl.rec {
    background: var(--ink);
    animation: pois-pulse 1.1s ease-in-out infinite;
}

@keyframes pois-pulse {
    0%, 100% { box-shadow: 0 0 0 0 rgba(0,0,0,0.55); }
    50%      { box-shadow: 0 0 0 14px rgba(0,0,0,0); }
}

.circle {
    width: 16px; height: 16px;
    border-radius: var(--r-circle);
    background: var(--ink);
}
.ctl.rec .circle { background: var(--paper); }

.pause-bars { display: flex; gap: 3px; }
.pause-bars span {
    display: block;
    width: 4px; height: 15px;
    background: var(--ink);
    border-radius: 1px;
}

.square {
    width: 12px; height: 12px;
    background: var(--ink);
    border-radius: 2px;
}

.tri {
    width: 0; height: 0;
    border-left: 11px solid var(--ink);
    border-top: 7px solid transparent;
    border-bottom: 7px solid transparent;
    margin-left: 3px;
}

.timer {
    font-family: var(--font-mono);
    font-size: 26px;
    letter-spacing: 1.5px;
    font-variant-numeric: tabular-nums;
    color: var(--ink);
    text-align: center;
    line-height: 1;
}

.vumeter {
    position: relative;
    width: 180px;
    height: 6px;
    background: var(--ink-05);
    border-radius: var(--r-pill);
    overflow: hidden;
}
.vu-bar {
    position: absolute;
    left: 0; top: 0; bottom: 0;
    background: var(--ink);
    border-radius: var(--r-pill);
    transition: width 60ms linear;
}

/* ── right column: form rows ── */
.right {
    display: flex;
    flex-direction: column;
    gap: 10px;
    justify-content: center;
    min-width: 0;
}

.row {
    display: flex;
    align-items: center;
    gap: 8px;
}

.row > label {
    flex: 0 0 92px;
    font-size: 12px;
    color: var(--fg-muted);
}

.input, .select {
    flex: 1;
    min-width: 0;
    background: var(--paper);
    color: var(--ink);
    border: 1px solid var(--hairline);
    border-radius: var(--r-sm);
    padding: 6px 10px;
    font-family: var(--font-sans);
    font-size: 12px;
    outline: none;
    transition: border-color var(--dur-fast) var(--ease-out);
}
.input:focus, .select:focus { border-color: var(--ink); }

.browse {
    background: var(--paper);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--ink);
    border: none;
    border-radius: var(--r-pill);
    padding: 6px 14px;
    font-family: var(--font-sans);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out),
                transform var(--dur-fast) var(--ease-out);
}
.browse:hover { background: var(--ink-05); }
.browse:active { transform: scale(0.97); }

.footer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
}
.footer-left { flex: 1; min-width: 0; }

.footer {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--fg-faint);
    letter-spacing: 0.04em;
}

.error {
    background: var(--ink);
    color: var(--paper);
    padding: 6px 10px;
    border-radius: var(--r-sm);
    font-size: 11px;
    line-height: 1.4;
}

/* ── gear button ── */
.settings-btn {
    flex-shrink: 0;
    background: none;
    border: none;
    color: var(--fg-faint);
    font-size: 15px;
    line-height: 1;
    padding: 4px 6px;
    border-radius: var(--r-sm);
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease-out),
                background var(--dur-fast) var(--ease-out);
}
.settings-btn:hover { color: var(--ink); background: var(--ink-05); }

/* ── settings modal ── */
.modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.45);
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
}
.modal {
    background: var(--paper);
    border-radius: var(--r-md);
    width: 520px;
    max-height: 82vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px rgba(0,0,0,0.2);
    overflow: hidden;
}
.modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
    border-bottom: 1px solid var(--hairline);
    flex-shrink: 0;
}
.modal-title { font-size: 13px; font-weight: 600; }
.modal-close {
    background: none;
    border: none;
    font-size: 14px;
    color: var(--fg-muted);
    cursor: pointer;
    padding: 2px 6px;
    border-radius: var(--r-sm);
    line-height: 1;
}
.modal-close:hover { background: var(--ink-05); color: var(--ink); }
.modal-tabs {
    display: flex;
    padding: 0 18px;
    border-bottom: 1px solid var(--hairline);
    flex-shrink: 0;
}
.modal-tab {
    padding: 8px 14px;
    font-size: 12px;
    font-family: var(--font-sans);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    color: var(--fg-muted);
    transition: color var(--dur-fast) var(--ease-out);
}
.modal-tab.active { border-bottom-color: var(--ink); color: var(--ink); font-weight: 500; }
.modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
}
.form-row { display: flex; flex-direction: column; gap: 3px; }
.form-label { font-size: 11px; color: var(--fg-muted); font-weight: 500; }
.form-input {
    background: var(--paper);
    color: var(--ink);
    border: 1px solid var(--hairline);
    border-radius: var(--r-sm);
    padding: 6px 10px;
    font-family: var(--font-sans);
    font-size: 12px;
    outline: none;
    width: 100%;
    transition: border-color var(--dur-fast) var(--ease-out);
}
.form-input:focus { border-color: var(--ink); }
.form-input:disabled { opacity: 0.45; cursor: default; }
.form-textarea {
    resize: vertical;
    min-height: 90px;
    font-family: var(--font-mono);
    font-size: 11px;
    line-height: 1.5;
}
.form-hint { font-size: 10px; color: var(--fg-faint); font-family: var(--font-mono); }
.form-check-row {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    cursor: pointer;
    user-select: none;
    color: var(--fg);
}
.form-check-row input[type="checkbox"] { cursor: pointer; }
.modal-warning {
    font-size: 10px;
    color: var(--fg-muted);
    background: var(--ink-05);
    border-radius: var(--r-sm);
    padding: 7px 10px;
    margin-top: 2px;
}
.modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 18px;
    border-top: 1px solid var(--hairline);
    flex-shrink: 0;
}
.btn {
    border: none;
    border-radius: var(--r-pill);
    padding: 7px 18px;
    font-family: var(--font-sans);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: opacity var(--dur-fast) var(--ease-out),
                background var(--dur-fast) var(--ease-out);
}
.btn-ghost {
    background: var(--paper);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--hairline);
}
.btn-ghost:hover { background: var(--ink-05); }
.btn-primary { background: var(--ink); color: var(--paper); }
.btn-primary:hover { opacity: 0.85; }
.modal-err { font-size: 11px; color: #c00; }

/* ── app grid with expanded transcript panel ── */
.app.panel-open {
    grid-template-columns: 220px 1fr 260px;
}

/* ── transcript panel ── */
.panel {
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--hairline);
    padding: 10px 12px 10px 14px;
    overflow: hidden;
    gap: 6px;
    min-width: 0;
}
.panel-hdr {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--fg-faint);
    flex-shrink: 0;
}
.panel-body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
}
.panel-line {
    font-size: 11px;
    line-height: 1.55;
    color: var(--fg-muted);
}
.panel-notice {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 100%;
    text-align: center;
    font-size: 11px;
    color: var(--fg-faint);
}
.panel-notice .btn-setup {
    margin-top: 2px;
}

/* ── "configure transcription" shortcut button ── */
.btn-setup {
    background: var(--paper);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--hairline);
    border: none;
    border-radius: var(--r-pill);
    padding: 5px 12px;
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
}
.btn-setup:hover { background: var(--ink-05); }

/* ── record-blocked hint below controls ── */
.rec-blocked-hint {
    font-size: 10px;
    color: var(--fg-faint);
    text-align: center;
    line-height: 1.4;
    max-width: 160px;
}

/* ── chevron toggle button (fixed right edge) ── */
.chevron-btn {
    position: fixed;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
    background: var(--ink-05);
    border: none;
    border-left: 1px solid var(--hairline);
    border-radius: var(--r-sm) 0 0 var(--r-sm);
    color: var(--fg-muted);
    font-size: 12px;
    line-height: 1;
    padding: 10px 3px;
    cursor: pointer;
    z-index: 10;
    transition: background var(--dur-fast) var(--ease-out),
                color var(--dur-fast) var(--ease-out);
}
.chevron-btn:hover { background: var(--ink-10); color: var(--ink); }

/* ── participants modal spinner / generating hint ── */
.generating-hint {
    font-size: 11px;
    color: var(--fg-muted);
    text-align: center;
    padding: 4px 0;
    font-style: italic;
}

/* ── toast notification (bottom-center) ── */
.toast {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--ink);
    color: var(--paper);
    border-radius: var(--r-md);
    padding: 12px 16px;
    min-width: 280px;
    max-width: 520px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-shadow: 0 4px 16px rgba(0,0,0,0.25);
    z-index: 200;
    font-size: 12px;
}
.toast-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
}
.toast-message {
    line-height: 1.5;
    flex: 1;
}
.toast-close {
    background: none;
    border: none;
    color: rgba(255,255,255,0.6);
    font-size: 14px;
    cursor: pointer;
    padding: 0 2px;
    border-radius: var(--r-sm);
    line-height: 1;
    flex-shrink: 0;
}
.toast-close:hover { color: var(--paper); }
.toast-paths {
    font-family: var(--font-mono);
    font-size: 10px;
    opacity: 0.75;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-wrap: anywhere;
}
.toast-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
}
.btn-folder {
    background: rgba(255,255,255,0.12);
    color: var(--paper);
    border: 1px solid rgba(255,255,255,0.3);
    border-radius: var(--r-pill);
    padding: 4px 14px;
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
}
.btn-folder:hover { background: rgba(255,255,255,0.22); }
"#;

#[component]
pub fn App() -> Element {
    let controller = use_context::<Arc<AudioController>>();
    let initial = use_hook(Config::load);

    let mut output_folder = use_signal(|| {
        initial
            .output_folder
            .clone()
            .unwrap_or_else(Config::default_output_folder)
    });
    let mut selected_device = use_signal(|| {
        initial
            .input_device
            .clone()
            .unwrap_or_else(|| SYSTEM_DEFAULT_ID.to_string())
    });
    let devices = use_signal(list_input_devices);

    let window = use_window();

    let mut recording = use_signal(|| false);
    let mut paused = use_signal(|| false);
    let mut elapsed_ms = use_signal(|| 0u64);
    let mut peak_db = use_signal(|| f32::NEG_INFINITY);
    let mut error = use_signal::<Option<String>>(|| None);

    // Transcript state polled from AudioState.
    let mut transcript_text = use_signal(String::new);
    let mut transcript_waiting = use_signal(|| false);
    let mut transcript_version_seen = use_signal(|| 0u64);

    // Extended config sections — only updated via the settings modal Save button.
    let mut saved_transcription = use_signal(|| initial.transcription.clone());
    let mut saved_summary = use_signal(|| initial.summary.clone());
    let mut saved_ui_cfg = use_signal(|| initial.ui.clone());

    // Session-frozen copy of TranscriptionConfig set at record start and cleared at stop.
    // The transcription worker reads from this snapshot, not from live signals.
    let mut session_transcription = use_signal::<Option<TranscriptionConfig>>(|| None);

    // Transcript lines produced by the transcription worker (future commit).
    // Panel renders these as they arrive; empty = no transcript yet.
    let transcript_lines = use_signal::<Vec<String>>(Vec::new);

    // Modal visibility and active tab (0 = Transcription, 1 = Résumé)
    let mut show_settings = use_signal(|| false);
    let mut settings_tab = use_signal(|| 0u8);
    let mut modal_error = use_signal::<Option<String>>(|| None);

    // ── Stop sequence ──────────────────────────────────────────────────────
    // MP3 path for the current recording session (set at record start).
    let mut session_mp3_path = use_signal::<Option<PathBuf>>(|| None);

    // Participants modal
    let mut show_participants = use_signal(|| false);
    let mut participants_text = use_signal(String::new);
    let mut summary_generating = use_signal(|| false);
    let mut summary_error = use_signal::<Option<String>>(|| None);
    // Transcript and MP3 path captured at stop time, used by the modal.
    let mut pending_transcript = use_signal(String::new);
    let mut pending_mp3_path = use_signal::<Option<PathBuf>>(|| None);

    // Toast notification
    let mut toast = use_signal::<Option<Toast>>(|| None);

    // Draft signals — initialized from saved values when the modal opens.
    let mut d_t_enabled = use_signal(|| initial.transcription.enabled);
    let mut d_t_base_url = use_signal(|| initial.transcription.base_url.clone());
    let mut d_t_api_key = use_signal(|| initial.transcription.api_key.clone());
    let mut d_t_model = use_signal(|| initial.transcription.model.clone());
    let mut d_t_chunk = use_signal(|| initial.transcription.chunk_seconds.to_string());
    let mut d_t_lang = use_signal(|| initial.transcription.language.clone().unwrap_or_default());
    let mut d_s_base_url = use_signal(|| initial.summary.base_url.clone());
    let mut d_s_api_key = use_signal(|| initial.summary.api_key.clone());
    let mut d_s_model = use_signal(|| initial.summary.model.clone());
    let mut d_s_prompt = use_signal(|| initial.summary.prompt_template.clone());
    let mut d_same_key = use_signal(|| false);

    // ── poll the audio thread state at ~30 Hz ───────────────────────────
    {
        let controller = Arc::clone(&controller);
        use_future(move || {
            let state = Arc::clone(&controller.state);
            async move {
                loop {
                    futures_timer::Delay::new(Duration::from_millis(33)).await;
                    recording.set(state.recording.load(Ordering::Acquire));
                    paused.set(state.paused.load(Ordering::Acquire));
                    elapsed_ms.set(state.elapsed_ms.load(Ordering::Acquire));
                    peak_db.set(state.peak_dbfs());
                    transcript_waiting.set(state.transcript_waiting.load(Ordering::Acquire));

                    let ver = state.transcript_version.load(Ordering::Acquire);
                    if ver != *transcript_version_seen.peek() {
                        transcript_version_seen.set(ver);
                        transcript_text.set(state.transcript.lock().clone());
                    }

                    if let Some(msg) = state.take_error() {
                        error.set(Some(msg));
                    }
                }
            }
        });
    }

    // ── persist config when any saved field changes ──────────────────────
    use_effect(move || {
        let cfg = Config {
            output_folder: Some(output_folder.read().clone()),
            input_device: Some(selected_device.read().clone()),
            transcription: saved_transcription.read().clone(),
            summary: saved_summary.read().clone(),
            ui: saved_ui_cfg.read().clone(),
        };
        if let Err(e) = cfg.save() {
            log::warn!("config save failed: {e:#}");
        }
    });

    // ── derived view state ──────────────────────────────────────────────
    let folder_str = output_folder.read().to_string_lossy().to_string();
    let device_value = selected_device.read().clone();
    let recording_now = *recording.read();
    let paused_now = *paused.read();
    let elapsed = *elapsed_ms.read();
    let total_seconds = elapsed / 1000;
    let mm = total_seconds / 60;
    let ss = total_seconds % 60;
    let timer_text = format!("{:02}:{:02}", mm, ss);

    let db = *peak_db.read();
    let meter_pct: f32 = if db.is_finite() {
        ((db + 60.0) / 60.0 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    let tab = *settings_tab.read();
    let same_key = *d_same_key.read();

    let panel_expanded = saved_ui_cfg.read().transcription_panel_expanded;
    let transcription_unconfigured = {
        let t = saved_transcription.read();
        t.enabled && t.api_key.trim().is_empty()
    };

    // ── handlers ────────────────────────────────────────────────────────
    let on_browse = move |_| {
        spawn(async move {
            let start = output_folder.read().clone();
            let dialog = rfd::AsyncFileDialog::new().set_directory(&start);
            if let Some(p) = dialog.pick_folder().await {
                output_folder.set(p.path().to_path_buf());
            }
        });
    };

    let on_folder_input = move |evt: Event<FormData>| {
        output_folder.set(PathBuf::from(evt.value()));
    };

    let on_device_change = move |evt: Event<FormData>| {
        selected_device.set(evt.value());
    };

    let start_recording = {
        let controller = Arc::clone(&controller);
        move |_| {
            error.set(None);
            let folder = output_folder.read().clone();
            if !folder.is_dir() {
                error.set(Some(format!(
                    "Output folder does not exist: {}",
                    folder.display()
                )));
                return;
            }
            let transcription = saved_transcription.read().clone();
            if transcription.enabled
                && (transcription.base_url.trim().is_empty()
                    || transcription.api_key.trim().is_empty())
            {
                error.set(Some(
                    "Transcription activée : URL de base ou clé API manquante. Vérifiez ⚙."
                        .to_string(),
                ));
                return;
            }
            session_transcription.set(Some(transcription.clone()));

            let stamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
            let path = folder.join(format!("meeting_{}.mp3", stamp));
            session_mp3_path.set(Some(path.clone()));
            let device = selected_device.read().clone();
            let tc = if transcription.enabled { Some(transcription) } else { None };
            controller.start(device, path, tc);
        }
    };

    let pause_recording = {
        let controller = Arc::clone(&controller);
        move |_| controller.pause()
    };
    let resume_recording = {
        let controller = Arc::clone(&controller);
        move |_| controller.resume()
    };
    let stop_recording = {
        let controller = Arc::clone(&controller);
        move |_| {
            // Capture session state before stopping.
            let transcript = transcript_text.peek().clone();
            let was_tc_enabled = session_transcription.peek().is_some();
            let mp3 = session_mp3_path.peek().clone();

            controller.stop();
            session_transcription.set(None);

            let is_empty = transcript.trim().is_empty();
            if !was_tc_enabled {
                toast.set(Some(Toast::Simple("Enregistrement terminé".to_string())));
            } else if is_empty {
                toast.set(Some(Toast::Simple(
                    "Pas de transcript disponible pour le résumé".to_string(),
                )));
            } else {
                pending_transcript.set(transcript);
                pending_mp3_path.set(mp3);
                participants_text.set(String::new());
                summary_error.set(None);
                summary_generating.set(false);
                show_participants.set(true);
            }
        }
    };

    // ── panel toggle ────────────────────────────────────────────────────
    let toggle_panel = {
        let window = window.clone();
        move |_| {
            let new_val = !saved_ui_cfg.read().transcription_panel_expanded;
            saved_ui_cfg.set(UiConfig { transcription_panel_expanded: new_val });
            let w: f64 = if new_val { 860.0 } else { 580.0 };
            window.set_inner_size(LogicalSize::new(w, 240.0_f64));
        }
    };

    // ── summary generation handler ───────────────────────────────────────
    let generate_summary = move |_| {
        let participants = participants_text.peek().clone();
        let transcript = pending_transcript.peek().clone();
        let config = saved_summary.peek().clone();
        let mp3 = pending_mp3_path.peek().clone().unwrap_or_default();

        summary_generating.set(true);
        summary_error.set(None);

        // Run the blocking HTTP call on a separate thread; poll result async.
        let cell: Arc<Mutex<Option<Result<ParsedSummary, String>>>> =
            Arc::new(Mutex::new(None));
        let cell_t = Arc::clone(&cell);

        std::thread::spawn(move || {
            let result = call_summary_api(&config, &transcript, &participants)
                .map_err(|e| e.to_string());
            *cell_t.lock().unwrap() = Some(result);
        });

        spawn(async move {
            loop {
                futures_timer::Delay::new(Duration::from_millis(200)).await;
                if let Some(result) = cell.lock().unwrap().take() {
                    match result {
                        Ok(parsed) => {
                            let degraded = parsed.is_degraded();
                            let markdown = parsed.into_markdown();
                            let md_path = mp3.with_extension("md");
                            let txt_path = mp3.with_extension("txt");
                            if let Err(e) = std::fs::write(&md_path, &markdown) {
                                summary_error
                                    .set(Some(format!("Erreur d'écriture du .md : {e}")));
                            } else {
                                show_participants.set(false);
                                if degraded {
                                    toast.set(Some(Toast::Degraded {
                                        mp3: mp3.clone(),
                                        txt: txt_path,
                                        md: md_path,
                                    }));
                                } else {
                                    toast.set(Some(Toast::Success {
                                        mp3: mp3.clone(),
                                        txt: txt_path,
                                        md: md_path,
                                    }));
                                }
                            }
                        }
                        Err(e) => {
                            summary_error.set(Some(e));
                        }
                    }
                    summary_generating.set(false);
                    break;
                }
            }
        });
    };

    let close_toast = move |_| toast.set(None);
    let open_folder = move |_| {
        let mp3 = match toast.peek().clone() {
            Some(Toast::Success { mp3, .. }) | Some(Toast::Degraded { mp3, .. }) => mp3,
            _ => return,
        };
        if let Some(parent) = mp3.parent() {
            open_folder_cmd(parent);
        }
    };

    // ── settings modal handlers ──────────────────────────────────────────
    let open_settings = move |_| {
        let t = saved_transcription.read().clone();
        let s = saved_summary.read().clone();
        d_t_enabled.set(t.enabled);
        d_t_base_url.set(t.base_url);
        d_t_api_key.set(t.api_key.clone());
        d_t_model.set(t.model);
        d_t_chunk.set(t.chunk_seconds.to_string());
        d_t_lang.set(t.language.unwrap_or_default());
        d_s_base_url.set(s.base_url);
        d_s_api_key.set(s.api_key);
        d_s_model.set(s.model);
        d_s_prompt.set(s.prompt_template);
        d_same_key.set(false);
        modal_error.set(None);
        settings_tab.set(0);
        show_settings.set(true);
    };

    // Shortcut from blocked-record hint (left column) → opens settings on tab 0.
    let configure_tc_hint = move |_| {
        let t = saved_transcription.read().clone();
        let s = saved_summary.read().clone();
        d_t_enabled.set(t.enabled);
        d_t_base_url.set(t.base_url);
        d_t_api_key.set(t.api_key.clone());
        d_t_model.set(t.model);
        d_t_chunk.set(t.chunk_seconds.to_string());
        d_t_lang.set(t.language.unwrap_or_default());
        d_s_base_url.set(s.base_url);
        d_s_api_key.set(s.api_key);
        d_s_model.set(s.model);
        d_s_prompt.set(s.prompt_template);
        d_same_key.set(false);
        modal_error.set(None);
        settings_tab.set(0);
        show_settings.set(true);
    };

    // Same as above but for transcript panel buttons (separate closure to avoid double-move).
    let configure_tc_panel = move |_| {
        let t = saved_transcription.read().clone();
        let s = saved_summary.read().clone();
        d_t_enabled.set(t.enabled);
        d_t_base_url.set(t.base_url);
        d_t_api_key.set(t.api_key.clone());
        d_t_model.set(t.model);
        d_t_chunk.set(t.chunk_seconds.to_string());
        d_t_lang.set(t.language.unwrap_or_default());
        d_s_base_url.set(s.base_url);
        d_s_api_key.set(s.api_key);
        d_s_model.set(s.model);
        d_s_prompt.set(s.prompt_template);
        d_same_key.set(false);
        modal_error.set(None);
        settings_tab.set(0);
        show_settings.set(true);
    };

    let save_settings = move |_| {
        let enabled = *d_t_enabled.read();
        let t_url = d_t_base_url.read().clone();
        let t_key = d_t_api_key.read().clone();

        if enabled && (t_url.trim().is_empty() || t_key.trim().is_empty()) {
            modal_error.set(Some(
                "Transcription activée : URL de base et clé API sont requises.".to_string(),
            ));
            return;
        }

        let s_key = if *d_same_key.read() {
            t_key.clone()
        } else {
            d_s_api_key.read().clone()
        };
        let chunk: u32 = d_t_chunk.read().parse().unwrap_or(8);
        let lang = d_t_lang.read().clone();

        saved_transcription.set(TranscriptionConfig {
            enabled,
            base_url: t_url,
            api_key: t_key,
            model: d_t_model.read().clone(),
            chunk_seconds: chunk,
            language: if lang.trim().is_empty() {
                None
            } else {
                Some(lang)
            },
        });
        saved_summary.set(SummaryConfig {
            base_url: d_s_base_url.read().clone(),
            api_key: s_key,
            model: d_s_model.read().clone(),
            prompt_template: d_s_prompt.read().clone(),
        });
        show_settings.set(false);
    };

    rsx! {
        style { {STYLE} }
        div { class: if panel_expanded { "app panel-open" } else { "app" },

// ── left column: transport ────────────────────────────
            div { class: "left",
                div { class: "controls",
                    if !recording_now {
                        button {
                            class: "ctl",
                            title: if transcription_unconfigured { "Transcription non configurée" } else { "Record" },
                            disabled: transcription_unconfigured,
                            onclick: start_recording,
                            span { class: "circle" }
                        }
                    } else {
                        if paused_now {
                            button {
                                class: "ctl",
                                title: "Resume",
                                onclick: resume_recording,
                                span { class: "tri" }
                            }
                        } else {
                            button {
                                class: "ctl rec",
                                title: "Recording",
                                disabled: true,
                                span { class: "circle" }
                            }
                            button {
                                class: "ctl",
                                title: "Pause",
                                onclick: pause_recording,
                                span { class: "pause-bars",
                                    span {}
                                    span {}
                                }
                            }
                        }
                        button {
                            class: "ctl",
                            title: "Stop",
                            onclick: stop_recording,
                            span { class: "square" }
                        }
                    }
                }

                div { class: "timer", "{timer_text}" }

                div { class: "vumeter",
                    div {
                        class: "vu-bar",
                        style: "width: {meter_pct}%;",
                    }
                }
            }

            // ── right column: settings ────────────────────────────
            div { class: "right",
                div { class: "row",
                    label { "Output folder" }
                    input {
                        class: "input",
                        r#type: "text",
                        value: "{folder_str}",
                        oninput: on_folder_input,
                    }
                    button {
                        class: "browse",
                        onclick: on_browse,
                        "Browse…"
                    }
                }

                div { class: "row",
                    label { "Input device" }
                    select {
                        class: "select",
                        value: "{device_value}",
                        onchange: on_device_change,
                        for entry in devices.read().iter() {
                            option {
                                value: "{entry.id}",
                                selected: entry.id == device_value,
                                "{entry.label}"
                            }
                        }
                    }
                }

                div { class: "footer-row",
                    div { class: "footer-left",
                        if let Some(msg) = error.read().clone() {
                            div { class: "error", "{msg}" }
                        } else {
                            div { class: "footer", "mono · 32 kbps · 16 kHz" }
                        }
                    }
                    button {
                        class: "settings-btn",
                        title: "Paramètres",
                        onclick: open_settings,
                        "⚙"
                    }
                }
            }

            if transcription_unconfigured {
                div { class: "rec-blocked-hint",
                    "Transcription activée sans clé API"
                    button {
                        class: "btn-setup",
                        style: "display: block; margin-top: 4px;",
                        onclick: configure_tc_hint,
                        "⚙ Configurer"
                    }
                }
            }
            // ── transcript panel (third column, only when expanded) ──
            if panel_expanded {
                div { class: "panel",
                    div { class: "panel-hdr", "Transcription" }
                    div { class: "panel-body",
                        if transcription_unconfigured {
                            div { class: "panel-notice",
                                "Clé API manquante — l'enregistrement est désactivé."
                                button {
                                    class: "btn-setup",
                                    onclick: configure_tc_panel,
                                    "⚙ Configurer la transcription"
                                }
                            }
                        } else if !saved_transcription.read().enabled {
                            div { class: "panel-notice",
                                "Transcription désactivée."
                                button {
                                    class: "btn-setup",
                                    onclick: configure_tc_panel,
                                    "⚙ Activer dans les paramètres"
                                }
                            }
                        } else if recording_now {
                            if transcript_lines.read().is_empty() {
                                div { class: "panel-notice", "Transcription en cours…" }
                            } else {
                                for line in transcript_lines.read().iter() {
                                    div { class: "panel-line", "{line}" }
                                }
                            }
                        } else {
                            div { class: "panel-notice",
                                "Lancez un enregistrement pour démarrer la transcription."
                            }
                        }
                    }
                }
            }
        }

        // ── chevron toggle (fixed right edge) ────────────────────
        button {
            class: "chevron-btn",
            title: if panel_expanded { "Réduire le panel" } else { "Afficher la transcription" },
            onclick: toggle_panel,
            if panel_expanded { "‹" } else { "›" }
        }

        // ── participants modal ────────────────────────────────────
        if *show_participants.read() {
            div { class: "modal-backdrop",
                div { class: "modal",
                    div { class: "modal-header",
                        span { class: "modal-title", "Participants" }
                    }
                    div { class: "modal-body",
                        div { class: "form-row",
                            span { class: "form-label", "Participants" }
                            textarea {
                                class: "form-input form-textarea",
                                placeholder: "Un participant par ligne, ou format libre…",
                                value: "{participants_text}",
                                disabled: *summary_generating.read(),
                                oninput: move |e| { participants_text.set(e.value()); },
                            }
                            span { class: "form-hint",
                                "Laissez vide si les participants ne sont pas connus."
                            }
                        }
                        if *summary_generating.read() {
                            div { class: "generating-hint", "Génération du résumé en cours…" }
                        }
                        if let Some(err) = summary_error.read().clone() {
                            div { class: "modal-err", "{err}" }
                        }
                    }
                    div { class: "modal-footer",
                        button {
                            class: "btn btn-ghost",
                            disabled: *summary_generating.read(),
                            onclick: move |_| { show_participants.set(false); },
                            "Annuler"
                        }
                        if *summary_generating.read() {
                            button {
                                class: "btn btn-primary",
                                disabled: true,
                                "Génération…"
                            }
                        } else if summary_error.read().is_some() {
                            button {
                                class: "btn btn-primary",
                                onclick: generate_summary,
                                "Réessayer"
                            }
                        } else {
                            button {
                                class: "btn btn-primary",
                                onclick: generate_summary,
                                "Générer le résumé"
                            }
                        }
                    }
                }
            }
        }

        // ── settings modal ────────────────────────────────────────
        if *show_settings.read() {
            div { class: "modal-backdrop",
                div { class: "modal",

                    div { class: "modal-header",
                        span { class: "modal-title", "Paramètres" }
                        button {
                            class: "modal-close",
                            onclick: move |_| { show_settings.set(false); },
                            "✕"
                        }
                    }

                    div { class: "modal-tabs",
                        button {
                            class: if tab == 0 { "modal-tab active" } else { "modal-tab" },
                            onclick: move |_| { settings_tab.set(0); },
                            "Transcription"
                        }
                        button {
                            class: if tab == 1 { "modal-tab active" } else { "modal-tab" },
                            onclick: move |_| { settings_tab.set(1); },
                            "Résumé"
                        }
                    }

                    div { class: "modal-body",
                        if tab == 0 {
                            label { class: "form-check-row",
                                input {
                                    r#type: "checkbox",
                                    checked: *d_t_enabled.read(),
                                    onclick: move |_| {
                                        let current = *d_t_enabled.read();
                                        d_t_enabled.set(!current);
                                    },
                                }
                                "Activer la transcription live"
                            }
                            div { class: "form-row",
                                span { class: "form-label", "URL de base" }
                                input {
                                    class: "form-input",
                                    r#type: "text",
                                    placeholder: "https://openrouter.ai/api/v1",
                                    value: "{d_t_base_url}",
                                    oninput: move |e| { d_t_base_url.set(e.value()); },
                                }
                            }
                            div { class: "form-row",
                                span { class: "form-label", "Clé API" }
                                input {
                                    class: "form-input",
                                    r#type: "password",
                                    placeholder: "sk-…",
                                    value: "{d_t_api_key}",
                                    oninput: move |e| {
                                        let val = e.value();
                                        d_t_api_key.set(val.clone());
                                        if *d_same_key.peek() {
                                            d_s_api_key.set(val);
                                        }
                                    },
                                }
                            }
                            div { class: "form-row",
                                span { class: "form-label", "Modèle STT" }
                                input {
                                    class: "form-input",
                                    r#type: "text",
                                    placeholder: "openai/whisper-1",
                                    value: "{d_t_model}",
                                    oninput: move |e| { d_t_model.set(e.value()); },
                                }
                            }
                            div { class: "form-row",
                                span { class: "form-label", "Durée chunk (s)" }
                                input {
                                    class: "form-input",
                                    r#type: "number",
                                    min: "1",
                                    max: "60",
                                    value: "{d_t_chunk}",
                                    oninput: move |e| { d_t_chunk.set(e.value()); },
                                }
                            }
                            div { class: "form-row",
                                span { class: "form-label", "Langue (optionnel)" }
                                input {
                                    class: "form-input",
                                    r#type: "text",
                                    placeholder: "fr, en, … (vide = auto)",
                                    value: "{d_t_lang}",
                                    oninput: move |e| { d_t_lang.set(e.value()); },
                                }
                            }
                        } else {
                            div { class: "form-row",
                                span { class: "form-label", "URL de base" }
                                input {
                                    class: "form-input",
                                    r#type: "text",
                                    placeholder: "https://openrouter.ai/api/v1",
                                    value: "{d_s_base_url}",
                                    oninput: move |e| { d_s_base_url.set(e.value()); },
                                }
                            }
                            label { class: "form-check-row",
                                input {
                                    r#type: "checkbox",
                                    checked: same_key,
                                    onclick: move |_| {
                                        let new_val = !*d_same_key.peek();
                                        d_same_key.set(new_val);
                                        if new_val {
                                            d_s_api_key.set(d_t_api_key.peek().clone());
                                        }
                                    },
                                }
                                "Utiliser la même clé API que pour la transcription"
                            }
                            div { class: "form-row",
                                span { class: "form-label", "Clé API" }
                                input {
                                    class: "form-input",
                                    r#type: "password",
                                    placeholder: "sk-…",
                                    value: "{d_s_api_key}",
                                    disabled: same_key,
                                    oninput: move |e| {
                                        if !*d_same_key.peek() {
                                            d_s_api_key.set(e.value());
                                        }
                                    },
                                }
                            }
                            div { class: "form-row",
                                span { class: "form-label", "Modèle chat" }
                                input {
                                    class: "form-input",
                                    r#type: "text",
                                    placeholder: "openai/gpt-4o-mini",
                                    value: "{d_s_model}",
                                    oninput: move |e| { d_s_model.set(e.value()); },
                                }
                            }
                            div { class: "form-row",
                                span { class: "form-label", "Template prompt" }
                                textarea {
                                    class: "form-input form-textarea",
                                    value: "{d_s_prompt}",
                                    oninput: move |e| { d_s_prompt.set(e.value()); },
                                }
                                span { class: "form-hint",
                                    "Jetons disponibles : {{transcript}}, {{participants}}"
                                }
                            }
                        }

                        div { class: "modal-warning",
                            "⚠ Les clés sont stockées en clair dans config.toml"
                        }

                        if let Some(err) = modal_error.read().clone() {
                            div { class: "modal-err", "{err}" }
                        }
                    }

                    div { class: "modal-footer",
                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| { show_settings.set(false); },
                            "Annuler"
                        }
                        button {
                            class: "btn btn-primary",
                            onclick: save_settings,
                            "Enregistrer"
                        }
                    }
                }
            }
        }

        // ── toast notification ────────────────────────────────────
        if let Some(t) = toast.read().clone() {
            div { class: "toast",
                div { class: "toast-header",
                    div { class: "toast-message",
                        match &t {
                            Toast::Simple(msg) => rsx! { "{msg}" },
                            Toast::Success { .. } => rsx! { "Résumé généré avec succès !" },
                            Toast::Degraded { .. } => rsx! {
                                "Modèle n'a pas honoré tool_choice — résumé sauvé en mode dégradé"
                            },
                        }
                    }
                    button {
                        class: "toast-close",
                        onclick: close_toast,
                        "✕"
                    }
                }
                if let Toast::Success { mp3, txt, md } | Toast::Degraded { mp3, txt, md } = &t {
                    div { class: "toast-paths",
                        span { "{mp3.display()}" }
                        span { "{txt.display()}" }
                        span { "{md.display()}" }
                    }
                    div { class: "toast-actions",
                        button {
                            class: "btn-folder",
                            onclick: open_folder,
                            "Ouvrir le dossier"
                        }
                    }
                }
            }
        }
    }
}
