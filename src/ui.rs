use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use dioxus::prelude::*;

use crate::audio::AudioController;
use crate::config::Config;
use crate::devices::{list_input_devices, SYSTEM_DEFAULT};

const STYLE: &str = r#"
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; height: 100%; }
body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: #1d1f23;
    color: #e6e6e6;
    user-select: none;
}
.app {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
}
.row {
    display: flex;
    align-items: center;
    gap: 8px;
}
.row > label {
    flex: 0 0 110px;
    font-size: 13px;
    color: #aaa;
}
.row > input[type="text"], .row > select {
    flex: 1;
    background: #2a2d33;
    color: #e6e6e6;
    border: 1px solid #3a3d44;
    border-radius: 6px;
    padding: 7px 9px;
    font-size: 13px;
    outline: none;
}
.row > input[type="text"]:focus, .row > select:focus {
    border-color: #5a8dee;
}
.row > button {
    background: #3a3d44;
    color: #e6e6e6;
    border: none;
    border-radius: 6px;
    padding: 7px 14px;
    font-size: 13px;
    cursor: pointer;
}
.row > button:hover { background: #474a52; }

.controls {
    display: flex;
    justify-content: center;
    gap: 18px;
    margin-top: 4px;
}
.ctl-btn {
    width: 56px;
    height: 56px;
    border-radius: 50%;
    border: none;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #2a2d33;
    transition: background 120ms ease;
}
.ctl-btn:hover { background: #3a3d44; }
.ctl-btn.stop { background: #2a2d33; }
.ctl-btn .circle {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: #e74c3c;
    box-shadow: 0 0 0 2px rgba(231,76,60,0.18);
}
.ctl-btn.pulsing .circle {
    animation: pulse 1.1s ease-in-out infinite;
}
@keyframes pulse {
    0%, 100% { box-shadow: 0 0 0 0 rgba(231,76,60,0.55); }
    50% { box-shadow: 0 0 0 14px rgba(231,76,60,0.0); }
}
.ctl-btn .pause-bars {
    display: flex;
    gap: 4px;
}
.ctl-btn .pause-bars span {
    display: block;
    width: 5px;
    height: 20px;
    background: #e6e6e6;
    border-radius: 1px;
}
.ctl-btn .square {
    width: 18px;
    height: 18px;
    background: #e6e6e6;
    border-radius: 2px;
}
.ctl-btn .play-tri {
    width: 0;
    height: 0;
    border-left: 14px solid #e6e6e6;
    border-top: 9px solid transparent;
    border-bottom: 9px solid transparent;
    margin-left: 4px;
}
.timer {
    text-align: center;
    font-size: 32px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 2px;
    color: #f0f0f0;
}
.vumeter {
    position: relative;
    height: 10px;
    background: #2a2d33;
    border-radius: 5px;
    overflow: hidden;
}
.vu-bar {
    position: absolute;
    left: 0; top: 0; bottom: 0;
    background: linear-gradient(90deg, #4caf50 0%, #ffeb3b 70%, #e74c3c 100%);
    transition: width 60ms linear;
}
.error {
    background: #5a1f1f;
    color: #ffd9d9;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
}
.hint {
    text-align: center;
    color: #777;
    font-size: 12px;
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
            .unwrap_or_else(|| SYSTEM_DEFAULT.to_string())
    });
    let devices = use_signal(list_input_devices);

    let mut recording = use_signal(|| false);
    let mut paused = use_signal(|| false);
    let mut elapsed_ms = use_signal(|| 0u64);
    let mut peak_db = use_signal(|| f32::NEG_INFINITY);
    let mut error = use_signal::<Option<String>>(|| None);

    // Poll the audio thread state at ~30 Hz.
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

    // Persist config when folder or device changes.
    use_effect(move || {
        let cfg = Config {
            output_folder: Some(output_folder.read().clone()),
            input_device: Some(selected_device.read().clone()),
        };
        if let Err(e) = cfg.save() {
            log::warn!("config save failed: {e:#}");
        }
    });

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
    // Map [-60dB .. 0dB] → [0 .. 100]%.
    let meter_pct: f32 = if db.is_finite() {
        ((db + 60.0) / 60.0 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

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
        let v = evt.value();
        output_folder.set(PathBuf::from(v));
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

            div { class: "row",
                label { "Output folder" }
                input {
                    r#type: "text",
                    value: "{folder_str}",
                    oninput: on_folder_input,
                }
                button { onclick: on_browse, "Browse…" }
            }

            div { class: "row",
                label { "Input device" }
                select {
                    value: "{device_value}",
                    onchange: on_device_change,
                    for d in devices.read().iter() {
                        option { value: "{d}", selected: *d == device_value, "{d}" }
                    }
                }
            }

            div { class: "controls",
                if !recording_now {
                    button {
                        class: "ctl-btn",
                        title: "Record",
                        onclick: start_recording,
                        span { class: "circle" }
                    }
                } else {
                    if paused_now {
                        button {
                            class: "ctl-btn",
                            title: "Resume",
                            onclick: resume_recording,
                            span { class: "play-tri" }
                        }
                    } else {
                        button {
                            class: "ctl-btn pulsing",
                            title: "Recording",
                            disabled: true,
                            span { class: "circle" }
                        }
                        button {
                            class: "ctl-btn",
                            title: "Pause",
                            onclick: pause_recording,
                            span { class: "pause-bars",
                                span {}
                                span {}
                            }
                        }
                    }
                    button {
                        class: "ctl-btn stop",
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

            if let Some(msg) = error.read().clone() {
                div { class: "error", "{msg}" }
            }
        }
    }
}
