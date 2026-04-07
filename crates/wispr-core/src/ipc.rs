use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use crate::{
    AppConfig,
    error::{Result, WisprError},
};

pub const DICTATION_SERVICE: &str = "io.wispr.Dictation";
pub const DICTATION_OBJECT_PATH: &str = "/io/wispr/Dictation";
pub const DICTATION_INTERFACE: &str = "io.wispr.Dictation1";

pub fn daemon_socket_path() -> Result<PathBuf> {
    Ok(AppConfig::config_dir()?.join("wisprd.sock"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationCommand {
    Toggle,
    Start,
    Stop,
    Status,
    OpenSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationIpcRequest {
    pub command: DictationCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationIpcResponse {
    pub ok: bool,
    pub message: String,
}

pub struct DictationProxy;

impl DictationProxy {
    pub async fn new() -> Result<Self> {
        Ok(Self)
    }

    pub async fn toggle(&self) -> Result<String> {
        send_command(DictationCommand::Toggle).await
    }

    pub async fn start(&self) -> Result<String> {
        send_command(DictationCommand::Start).await
    }

    pub async fn stop(&self) -> Result<String> {
        send_command(DictationCommand::Stop).await
    }

    pub async fn status(&self) -> Result<String> {
        send_command(DictationCommand::Status).await
    }

    pub async fn open_settings(&self) -> Result<String> {
        send_command(DictationCommand::OpenSettings).await
    }
}

async fn send_command(command: DictationCommand) -> Result<String> {
    let socket_path = daemon_socket_path()?;
    let mut stream = UnixStream::connect(&socket_path).await.map_err(|error| {
        WisprError::InvalidState(format!(
            "failed to connect to daemon socket {}: {error}",
            socket_path.display()
        ))
    })?;

    let request = DictationIpcRequest { command };
    let payload = serde_json::to_vec(&request)?;
    stream.write_all(&payload).await?;
    stream.shutdown().await?;

    let mut response_bytes = Vec::new();
    stream.read_to_end(&mut response_bytes).await?;
    if response_bytes.is_empty() {
        return Err(WisprError::InvalidState(
            "daemon returned an empty IPC response".to_string(),
        ));
    }

    let response: DictationIpcResponse = serde_json::from_slice(&response_bytes)?;
    if response.ok {
        Ok(response.message)
    } else {
        Err(WisprError::InvalidState(response.message))
    }
}
