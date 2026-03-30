pub mod config;
pub mod error;
pub mod install;
pub mod ipc;
pub mod llm;
pub mod models;
pub mod secrets;
pub mod typing;

pub use config::AppConfig;
pub use error::{Result, WisprError};
pub use ipc::{DICTATION_INTERFACE, DICTATION_OBJECT_PATH, DICTATION_SERVICE, DictationProxy};
pub use llm::{InterpreterOutput, LlmInterpreter};
pub use models::{
    ActionCommand, ActionKey, ActionScope, ActionType, CommandMode, CorrectionScope, DaemonStatus,
    DecisionKind, DeviceChoice, DictationState, FormatKind, FormattingTriggerPolicy, HotkeyBinding,
    ModifierKey, PreferredListStyle, RewriteScope, SegmentDecision, SegmentDecisionRequest,
    TextOutputMode,
};
