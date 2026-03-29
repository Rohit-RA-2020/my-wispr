use gtk::{Align, Orientation, glib, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::runtime::Runtime;
use wispr_core::{AppConfig, DictationProxy, Result, models::DaemonStatus, secrets::SecretStore};

fn main() -> glib::ExitCode {
    let _ = adw::init();

    let app = adw::Application::builder()
        .application_id("io.wispr.Settings")
        .build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    let config = AppConfig::load().unwrap_or_default();
    let devices = enumerate_devices().unwrap_or_default();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Wispr Settings")
        .default_width(720)
        .default_height(560)
        .build();

    let toolbar_view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(Some("Wispr Settings"))));
    toolbar_view.add_top_bar(&header);

    let main_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let status_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .halign(Align::Fill)
        .label("Loading daemon status...")
        .build();

    let onboarding_group = adw::PreferencesGroup::builder()
        .title("Onboarding")
        .description("Select the microphone you want Wispr to use and store your Deepgram API key in the GNOME keyring.")
        .build();

    let mic_combo = gtk::ComboBoxText::new();
    for device in &devices {
        mic_combo.append(Some(&device.node_name), &device.display_name);
    }
    if let Some(selected) = config.selected_device.as_ref() {
        mic_combo.set_active_id(Some(&selected.node_name));
    } else if !devices.is_empty() {
        mic_combo.set_active(Some(0));
    }

    let api_key_entry = gtk::PasswordEntry::builder()
        .placeholder_text("Deepgram API key")
        .show_peek_icon(true)
        .build();
    api_key_entry.set_text(&load_api_key().unwrap_or_default());

    let trigger_entry = gtk::Entry::builder()
        .text(&config.hotkey.preferred_trigger)
        .build();

    let autostart_switch = gtk::Switch::builder().active(config.autostart).build();
    let overlay_switch = gtk::Switch::builder()
        .active(config.overlay.enabled)
        .build();

    onboarding_group.add(&row_with_widget("Microphone", &mic_combo));
    onboarding_group.add(&row_with_widget("Deepgram API Key", &api_key_entry));
    onboarding_group.add(&row_with_widget("Preferred Shortcut", &trigger_entry));
    onboarding_group.add(&row_with_widget("Start on Login", &autostart_switch));
    onboarding_group.add(&row_with_widget("Show Overlay", &overlay_switch));

    let actions_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .halign(Align::Start)
        .build();
    let save_button = gtk::Button::with_label("Save Settings");
    let refresh_button = gtk::Button::with_label("Refresh Status");
    let toggle_button = gtk::Button::with_label("Toggle Dictation");
    actions_box.append(&save_button);
    actions_box.append(&refresh_button);
    actions_box.append(&toggle_button);

    let help_group = adw::PreferencesGroup::builder()
        .title("System Setup")
        .description("Direct typing on Wayland requires one privileged install step for /dev/uinput. Run `sudo wisprctl setup-uinput` once, then log out and back in.")
        .build();
    help_group.add(
        &adw::ActionRow::builder()
            .title("Command")
            .subtitle("sudo wisprctl setup-uinput")
            .build(),
    );

    main_box.append(&status_label);
    main_box.append(&onboarding_group);
    main_box.append(&actions_box);
    main_box.append(&help_group);

    toolbar_view.set_content(Some(&main_box));
    window.set_content(Some(&toolbar_view));

    {
        let mic_combo = mic_combo.clone();
        let api_key_entry = api_key_entry.clone();
        let trigger_entry = trigger_entry.clone();
        let autostart_switch = autostart_switch.clone();
        let overlay_switch = overlay_switch.clone();
        let status_label = status_label.clone();
        let devices = devices.clone();
        save_button.connect_clicked(move |_| {
            let mut config = AppConfig::load().unwrap_or_default();
            config.autostart = autostart_switch.is_active();
            config.overlay.enabled = overlay_switch.is_active();
            config.hotkey.preferred_trigger = trigger_entry.text().to_string();
            if let Some(active_id) = mic_combo.active_id() {
                if let Some(device) = devices.iter().find(|device| device.node_name == active_id) {
                    config.selected_device = Some(device.clone());
                }
            }

            match save_config_and_key(&config, api_key_entry.text().as_str()) {
                Ok(()) => status_label.set_label(
                    "Saved settings. Restart wisprd or rebind the shortcut if you changed it.",
                ),
                Err(error) => status_label.set_label(&format!("Save failed: {error}")),
            }
        });
    }

    {
        let status_label = status_label.clone();
        refresh_button.connect_clicked(move |_| match fetch_status() {
            Ok(status) => status_label.set_label(&format_status(&status)),
            Err(error) => status_label.set_label(&format!("Daemon status failed: {error}")),
        });
    }

    {
        let status_label = status_label.clone();
        toggle_button.connect_clicked(move |_| match call_daemon_toggle() {
            Ok(message) => status_label.set_label(&message),
            Err(error) => status_label.set_label(&format!("Toggle failed: {error}")),
        });
    }

    status_label.set_label(
        &fetch_status()
            .map(|status| format_status(&status))
            .unwrap_or_else(|error| format!("Daemon status unavailable: {error}")),
    );
    window.present();
}

