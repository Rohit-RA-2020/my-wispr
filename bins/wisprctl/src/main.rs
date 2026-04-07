#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;
use std::{env, fs};

use clap::{Parser, Subcommand};
use wispr_core::{
    ActiveAppClass, ActiveAppContext, AppConfig, CommandMode, CorrectionScope, DictationProxy,
    FormattingTriggerPolicy, LlmInterpreter, PreferredListStyle, Result, SegmentDecisionRequest,
    TextOutputMode, TranscriptionProvider,
    install::{install_uinput_rule, remove_launch_agent, write_autostart, write_default_config},
    resolve_actions,
    secrets::SecretStore,
};

#[derive(Parser)]
#[command(name = "wisprctl")]
#[command(about = "Manage the Wispr dictation daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Toggle,
    Start,
    Stop,
    Status,
    OpenSettings,
    SetupUinput {
        #[arg(long, env = "SUDO_USER")]
        user: Option<String>,
    },
    InstallAutostart,
    RemoveAutostart,
    WriteDefaultConfig,
    ShowConfig,
    SettingsJson,
    SetDeepgramKey {
        key: String,
    },
    SetLlmKey {
        key: String,
    },
    SetLlmBaseUrl {
        base_url: String,
    },
    SetLlmModel {
        model: String,
    },
    SetProvider {
        provider: String,
    },
    SetWhisperModel {
        model: String,
    },
    WhisperStatus,
    InstallWhisperRuntime,
    DownloadWhisperModel {
        model: String,
    },
    DeleteWhisperModel {
        model: String,
    },
    TestWhisperModel {
        model: String,
    },
    TestLlm {
        #[arg(default_value = "hello enter")]
        text: String,
        #[arg(long)]
        app_class: Option<String>,
        #[arg(long)]
        app_id: Option<String>,
    },
    Doctor,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Toggle => println!("{}", DictationProxy::new().await?.toggle().await?),
        Command::Start => println!("{}", DictationProxy::new().await?.start().await?),
        Command::Stop => println!("{}", DictationProxy::new().await?.stop().await?),
        Command::Status => println!("{}", DictationProxy::new().await?.status().await?),
        Command::OpenSettings => {
            println!("{}", DictationProxy::new().await?.open_settings().await?)
        }
        Command::SetupUinput { user } => {
            let user = user.or_else(|| env::var("USER").ok()).ok_or_else(|| {
                wispr_core::WisprError::InvalidState("could not determine target user".to_string())
            })?;
            println!("{}", install_uinput_rule(&user)?);
        }
        Command::InstallAutostart => {
            let bin_dir = resolve_bin_dir_for_autostart()?;
            println!("{}", write_autostart(&bin_dir)?);
        }
        Command::RemoveAutostart => {
            println!("{}", remove_launch_agent()?);
        }
        Command::WriteDefaultConfig => {
            println!("{}", write_default_config()?);
        }
        Command::ShowConfig => {
            let config = AppConfig::load()?;
            println!("{}", toml::to_string_pretty(&config)?);
        }
        Command::SettingsJson => {
            println!("{}", settings_json().await?);
        }
        Command::SetDeepgramKey { key } => {
            let store = SecretStore::connect().await?;
            store.set_api_key(key.trim()).await?;
            println!("Saved Deepgram API key.");
        }
        Command::SetLlmKey { key } => {
            let store = SecretStore::connect().await?;
            store.set_llm_api_key(key.trim()).await?;
            println!("Saved LLM API key.");
        }
        Command::SetLlmBaseUrl { base_url } => {
            let mut config = AppConfig::load()?;
            config.intelligence.base_url = base_url.trim().to_string();
            config.save()?;
            println!("Updated intelligence.base_url.");
        }
        Command::SetLlmModel { model } => {
            let mut config = AppConfig::load()?;
            config.intelligence.model = model.trim().to_string();
            config.save()?;
            println!("Updated intelligence.model.");
        }
        Command::SetProvider { provider } => {
            let normalized = provider.trim().to_ascii_lowercase();
            let provider = match normalized.as_str() {
                "deepgram" | "cloud" => TranscriptionProvider::Deepgram,
                "whisper_local" | "whisper" | "local" => TranscriptionProvider::WhisperLocal,
                other => {
                    return Err(wispr_core::WisprError::InvalidState(format!(
                        "unsupported provider: {other}. Use deepgram/cloud or whisper_local/local."
                    )));
                }
            };
            let mut config = AppConfig::load()?;
            config.transcription.provider = provider;
            config.save()?;
            println!("Updated transcription provider.");
        }
        Command::SetWhisperModel { model } => {
            let mut config = AppConfig::load()?;
            config.transcription.whisper_local.model = model.trim().to_string();
            config.save()?;
            println!("Updated whisper local model.");
        }
        Command::WhisperStatus => {
            let config = AppConfig::load()?;
            let status =
                wispr_core::whisper::collect_manager_status(&config.transcription.whisper_local);
            let runtime = status.runtime;
            println!("python_ready={}", runtime.python_ready);
            println!("whisper_ready={}", runtime.whisper_ready);
            println!("ffmpeg_ready={}", runtime.ffmpeg_ready);
            println!("available_models={}", runtime.available_models.join(","));
            println!("installed_models={}", status.installed_models.join(","));
            if let Some(detail) = runtime.detail {
                println!("detail={detail}");
            }
        }
        Command::InstallWhisperRuntime => {
            println!("{}", wispr_core::whisper::install_runtime()?);
        }
        Command::DownloadWhisperModel { model } => {
            let config = AppConfig::load()?;
            println!(
                "{}",
                wispr_core::whisper::download_model(
                    &config.transcription.whisper_local,
                    model.trim()
                )?
            );
        }
        Command::DeleteWhisperModel { model } => {
            let config = AppConfig::load()?;
            println!(
                "{}",
                wispr_core::whisper::delete_model(
                    &config.transcription.whisper_local,
                    model.trim()
                )?
            );
        }
        Command::TestWhisperModel { model } => {
            let config = AppConfig::load()?;
            println!(
                "{}",
                wispr_core::whisper::test_model_load(
                    &config.transcription.whisper_local,
                    model.trim()
                )?
            );
        }
        Command::TestLlm {
            text,
            app_class,
            app_id,
        } => {
            println!(
                "{}",
                test_llm(&text, app_class.as_deref(), app_id.as_deref()).await?
            );
        }
        Command::Doctor => {
            println!("{}", doctor().await);
        }
    }

    Ok(())
}

