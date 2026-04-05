use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

use crate::{
    Result, WisprError,
    models::{TranscriptionProvider, WhisperLocalConfig},
};

const CORE_ENGLISH_MODELS: &[&str] = &["tiny.en", "base.en", "small.en", "medium.en"];
const OPTIONAL_MODELS: &[&str] = &["turbo", "large"];
const VIRTUALENV_BOOTSTRAP_URL: &str = "https://bootstrap.pypa.io/virtualenv.pyz";

#[derive(Debug, Clone, Default)]
pub struct WhisperRuntimeStatus {
    pub python_ready: bool,
    pub whisper_ready: bool,
    pub ffmpeg_ready: bool,
    pub available_models: Vec<String>,
    pub detail: Option<String>,
}

impl WhisperRuntimeStatus {
    pub fn backend_ready(&self) -> bool {
        self.python_ready && self.whisper_ready && self.ffmpeg_ready
    }
}

#[derive(Debug, Clone, Default)]
pub struct WhisperManagerStatus {
    pub runtime: WhisperRuntimeStatus,
    pub curated_models: Vec<String>,
    pub installed_models: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WhisperJsonOutput {
    #[serde(default)]
    text: String,
}

pub fn default_model_dir() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("wispr/whisper")
}

pub fn default_venv_dir() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("wispr/whisper-venv")
}

fn bootstrap_dir() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("wispr/bootstrap")
}

fn virtualenv_bootstrap_path() -> PathBuf {
    bootstrap_dir().join("virtualenv.pyz")
}

pub fn venv_python_path() -> PathBuf {
    default_venv_dir().join("bin/python")
}

pub fn venv_pip_path() -> PathBuf {
    default_venv_dir().join("bin/pip")
}

pub fn ensure_model_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

pub fn install_runtime() -> Result<String> {
    let venv_dir = default_venv_dir();
    if let Some(parent) = venv_dir.parent() {
        fs::create_dir_all(parent)?;
    }

    if venv_dir.exists() && (!venv_python_path().exists() || !venv_pip_ready()) {
        fs::remove_dir_all(&venv_dir)?;
    }

    let creation_method = if venv_python_path().exists() {
        "existing virtualenv".to_string()
    } else {
        create_virtualenv(&venv_dir)?
    };

    run_install_command(
        Command::new(venv_python_path()).args(["-m", "pip", "install", "-U", "pip", "wheel"]),
        "Failed to prepare pip inside the Wispr Whisper virtualenv.",
    )?;
    run_install_command(
        Command::new(venv_python_path()).args(["-m", "pip", "install", "-U", "openai-whisper"]),
        "Failed to install openai-whisper into the Wispr Whisper virtualenv.",
    )?;

    Ok(format!(
        "Installed Wispr Whisper runtime in {} using {}.",
        venv_dir.display(),
        creation_method
    ))
}

pub fn collect_manager_status(config: &WhisperLocalConfig) -> WhisperManagerStatus {
    let runtime = collect_runtime_status();
    WhisperManagerStatus {
        curated_models: curated_models(&runtime),
        installed_models: list_installed_models(config),
        runtime,
    }
}

pub fn curated_models(runtime: &WhisperRuntimeStatus) -> Vec<String> {
    let available = runtime
        .available_models
        .iter()
        .map(|model| model.as_str())
        .collect::<HashSet<_>>();

    let mut models = CORE_ENGLISH_MODELS
        .iter()
        .map(|model| (*model).to_string())
        .collect::<Vec<_>>();

    for model in OPTIONAL_MODELS {
        if available.contains(model) {
            models.push((*model).to_string());
        }
    }

    models
}

