use gtk::{Align, Orientation, glib, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::runtime::Runtime;
use wispr_core::{
    AppConfig, DictationProxy, LlmInterpreter, Result, models::DaemonStatus, secrets::SecretStore,
};

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
        .default_width(780)
        .default_height(720)
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
        .title("Speech")
        .description("Select the microphone Wispr should use and store your Deepgram API key in the GNOME keyring.")
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

    let deepgram_api_key_entry = gtk::PasswordEntry::builder()
        .placeholder_text("Deepgram API key")
        .show_peek_icon(true)
        .build();
    deepgram_api_key_entry.set_text(&load_deepgram_api_key().unwrap_or_default());

    let trigger_entry = gtk::Entry::builder()
        .text(&config.hotkey.preferred_trigger)
        .build();

    let autostart_switch = gtk::Switch::builder().active(config.autostart).build();
    let overlay_switch = gtk::Switch::builder()
        .active(config.overlay.enabled)
        .build();

    onboarding_group.add(&row_with_widget("Microphone", &mic_combo));
    onboarding_group.add(&row_with_widget(
        "Deepgram API Key",
        &deepgram_api_key_entry,
    ));
    onboarding_group.add(&row_with_widget("Preferred Shortcut", &trigger_entry));
    onboarding_group.add(&row_with_widget("Start on Login", &autostart_switch));
    onboarding_group.add(&row_with_widget("Show Overlay", &overlay_switch));

    let intelligence_group = adw::PreferencesGroup::builder()
        .title("Intelligence")
        .description("Configure the OpenAI-compatible backend used to interpret finalized spoken segments into text and editing actions.")
        .build();

    let intelligence_enabled = gtk::Switch::builder()
        .active(config.intelligence.enabled)
        .build();
    let llm_base_url_entry = gtk::Entry::builder()
        .text(&config.intelligence.base_url)
        .placeholder_text("https://api.openai.com/v1")
        .build();
    let llm_model_entry = gtk::Entry::builder()
        .text(&config.intelligence.model)
        .placeholder_text("gpt-4o-mini")
        .build();
    let llm_timeout_entry = gtk::SpinButton::with_range(250.0, 30_000.0, 250.0);
    llm_timeout_entry.set_value(config.intelligence.timeout_ms as f64);
    let generation_timeout_entry = gtk::SpinButton::with_range(1_000.0, 600_000.0, 1_000.0);
    generation_timeout_entry.set_value(config.intelligence.generation_timeout_ms as f64);
    let llm_debug_switch = gtk::Switch::builder()
        .active(config.intelligence.debug_overlay)
        .build();
    let llm_api_key_entry = gtk::PasswordEntry::builder()
        .placeholder_text("LLM API key")
        .show_peek_icon(true)
        .build();
    llm_api_key_entry.set_text(&load_llm_api_key().unwrap_or_default());
    let dynamic_shortcuts_switch = gtk::Switch::builder()
        .active(config.intelligence.dynamic_shortcuts_enabled)
        .build();
    let semantic_commands_switch = gtk::Switch::builder()
        .active(config.intelligence.semantic_commands_enabled)
        .build();
    let generation_switch = gtk::Switch::builder()
        .active(config.intelligence.generation_enabled)
        .build();
    let denylist_profile_combo = gtk::ComboBoxText::new();
    denylist_profile_combo.append(Some("minimal"), "Minimal");
    denylist_profile_combo.set_active_id(Some("minimal"));
    let allowlist_entry = gtk::Entry::builder()
        .text(&config.intelligence.shortcut_allowlist.join(", "))
        .placeholder_text("Ctrl+T, Ctrl+L")
        .build();
    let denylist_entry = gtk::Entry::builder()
        .text(&config.intelligence.shortcut_denylist.join(", "))
        .placeholder_text("Super+Left")
        .build();

    intelligence_group.add(&row_with_widget(
        "Enable Intelligence",
        &intelligence_enabled,
    ));
    intelligence_group.add(&row_with_widget("Base URL", &llm_base_url_entry));
    intelligence_group.add(&row_with_widget("Model", &llm_model_entry));
    intelligence_group.add(&row_with_widget("Command Timeout (ms)", &llm_timeout_entry));
    intelligence_group.add(&row_with_widget(
        "Generation Timeout (ms)",
        &generation_timeout_entry,
    ));
    intelligence_group.add(&row_with_widget(
        "Show Interpreter Status",
        &llm_debug_switch,
    ));
    intelligence_group.add(&row_with_widget("LLM API Key", &llm_api_key_entry));
    intelligence_group.add(&row_with_widget(
        "Enable Dynamic Shortcuts",
        &dynamic_shortcuts_switch,
    ));
    intelligence_group.add(&row_with_widget(
        "Enable Semantic Commands",
        &semantic_commands_switch,
    ));
    intelligence_group.add(&row_with_widget(
        "Enable Autonomous Writing",
        &generation_switch,
    ));
    intelligence_group.add(&row_with_widget(
        "Denylist Profile",
        &denylist_profile_combo,
    ));
    intelligence_group.add(&row_with_widget("Shortcut Allowlist", &allowlist_entry));
    intelligence_group.add(&row_with_widget("Shortcut Denylist", &denylist_entry));
    intelligence_group.add(
        &adw::ActionRow::builder()
            .title("Generation Trigger")
            .subtitle("Explicit requests only")
            .build(),
    );
    intelligence_group.add(
        &adw::ActionRow::builder()
            .title("Generation Insert Mode")
            .subtitle("Replace spoken request with generated text")
            .build(),
    );
    intelligence_group.add(
        &adw::ActionRow::builder()
            .title("Generation Target Scope")
            .subtitle("Any focused text field")
            .build(),
    );

    let actions_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .halign(Align::Start)
        .build();
    let save_button = gtk::Button::with_label("Save Settings");
    let refresh_button = gtk::Button::with_label("Refresh Status");
    let toggle_button = gtk::Button::with_label("Toggle Dictation");
    let test_llm_button = gtk::Button::with_label("Test LLM");
    actions_box.append(&save_button);
    actions_box.append(&refresh_button);
    actions_box.append(&toggle_button);
    actions_box.append(&test_llm_button);

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
    main_box.append(&intelligence_group);
    main_box.append(&actions_box);
    main_box.append(&help_group);

    toolbar_view.set_content(Some(&main_box));
    window.set_content(Some(&toolbar_view));

    {
        let mic_combo = mic_combo.clone();
        let deepgram_api_key_entry = deepgram_api_key_entry.clone();
        let trigger_entry = trigger_entry.clone();
        let autostart_switch = autostart_switch.clone();
        let overlay_switch = overlay_switch.clone();
        let status_label = status_label.clone();
        let devices = devices.clone();
        let intelligence_enabled = intelligence_enabled.clone();
        let llm_base_url_entry = llm_base_url_entry.clone();
        let llm_model_entry = llm_model_entry.clone();
        let llm_timeout_entry = llm_timeout_entry.clone();
        let generation_timeout_entry = generation_timeout_entry.clone();
        let llm_debug_switch = llm_debug_switch.clone();
        let llm_api_key_entry = llm_api_key_entry.clone();
        let dynamic_shortcuts_switch = dynamic_shortcuts_switch.clone();
        let semantic_commands_switch = semantic_commands_switch.clone();
        let generation_switch = generation_switch.clone();
        let denylist_profile_combo = denylist_profile_combo.clone();
        let allowlist_entry = allowlist_entry.clone();
        let denylist_entry = denylist_entry.clone();

        save_button.connect_clicked(move |_| {
            let mut config = AppConfig::load().unwrap_or_default();
            config.autostart = autostart_switch.is_active();
            config.overlay.enabled = overlay_switch.is_active();
            config.hotkey.preferred_trigger = trigger_entry.text().to_string();
            config.intelligence.enabled = intelligence_enabled.is_active();
            config.intelligence.base_url = llm_base_url_entry.text().to_string();
            config.intelligence.model = llm_model_entry.text().to_string();
            config.intelligence.timeout_ms = llm_timeout_entry.value() as u64;
            config.intelligence.generation_timeout_ms = generation_timeout_entry.value() as u64;
            config.intelligence.debug_overlay = llm_debug_switch.is_active();
            config.intelligence.dynamic_shortcuts_enabled = dynamic_shortcuts_switch.is_active();
            config.intelligence.semantic_commands_enabled = semantic_commands_switch.is_active();
            config.intelligence.generation_enabled = generation_switch.is_active();
            config.intelligence.shortcut_denylist_profile =
                parse_denylist_profile(denylist_profile_combo.active_id().as_deref());
            config.intelligence.shortcut_allowlist =
                parse_combo_list(allowlist_entry.text().as_str());
            config.intelligence.shortcut_denylist =
                parse_combo_list(denylist_entry.text().as_str());

            if let Some(active_id) = mic_combo.active_id() {
                if let Some(device) = devices.iter().find(|device| device.node_name == active_id) {
                    config.selected_device = Some(device.clone());
                }
            }

            match save_config_and_keys(
                &config,
                deepgram_api_key_entry.text().as_str(),
                llm_api_key_entry.text().as_str(),
            ) {
                Ok(()) => match restart_daemon() {
                    Ok(()) => status_label.set_label("Saved settings and restarted wisprd."),
                    Err(error) => status_label.set_label(&format!(
                        "Saved settings, but failed to restart wisprd: {error}"
                    )),
                },
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

    {
        let status_label = status_label.clone();
        let intelligence_enabled = intelligence_enabled.clone();
        let llm_base_url_entry = llm_base_url_entry.clone();
        let llm_model_entry = llm_model_entry.clone();
        let llm_timeout_entry = llm_timeout_entry.clone();
        let generation_timeout_entry = generation_timeout_entry.clone();
        let llm_debug_switch = llm_debug_switch.clone();
        let llm_api_key_entry = llm_api_key_entry.clone();
        let dynamic_shortcuts_switch = dynamic_shortcuts_switch.clone();
        let semantic_commands_switch = semantic_commands_switch.clone();
        let generation_switch = generation_switch.clone();
        let denylist_profile_combo = denylist_profile_combo.clone();
        let allowlist_entry = allowlist_entry.clone();
        let denylist_entry = denylist_entry.clone();
        test_llm_button.connect_clicked(move |_| {
            let mut config = AppConfig::load().unwrap_or_default();
            config.intelligence.enabled = intelligence_enabled.is_active();
            config.intelligence.base_url = llm_base_url_entry.text().to_string();
            config.intelligence.model = llm_model_entry.text().to_string();
            config.intelligence.timeout_ms = llm_timeout_entry.value() as u64;
            config.intelligence.generation_timeout_ms = generation_timeout_entry.value() as u64;
            config.intelligence.debug_overlay = llm_debug_switch.is_active();
            config.intelligence.dynamic_shortcuts_enabled = dynamic_shortcuts_switch.is_active();
            config.intelligence.semantic_commands_enabled = semantic_commands_switch.is_active();
            config.intelligence.generation_enabled = generation_switch.is_active();
            config.intelligence.shortcut_denylist_profile =
                parse_denylist_profile(denylist_profile_combo.active_id().as_deref());
            config.intelligence.shortcut_allowlist =
                parse_combo_list(allowlist_entry.text().as_str());
            config.intelligence.shortcut_denylist =
                parse_combo_list(denylist_entry.text().as_str());

            match test_llm(config, llm_api_key_entry.text().as_str()) {
                Ok(message) => status_label.set_label(&message),
                Err(error) => status_label.set_label(&format!("LLM test failed: {error}")),
            }
        });
    }

    status_label.set_label(
        &fetch_status()
            .map(|status| format_status(&status))
            .unwrap_or_else(|error| format!("Daemon status unavailable: {error}")),
    );
    window.present();
}

fn parse_combo_list(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_denylist_profile(value: Option<&str>) -> wispr_core::ShortcutDenylistProfile {
    match value {
        Some("minimal") | None => wispr_core::ShortcutDenylistProfile::Minimal,
        Some(_) => wispr_core::ShortcutDenylistProfile::Minimal,
    }
}

fn row_with_widget(title: &str, widget: &impl IsA<gtk::Widget>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(widget);
    row
}

fn save_config_and_keys(
    config: &AppConfig,
    deepgram_api_key: &str,
    llm_api_key: &str,
) -> Result<()> {
    config.save()?;
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let store = SecretStore::connect().await?;
        if !deepgram_api_key.trim().is_empty() {
            store.set_api_key(deepgram_api_key.trim()).await?;
        }
        if !llm_api_key.trim().is_empty() {
            store.set_llm_api_key(llm_api_key.trim()).await?;
        }
        Result::<()>::Ok(())
    })?;
    Ok(())
}

fn load_deepgram_api_key() -> Result<String> {
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let store = SecretStore::connect().await?;
        Ok(store.get_api_key().await?.unwrap_or_default())
    })
}

fn load_llm_api_key() -> Result<String> {
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let store = SecretStore::connect().await?;
        Ok(store.get_llm_api_key().await?.unwrap_or_default())
    })
}

