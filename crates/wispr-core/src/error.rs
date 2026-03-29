use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WisprError {
    #[error("{0}")]
    Message(String),
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    #[error("toml deserialize error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("serde json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("secret-service error: {0}")]
    SecretService(String),
    #[error("dbus error: {0}")]
    Dbus(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
}

impl From<zbus::Error> for WisprError {
    fn from(value: zbus::Error) -> Self {
        Self::Dbus(value.to_string())
    }
}

impl From<secret_service::Error> for WisprError {
    fn from(value: secret_service::Error) -> Self {
        Self::SecretService(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, WisprError>;
