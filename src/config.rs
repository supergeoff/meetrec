use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub trait DirSource {
    fn config_base(&self) -> Option<PathBuf>;
}

struct SystemDirs;

impl DirSource for SystemDirs {
    fn config_base(&self) -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| b.config_dir().to_path_buf())
    }
}

pub fn data_dir_with(source: &dyn DirSource) -> Option<PathBuf> {
    source.config_base().map(|d| d.join("meetrec"))
}

pub fn data_dir() -> Option<PathBuf> {
    data_dir_with(&SystemDirs)
}

fn migrate_dirs(old_dir: &Path, new_dir: &Path) {
    if old_dir.exists() && !new_dir.exists() {
        match std::fs::rename(old_dir, new_dir) {
            Ok(()) => log::info!(
                "migrated data dir: {} → {}",
                old_dir.display(),
                new_dir.display()
            ),
            Err(e) => log::warn!(
                "could not migrate {}: {}",
                old_dir.display(),
                e
            ),
        }
    }
}

pub fn migrate_if_needed() {
    let new_dir = match data_dir() {
        Some(d) => d,
        None => return,
    };
    let old_dir = match directories::ProjectDirs::from("com", "geoffroy", "meetrec")
        .map(|d| d.config_dir().to_path_buf())
    {
        Some(d) => d,
        None => return,
    };
    migrate_dirs(&old_dir, &new_dir);
}

const PLAIN_TEXT_WARNING: &str = "\
# WARNING: API keys are stored in plain text in this file.\n\
# Do not share this file or commit it to version control.\n\n";

const DEFAULT_SUMMARY_PROMPT: &str = "\
Summarize the following meeting.\n\
\n\
Participants: {participants}\n\
\n\
Transcript:\n\
{transcript}\n\
\n\
---\n\
Key points:\n\
- \n\
Action items:\n\
- ";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub chunk_seconds: u32,
    pub language: Option<String>,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: String::new(),
            model: "openai/whisper-1".to_string(),
            chunk_seconds: 8,
            language: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub prompt_template: String,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: String::new(),
            model: "openai/gpt-4o-mini".to_string(),
            prompt_template: DEFAULT_SUMMARY_PROMPT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiConfig {
    pub transcription_panel_expanded: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub output_folder: Option<PathBuf>,
    pub input_device: Option<String>,
    #[serde(default)]
    pub transcription: TranscriptionConfig,
    #[serde(default)]
    pub summary: SummaryConfig,
    #[serde(default)]
    pub ui: UiConfig,
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
        let body = toml::to_string_pretty(self)?;
        let text = format!("{}{}", PLAIN_TEXT_WARNING, body);
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("config.toml"))
    }

    pub fn default_output_folder() -> PathBuf {
        directories::UserDirs::new()
            .and_then(|d| d.audio_dir().map(Path::to_path_buf))
            .or_else(|| directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDirSource(PathBuf);

    impl DirSource for FakeDirSource {
        fn config_base(&self) -> Option<PathBuf> {
            Some(self.0.clone())
        }
    }

    #[test]
    fn data_dir_last_segment_is_meetrec() {
        let src = FakeDirSource(PathBuf::from("/home/user/.config"));
        let dir = data_dir_with(&src).unwrap();
        assert_eq!(dir.file_name().unwrap().to_str().unwrap(), "meetrec");
    }

    #[test]
    fn data_dir_contains_no_geoffroy_segment() {
        let src = FakeDirSource(PathBuf::from("/home/user/.config"));
        let dir = data_dir_with(&src).unwrap();
        let s = dir.to_string_lossy();
        assert!(!s.contains("geoffroy"), "path must not contain 'geoffroy': {s}");
    }

    #[test]
    fn migrate_moves_old_dir_contents_to_new_dir() {
        let base = std::env::temp_dir().join("meetrec_test_migrate");
        let old_dir = base.join("old");
        let new_dir = base.join("new");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("config.toml"), b"data").unwrap();

        migrate_dirs(&old_dir, &new_dir);

        assert!(!old_dir.exists(), "old dir should be renamed away");
        assert!(new_dir.join("config.toml").exists(), "config.toml should be at new path");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_skips_when_new_dir_already_exists() {
        let base = std::env::temp_dir().join("meetrec_test_migrate_skip");
        let old_dir = base.join("old");
        let new_dir = base.join("new");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();
        std::fs::write(old_dir.join("config.toml"), b"old").unwrap();
        std::fs::write(new_dir.join("config.toml"), b"new").unwrap();

        migrate_dirs(&old_dir, &new_dir);

        let content = std::fs::read(new_dir.join("config.toml")).unwrap();
        assert_eq!(content, b"new", "new dir content must not be overwritten");
        assert!(old_dir.exists(), "old dir must be left intact when new dir exists");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn default_config_has_expected_values() {
        let cfg = Config::default();
        assert!(!cfg.transcription.enabled);
        assert_eq!(cfg.transcription.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(cfg.transcription.model, "openai/whisper-1");
        assert_eq!(cfg.transcription.chunk_seconds, 8);
        assert!(cfg.transcription.language.is_none());
        assert_eq!(cfg.summary.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(cfg.summary.model, "openai/gpt-4o-mini");
        assert!(cfg.summary.prompt_template.contains("{transcript}"));
        assert!(cfg.summary.prompt_template.contains("{participants}"));
        assert!(!cfg.ui.transcription_panel_expanded);
    }

    #[test]
    fn roundtrip_toml_preserves_all_sections() {
        let original = Config {
            output_folder: None,
            input_device: None,
            transcription: TranscriptionConfig {
                enabled: true,
                base_url: "https://example.com".to_string(),
                api_key: "sk-test".to_string(),
                model: "openai/whisper-1".to_string(),
                chunk_seconds: 12,
                language: Some("fr".to_string()),
            },
            summary: SummaryConfig {
                base_url: "https://example.com".to_string(),
                api_key: "sk-test2".to_string(),
                model: "openai/gpt-4o-mini".to_string(),
                prompt_template: "Custom {transcript} {participants}".to_string(),
            },
            ui: UiConfig {
                transcription_panel_expanded: true,
            },
        };
        let text = toml::to_string_pretty(&original).unwrap();
        let loaded: Config = toml::from_str(&text).unwrap();
        assert_eq!(loaded.transcription.enabled, original.transcription.enabled);
        assert_eq!(loaded.transcription.api_key, original.transcription.api_key);
        assert_eq!(loaded.transcription.chunk_seconds, 12);
        assert_eq!(loaded.transcription.language, Some("fr".to_string()));
        assert_eq!(loaded.summary.model, original.summary.model);
        assert_eq!(loaded.ui.transcription_panel_expanded, true);
    }

    #[test]
    fn existing_toml_without_new_sections_loads_with_defaults() {
        let legacy = r#"
output_folder = "/tmp"
input_device = "default"
"#;
        let cfg: Config = toml::from_str(legacy).unwrap();
        assert!(!cfg.transcription.enabled);
        assert_eq!(cfg.transcription.model, "openai/whisper-1");
        assert_eq!(cfg.summary.model, "openai/gpt-4o-mini");
        assert!(!cfg.ui.transcription_panel_expanded);
    }

    #[test]
    fn save_output_includes_warning_comment() {
        let cfg = Config::default();
        let body = toml::to_string_pretty(&cfg).unwrap();
        let text = format!("{}{}", PLAIN_TEXT_WARNING, body);
        assert!(text.starts_with("# WARNING:"));
        assert!(text.contains("plain text"));
    }
}
