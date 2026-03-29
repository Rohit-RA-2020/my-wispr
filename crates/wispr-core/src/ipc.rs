use zbus::proxy;

pub const DICTATION_SERVICE: &str = "io.wispr.Dictation";
pub const DICTATION_OBJECT_PATH: &str = "/io/wispr/Dictation";
pub const DICTATION_INTERFACE: &str = "io.wispr.Dictation1";

#[proxy(
    interface = "io.wispr.Dictation1",
    default_service = "io.wispr.Dictation",
    default_path = "/io/wispr/Dictation"
)]
pub trait Dictation {
    fn toggle(&self) -> zbus::Result<String>;
    fn start(&self) -> zbus::Result<String>;
    fn stop(&self) -> zbus::Result<String>;
    fn status(&self) -> zbus::Result<String>;
    fn open_settings(&self) -> zbus::Result<String>;
}
