use async_channel::Receiver;

use crate::{
    Result,
    models::{ActionCommand, ActiveAppContext, DaemonStatus},
    typing::TextPatch,
};

pub trait TypingEngine: Send {
    fn emit_patch(&mut self, patch: &TextPatch) -> Result<()>;
    fn emit_actions(&mut self, actions: &[ActionCommand]) -> Result<()>;
}

pub trait AudioCaptureEngine: Send + Sync {
    fn receiver(&self) -> Receiver<Vec<u8>>;
    fn stop(&self) -> Result<()>;
}

pub trait SecretStoreBackend: Send + Sync {
    fn get_api_key_blocking(&self) -> Result<Option<String>>;
    fn set_api_key_blocking(&self, api_key: &str) -> Result<()>;
    fn get_llm_api_key_blocking(&self) -> Result<Option<String>>;
    fn set_llm_api_key_blocking(&self, api_key: &str) -> Result<()>;
}

pub trait GlobalHotkey: Send + Sync {
    fn is_ready(&self) -> bool;
}

pub trait ActiveAppContextProvider: Send + Sync {
    fn detect_active_app_blocking(&self) -> Option<ActiveAppContext>;
}

pub trait ServiceManager: Send + Sync {
    fn install_autostart(&self) -> Result<String>;
}

pub trait OverlayPresenter: Send + Sync {
    fn push_status(&self, status: DaemonStatus);
}
