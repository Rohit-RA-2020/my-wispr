#[cfg(target_os = "linux")]
#[path = "overlay_linux.rs"]
mod overlay_linux;

#[cfg(target_os = "linux")]
pub use overlay_linux::OverlayHandle;

#[cfg(not(target_os = "linux"))]
#[derive(Clone)]
pub struct OverlayHandle;

#[cfg(not(target_os = "linux"))]
impl OverlayHandle {
    pub fn spawn() -> Self {
        Self
    }

    pub fn push(&self, _status: wispr_core::models::DaemonStatus) {}
}
