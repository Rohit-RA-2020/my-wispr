pub mod config;
pub mod error;
pub mod install;
pub mod ipc;
pub mod models;
pub mod secrets;
pub mod typing;

pub use config::AppConfig;
pub use error::{Result, WisprError};
pub use ipc::{DICTATION_INTERFACE, DICTATION_OBJECT_PATH, DICTATION_SERVICE, DictationProxy};
pub use models::{DaemonStatus, DeviceChoice, DictationState, HotkeyBinding};
