#[cfg(target_os = "linux")]
#[path = "audio_linux.rs"]
mod audio_linux;
#[cfg(target_os = "macos")]
#[path = "audio_macos.rs"]
mod audio_macos;

#[cfg(target_os = "linux")]
pub use audio_linux::{AudioCapture, enumerate_devices, resolve_selected_device};
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
pub use audio_macos::{AudioCapture, enumerate_devices, resolve_selected_device};

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported {
    use async_channel::Receiver;
    use wispr_core::{
        AppConfig, DeviceChoice,
        error::{Result, WisprError},
    };

    pub struct AudioCapture;

    impl AudioCapture {
        pub fn start(_selected: &DeviceChoice) -> Result<Self> {
            Err(WisprError::InvalidState(
                "audio capture backend is not implemented for this operating system".to_string(),
            ))
        }

        pub fn receiver(&self) -> Receiver<Vec<u8>> {
            let (_tx, rx) = async_channel::bounded::<Vec<u8>>(1);
            rx
        }

        pub fn stop(&self) -> Result<()> {
            Ok(())
        }
    }

    pub fn enumerate_devices() -> Result<Vec<DeviceChoice>> {
        Ok(Vec::new())
    }

    pub fn resolve_selected_device(_config: &AppConfig) -> Option<DeviceChoice> {
        None
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use unsupported::{AudioCapture, enumerate_devices, resolve_selected_device};