async fn doctor() -> String {
    let mut lines = Vec::new();

    match wispr_core::daemon_socket_path() {
        Ok(path) => {
            lines.push(format!("socket_path={}", path.display()));
            lines.push(format!("socket_exists={}", path.exists()));
            if path.exists() {
                let kind = fs::metadata(&path)
                    .map(|m| {
                        if m.file_type().is_socket() {
                            "socket"
                        } else {
                            "non-socket"
                        }
                    })
                    .unwrap_or("unknown");
                lines.push(format!("socket_file_type={kind}"));
            }
        }
        Err(error) => lines.push(format!("socket_path_error={error}")),
    }

    match DictationProxy::new().await {
        Ok(proxy) => match proxy.status().await {
            Ok(_) => lines.push("daemon_status_call=ok".to_string()),
            Err(error) => lines.push(format!("daemon_status_call=err({error})")),
        },
        Err(error) => lines.push(format!("daemon_proxy_error={error}")),
    }

    lines.join("\n")
}

async fn settings_json() -> Result<String> {
    let config = AppConfig::load()?;
    let store = SecretStore::connect().await?;
    let deepgram_key_configured = store
        .get_api_key()
        .await?
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let llm_key_configured = store
        .get_llm_api_key()
        .await?
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    let provider = match config.transcription.provider {
        TranscriptionProvider::Deepgram => "deepgram",
        TranscriptionProvider::WhisperLocal => "whisper_local",
    };

    Ok(serde_json::json!({
        "provider": provider,
        "whisper_model": config.transcription.whisper_local.model,
        "llm_base_url": config.intelligence.base_url,
        "llm_model": config.intelligence.model,
        "intelligence_enabled": config.intelligence.enabled,
        "deepgram_key_configured": deepgram_key_configured,
        "llm_key_configured": llm_key_configured
    })
    .to_string())
}

