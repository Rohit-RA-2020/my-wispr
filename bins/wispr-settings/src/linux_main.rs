use std::{
    sync::mpsc::{self, TryRecvError},
    thread,
    time::Duration as StdDuration,
};

use gtk::{Align, Orientation, glib, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::runtime::Runtime;
use wispr_core::{
    AppConfig, DictationProxy, LlmInterpreter, Result,
    models::{DaemonStatus, TranscriptionProvider},
    secrets::SecretStore,
    whisper,
};

pub fn main() -> glib::ExitCode {
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
    let whisper_status = whisper::collect_manager_status(&config.transcription.whisper_local);

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

    let transcription_group = adw::PreferencesGroup::builder()
        .title("Transcription")
        .description("Choose the transcription backend and microphone.")
        .build();

    let transcription_provider_combo = gtk::ComboBoxText::new();
    transcription_provider_combo.append(Some("deepgram"), "Cloud (Deepgram)");
    transcription_provider_combo.append(Some("whisper_local"), "Local (Whisper)");
    transcription_provider_combo.set_active_id(Some(match config.transcription.provider {
        TranscriptionProvider::Deepgram => "deepgram",
        TranscriptionProvider::WhisperLocal => "whisper_local",
    }));

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
    let deepgram_api_key_row = row_with_widget("Deepgram API Key", &deepgram_api_key_entry);

    let whisper_model_combo = gtk::ComboBoxText::new();
    populate_whisper_model_combo(
        &whisper_model_combo,
        &whisper_status.curated_models,
        &config.transcription.whisper_local.model,
    );

    let whisper_backend_status_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(48)
        .label(&format_whisper_runtime_status(
            &config.transcription.whisper_local,
            &whisper_status,
        ))
        .build();
    let whisper_backend_status_row =
        row_with_widget("Local Backend Status", &whisper_backend_status_label);

    let whisper_installed_models_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(48)
        .label(&format_installed_models(&whisper_status.installed_models))
        .build();
    let whisper_installed_models_row =
        row_with_widget("Installed Models", &whisper_installed_models_label);

    let whisper_model_row = row_with_widget("Whisper Model", &whisper_model_combo);
    let whisper_install_button = gtk::Button::with_label("Install Whisper");
    let whisper_download_button = gtk::Button::with_label("Download Model");
    let whisper_test_button = gtk::Button::with_label("Test Local");
    let whisper_delete_button = gtk::Button::with_label("Delete Model");

    let trigger_entry = gtk::Entry::builder()
        .text(&config.hotkey.preferred_trigger)
        .build();

    let autostart_row = adw::SwitchRow::builder()
        .title("Start on Login")
        .active(config.autostart)
        .build();
    let overlay_row = adw::SwitchRow::builder()
        .title("Show Overlay")
        .active(config.overlay.enabled)
        .build();

    let cloud_group = adw::PreferencesGroup::builder()
        .title("Cloud Transcription")
        .description("Deepgram setup for streaming cloud transcription.")
        .build();
    cloud_group.add(&deepgram_api_key_row);

    let whisper_group = adw::PreferencesGroup::builder()
        .title("Local Whisper")
        .description(
            "English-only local transcription with Wispr-managed model downloads and cleanup.",
        )
        .build();
    whisper_group.add(&whisper_model_row);
    whisper_group.add(&whisper_backend_status_row);
    whisper_group.add(&whisper_installed_models_row);
    whisper_group.add(&buttons_row(
        "Runtime Actions",
        &[&whisper_install_button, &whisper_test_button],
    ));
    whisper_group.add(&buttons_row(
        "Model Actions",
        &[&whisper_download_button, &whisper_delete_button],
    ));

    let general_group = adw::PreferencesGroup::builder()
        .title("General")
        .description("Device, shortcut, and startup preferences.")
        .build();

    transcription_group.add(&row_with_widget(
        "Transcription Backend",
        &transcription_provider_combo,
    ));
    transcription_group.add(&row_with_widget("Microphone", &mic_combo));
    general_group.add(&row_with_widget("Preferred Shortcut", &trigger_entry));
    general_group.add(&autostart_row);
    general_group.add(&overlay_row);

    let intelligence_group = adw::PreferencesGroup::builder()
        .title("Intelligence")
        .description("Configure the OpenAI-compatible backend used to interpret finalized spoken segments into text and editing actions.")
        .build();

    let intelligence_enabled_row = adw::SwitchRow::builder()
        .title("Enable Intelligence")
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
    let llm_debug_row = adw::SwitchRow::builder()
        .title("Show Interpreter Status")
        .active(config.intelligence.debug_overlay)
        .build();
    let llm_api_key_entry = gtk::PasswordEntry::builder()
        .placeholder_text("LLM API key")
        .show_peek_icon(true)
        .build();
    llm_api_key_entry.set_text(&load_llm_api_key().unwrap_or_default());
    let dynamic_shortcuts_row = adw::SwitchRow::builder()
        .title("Enable Dynamic Shortcuts")
        .active(config.intelligence.dynamic_shortcuts_enabled)
        .build();
    let semantic_commands_row = adw::SwitchRow::builder()
        .title("Enable Semantic Commands")
        .active(config.intelligence.semantic_commands_enabled)
        .build();
    let generation_row = adw::SwitchRow::builder()
        .title("Enable Autonomous Writing")
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

    intelligence_group.add(&intelligence_enabled_row);
    intelligence_group.add(&row_with_widget("Base URL", &llm_base_url_entry));
    intelligence_group.add(&row_with_widget("Model", &llm_model_entry));
    intelligence_group.add(&row_with_widget("Command Timeout (ms)", &llm_timeout_entry));
    intelligence_group.add(&row_with_widget(
        "Generation Timeout (ms)",
        &generation_timeout_entry,
    ));
    intelligence_group.add(&llm_debug_row);
    intelligence_group.add(&row_with_widget("LLM API Key", &llm_api_key_entry));
    intelligence_group.add(&dynamic_shortcuts_row);
    intelligence_group.add(&semantic_commands_row);
    intelligence_group.add(&generation_row);
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
    main_box.append(&transcription_group);
    main_box.append(&cloud_group);
    main_box.append(&whisper_group);
    main_box.append(&general_group);
    main_box.append(&intelligence_group);
    main_box.append(&actions_box);
    main_box.append(&help_group);

    let clamp = adw::Clamp::builder()
        .maximum_size(860)
        .tightening_threshold(620)
        .child(&main_box)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&clamp)
        .build();

    toolbar_view.set_content(Some(&scroller));
    window.set_content(Some(&toolbar_view));

    refresh_transcription_visibility(
        current_transcription_provider(transcription_provider_combo.active_id().as_deref()),
        &cloud_group,
        &whisper_group,
    );

    {
        let mic_combo = mic_combo.clone();
        let transcription_provider_combo = transcription_provider_combo.clone();
        let whisper_model_combo = whisper_model_combo.clone();
        let deepgram_api_key_entry = deepgram_api_key_entry.clone();
        let trigger_entry = trigger_entry.clone();
        let autostart_row = autostart_row.clone();
        let overlay_row = overlay_row.clone();
        let status_label = status_label.clone();
        let devices = devices.clone();
        let intelligence_enabled_row = intelligence_enabled_row.clone();
        let llm_base_url_entry = llm_base_url_entry.clone();
        let llm_model_entry = llm_model_entry.clone();
        let llm_timeout_entry = llm_timeout_entry.clone();
        let generation_timeout_entry = generation_timeout_entry.clone();
        let llm_debug_row = llm_debug_row.clone();
        let llm_api_key_entry = llm_api_key_entry.clone();
        let dynamic_shortcuts_row = dynamic_shortcuts_row.clone();
        let semantic_commands_row = semantic_commands_row.clone();
        let generation_row = generation_row.clone();
        let denylist_profile_combo = denylist_profile_combo.clone();
        let allowlist_entry = allowlist_entry.clone();
        let denylist_entry = denylist_entry.clone();

        save_button.connect_clicked(move |_| {
            let mut config = AppConfig::load().unwrap_or_default();
            config.autostart = autostart_row.is_active();
            config.overlay.enabled = overlay_row.is_active();
            config.hotkey.preferred_trigger = trigger_entry.text().to_string();
            config.transcription.provider =
                current_transcription_provider(transcription_provider_combo.active_id().as_deref());
            if let Some(active_id) = whisper_model_combo.active_id() {
                config.transcription.whisper_local.model = active_id.to_string();
            }
            config.intelligence.enabled = intelligence_enabled_row.is_active();
            config.intelligence.base_url = llm_base_url_entry.text().to_string();
            config.intelligence.model = llm_model_entry.text().to_string();
            config.intelligence.timeout_ms = llm_timeout_entry.value() as u64;
            config.intelligence.generation_timeout_ms = generation_timeout_entry.value() as u64;
            config.intelligence.debug_overlay = llm_debug_row.is_active();
            config.intelligence.dynamic_shortcuts_enabled = dynamic_shortcuts_row.is_active();
            config.intelligence.semantic_commands_enabled = semantic_commands_row.is_active();
            config.intelligence.generation_enabled = generation_row.is_active();
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
        let transcription_provider_combo = transcription_provider_combo.clone();
        let cloud_group = cloud_group.clone();
        let whisper_group = whisper_group.clone();
        transcription_provider_combo.connect_changed(move |combo| {
            refresh_transcription_visibility(
                current_transcription_provider(combo.active_id().as_deref()),
                &cloud_group,
                &whisper_group,
            );
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
        let whisper_model_combo = whisper_model_combo.clone();
        let whisper_backend_status_label = whisper_backend_status_label.clone();
        let whisper_installed_models_label = whisper_installed_models_label.clone();
        let whisper_install_button = whisper_install_button.clone();
        let whisper_download_button = whisper_download_button.clone();
        let whisper_test_button = whisper_test_button.clone();
        let whisper_delete_button = whisper_delete_button.clone();
        let install_button_for_handler = whisper_install_button.clone();
        let download_button_for_handler = whisper_download_button.clone();
        let test_button_for_handler = whisper_test_button.clone();
        let delete_button_for_handler = whisper_delete_button.clone();
        whisper_install_button.connect_clicked(move |_| {
            run_whisper_manager_task(
                &status_label,
                &whisper_model_combo,
                &whisper_backend_status_label,
                &whisper_installed_models_label,
                &[
                    &install_button_for_handler,
                    &download_button_for_handler,
                    &test_button_for_handler,
                    &delete_button_for_handler,
                ],
                "Installing Whisper runtime...",
                "Whisper install failed",
                move || {
                    let config = AppConfig::load().unwrap_or_default();
                    (config, whisper::install_runtime())
                },
            );
        });
    }

    {
        let status_label = status_label.clone();
        let whisper_model_combo = whisper_model_combo.clone();
        let whisper_backend_status_label = whisper_backend_status_label.clone();
        let whisper_installed_models_label = whisper_installed_models_label.clone();
        let whisper_install_button = whisper_install_button.clone();
        let whisper_download_button = whisper_download_button.clone();
        let whisper_test_button = whisper_test_button.clone();
        let whisper_delete_button = whisper_delete_button.clone();
        let install_button_for_handler = whisper_install_button.clone();
        let download_button_for_handler = whisper_download_button.clone();
        let test_button_for_handler = whisper_test_button.clone();
        let delete_button_for_handler = whisper_delete_button.clone();
        whisper_download_button.connect_clicked(move |_| {
            let selected_model = whisper_model_combo
                .active_id()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "base.en".to_string());
            run_whisper_manager_task(
                &status_label,
                &whisper_model_combo,
                &whisper_backend_status_label,
                &whisper_installed_models_label,
                &[
                    &install_button_for_handler,
                    &download_button_for_handler,
                    &test_button_for_handler,
                    &delete_button_for_handler,
                ],
                &format!("Downloading Whisper model {selected_model}..."),
                "Whisper download failed",
                move || {
                    let mut config = AppConfig::load().unwrap_or_default();
                    config.transcription.whisper_local.model = selected_model.clone();
                    let model = config.transcription.whisper_local.model.clone();
                    let result =
                        whisper::download_model(&config.transcription.whisper_local, &model);
                    (config, result)
                },
            );
        });
    }

    {
        let status_label = status_label.clone();
        let whisper_model_combo = whisper_model_combo.clone();
        let whisper_backend_status_label = whisper_backend_status_label.clone();
        let whisper_installed_models_label = whisper_installed_models_label.clone();
        let whisper_install_button = whisper_install_button.clone();
        let whisper_download_button = whisper_download_button.clone();
        let whisper_test_button = whisper_test_button.clone();
        let whisper_delete_button = whisper_delete_button.clone();
        let install_button_for_handler = whisper_install_button.clone();
        let download_button_for_handler = whisper_download_button.clone();
        let test_button_for_handler = whisper_test_button.clone();
        let delete_button_for_handler = whisper_delete_button.clone();
        whisper_test_button.connect_clicked(move |_| {
            let selected_model = whisper_model_combo
                .active_id()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "base.en".to_string());
            run_whisper_manager_task(
                &status_label,
                &whisper_model_combo,
                &whisper_backend_status_label,
                &whisper_installed_models_label,
                &[
                    &install_button_for_handler,
                    &download_button_for_handler,
                    &test_button_for_handler,
                    &delete_button_for_handler,
                ],
                &format!("Testing Whisper model {selected_model}..."),
                "Whisper test failed",
                move || {
                    let mut config = AppConfig::load().unwrap_or_default();
                    config.transcription.whisper_local.model = selected_model.clone();
                    let model = config.transcription.whisper_local.model.clone();
                    let result =
                        whisper::test_model_load(&config.transcription.whisper_local, &model);
                    (config, result)
                },
            );
        });
    }

    {
        let status_label = status_label.clone();
        let whisper_model_combo = whisper_model_combo.clone();
        let whisper_backend_status_label = whisper_backend_status_label.clone();
        let whisper_installed_models_label = whisper_installed_models_label.clone();
        let whisper_install_button = whisper_install_button.clone();
        let whisper_download_button = whisper_download_button.clone();
        let whisper_test_button = whisper_test_button.clone();
        let whisper_delete_button = whisper_delete_button.clone();
        let install_button_for_handler = whisper_install_button.clone();
        let download_button_for_handler = whisper_download_button.clone();
        let test_button_for_handler = whisper_test_button.clone();
        let delete_button_for_handler = whisper_delete_button.clone();
        whisper_delete_button.connect_clicked(move |_| {
            let selected_model = whisper_model_combo
                .active_id()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "base.en".to_string());
            run_whisper_manager_task(
                &status_label,
                &whisper_model_combo,
                &whisper_backend_status_label,
                &whisper_installed_models_label,
                &[
                    &install_button_for_handler,
                    &download_button_for_handler,
                    &test_button_for_handler,
                    &delete_button_for_handler,
                ],
                &format!("Deleting Whisper model {selected_model}..."),
                "Whisper delete failed",
                move || {
                    let mut config = AppConfig::load().unwrap_or_default();
                    config.transcription.whisper_local.model = selected_model.clone();
                    let model = config.transcription.whisper_local.model.clone();
                    let result = whisper::delete_model(&config.transcription.whisper_local, &model);
                    (config, result)
                },
            );
        });
    }

    {
        let status_label = status_label.clone();
        let intelligence_enabled_row = intelligence_enabled_row.clone();
        let llm_base_url_entry = llm_base_url_entry.clone();
        let llm_model_entry = llm_model_entry.clone();
        let llm_timeout_entry = llm_timeout_entry.clone();
        let generation_timeout_entry = generation_timeout_entry.clone();
        let llm_debug_row = llm_debug_row.clone();
        let llm_api_key_entry = llm_api_key_entry.clone();
        let dynamic_shortcuts_row = dynamic_shortcuts_row.clone();
        let semantic_commands_row = semantic_commands_row.clone();
        let generation_row = generation_row.clone();
        let denylist_profile_combo = denylist_profile_combo.clone();
        let allowlist_entry = allowlist_entry.clone();
        let denylist_entry = denylist_entry.clone();
        test_llm_button.connect_clicked(move |_| {
            let mut config = AppConfig::load().unwrap_or_default();
            config.intelligence.enabled = intelligence_enabled_row.is_active();
            config.intelligence.base_url = llm_base_url_entry.text().to_string();
            config.intelligence.model = llm_model_entry.text().to_string();
            config.intelligence.timeout_ms = llm_timeout_entry.value() as u64;
            config.intelligence.generation_timeout_ms = generation_timeout_entry.value() as u64;
            config.intelligence.debug_overlay = llm_debug_row.is_active();
            config.intelligence.dynamic_shortcuts_enabled = dynamic_shortcuts_row.is_active();
            config.intelligence.semantic_commands_enabled = semantic_commands_row.is_active();
            config.intelligence.generation_enabled = generation_row.is_active();
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
    let align_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .halign(Align::End)
        .valign(Align::Center)
        .build();
    align_box.append(widget);
    row.add_suffix(&align_box);
    row.set_activatable_widget(Some(widget));
    row
}

fn buttons_row(title: &str, buttons: &[&gtk::Button]) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    let box_ = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::End)
        .build();
    for button in buttons {
        box_.append(*button);
    }
    row.add_suffix(&box_);
    row
}

fn current_transcription_provider(value: Option<&str>) -> TranscriptionProvider {
    match value {
        Some("whisper_local") => TranscriptionProvider::WhisperLocal,
        Some("deepgram") | None => TranscriptionProvider::Deepgram,
        Some(_) => TranscriptionProvider::Deepgram,
    }
}

fn refresh_transcription_visibility(
    provider: TranscriptionProvider,
    cloud_group: &adw::PreferencesGroup,
    whisper_group: &adw::PreferencesGroup,
) {
    let is_local = matches!(provider, TranscriptionProvider::WhisperLocal);
    cloud_group.set_visible(!is_local);
    whisper_group.set_visible(is_local);
}

fn populate_whisper_model_combo(
    combo: &gtk::ComboBoxText,
    models: &[String],
    selected_model: &str,
) {
    combo.remove_all();
    let mut seen_selected = false;
    for model in models {
        combo.append(Some(model), model);
        if model == selected_model {
            seen_selected = true;
        }
    }
    if !seen_selected {
        combo.append(Some(selected_model), selected_model);
    }
    combo.set_active_id(Some(selected_model));
}

fn refresh_whisper_manager_ui(
    config: &AppConfig,
    model_combo: &gtk::ComboBoxText,
    backend_status_label: &gtk::Label,
    installed_models_label: &gtk::Label,
) {
    let status = whisper::collect_manager_status(&config.transcription.whisper_local);
    populate_whisper_model_combo(
        model_combo,
        &status.curated_models,
        &config.transcription.whisper_local.model,
    );
    backend_status_label.set_label(&format_whisper_runtime_status(
        &config.transcription.whisper_local,
        &status,
    ));
    installed_models_label.set_label(&format_installed_models(&status.installed_models));
}

fn run_whisper_manager_task<F>(
    status_label: &gtk::Label,
    model_combo: &gtk::ComboBoxText,
    backend_status_label: &gtk::Label,
    installed_models_label: &gtk::Label,
    buttons: &[&gtk::Button],
    pending_message: &str,
    error_prefix: &str,
    task: F,
) where
    F: FnOnce() -> (AppConfig, Result<String>) + Send + 'static,
{
    status_label.set_label(pending_message);
    for button in buttons {
        button.set_sensitive(false);
    }

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(task());
    });

    let status_label = status_label.clone();
    let model_combo = model_combo.clone();
    let backend_status_label = backend_status_label.clone();
    let installed_models_label = installed_models_label.clone();
    let buttons = buttons
        .iter()
        .map(|button| (*button).clone())
        .collect::<Vec<_>>();
    let error_prefix = error_prefix.to_string();

    glib::timeout_add_local(StdDuration::from_millis(100), move || match rx.try_recv() {
        Ok((config, result)) => {
            refresh_whisper_manager_ui(
                &config,
                &model_combo,
                &backend_status_label,
                &installed_models_label,
            );
            for button in &buttons {
                button.set_sensitive(true);
            }
            match result {
                Ok(message) => status_label.set_label(&message),
                Err(error) => status_label.set_label(&format!("{error_prefix}: {error}")),
            }
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => {
            for button in &buttons {
                button.set_sensitive(true);
            }
            status_label.set_label("Whisper task failed unexpectedly.");
            glib::ControlFlow::Break
        }
    });
}

fn format_whisper_runtime_status(
    config: &wispr_core::WhisperLocalConfig,
    status: &whisper::WhisperManagerStatus,
) -> String {
    let mut parts = vec![
        format!("venv python: {}", yes_no(status.runtime.python_ready)),
        format!("Whisper: {}", yes_no(status.runtime.whisper_ready)),
        format!("ffmpeg: {}", yes_no(status.runtime.ffmpeg_ready)),
    ];
    if let Some(detail) = &status.runtime.detail {
        parts.push(detail.clone());
    }
    parts.push(format!("venv: {}", whisper::default_venv_dir().display()));
    parts.push(format!("model dir: {}", config.model_dir.display()));
    parts.join(" | ")
}

fn format_installed_models(models: &[String]) -> String {
    if models.is_empty() {
        "No models installed in the Wispr Whisper directory".to_string()
    } else {
        models.join(", ")
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "ready" } else { "missing" }
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
        let proxy = DictationProxy::new().await?;
        let raw = proxy.status().await?;
        Ok(serde_json::from_str::<DaemonStatus>(&raw)?)
    })
}