fn test_llm(config: AppConfig, llm_api_key: &str) -> Result<String> {
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async move {
        let interpreter = LlmInterpreter::new(config.intelligence.clone(), llm_api_key.trim())?;
        let result = interpreter.test_connection().await?;
        Ok(format!(
            "LLM test succeeded. Decision: {} | Text: {}{}",
            result.decision.kind.as_label(),
            result.decision.text_to_emit,
            result
                .decision
                .generation_prompt
                .as_ref()
                .map(|prompt| format!(" | Generation prompt: {prompt}"))
                .unwrap_or_default(),
        ))
    })
}

fn restart_daemon() -> Result<()> {
    let status = std::process::Command::new("systemctl")
        .args(["--user", "restart", "wisprd.service"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(wispr_core::WisprError::Message(format!(
            "systemctl exited with status {status}"
        )))
    }
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
        "State: {:?} | Mic ready: {} | Typing ready: {} | Hotkey ready: {} | Intelligence ready: {} | LLM ready: {} | Generation ready: {}{}{}{}{}{}{}{}{}",
        status.state,
        status.mic_ready,
        status.typing_ready,
        status.hotkey_ready,
        status.intelligence_ready,
        status.llm_ready,
        status.generation_ready,
        status
            .current_mic
            .as_ref()
            .map(|mic| format!(" | Mic: {}", mic.display_name))
            .unwrap_or_default(),
        status
            .intelligence_state
            .as_ref()
            .map(|value| format!(" | Intelligence: {value}"))
            .unwrap_or_default(),
        status
            .last_llm_error
            .as_ref()
            .map(|err| format!(" | LLM error: {err}"))
            .unwrap_or_default(),
        status
            .generation_state
            .as_ref()
            .map(|value| format!(" | Generation: {value}"))
            .unwrap_or_default(),
        status
            .last_generation_error
            .as_ref()
            .map(|err| format!(" | Generation error: {err}"))
            .unwrap_or_default(),
        status
            .last_error
            .as_ref()
            .map(|err| format!(" | Error: {err}"))
            .unwrap_or_default(),
        status
            .active_app
            .as_ref()
            .map(|app| format!(
                " | App: {:?}{}",
                app.app_class,
                app.app_id
                    .as_ref()
                    .map(|id| format!(" ({id})"))
                    .unwrap_or_default()
            ))
            .unwrap_or_default(),
        status
            .last_resolution
            .as_ref()
            .map(|value| format!(" | Resolution: {value}"))
            .unwrap_or_default(),
    )
}
