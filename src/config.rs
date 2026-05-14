use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub output_folder: Option<PathBuf>,
    pub input_device: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        match Self::path() {
            Some(path) if path.exists() => match std::fs::read_to_string(&path) {
                Ok(text) => toml::from_str(&text).unwrap_or_default(),
                Err(_) => Self::default(),
            },
            _ => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path().context("could not determine config dir")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn path() -> Option<PathBuf> {
        ProjectDirs::from("com", "geoffroy", "meetrec")
            .map(|d| d.config_dir().join("config.toml"))
    }

    pub fn default_output_folder() -> PathBuf {
        directories::UserDirs::new()
            .and_then(|d| d.audio_dir().map(Path::to_path_buf))
            .or_else(|| directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
