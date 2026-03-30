use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, WisprError},
    models::{
        ActionScope, CommandMode, DeviceChoice, HotkeyBinding, ShortcutDenylistProfile,
        TextOutputMode,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
pub struct IntelligenceConfig {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub timeout_ms: u64,
    pub max_recent_chars: usize,
    pub command_mode: CommandMode,
    pub text_output_mode: TextOutputMode,
    pub action_scope: ActionScope,
    pub debug_overlay: bool,
    pub dynamic_shortcuts_enabled: bool,
    pub semantic_commands_enabled: bool,
    pub shortcut_denylist_profile: ShortcutDenylistProfile,
    pub shortcut_allowlist: Vec<String>,
    pub shortcut_denylist: Vec<String>,
}

impl Default for IntelligenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
            timeout_ms: 2_500,
            max_recent_chars: 256,
            command_mode: CommandMode::AlwaysInfer,
            text_output_mode: TextOutputMode::Literal,
            action_scope: ActionScope::EditingOnly,
            debug_overlay: true,
            dynamic_shortcuts_enabled: true,
            semantic_commands_enabled: true,
            shortcut_denylist_profile: ShortcutDenylistProfile::Minimal,
            shortcut_allowlist: Vec::new(),
            shortcut_denylist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub overlay: OverlayConfig,
    pub typing: TypingConfig,
    pub intelligence: IntelligenceConfig,
    pub autostart: bool,
    pub selected_device: Option<DeviceChoice>,
    pub hotkey: HotkeyBinding,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            overlay: OverlayConfig::default(),
            typing: TypingConfig::default(),
            intelligence: IntelligenceConfig::default(),
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