fn row_with_widget(title: &str, widget: &impl IsA<gtk::Widget>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(widget);
    row
}

fn save_config_and_key(config: &AppConfig, api_key: &str) -> Result<()> {
    config.save()?;
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let store = SecretStore::connect().await?;
        if !api_key.trim().is_empty() {
            store.set_api_key(api_key.trim()).await?;
        }
        Result::<()>::Ok(())
    })?;
    Ok(())
}

fn load_api_key() -> Result<String> {
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let store = SecretStore::connect().await?;
        Ok(store.get_api_key().await?.unwrap_or_default())
    })
}

fn fetch_status() -> Result<DaemonStatus> {
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let connection = zbus::Connection::session().await?;
        let proxy = DictationProxy::new(&connection).await?;
        let raw = proxy.status().await?;
        Ok(serde_json::from_str::<DaemonStatus>(&raw)?)
    })
}

fn call_daemon_toggle() -> Result<String> {
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let connection = zbus::Connection::session().await?;
        let proxy = DictationProxy::new(&connection).await?;
        Ok(proxy.toggle().await?)
    })
}

fn enumerate_devices() -> Result<Vec<wispr_core::DeviceChoice>> {
    use gstreamer as gst;
    use gstreamer::prelude::*;

    gst::init().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    let monitor = gst::DeviceMonitor::new();
    monitor.add_filter(Some("Audio/Source"), None);
    monitor
        .start()
        .map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;

    let devices = monitor
        .devices()
        .into_iter()
        .map(|device| {
            let display_name = device.display_name().to_string();
            let props = device.properties();
            wispr_core::DeviceChoice {
                node_name: props
                    .as_ref()
                    .and_then(|p| p.get::<String>("node.name").ok())
                    .unwrap_or_else(|| display_name.clone()),
                display_name: display_name.clone(),
                fallback_description: props
                    .as_ref()
                    .and_then(|p| p.get::<String>("node.description").ok())
                    .unwrap_or(display_name),
            }
        })
        .collect();

    monitor.stop();
    Ok(devices)
}

fn format_status(status: &DaemonStatus) -> String {
    format!(
        "State: {:?} | Mic ready: {} | Typing ready: {} | Hotkey ready: {}{}{}",
        status.state,
        status.mic_ready,
        status.typing_ready,
        status.hotkey_ready,
        status
            .current_mic
            .as_ref()
            .map(|mic| format!(" | Mic: {}", mic.display_name))
            .unwrap_or_default(),
        status
            .last_error
            .as_ref()
            .map(|err| format!(" | Error: {err}"))
            .unwrap_or_default(),
    )
}
