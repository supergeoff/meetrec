#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod config;
mod devices;
mod encoder;
mod session;
mod summary;
mod transcription;
mod ui;

use std::sync::Arc;

use dioxus::prelude::*;
use dioxus_desktop::{tao::window::Icon, Config as DesktopConfig, LogicalSize, WindowBuilder};

use crate::audio::AudioController;
use crate::config::Config;
use crate::ui::App;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    let icon = load_icon();

    let initial_cfg = Config::load();
    let initial_width: f64 = if initial_cfg.ui.transcription_panel_expanded {
        860.0
    } else {
        580.0
    };

    let window = WindowBuilder::new()
        .with_title("meetrec")
        .with_inner_size(LogicalSize::new(initial_width, 240.0))
        .with_min_inner_size(LogicalSize::new(500.0, 220.0))
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
