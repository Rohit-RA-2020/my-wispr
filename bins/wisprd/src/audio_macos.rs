use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
};

use async_channel::Receiver;
use wispr_core::{
    AppConfig, DeviceChoice,
    error::{Result, WisprError},
};

pub struct AudioCapture {
    child: Mutex<Option<Child>>,
    receiver: Receiver<Vec<u8>>,
}

impl AudioCapture {
    pub fn start(selected: &DeviceChoice) -> Result<Self> {
        enumerate_devices()?
            .into_iter()
            .find(|device| device.node_name == selected.node_name)
            .ok_or_else(|| {
                WisprError::InvalidState(format!(
                    "selected microphone {} is not currently available",
                    selected.display_name
                ))
            })
            .map(|_| ())?;

        let mut command = ffmpeg_command()?;
        let mut child = command
            .args([
                "-f",
                "avfoundation",
                "-i",
                &format!(":{}", selected.node_name),
                "-ac",
                "1",
                "-ar",
                "16000",
                "-f",
                "s16le",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                WisprError::InvalidState(format!(
                    "failed to start ffmpeg avfoundation capture: {error}"
                ))
            })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            WisprError::InvalidState("ffmpeg did not expose a readable stdout stream".to_string())
        })?;

        let (sender, receiver) = async_channel::bounded::<Vec<u8>>(64);
        thread::spawn(move || {
            let mut reader = stdout;
            let mut buffer = vec![0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => {
                        if sender.send_blocking(buffer[..size].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            child: Mutex::new(Some(child)),
            receiver,
        })
    }

    pub fn receiver(&self) -> Receiver<Vec<u8>> {
        self.receiver.clone()
    }

    pub fn stop(&self) -> Result<()> {
        if let Some(mut child) = self.child.lock().expect("poisoned audio child").take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub fn enumerate_devices() -> Result<Vec<DeviceChoice>> {
    let output = ffmpeg_command()?
        .args(["-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .output()
        .map_err(|error| {
            WisprError::InvalidState(format!("failed to list audio devices: {error}"))
        })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_avfoundation_devices(&stderr))
}

pub fn resolve_selected_device(config: &AppConfig) -> Option<DeviceChoice> {
    let available = enumerate_devices().ok()?;
    let Some(selected) = config.selected_device.as_ref() else {
        return available.into_iter().next();
    };

    available
        .into_iter()
        .find(|device| device.node_name == selected.node_name)
        .or_else(|| {
            enumerate_devices().ok()?.into_iter().find(|device| {
                device.display_name == selected.display_name
                    || device.fallback_description == selected.fallback_description
            })
        })
        .or_else(|| enumerate_devices().ok()?.into_iter().next())
}

fn parse_avfoundation_devices(stderr: &str) -> Vec<DeviceChoice> {
    let mut in_audio_section = false;
    let mut devices = Vec::new();

    for raw_line in stderr.lines() {
        let line = raw_line.trim();
        if line.contains("AVFoundation audio devices") {
            in_audio_section = true;
            continue;
        }
        if line.contains("AVFoundation video devices") {
            in_audio_section = false;
            continue;
        }
        if !in_audio_section {
            continue;
        }

        let Some((index, name)) = parse_device_line(line) else {
            continue;
        };
        if index.is_empty() || name.is_empty() {
            continue;
        }

        devices.push(DeviceChoice {
            node_name: index.to_string(),
            display_name: name.to_string(),
            fallback_description: name.to_string(),
        });
    }

    devices
}

fn parse_device_line(line: &str) -> Option<(&str, &str)> {
    let prefix_end = line.find(']')?;
    let suffix = line.get(prefix_end + 1..)?.trim();
    let suffix = suffix.strip_prefix('[')?;
    let index_end = suffix.find(']')?;
    let index = suffix.get(..index_end)?.trim();
    let name = suffix.get(index_end + 1..)?.trim();
    Some((index, name))
}

fn ffmpeg_command() -> Result<Command> {
    let executable = resolve_ffmpeg_path().ok_or_else(|| {
        WisprError::InvalidState(
            "ffmpeg is not available. Install ffmpeg or expose it on PATH to use macOS microphone capture."
                .to_string(),
        )
    })?;
    Ok(Command::new(executable))
}

fn resolve_ffmpeg_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("WISPR_FFMPEG_PATH") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Ok(path_env) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path_env) {
            let candidate = entry.join("ffmpeg");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::parse_avfoundation_devices;

    #[test]
    fn parses_avfoundation_audio_devices_after_log_prefix() {
        let stderr = r#"
[AVFoundation indev @ 0x111111111] AVFoundation video devices:
[AVFoundation indev @ 0x111111111] [0] FaceTime HD Camera
[AVFoundation indev @ 0x111111111] AVFoundation audio devices:
[AVFoundation indev @ 0x111111111] [0] MacBook Pro Microphone
[AVFoundation indev @ 0x111111111] [1] External USB Mic
"#;

        let devices = parse_avfoundation_devices(stderr);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].node_name, "0");
        assert_eq!(devices[0].display_name, "MacBook Pro Microphone");
        assert_eq!(devices[1].node_name, "1");
        assert_eq!(devices[1].fallback_description, "External USB Mic");
    }
}
