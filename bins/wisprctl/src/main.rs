use std::env;

use clap::{Parser, Subcommand};
use wispr_core::{
    AppConfig, CommandMode, CorrectionScope, DictationProxy, FormattingTriggerPolicy,
    LlmInterpreter, PreferredListStyle, Result, SegmentDecisionRequest, TextOutputMode,
    install::{install_uinput_rule, write_default_config, write_user_service},
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
        Command::TestLlm { text } => {
            println!("{}", test_llm(&text).await?);
        }
    }

    Ok(())
}

async fn test_llm(text: &str) -> Result<String> {
    let config = AppConfig::load()?;
    let secret_store = SecretStore::connect().await?;
    let api_key = secret_store.get_llm_api_key().await?.ok_or_else(|| {
        wispr_core::WisprError::InvalidState("no LLM API key is configured".to_string())
    })?;
    let interpreter = LlmInterpreter::new(config.intelligence.clone(), api_key)?;
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
        })
        .await?;

    Ok(format!(
        "decision={} rewrite_scope={:?} format_kind={:?} keep_block_open={} text_to_emit={:?} actions={:?} raw={}",
        output.decision.kind.as_label(),
        output.decision.rewrite_scope,
        output.decision.format_kind,
        output.decision.keep_block_open,
        output.decision.text_to_emit,
        output.decision.actions,
        output.streamed_text
    ))
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
