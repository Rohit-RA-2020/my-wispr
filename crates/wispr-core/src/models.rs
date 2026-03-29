use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DictationState {
    Idle,
    Listening,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub id: String,
    pub description: String,
    pub preferred_trigger: String,
    pub trigger_description: Option<String>,
}

impl Default for HotkeyBinding {
    fn default() -> Self {
        Self {
            id: "toggle-dictation".to_string(),
            description: "Toggle Wispr dictation".to_string(),
            preferred_trigger: "<Super>d".to_string(),
            trigger_description: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceChoice {
    pub node_name: String,
    pub display_name: String,
    pub fallback_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub state: DictationState,
    pub mic_ready: bool,
    pub typing_ready: bool,
    pub hotkey_ready: bool,
    pub current_mic: Option<DeviceChoice>,
    pub partial_transcript: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for DaemonStatus {
    fn default() -> Self {
        Self {
            state: DictationState::Idle,
            mic_ready: false,
            typing_ready: false,
            hotkey_ready: false,
            current_mic: None,
            partial_transcript: None,
            last_error: None,
            updated_at: Utc::now(),
        }
    }
}
