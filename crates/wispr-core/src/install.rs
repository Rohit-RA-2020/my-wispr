use std::{fs, path::Path, process::Command};

use crate::{
    AppConfig,
    error::{Result, WisprError},
};

const GROUP_NAME: &str = "wisprinput";
const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/85-wispr-uinput.rules";

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

pub fn write_default_config() -> Result<String> {
    let config = AppConfig::load().or_else(|_| {
        let config = AppConfig::default();
        config.save()?;
        Result::<AppConfig>::Ok(config)
    })?;
    config.save()?;
    Ok("Wrote default config under ~/.config/wispr/config.toml".to_string())
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
