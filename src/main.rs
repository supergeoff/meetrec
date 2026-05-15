#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod config;
mod devices;
mod encoder;
mod summary;
mod transcription;
mod ui;

use std::sync::Arc;

use dioxus::prelude::*;
use dioxus_desktop::{tao::window::Icon, Config as DesktopConfig, LogicalSize, WindowBuilder};
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};

use crate::audio::AudioController;
use crate::config::Config;
use crate::ui::App;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn init_logger(data_dir: &std::path::Path) {
    let file_spec = FileSpec::default()
        .directory(data_dir)
        .basename("meetrec")
        .suppress_timestamp()
        .suffix("log");

    Logger::try_with_env_or_str("info")
        .expect("invalid RUST_LOG value")
        .log_to_file(file_spec)
        .duplicate_to_stderr(Duplicate::All)
        .append()
        .rotate(
            Criterion::Size(5 * 1024 * 1024),
            Naming::Numbers,
            Cleanup::KeepLogFiles(1),
        )
        .start()
        .expect("failed to start logger");
}

fn main() {
    let data_dir = config::data_dir().expect("cannot determine app data directory");
    std::fs::create_dir_all(&data_dir).expect("cannot create app data directory");

    init_logger(&data_dir);

    config::migrate_if_needed();

    log::info!(
        "meetrec {} starting (OS: {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    );
    log::info!("config: {}", Config::path().map(|p| p.display().to_string()).unwrap_or_default());
    log::info!("log:    {}", data_dir.join("meetrec.log").display());

    let icon = load_icon();

    let initial_cfg = Config::load();
    let initial_height: f64 = if initial_cfg.ui.transcription_panel_expanded {
        440.0
    } else {
        240.0
    };

    let window = WindowBuilder::new()
        .with_title("meetrec")
        .with_inner_size(LogicalSize::new(580.0, initial_height))
        .with_min_inner_size(LogicalSize::new(500.0, 200.0))
        .with_resizable(true)
        .with_window_icon(icon);

    // `with_menu(None)` hides the wry/tao default app menu bar
    // ("Window/Edit" on Windows) — we don't need it.
    let desktop_cfg = DesktopConfig::new().with_window(window).with_menu(None);

    LaunchBuilder::desktop()
        .with_cfg(desktop_cfg)
        .with_context(Arc::new(AudioController::spawn()))
        .launch(App);
}

fn load_icon() -> Option<Icon> {
    let img = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}
