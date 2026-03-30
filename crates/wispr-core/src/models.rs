use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DictationState {
    Idle,
    Listening,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandMode {
    AlwaysInfer,
}

impl Default for CommandMode {
    fn default() -> Self {
        Self::AlwaysInfer
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextOutputMode {
    Literal,
}

impl Default for TextOutputMode {
    fn default() -> Self {
        Self::Literal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionScope {
    EditingOnly,
}

impl Default for ActionScope {
    fn default() -> Self {
        Self::EditingOnly
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Literal,
    Action,
    LiteralAndAction,
}

impl DecisionKind {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Action => "action",
            Self::LiteralAndAction => "literal_and_action",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Key,
    Shortcut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    Ctrl,
    Shift,
}

impl ModifierKey {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Shift => "Shift",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ActionKey {
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    A,
    C,
    V,
    X,
    Z,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionCommand {
    #[serde(rename = "type")]
    pub action_type: ActionType,
    pub key: ActionKey,
    #[serde(default)]
    pub modifiers: Vec<ModifierKey>,
    #[serde(default = "default_repeat")]
    pub repeat: u8,
}

fn default_repeat() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentDecision {
    pub kind: DecisionKind,
    pub text_to_emit: String,
    #[serde(default)]
    pub actions: Vec<ActionCommand>,
}

impl SegmentDecision {
    pub fn literal(text: impl Into<String>) -> Self {
        Self {
            kind: DecisionKind::Literal,
            text_to_emit: text.into(),
            actions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentDecisionRequest {
    pub segment_id: String,
    pub finalized_text: String,
    pub literal_text: String,
    pub recent_text: String,
    pub action_scope: ActionScope,
    pub command_mode: CommandMode,
    pub text_output_mode: TextOutputMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub state: DictationState,
    pub mic_ready: bool,
    pub typing_ready: bool,
    pub hotkey_ready: bool,
    pub intelligence_ready: bool,
    pub llm_ready: bool,
    pub current_mic: Option<DeviceChoice>,
    pub partial_transcript: Option<String>,
    pub last_error: Option<String>,
    pub last_llm_error: Option<String>,
    pub last_decision_kind: Option<DecisionKind>,
    pub intelligence_state: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for DaemonStatus {
    fn default() -> Self {
        Self {
            state: DictationState::Idle,
            mic_ready: false,
            typing_ready: false,
            hotkey_ready: false,
            intelligence_ready: false,
            llm_ready: false,
            current_mic: None,
            partial_transcript: None,
            last_error: None,
            last_llm_error: None,
            last_decision_kind: None,
            intelligence_state: None,
            updated_at: Utc::now(),
        }
    }
}
