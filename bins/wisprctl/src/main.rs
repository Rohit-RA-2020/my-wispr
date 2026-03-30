use std::env;

use clap::{Parser, Subcommand};
use wispr_core::{
    ActiveAppClass, ActiveAppContext, AppConfig, CommandMode, CorrectionScope, DictationProxy,
    FormattingTriggerPolicy, LlmInterpreter, PreferredListStyle, Result, SegmentDecisionRequest,
    TextOutputMode,
    install::{install_uinput_rule, write_default_config, write_user_service},
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
    WriteDefaultConfig,
    TestLlm {
        #[arg(default_value = "hello enter")]
        text: String,
        #[arg(long)]
        app_class: Option<String>,
        #[arg(long)]
        app_id: Option<String>,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Toggle => println!("{}", daemon_call(|proxy| Box::pin(proxy.toggle())).await?),
        Command::Start => println!("{}", daemon_call(|proxy| Box::pin(proxy.start())).await?),
        Command::Stop => println!("{}", daemon_call(|proxy| Box::pin(proxy.stop())).await?),
        Command::Status => println!("{}", daemon_call(|proxy| Box::pin(proxy.status())).await?),
        Command::OpenSettings => println!(
            "{}",
            daemon_call(|proxy| Box::pin(proxy.open_settings())).await?
        ),
        Command::SetupUinput { user } => {
            let user = user.or_else(|| env::var("USER").ok()).ok_or_else(|| {
                wispr_core::WisprError::InvalidState("could not determine target user".to_string())
            })?;
            println!("{}", install_uinput_rule(&user)?);
        }
        Command::InstallAutostart => {
            let home = env::var("HOME").map_err(|_| {
                wispr_core::WisprError::InvalidState(
                    "could not determine home directory".to_string(),
                )
            })?;
            let bin_dir = std::path::PathBuf::from(home).join(".local/bin");
            println!("{}", write_user_service(&bin_dir)?);
        }
        Command::WriteDefaultConfig => {
            println!("{}", write_default_config()?);
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
    }

    Ok(())
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
        "decision={} rewrite_scope={:?} format_kind={:?} keep_block_open={} text_to_emit={:?} actions={:?} resolved={} raw={}",
        output.decision.kind.as_label(),
        output.decision.rewrite_scope,
        output.decision.format_kind,
        output.decision.keep_block_open,
        output.decision.text_to_emit,
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

async fn daemon_call<F>(call: F) -> Result<String>
where
    F: for<'a> FnOnce(
        &'a DictationProxy<'a>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = zbus::Result<String>> + 'a>,
    >,
{
    let connection = zbus::Connection::session().await?;
    let proxy = DictationProxy::new(&connection).await?;
    Ok(call(&proxy).await?)
}
