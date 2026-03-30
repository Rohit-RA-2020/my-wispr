use crate::{
    config::IntelligenceConfig,
    error::{Result, WisprError},
    models::{
        ActionCommand, ActionKey, ActionType, ActiveAppClass, ActiveAppContext, ModifierKey,
        SemanticCommandId, ShortcutDenylistProfile,
    },
};

pub struct ResolvedActions {
    pub actions: Vec<ActionCommand>,
    pub description: Option<String>,
}

pub fn resolve_actions(
    config: &IntelligenceConfig,
    actions: &[ActionCommand],
    active_app: Option<&ActiveAppContext>,
) -> Result<ResolvedActions> {
    let mut resolved = Vec::new();
    let mut descriptions = Vec::new();

    for action in actions {
        let expanded = match action.action_type {
            ActionType::SemanticCommand => {
                if !config.semantic_commands_enabled {
                    return Err(WisprError::InvalidState(
                        "semantic commands are disabled by policy".to_string(),
                    ));
                }
                resolve_semantic_command(action, active_app)?
            }
            ActionType::Key | ActionType::Shortcut => vec![ActionCommand {
                action_type: action.action_type.clone(),
                key: action.key.clone(),
                modifiers: action.modifiers.clone(),
                repeat: action.repeat,
                command_id: None,
                target_app: None,
            }],
        };

        for expanded_action in expanded {
            validate_policy(config, &expanded_action)?;
            descriptions.push(describe_action(&expanded_action));
            resolved.push(expanded_action);
        }
    }

    Ok(ResolvedActions {
        actions: resolved,
        description: if descriptions.is_empty() {
            None
        } else {
            Some(descriptions.join(", "))
        },
    })
}

