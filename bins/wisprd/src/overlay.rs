use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use glib::ControlFlow;
use gtk::{Align, Orientation, glib, prelude::*};
use wispr_core::models::{DaemonStatus, DictationState};

#[derive(Clone)]
pub struct OverlayHandle {
    sender: Sender<DaemonStatus>,
}

impl OverlayHandle {
    pub fn spawn() -> Self {
        let (sender, receiver) = mpsc::channel::<DaemonStatus>();
        thread::spawn(move || run_overlay(receiver));
        Self { sender }
    }

    pub fn push(&self, status: DaemonStatus) {
        let _ = self.sender.send(status);
    }
}

fn run_overlay(receiver: Receiver<DaemonStatus>) {
    if gtk::init().is_err() {
        return;
    }

    let receiver = Arc::new(Mutex::new(receiver));
    let window = gtk::Window::builder()
        .title("Wispr Overlay")
        .default_width(380)
        .default_height(110)
        .decorated(false)
        .resizable(false)
        .deletable(false)
        .opacity(0.92)
        .build();
    window.set_focusable(false);
    let live_window = window.clone();

    let container = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let state_label = gtk::Label::builder()
        .xalign(0.0)
        .halign(Align::Start)
        .css_classes(["title-3"])
        .label("Idle")
        .build();
    let mic_label = gtk::Label::builder()
        .xalign(0.0)
        .halign(Align::Start)
        .label("No microphone selected")
        .build();
    let transcript_label = gtk::Label::builder()
        .xalign(0.0)
        .halign(Align::Start)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();

    container.append(&state_label);
    container.append(&mic_label);
    container.append(&transcript_label);
    window.set_child(Some(&container));

    glib::timeout_add_local(Duration::from_millis(60), move || {
        let Some(status) = receiver.lock().ok().and_then(|rx| rx.try_recv().ok()) else {
            return ControlFlow::Continue;
        };

        let state_text = match status.state {
            DictationState::Idle => "Idle",
            DictationState::Listening => "Listening",
            DictationState::Error => "Error",
        };
        state_label.set_label(state_text);
        mic_label.set_label(
            &status
                .current_mic
                .as_ref()
                .map(|mic| format!("Mic: {}", mic.display_name))
                .unwrap_or_else(|| "Mic: not selected".to_string()),
        );
        transcript_label.set_label(status.partial_transcript.as_deref().unwrap_or(""));

        if matches!(status.state, DictationState::Idle) {
            live_window.hide();
        } else {
            live_window.present();
        }

        ControlFlow::Continue
    });

    window.hide();
    let loop_ = glib::MainLoop::new(None, false);
    loop_.run();
}
