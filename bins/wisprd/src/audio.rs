use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
};

use async_channel::Receiver;
use gstreamer as gst;
use gstreamer::prelude::*;
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
        gst::init().map_err(|err| WisprError::Message(err.to_string()))?;

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

        let mut child = Command::new("pw-record")
            .args([
                "--target",
                selected.node_name.as_str(),
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
                "--raw",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| {
                WisprError::InvalidState(format!("failed to start pw-record for capture: {err}"))
            })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            WisprError::InvalidState(
                "pw-record did not expose a readable stdout stream".to_string(),
            )
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

pub fn enumerate_devices() -> Result<Vec<DeviceChoice>> {
    gst::init().map_err(|err| WisprError::Message(err.to_string()))?;
    let monitor = gst::DeviceMonitor::new();
    monitor.add_filter(Some("Audio/Source"), None);
    monitor
        .start()
        .map_err(|err| WisprError::Message(err.to_string()))?;

    let devices = monitor
        .devices()
        .into_iter()
        .map(|device| {
            let display_name = device.display_name().to_string();
            let fallback_description = device
                .properties()
                .and_then(|props| props.get::<String>("node.description").ok())
                .unwrap_or_else(|| display_name.clone());

            DeviceChoice {
                node_name: device_identity(&device).unwrap_or_else(|| display_name.clone()),
                display_name,
                fallback_description,
            }
        })
        .collect();

    monitor.stop();
    Ok(devices)
}

pub fn resolve_selected_device(config: &AppConfig) -> Option<DeviceChoice> {
    let available = enumerate_devices().ok()?;
    let selected = config.selected_device.as_ref()?;

    available
        .into_iter()
        .find(|device| device.node_name == selected.node_name)
        .or_else(|| {
            enumerate_devices().ok()?.into_iter().find(|device| {
                device.display_name == selected.display_name
                    || device.fallback_description == selected.fallback_description
            })
        })
}

fn device_identity(device: &gst::Device) -> Option<String> {
    let props = device.properties()?;
    props
        .get::<String>("node.name")
        .ok()
        .or_else(|| props.get::<String>("object.path").ok())
        .or_else(|| props.get::<String>("api.alsa.path").ok())
}
