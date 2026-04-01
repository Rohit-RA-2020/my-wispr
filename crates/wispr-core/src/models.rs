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
    Generation,
}

impl DecisionKind {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Action => "action",
            Self::LiteralAndAction => "literal_and_action",
            Self::Generation => "generation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStyle {
    Generic,
    PlainText,
    Email,
    Essay,
}

impl Default for GenerationStyle {
    fn default() -> Self {
        Self::Generic
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationTriggerMode {
    ExplicitRequests,
}

impl Default for GenerationTriggerMode {
    fn default() -> Self {
        Self::ExplicitRequests
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationInsertMode {
    ReplaceRequest,
}

impl Default for GenerationInsertMode {
    fn default() -> Self {
        Self::ReplaceRequest
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationTargetScope {
    AnyTextField,
}

impl Default for GenerationTargetScope {
    fn default() -> Self {
        Self::AnyTextField
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Key,
    Shortcut,
    SemanticCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    Ctrl,
    Shift,
    Alt,
    Super,
}

impl ModifierKey {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Shift => "Shift",
            Self::Alt => "Alt",
            Self::Super => "Super",
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
    Insert,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
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
#[serde(rename_all = "snake_case")]
pub enum ActiveAppClass {
    Browser,
    Editor,
    Terminal,
    Generic,
}

impl Default for ActiveAppClass {
    fn default() -> Self {
        Self::Generic
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ActiveAppContext {
    #[serde(default)]
    pub app_class: ActiveAppClass,
    #[serde(default)]
    pub app_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCommandId {
    NewTab,
    CloseTab,
    ReopenClosedTab,
    Refresh,
    Find,
    Save,
    Copy,
    Paste,
    Cut,
    Undo,
    Redo,
    FocusAddressBar,
    NextTab,
    PreviousTab,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutDenylistProfile {
    Minimal,
}

impl Default for ShortcutDenylistProfile {
    fn default() -> Self {
        Self::Minimal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RewriteScope {
    Segment,
    CurrentBlock,
}

impl Default for RewriteScope {
    fn default() -> Self {
        Self::Segment
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FormatKind {
    Plain,
    NumberedList,
    BulletList,
}

impl Default for FormatKind {
    fn default() -> Self {
        Self::Plain
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreferredListStyle {
    Numbered,
}

impl Default for PreferredListStyle {
    fn default() -> Self {
        Self::Numbered
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FormattingTriggerPolicy {
    ClearStructureOnly,
}

impl Default for FormattingTriggerPolicy {
    fn default() -> Self {
        Self::ClearStructureOnly
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionScope {
    CurrentBlockOnly,
}

impl Default for CorrectionScope {
    fn default() -> Self {
        Self::CurrentBlockOnly
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionCommand {
    #[serde(rename = "type")]
    pub action_type: ActionType,
    #[serde(default)]
    pub key: Option<ActionKey>,
    #[serde(default)]
    pub modifiers: Vec<ModifierKey>,
    #[serde(default = "default_repeat")]
    pub repeat: u8,
    #[serde(default)]
    pub command_id: Option<SemanticCommandId>,
    #[serde(default)]
    pub target_app: Option<ActiveAppClass>,
}

fn default_repeat() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentDecision {
    pub kind: DecisionKind,
    #[serde(default)]
    pub rewrite_scope: RewriteScope,
    #[serde(default)]
    pub format_kind: FormatKind,
    pub text_to_emit: String,
    #[serde(default)]
    pub keep_block_open: bool,
    #[serde(default)]
    pub actions: Vec<ActionCommand>,
    #[serde(default)]
    pub generation_prompt: Option<String>,
    #[serde(default)]
    pub generation_style: Option<GenerationStyle>,
    #[serde(default)]
    pub replace_current_segment: bool,
}

impl SegmentDecision {
    pub fn literal(text: impl Into<String>) -> Self {
        Self {
            kind: DecisionKind::Literal,
            rewrite_scope: RewriteScope::Segment,
            format_kind: FormatKind::Plain,
            text_to_emit: text.into(),
            keep_block_open: false,
            actions: Vec::new(),
            generation_prompt: None,
            generation_style: None,
            replace_current_segment: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub request_text: String,
    pub generation_prompt: String,
    #[serde(default)]
    pub generation_style: GenerationStyle,
    #[serde(default)]
    pub recent_text: String,
    #[serde(default)]
    pub active_app: Option<ActiveAppContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentDecisionRequest {
    pub segment_id: String,
    pub finalized_text: String,
    pub literal_text: String,
    pub recent_text: String,
    #[serde(default)]
    pub active_block_raw: String,
    #[serde(default)]
    pub active_block_rendered: String,
    pub action_scope: ActionScope,
    pub command_mode: CommandMode,
    pub text_output_mode: TextOutputMode,
    #[serde(default)]
    pub preferred_list_style: PreferredListStyle,
    #[serde(default)]
    pub formatting_trigger_policy: FormattingTriggerPolicy,
    #[serde(default)]
    pub correction_scope: CorrectionScope,
    #[serde(default)]
    pub active_app: Option<ActiveAppContext>,
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
    pub active_app: Option<ActiveAppContext>,
    pub last_resolution: Option<String>,
    pub generation_active: bool,
    pub generation_ready: bool,
    pub last_generation_error: Option<String>,
    pub generation_state: Option<String>,
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
            active_app: None,
            last_resolution: None,
            generation_active: false,
            generation_ready: false,
            last_generation_error: None,
            generation_state: None,
            updated_at: Utc::now(),
        }
    }
}
