use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, WisprError},
    models::{DeviceChoice, HotkeyBinding},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    pub enabled: bool,
    pub show_partial_text: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_partial_text: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingConfig {
    pub enabled: bool,
    pub emit_backspaces: bool,
}

impl Default for TypingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            emit_backspaces: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub overlay: OverlayConfig,
    pub typing: TypingConfig,
    pub autostart: bool,
    pub selected_device: Option<DeviceChoice>,
    pub hotkey: HotkeyBinding,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            overlay: OverlayConfig::default(),
            typing: TypingConfig::default(),
            autostart: true,
            selected_device: None,
            hotkey: HotkeyBinding::default(),
        }
    }
}

impl AppConfig {
    pub fn config_dir() -> Result<PathBuf> {
        let base = dirs::config_dir().ok_or_else(|| {
            WisprError::InvalidState("could not determine XDG config directory".to_string())
        })?;
        Ok(base.join("wispr"))
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn ensure_parent_dirs(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let default = Self::default();
            default.save()?;
            return Ok(default);
        }

        let contents = fs::read_to_string(path)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        Self::ensure_parent_dirs(&path)?;
        let toml = toml::to_string_pretty(self)?;
        fs::write(path, toml)?;
        Ok(())
    }
}
