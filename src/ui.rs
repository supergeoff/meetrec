use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use dioxus::prelude::*;

use crate::audio::AudioController;
use crate::config::Config;
use crate::devices::{list_input_devices, SYSTEM_DEFAULT_ID};

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
.ctl:disabled { cursor: default; }
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

.footer {
    margin-top: 2px;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--fg-faint);
    letter-spacing: 0.04em;
    text-align: center;
}

.error {
    background: var(--ink);
    color: var(--paper);
    padding: 6px 10px;
    border-radius: var(--r-sm);
    font-size: 11px;
    line-height: 1.4;
}
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

    let mut recording = use_signal(|| false);
    let mut paused = use_signal(|| false);
    let mut elapsed_ms = use_signal(|| 0u64);
    let mut peak_db = use_signal(|| f32::NEG_INFINITY);
    let mut error = use_signal::<Option<String>>(|| None);

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
                    if let Some(msg) = state.take_error() {
                        error.set(Some(msg));
                    }
                }
            }
        });
    }

    // ── persist config when folder or device changes ────────────────────
    use_effect(move || {
        let cfg = Config {
            output_folder: Some(output_folder.read().clone()),
            input_device: Some(selected_device.read().clone()),
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
            let stamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
            let path = folder.join(format!("meeting_{}.mp3", stamp));
            let device = selected_device.read().clone();
            controller.start(device, path);
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
        move |_| controller.stop()
    };

    rsx! {
        style { {STYLE} }
        div { class: "app",

            // ── left column: transport ────────────────────────────
            div { class: "left",
                div { class: "controls",
                    if !recording_now {
                        button {
                            class: "ctl",
                            title: "Record",
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

                if let Some(msg) = error.read().clone() {
                    div { class: "error", "{msg}" }
                } else {
                    div { class: "footer", "mono · 32 kbps · 16 kHz" }
                }
            }
        }
    }
}
