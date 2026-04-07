use std::{fs, path::Path, process::Command};

use crate::{
    AppConfig,
    error::{Result, WisprError},
};

const GROUP_NAME: &str = "wisprinput";
const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/85-wispr-uinput.rules";
const LAUNCH_AGENT_ID: &str = "io.wispr.wisprd";

pub fn install_uinput_rule(current_user: &str) -> Result<String> {
    require_root()?;

    let group_status = Command::new("getent")
        .arg("group")
        .arg(GROUP_NAME)
        .status()?;
    if !group_status.success() {
        ensure_success(
            Command::new("groupadd").arg(GROUP_NAME).status()?,
            "groupadd",
        )?;
    }

    let rule = format!("KERNEL==\"uinput\", GROUP=\"{GROUP_NAME}\", MODE=\"0660\"\n");
    fs::write(UDEV_RULE_PATH, rule)?;

    ensure_success(
        Command::new("usermod")
            .arg("-aG")
            .arg(GROUP_NAME)
            .arg(current_user)
            .status()?,
        "usermod",
    )?;
    ensure_success(
        Command::new("udevadm")
            .arg("control")
            .arg("--reload-rules")
            .status()?,
        "udevadm reload",
    )?;
    ensure_success(
        Command::new("udevadm")
            .arg("trigger")
            .arg("--name-match=uinput")
            .status()?,
        "udevadm trigger",
    )?;

    Ok(format!(
        "Installed uinput rule at {UDEV_RULE_PATH}. Log out and back in so {current_user} picks up the {GROUP_NAME} group."
    ))
}

pub fn write_user_service(bin_dir: &Path) -> Result<String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = bin_dir;
        return Err(WisprError::InvalidState(
            "systemd user service is only available on Linux".to_string(),
        ));
    }

    #[cfg(target_os = "linux")]
    {
        let systemd_dir = dirs::config_dir()
            .ok_or_else(|| {
                WisprError::InvalidState("could not determine config directory".to_string())
            })?
            .join("systemd/user");
        fs::create_dir_all(&systemd_dir)?;

        let service_body = format!(
            "[Unit]\nDescription=Wispr Dictation Daemon\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart={}/wisprd\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
            bin_dir.display()
        );
        let path = systemd_dir.join("wisprd.service");
        fs::write(&path, service_body)?;
        Ok(format!(
            "Wrote {}. Run `systemctl --user daemon-reload && systemctl --user enable --now wisprd.service`.",
            path.display()
        ))
    }
}

pub fn write_launch_agent(bin_dir: &Path) -> Result<String> {
    let home = dirs::home_dir().ok_or_else(|| {
        WisprError::InvalidState("could not determine home directory".to_string())
    })?;
    let launch_agents_dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&launch_agents_dir)?;

    let plist_path = launch_agents_dir.join(format!("{LAUNCH_AGENT_ID}.plist"));
    let plist_body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCH_AGENT_ID}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
"#,
        bin_dir.join("wisprd").display()
    );
    fs::write(&plist_path, plist_body)?;

    let _ = Command::new("launchctl")
        .args(["unload", plist_path.to_string_lossy().as_ref()])
        .status();
    let load_status = Command::new("launchctl")
        .args(["load", plist_path.to_string_lossy().as_ref()])
        .status()?;
    ensure_success(load_status, "launchctl load")?;

    Ok(format!(
        "Installed and loaded {}.",
        plist_path.display()
    ))
}

pub fn remove_launch_agent() -> Result<String> {
    let home = dirs::home_dir().ok_or_else(|| {
        WisprError::InvalidState("could not determine home directory".to_string())
    })?;
    let plist_path = home
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCH_AGENT_ID}.plist"));

    if plist_path.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", plist_path.to_string_lossy().as_ref()])
            .status();
        fs::remove_file(&plist_path)?;
        return Ok(format!("Removed {}.", plist_path.display()));
    }

    Ok(format!("No launch agent was installed at {}.", plist_path.display()))
}

pub fn write_autostart(bin_dir: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        write_launch_agent(bin_dir)
    }

    #[cfg(not(target_os = "macos"))]
    {
        write_user_service(bin_dir)
    }
}

pub fn write_default_config() -> Result<String> {
    let config = AppConfig::load().or_else(|_| {
        let config = AppConfig::default();
        config.save()?;
        Result::<AppConfig>::Ok(config)
    })?;
    config.save()?;
    Ok(format!(
        "Wrote default config under {}.",
        AppConfig::config_path()?.display()
    ))
}

fn require_root() -> Result<()> {
    if !nix_like_is_root() {
        return Err(WisprError::InvalidState(
            "setup-uinput must be run as root (for example with sudo)".to_string(),
        ));
    }
    Ok(())
}

fn nix_like_is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|uid| uid.trim() == "0")
        .unwrap_or(false)
}

fn ensure_success(status: std::process::ExitStatus, command: &str) -> Result<()> {
    if status.success() {
        return Ok(());
    }

    Err(WisprError::Message(format!(
        "{command} failed with status {status}"
    )))
}