pub fn collect_runtime_status() -> WhisperRuntimeStatus {
    let python_path = venv_python_path();
    let python_ready = python_path.exists();
    let ffmpeg_ready = command_ready("ffmpeg", &["-version"]);

    if !python_ready {
        return WhisperRuntimeStatus {
            python_ready,
            whisper_ready: false,
            ffmpeg_ready,
            available_models: Vec::new(),
            detail: Some(format!(
                "Wispr Whisper virtualenv is missing. Create it with `python3 -m venv {}` and install openai-whisper inside it.",
                default_venv_dir().display()
            )),
        };
    }

    match python3_json(
        r#"
import json
import whisper

print(json.dumps({"models": whisper.available_models()}))
"#,
    ) {
        Ok(value) => {
            let available_models = value
                .get("models")
                .and_then(|models| models.as_array())
                .map(|models| {
                    models
                        .iter()
                        .filter_map(|model| model.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            WhisperRuntimeStatus {
                python_ready,
                whisper_ready: true,
                ffmpeg_ready,
                available_models,
                detail: None,
            }
        }
        Err(error) => WhisperRuntimeStatus {
            python_ready,
            whisper_ready: command_ready("whisper", &["--help"]),
            ffmpeg_ready,
            available_models: Vec::new(),
            detail: Some(friendly_runtime_error(&error.to_string())),
        },
    }
}

pub fn ensure_backend_ready(config: &WhisperLocalConfig) -> Result<()> {
    let status = collect_runtime_status();
    if !status.python_ready {
        return Err(WisprError::InvalidState(format!(
            "Wispr Whisper virtualenv is not installed. Create {} and install openai-whisper into it.",
            default_venv_dir().display()
        )));
    }
    if !status.ffmpeg_ready {
        return Err(WisprError::InvalidState(
            "ffmpeg is not available. Install ffmpeg to use local Whisper transcription."
                .to_string(),
        ));
    }
    if !status.whisper_ready {
        return Err(WisprError::InvalidState(
            "Whisper is not installed in the Wispr Whisper virtualenv. Install openai-whisper to use local transcription."
                .to_string(),
        ));
    }
    if !is_model_installed(config, &config.model) {
        return Err(WisprError::InvalidState(format!(
            "Whisper model {} is not installed in {}.",
            config.model,
            config.model_dir.display()
        )));
    }
    Ok(())
}

pub fn list_installed_models(config: &WhisperLocalConfig) -> Vec<String> {
    curated_models(&collect_runtime_status())
        .into_iter()
        .filter(|model| is_model_installed(config, model))
        .collect()
}

pub fn is_model_installed(config: &WhisperLocalConfig, model: &str) -> bool {
    expected_model_path(&config.model_dir, model)
        .map(|path| path.exists())
        .unwrap_or(false)
}

pub fn download_model(config: &WhisperLocalConfig, model: &str) -> Result<String> {
    ensure_model_dir(&config.model_dir)?;
    python3_check(
        r#"
import sys
import whisper

model = sys.argv[1]
model_dir = sys.argv[2]
whisper.load_model(model, download_root=model_dir)
"#,
        &[model, &config.model_dir.to_string_lossy()],
    )?;
    Ok(format!(
        "Downloaded Whisper model {} into {}.",
        model,
        config.model_dir.display()
    ))
}

pub fn delete_model(config: &WhisperLocalConfig, model: &str) -> Result<String> {
    let path = expected_model_path(&config.model_dir, model)
        .ok_or_else(|| WisprError::InvalidState(format!("unsupported Whisper model: {model}")))?;
    if !path.exists() {
        return Err(WisprError::InvalidState(format!(
            "Whisper model {} is not installed in {}.",
            model,
            config.model_dir.display()
        )));
    }
    fs::remove_file(&path)?;
    Ok(format!("Deleted Whisper model {}.", model))
}

pub fn test_model_load(config: &WhisperLocalConfig, model: &str) -> Result<String> {
    ensure_model_dir(&config.model_dir)?;
    python3_check(
        r#"
import os
import sys
import urllib.parse
import whisper

model = sys.argv[1]
model_dir = sys.argv[2]
model_url = whisper._MODELS[model]
model_path = os.path.join(model_dir, os.path.basename(urllib.parse.urlparse(model_url).path))
if not os.path.isfile(model_path):
    raise RuntimeError(f"model {model} is not installed")
whisper.load_model(model, download_root=model_dir)
"#,
        &[model, &config.model_dir.to_string_lossy()],
    )?;
    Ok(format!("Whisper model {} loaded successfully.", model))
}

pub fn transcribe_wav(config: &WhisperLocalConfig, wav_path: &Path) -> Result<String> {
    ensure_model_dir(&config.model_dir)?;
    let temp_root = std::env::temp_dir().join(format!("wispr-whisper-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_root)?;
    let output = run_whisper_cli(config, wav_path, &temp_root);
    let result = output.and_then(|()| {
        let json_path = temp_root.join(format!(
            "{}.json",
            wav_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("transcript")
        ));
        let text = fs::read_to_string(&json_path)?;
        let parsed = serde_json::from_str::<WhisperJsonOutput>(&text)?;
        Ok(parsed.text.trim().to_string())
    });
    let _ = fs::remove_dir_all(&temp_root);
    result
}

pub fn provider_label(provider: &TranscriptionProvider) -> &'static str {
    match provider {
        TranscriptionProvider::Deepgram => "Cloud (Deepgram)",
        TranscriptionProvider::WhisperLocal => "Local (Whisper)",
    }
}

fn run_whisper_cli(config: &WhisperLocalConfig, wav_path: &Path, output_dir: &Path) -> Result<()> {
    let args = vec![
        wav_path.to_string_lossy().to_string(),
        "--model".to_string(),
        config.model.clone(),
        "--language".to_string(),
        "en".to_string(),
        "--task".to_string(),
        "transcribe".to_string(),
        "--verbose".to_string(),
        "False".to_string(),
        "--output_format".to_string(),
        "json".to_string(),
        "--output_dir".to_string(),
        output_dir.to_string_lossy().to_string(),
        "--model_dir".to_string(),
        config.model_dir.to_string_lossy().to_string(),
        "--fp16".to_string(),
        "False".to_string(),
    ];

    let python_status = Command::new(venv_python_path())
        .args(["-m", "whisper"])
        .args(&args)
        .output();
    if let Ok(output) = python_status {
        if output.status.success() {
            return Ok(());
        }
    }

    let output = Command::new("whisper")
        .args(&args)
        .output()
        .map_err(|error| {
            WisprError::InvalidState(format!("failed to execute Whisper CLI: {error}"))
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WisprError::InvalidState(format!(
            "Whisper CLI failed: {}",
            stderr_text(&output.stderr)
        )))
    }
}

fn command_ready(command: &str, args: &[&str]) -> bool {
    Command::new(command)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn venv_pip_ready() -> bool {
    Command::new(venv_python_path())
        .args(["-m", "pip", "--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn create_virtualenv(venv_dir: &Path) -> Result<String> {
    match run_install_command(
        Command::new("python3").args(["-m", "venv"]).arg(venv_dir),
        "Failed to create the Wispr Whisper virtualenv.",
    ) {
        Ok(()) => Ok("python3 -m venv".to_string()),
        Err(venv_error) => {
            if venv_dir.exists() {
                let _ = fs::remove_dir_all(venv_dir);
            }
            download_virtualenv_bootstrap()?;
            run_install_command(
                Command::new("python3")
                    .arg(virtualenv_bootstrap_path())
                    .arg(venv_dir),
                "Failed to create the Wispr Whisper virtualenv with the PyPA virtualenv bootstrap.",
            )
            .map(|()| "PyPA virtualenv bootstrap".to_string())
            .map_err(|bootstrap_error| {
                WisprError::InvalidState(format!(
                    "{venv_error} Fallback bootstrap also failed: {bootstrap_error}"
                ))
            })
        }
    }
}

fn download_virtualenv_bootstrap() -> Result<()> {
    let bootstrap_path = virtualenv_bootstrap_path();
    if bootstrap_path.exists() {
        return Ok(());
    }

    if let Some(parent) = bootstrap_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if let Ok(output) = Command::new("curl")
        .args(["-fsSL", VIRTUALENV_BOOTSTRAP_URL, "-o"])
        .arg(&bootstrap_path)
        .output()
    {
        if output.status.success() {
            return Ok(());
        }
    }

    let wget_output = Command::new("wget")
        .args(["-q", "-O"])
        .arg(&bootstrap_path)
        .arg(VIRTUALENV_BOOTSTRAP_URL)
        .output()
        .map_err(|error| {
            WisprError::InvalidState(format!(
                "Failed to download the PyPA virtualenv bootstrap with curl and wget: {error}"
            ))
        })?;
    if wget_output.status.success() {
        Ok(())
    } else {
        Err(WisprError::InvalidState(format!(
            "Failed to download the PyPA virtualenv bootstrap. {}",
            install_command_error(&wget_output)
        )))
    }
}

fn run_install_command(command: &mut Command, context: &str) -> Result<()> {
    let output = command
        .output()
        .map_err(|error| WisprError::InvalidState(format!("{context} {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WisprError::InvalidState(format!(
            "{context} {}",
            install_command_error(&output)
        )))
    }
}

fn python3_json(script: &str) -> Result<serde_json::Value> {
    let output = Command::new(venv_python_path())
        .args(["-c", script])
        .output()
        .map_err(|error| {
            WisprError::InvalidState(format!(
                "failed to execute Wispr Whisper virtualenv python: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(WisprError::InvalidState(format!(
            "Wispr Whisper probe failed: {}",
            stderr_text(&output.stderr)
        )));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn python3_check(script: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(venv_python_path())
        .arg("-c")
        .arg(script)
        .args(args)
        .output()
        .map_err(|error| {
            WisprError::InvalidState(format!(
                "failed to execute Wispr Whisper virtualenv python: {error}"
            ))
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WisprError::InvalidState(stderr_text(&output.stderr)))
    }
}

fn stderr_text(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr).trim().to_string();
    if text.is_empty() {
        "command failed without stderr output".to_string()
    } else {
        text
    }
}

fn install_command_error(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    format!("command exited with status {}", output.status)
}

fn friendly_runtime_error(message: &str) -> String {
    if message.contains("No module named 'whisper'") {
        return "openai-whisper is not installed in the Wispr Whisper virtualenv".to_string();
    }
    if message.contains("Wispr Whisper probe failed") {
        return "Whisper could not be imported from the Wispr Whisper virtualenv".to_string();
    }
    if message.contains("failed to execute Wispr Whisper virtualenv python") {
        return "The Wispr Whisper virtualenv python could not be executed".to_string();
    }
    message.lines().next().unwrap_or(message).trim().to_string()
}

fn expected_model_path(model_dir: &Path, model: &str) -> Option<PathBuf> {
    let filename = match model {
        "tiny.en" => "tiny.en.pt",
        "base.en" => "base.en.pt",
        "small.en" => "small.en.pt",
        "medium.en" => "medium.en.pt",
        "turbo" => "large-v3-turbo.pt",
        "large" => "large-v3.pt",
        _ => return None,
    };
    Some(model_dir.join(filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_models_start_with_english_defaults() {
        let runtime = WhisperRuntimeStatus::default();
        assert_eq!(
            curated_models(&runtime),
            vec![
                "tiny.en".to_string(),
                "base.en".to_string(),
                "small.en".to_string(),
                "medium.en".to_string()
            ]
        );
    }

    #[test]
    fn expected_model_path_matches_curated_aliases() {
        let dir = PathBuf::from("/tmp/models");
        assert_eq!(
            expected_model_path(&dir, "large").unwrap(),
            PathBuf::from("/tmp/models/large-v3.pt")
        );
        assert_eq!(
            expected_model_path(&dir, "turbo").unwrap(),
            PathBuf::from("/tmp/models/large-v3-turbo.pt")
        );
    }

    #[test]
    fn parses_whisper_json_output() {
        let parsed = serde_json::from_str::<WhisperJsonOutput>(r#"{"text":"hello world"}"#)
            .expect("parse whisper json");
        assert_eq!(parsed.text, "hello world");
    }
}
