use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use glib::ControlFlow;
use gtk::{Align, Orientation, gdk, glib, prelude::*};
use wispr_core::models::{DaemonStatus, DictationState, TranscriptionProvider};

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

    install_overlay_css();

    let receiver = Arc::new(Mutex::new(receiver));
    let window = gtk::Window::builder()
        .title("Wispr Overlay")
        .default_width(340)
        .default_height(88)
        .decorated(false)
        .resizable(false)
        .deletable(false)
        .build();
    window.set_focusable(false);
    window.add_css_class("wispr-overlay-window");
    window.set_opacity(0.96);

    let shell = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .margin_top(14)
        .margin_bottom(14)
        .margin_start(14)
        .margin_end(14)
        .build();

    let card = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();
    card.add_css_class("overlay-card");
    card.set_tooltip_text(Some("Drag to move"));

    let header = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .build();

    let indicator = gtk::Box::new(Orientation::Horizontal, 0);
    indicator.add_css_class("overlay-indicator");
    indicator.set_size_request(10, 10);
    indicator.set_valign(Align::Center);

    let text_stack = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(3)
        .hexpand(true)
        .build();

    let title_label = gtk::Label::builder()
        .xalign(0.0)
        .halign(Align::Start)
        .label("Listening")
        .build();
    title_label.add_css_class("overlay-title");

    let message_label = gtk::Label::builder()
        .xalign(0.0)
        .halign(Align::Start)
        .build();
    message_label.add_css_class("overlay-message");
    message_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    message_label.set_max_width_chars(48);
    message_label.set_single_line_mode(true);

    text_stack.append(&title_label);
    text_stack.append(&message_label);

    let provider_badge = gtk::Label::builder().label("Cloud").build();
    provider_badge.add_css_class("overlay-badge");

    header.append(&indicator);
    header.append(&text_stack);
    header.append(&provider_badge);

    let detail_label = gtk::Label::builder()
        .xalign(0.0)
        .halign(Align::Start)
        .visible(false)
        .build();
    detail_label.add_css_class("overlay-detail");
    detail_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    detail_label.set_max_width_chars(56);
    detail_label.set_single_line_mode(true);

    card.append(&header);
    card.append(&detail_label);
    shell.append(&card);
    window.set_child(Some(&shell));

    attach_drag_controller(&window, &card);

    let live_window = window.clone();
    glib::timeout_add_local(Duration::from_millis(60), move || {
        let Some(status) = latest_status(&receiver) else {
            return ControlFlow::Continue;
        };

        apply_overlay_status(
            &live_window,
            &card,
            &indicator,
            &title_label,
            &message_label,
            &provider_badge,
            &detail_label,
            status,
        );

        ControlFlow::Continue
    });

    window.hide();
    let loop_ = glib::MainLoop::new(None, false);
    loop_.run();
}

fn attach_drag_controller(window: &gtk::Window, card: &gtk::Box) {
    let gesture = gtk::GestureClick::builder().button(1).build();
    let window = window.clone();
    gesture.connect_pressed(move |gesture, _, x, y| {
        let Some(device) = gesture.current_event_device() else {
            return;
        };
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.dynamic_cast::<gdk::Toplevel>() else {
            return;
        };

        toplevel.begin_move(
            &device,
            gesture.current_button() as i32,
            x,
            y,
            gesture.current_event_time(),
        );
    });
    card.add_controller(gesture);
}

fn latest_status(receiver: &Arc<Mutex<Receiver<DaemonStatus>>>) -> Option<DaemonStatus> {
    let Ok(rx) = receiver.lock() else {
        return None;
    };

    let mut latest = None;
    while let Ok(status) = rx.try_recv() {
        latest = Some(status);
    }
    latest
}

