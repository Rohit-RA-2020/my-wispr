#[cfg(target_os = "linux")]
#[path = "linux_main.rs"]
mod linux_main;

#[cfg(target_os = "linux")]
fn main() -> gtk::glib::ExitCode {
    linux_main::main()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("wispr-settings GTK UI is Linux-only. On macOS use the Wispr menu bar app.");
}