async fn test_llm(text: &str, app_class: Option<&str>, app_id: Option<&str>) -> Result<String> {
    let config = AppConfig::load()?;
    let secret_store = SecretStore::connect().await?;
    let api_key = secret_store.get_llm_api_key().await?.ok_or_else(|| {
        wispr_core::WisprError::InvalidState("no LLM API key is configured".to_string())
    })?;
    let interpreter = LlmInterpreter::new(config.intelligence.clone(), api_key)?;
    let active_app = build_active_app(app_class, app_id)?;
    let output = interpreter
        .decide(&SegmentDecisionRequest {
            segment_id: "wisprctl-test".to_string(),
            finalized_text: text.to_string(),
            literal_text: text.to_string(),
            recent_text: String::new(),
            active_block_raw: String::new(),
            active_block_rendered: String::new(),
            action_scope: config.intelligence.action_scope.clone(),
            command_mode: CommandMode::AlwaysInfer,
            text_output_mode: TextOutputMode::Literal,
            preferred_list_style: PreferredListStyle::Numbered,
            formatting_trigger_policy: FormattingTriggerPolicy::ClearStructureOnly,
            correction_scope: CorrectionScope::CurrentBlockOnly,
            active_app: active_app.clone(),
        })
        .await?;
    let resolved = resolve_actions(
        &config.intelligence,
        &output.decision.actions,
        active_app.as_ref(),
    );

    Ok(format!(
        "decision={} rewrite_scope={:?} format_kind={:?} keep_block_open={} text_to_emit={:?} generation_prompt={:?} generation_style={:?} replace_current_segment={} actions={:?} resolved={} raw={}",
        output.decision.kind.as_label(),
        output.decision.rewrite_scope,
        output.decision.format_kind,
        output.decision.keep_block_open,
        output.decision.text_to_emit,
        output.decision.generation_prompt,
        output.decision.generation_style,
        output.decision.replace_current_segment,
        output.decision.actions,
        match resolved {
            Ok(actions) => format!("{:?} ({:?})", actions.actions, actions.description),
            Err(error) => format!("ERR({error})"),
        },
        output.streamed_text
    ))
}

fn build_active_app(
    app_class: Option<&str>,
    app_id: Option<&str>,
) -> Result<Option<ActiveAppContext>> {
    let Some(app_class) = app_class else {
        return Ok(None);
    };

    let app_class = match app_class.trim().to_ascii_lowercase().as_str() {
        "browser" => ActiveAppClass::Browser,
        "editor" => ActiveAppClass::Editor,
        "terminal" => ActiveAppClass::Terminal,
        "generic" => ActiveAppClass::Generic,
        other => {
            return Err(wispr_core::WisprError::InvalidState(format!(
                "unsupported app class: {other}"
            )));
        }
    };

    Ok(Some(ActiveAppContext {
        app_class,
        app_id: app_id.map(ToString::to_string),
    }))
}

fn resolve_bin_dir_for_autostart() -> Result<std::path::PathBuf> {
    if let Ok(explicit) = env::var("WISPRD_PATH") {
        let path = std::path::PathBuf::from(explicit);
        if path.exists() {
            return path.parent().map(|parent| parent.to_path_buf()).ok_or_else(|| {
                wispr_core::WisprError::InvalidState(
                    "WISPRD_PATH does not have a parent directory".to_string(),
                )
            });
        }
    }

    if let Ok(current_exe) = env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        let sibling = parent.join("wisprd");
        if sibling.exists() {
            return Ok(parent.to_path_buf());
        }
    }

    let cwd = env::current_dir().map_err(|error| {
        wispr_core::WisprError::InvalidState(format!(
            "could not determine current working directory: {error}"
        ))
    })?;

    for candidate in [
        cwd.join("target/debug"),
        cwd.join("../target/debug"),
        cwd.join("../../target/debug"),
    ] {
        if candidate.join("wisprd").exists() {
            return Ok(candidate);
        }
    }

    Err(wispr_core::WisprError::InvalidState(
        "could not locate wisprd for autostart installation".to_string(),
    ))
}