#[allow(clippy::too_many_arguments)]
fn apply_overlay_status(
    window: &gtk::Window,
    card: &gtk::Box,
    indicator: &gtk::Box,
    title_label: &gtk::Label,
    message_label: &gtk::Label,
    provider_badge: &gtk::Label,
    detail_label: &gtk::Label,
    status: DaemonStatus,
) {
    title_label.set_label(primary_title(&status));
    message_label.set_label(&compact_message(&status));
    provider_badge.set_label(provider_label(status.transcription_provider.clone()));

    indicator.remove_css_class("overlay-indicator-live");
    indicator.remove_css_class("overlay-indicator-idle");
    indicator.remove_css_class("overlay-indicator-error");
    card.remove_css_class("overlay-card-error");

    match status.state {
        DictationState::Idle => indicator.add_css_class("overlay-indicator-idle"),
        DictationState::Listening => indicator.add_css_class("overlay-indicator-live"),
        DictationState::Error => {
            indicator.add_css_class("overlay-indicator-error");
            card.add_css_class("overlay-card-error");
        }
    }

    let detail = detail_text(&status);
    detail_label.set_label(&detail);
    detail_label.set_visible(!detail.is_empty());

    if matches!(status.state, DictationState::Idle) {
        window.hide();
    } else {
        window.present();
    }
}

fn install_overlay_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        window.wispr-overlay-window {
            background: transparent;
        }

        .overlay-card {
            background: rgba(20, 25, 33, 0.68);
            border-radius: 20px;
            padding: 12px 14px;
            border: 1px solid rgba(255, 255, 255, 0.08);
            box-shadow:
                0 10px 28px rgba(0, 0, 0, 0.18),
                inset 0 1px 0 rgba(255, 255, 255, 0.04);
        }

        .overlay-card-error {
            background: rgba(54, 22, 22, 0.82);
            border-color: rgba(255, 159, 159, 0.18);
        }

        .overlay-indicator {
            border-radius: 999px;
            min-width: 10px;
            min-height: 10px;
        }

        .overlay-indicator-live {
            background: #59d68d;
        }

        .overlay-indicator-idle {
            background: rgba(255, 255, 255, 0.32);
        }

        .overlay-indicator-error {
            background: #ff7b7b;
        }

        .overlay-title {
            color: rgba(248, 250, 252, 0.96);
            font-weight: 700;
            font-size: 0.97rem;
            letter-spacing: -0.01em;
        }

        .overlay-message {
            color: rgba(232, 237, 242, 0.72);
            font-size: 0.90rem;
        }

        .overlay-badge {
            background: rgba(255, 255, 255, 0.07);
            color: rgba(241, 245, 249, 0.78);
            border-radius: 999px;
            padding: 4px 10px;
            font-size: 0.76rem;
            font-weight: 700;
        }

        .overlay-detail {
            color: rgba(255, 210, 210, 0.90);
            font-size: 0.84rem;
        }
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn primary_title(status: &DaemonStatus) -> &'static str {
    match status.state {
        DictationState::Idle => "Idle",
        DictationState::Listening => match status.transcription_provider {
            TranscriptionProvider::Deepgram => "Listening",
            TranscriptionProvider::WhisperLocal => "Listening locally",
        },
        DictationState::Error => "Transcription issue",
    }
}

fn compact_message(status: &DaemonStatus) -> String {
    if let Some(text) = status
        .partial_transcript
        .as_deref()
        .map(compact_text)
        .filter(|value| !value.is_empty())
    {
        return clip_tail(&text, 110);
    }

    if let Some(state) = status
        .transcription_state
        .as_deref()
        .map(compact_text)
        .filter(|value| !value.is_empty())
    {
        return clip_tail(&state, 110);
    }

    status
        .current_mic
        .as_ref()
        .map(|mic| format!("Mic: {}", mic.display_name))
        .unwrap_or_else(|| "Waiting for speech".to_string())
}

fn detail_text(status: &DaemonStatus) -> String {
    if let Some(error) = status
        .last_transcription_error
        .as_deref()
        .or(status.last_error.as_deref())
        .map(compact_text)
        .filter(|value| !value.is_empty())
    {
        return clip_tail(&error, 140);
    }

    if matches!(status.state, DictationState::Error) {
        return "Check transcription backend readiness.".to_string();
    }

    String::new()
}

fn provider_label(provider: TranscriptionProvider) -> &'static str {
    match provider {
        TranscriptionProvider::Deepgram => "Cloud",
        TranscriptionProvider::WhisperLocal => "Local",
    }
}

fn compact_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip_tail(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    let clipped = chars[chars.len().saturating_sub(max_chars)..]
        .iter()
        .collect::<String>();
    format!("…{clipped}")
}