fn resolve_semantic_command(
    action: &ActionCommand,
    active_app: Option<&ActiveAppContext>,
) -> Result<Vec<ActionCommand>> {
    let command = action.command_id.clone().ok_or_else(|| {
        WisprError::InvalidState("semantic command is missing command_id".to_string())
    })?;
    let target = action
        .target_app
        .clone()
        .or_else(|| active_app.map(|app| app.app_class.clone()))
        .unwrap_or(ActiveAppClass::Generic);

    let repeat = action.repeat.max(1);
    let resolved = match command {
        SemanticCommandId::NewTab => shortcut(
            letter_key_for(target.clone(), ActionKey::T),
            &[ModifierKey::Ctrl],
            repeat,
        ),
        SemanticCommandId::CloseTab => shortcut(
            letter_key_for(target.clone(), ActionKey::W),
            &[ModifierKey::Ctrl],
            repeat,
        ),
        SemanticCommandId::ReopenClosedTab => shortcut(
            ActionKey::T,
            &[ModifierKey::Ctrl, ModifierKey::Shift],
            repeat,
        ),
        SemanticCommandId::Refresh => shortcut(ActionKey::R, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::Find => shortcut(ActionKey::F, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::Save => shortcut(ActionKey::S, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::Copy => shortcut(ActionKey::C, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::Paste => shortcut(ActionKey::V, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::Cut => shortcut(ActionKey::X, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::Undo => shortcut(ActionKey::Z, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::Redo => shortcut(
            ActionKey::Z,
            &[ModifierKey::Ctrl, ModifierKey::Shift],
            repeat,
        ),
        SemanticCommandId::FocusAddressBar => shortcut(ActionKey::L, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::NextTab => shortcut(ActionKey::PageDown, &[ModifierKey::Ctrl], repeat),
        SemanticCommandId::PreviousTab => shortcut(ActionKey::PageUp, &[ModifierKey::Ctrl], repeat),
    };

    Ok(vec![resolved])
}

fn letter_key_for(_target: ActiveAppClass, default: ActionKey) -> ActionKey {
    default
}

fn shortcut(key: ActionKey, modifiers: &[ModifierKey], repeat: u8) -> ActionCommand {
    ActionCommand {
        action_type: ActionType::Shortcut,
        key: Some(key),
        modifiers: modifiers.to_vec(),
        repeat,
        command_id: None,
        target_app: None,
    }
}

fn validate_policy(config: &IntelligenceConfig, action: &ActionCommand) -> Result<()> {
    match action.action_type {
        ActionType::Key => Ok(()),
        ActionType::Shortcut => validate_shortcut_policy(config, action),
        ActionType::SemanticCommand => Err(WisprError::InvalidState(
            "unresolved semantic command reached keyboard policy".to_string(),
        )),
    }
}

fn validate_shortcut_policy(config: &IntelligenceConfig, action: &ActionCommand) -> Result<()> {
    if !config.dynamic_shortcuts_enabled {
        return Err(WisprError::InvalidState(
            "dynamic shortcuts are disabled by policy".to_string(),
        ));
    }

    let combo = combo_id(action)?;
    let normalized_combo = normalize_combo_text(&combo);
    let allowlist = parse_combo_list(&config.shortcut_allowlist);
    let denylist = parse_combo_list(&config.shortcut_denylist);

    if !allowlist.is_empty() && !allowlist.contains(&normalized_combo) {
        return Err(WisprError::InvalidState(format!(
            "shortcut {combo} is not in the configured allowlist"
        )));
    }

    if denylist.contains(&normalized_combo) {
        return Err(WisprError::InvalidState(format!(
            "shortcut {combo} is blocked by the configured denylist"
        )));
    }

    if builtin_denylist(config.shortcut_denylist_profile.clone()).contains(&normalized_combo) {
        return Err(WisprError::InvalidState(format!(
            "shortcut {combo} is blocked by the built-in safety policy"
        )));
    }

    Ok(())
}

fn builtin_denylist(profile: ShortcutDenylistProfile) -> Vec<String> {
    match profile {
        ShortcutDenylistProfile::Minimal => vec![
            normalize_combo_text("Ctrl+Alt+Delete"),
            normalize_combo_text("Super+L"),
        ],
    }
}

fn parse_combo_list(items: &[String]) -> Vec<String> {
    items
        .iter()
        .flat_map(|item| item.split([',', '\n']).map(str::trim).collect::<Vec<_>>())
        .filter(|item| !item.is_empty())
        .map(normalize_combo_text)
        .collect()
}

fn normalize_combo_text(input: &str) -> String {
    input
        .split('+')
        .map(|part| part.trim().to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("+")
}

fn combo_id(action: &ActionCommand) -> Result<String> {
    let key = action
        .key
        .as_ref()
        .ok_or_else(|| WisprError::InvalidState("shortcut is missing a primary key".to_string()))?;
    let mut parts = action
        .modifiers
        .iter()
        .map(|modifier| modifier.as_label().to_string())
        .collect::<Vec<_>>();
    parts.push(key_label(key).to_string());
    Ok(parts.join("+"))
}

fn describe_action(action: &ActionCommand) -> String {
    match action.action_type {
        ActionType::Key => action
            .key
            .as_ref()
            .map(key_label)
            .unwrap_or("Unknown")
            .to_string(),
        ActionType::Shortcut => combo_id(action).unwrap_or_else(|_| "unknown shortcut".to_string()),
        ActionType::SemanticCommand => action
            .command_id
            .as_ref()
            .map(|command| format!("{command:?}"))
            .unwrap_or_else(|| "unknown semantic command".to_string()),
    }
}

fn key_label(key: &ActionKey) -> &'static str {
    match key {
        ActionKey::Space => "Space",
        ActionKey::Enter => "Enter",
        ActionKey::Tab => "Tab",
        ActionKey::Escape => "Escape",
        ActionKey::Backspace => "Backspace",
        ActionKey::Delete => "Delete",
        ActionKey::Insert => "Insert",
        ActionKey::Left => "Left",
        ActionKey::Right => "Right",
        ActionKey::Up => "Up",
        ActionKey::Down => "Down",
        ActionKey::Home => "Home",
        ActionKey::End => "End",
        ActionKey::PageUp => "PageUp",
        ActionKey::PageDown => "PageDown",
        ActionKey::A => "A",
        ActionKey::B => "B",
        ActionKey::C => "C",
        ActionKey::D => "D",
        ActionKey::E => "E",
        ActionKey::F => "F",
        ActionKey::G => "G",
        ActionKey::H => "H",
        ActionKey::I => "I",
        ActionKey::J => "J",
        ActionKey::K => "K",
        ActionKey::L => "L",
        ActionKey::M => "M",
        ActionKey::N => "N",
        ActionKey::O => "O",
        ActionKey::P => "P",
        ActionKey::Q => "Q",
        ActionKey::R => "R",
        ActionKey::S => "S",
        ActionKey::T => "T",
        ActionKey::U => "U",
        ActionKey::V => "V",
        ActionKey::W => "W",
        ActionKey::X => "X",
        ActionKey::Y => "Y",
        ActionKey::Z => "Z",
        ActionKey::Digit0 => "0",
        ActionKey::Digit1 => "1",
        ActionKey::Digit2 => "2",
        ActionKey::Digit3 => "3",
        ActionKey::Digit4 => "4",
        ActionKey::Digit5 => "5",
        ActionKey::Digit6 => "6",
        ActionKey::Digit7 => "7",
        ActionKey::Digit8 => "8",
        ActionKey::Digit9 => "9",
        ActionKey::F1 => "F1",
        ActionKey::F2 => "F2",
        ActionKey::F3 => "F3",
        ActionKey::F4 => "F4",
        ActionKey::F5 => "F5",
        ActionKey::F6 => "F6",
        ActionKey::F7 => "F7",
        ActionKey::F8 => "F8",
        ActionKey::F9 => "F9",
        ActionKey::F10 => "F10",
        ActionKey::F11 => "F11",
        ActionKey::F12 => "F12",
    }
}