fn call_daemon_toggle() -> Result<String> {
    let runtime = Runtime::new().map_err(|err| wispr_core::WisprError::Message(err.to_string()))?;
    runtime.block_on(async {
        let proxy = DictationProxy::new().await?;
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
    let mut parts = vec![
        format!("State: {:?}", status.state),
        format!("Provider: {:?}", status.transcription_provider),
        format!("Transcription ready: {}", status.transcription_ready),
        format!("Mic ready: {}", status.mic_ready),
        format!("Typing ready: {}", status.typing_ready),
        format!("Hotkey ready: {}", status.hotkey_ready),
        format!("Intelligence ready: {}", status.intelligence_ready),
        format!("LLM ready: {}", status.llm_ready),
        format!("Generation ready: {}", status.generation_ready),
    ];

    if let Some(value) = &status.transcription_state {
        parts.push(format!("Transcription: {value}"));
    }
    if let Some(value) = &status.selected_whisper_model {
        parts.push(format!("Whisper model: {value}"));
    }
    if let Some(err) = &status.last_transcription_error {
        parts.push(format!("Transcription error: {err}"));
    }
    if let Some(mic) = &status.current_mic {
        parts.push(format!("Mic: {}", mic.display_name));
    }
    if let Some(value) = &status.intelligence_state {
        parts.push(format!("Intelligence: {value}"));
    }
    if let Some(err) = &status.last_llm_error {
        parts.push(format!("LLM error: {err}"));
    }
    if let Some(value) = &status.generation_state {
        parts.push(format!("Generation: {value}"));
    }
    if let Some(err) = &status.last_generation_error {
        parts.push(format!("Generation error: {err}"));
    }
    if let Some(err) = &status.last_error {
        parts.push(format!("Error: {err}"));
    }
    if let Some(app) = &status.active_app {
        parts.push(format!(
            "App: {:?}{}",
            app.app_class,
            app.app_id
                .as_ref()
                .map(|id| format!(" ({id})"))
                .unwrap_or_default()
        ));
    }
    if let Some(value) = &status.last_resolution {
        parts.push(format!("Resolution: {value}"));
    }
    if let Some(value) = status.accessibility_permission {
        parts.push(format!("Accessibility permission: {value}"));
    }
    if let Some(value) = status.input_monitoring_permission {
        parts.push(format!("Input Monitoring permission: {value}"));
    }
    if let Some(value) = status.microphone_permission {
        parts.push(format!("Microphone permission: {value}"));
    }

    parts.join(" | ")
}
