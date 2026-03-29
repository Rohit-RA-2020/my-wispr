use std::env;

use clap::{Parser, Subcommand};
use wispr_core::{
    DictationProxy, Result,
    install::{install_uinput_rule, write_default_config, write_user_service},
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
    }

    Ok(())
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
