use std::{collections::HashMap, io, thread, time::Duration};

use evdev::{AttributeSet, EventType, InputEvent, KeyCode, KeyEvent, uinput::VirtualDevice};

use crate::{
    error::{Result, WisprError},
    models::{ActionCommand, ActionKey, ModifierKey},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPatch {
    pub backspaces: usize,
    pub insertion: String,
}

pub fn diff_patch(previous: &str, next: &str) -> TextPatch {
    let mut common_prefix_bytes = 0usize;

    for (left, right) in previous.chars().zip(next.chars()) {
        if left != right {
            break;
        }
        common_prefix_bytes += left.len_utf8();
    }

    let previous_suffix = &previous[common_prefix_bytes..];
    let next_suffix = &next[common_prefix_bytes..];

    TextPatch {
        backspaces: previous_suffix.chars().count(),
        insertion: next_suffix.to_string(),
    }
}

pub struct UInputKeyboard {
    device: VirtualDevice,
    keymap: HashMap<char, KeyStroke>,
}

#[derive(Debug, Clone, Copy)]
struct KeyStroke {
    key: KeyCode,
    shift: bool,
}

impl UInputKeyboard {
    pub fn open() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in supported_keys() {
            keys.insert(key);
        }

        let device = VirtualDevice::builder()
            .map_err(map_io)?
            .name("Wispr Virtual Keyboard")
            .with_keys(&keys)
            .map_err(map_io)?
            .build()
            .map_err(map_io)?;

        Ok(Self {
            device,
            keymap: build_keymap(),
        })
    }

    pub fn emit_patch(&mut self, patch: &TextPatch) -> Result<()> {
        for _ in 0..patch.backspaces {
            self.tap(KeyCode::KEY_BACKSPACE, false)?;
        }
        for ch in patch.insertion.chars() {
            self.type_char(ch)?;
        }
        Ok(())
    }

    pub fn emit_actions(&mut self, actions: &[ActionCommand]) -> Result<()> {
        for action in actions {
            self.emit_action(action)?;
        }
        Ok(())
    }

    pub fn sync_delay(&self) {
        thread::sleep(Duration::from_millis(6));
    }

    fn emit_action(&mut self, action: &ActionCommand) -> Result<()> {
        let key = map_action_key(action.key.as_ref().ok_or_else(|| {
            WisprError::InvalidState("resolved action is missing a primary key".to_string())
        })?);
        let repeat = action.repeat.max(1);

        for _ in 0..repeat {
            for modifier in modifier_press_order(&action.modifiers) {
                self.emit_key(modifier, 1)?;
            }

            self.emit_key(key, 1)?;
            self.emit_key(key, 0)?;

            for modifier in modifier_release_order(&action.modifiers) {
                self.emit_key(modifier, 0)?;
            }

            self.sync_delay();
        }
        Ok(())
    }

    fn type_char(&mut self, ch: char) -> Result<()> {
        let stroke = self.keymap.get(&ch).copied().ok_or_else(|| {
            WisprError::InvalidState(format!("unsupported typed character: {ch:?}"))
        })?;
        self.tap(stroke.key, stroke.shift)
    }

    fn tap(&mut self, key: KeyCode, shift: bool) -> Result<()> {
        if shift {
            self.emit_key(KeyCode::KEY_LEFTSHIFT, 1)?;
        }

        self.emit_key(key, 1)?;
        self.emit_key(key, 0)?;

        if shift {
            self.emit_key(KeyCode::KEY_LEFTSHIFT, 0)?;
        }

        self.sync_delay();
        Ok(())
    }

    fn emit_key(&mut self, key: KeyCode, value: i32) -> Result<()> {
        let event = if value == 1 {
            *KeyEvent::new(key, 1)
        } else {
            InputEvent::new(EventType::KEY.0, key.code(), 0)
        };
        self.device.emit(&[event]).map_err(map_io)?;
        Ok(())
    }
}

fn map_action_key(key: &ActionKey) -> KeyCode {
    match key {
        ActionKey::Space => KeyCode::KEY_SPACE,
        ActionKey::Enter => KeyCode::KEY_ENTER,
        ActionKey::Tab => KeyCode::KEY_TAB,
        ActionKey::Escape => KeyCode::KEY_ESC,
        ActionKey::Backspace => KeyCode::KEY_BACKSPACE,
        ActionKey::Delete => KeyCode::KEY_DELETE,
        ActionKey::Insert => KeyCode::KEY_INSERT,
        ActionKey::Left => KeyCode::KEY_LEFT,
        ActionKey::Right => KeyCode::KEY_RIGHT,
        ActionKey::Up => KeyCode::KEY_UP,
        ActionKey::Down => KeyCode::KEY_DOWN,
        ActionKey::Home => KeyCode::KEY_HOME,
        ActionKey::End => KeyCode::KEY_END,
        ActionKey::PageUp => KeyCode::KEY_PAGEUP,
        ActionKey::PageDown => KeyCode::KEY_PAGEDOWN,
        ActionKey::A => KeyCode::KEY_A,
        ActionKey::B => KeyCode::KEY_B,
        ActionKey::C => KeyCode::KEY_C,
        ActionKey::D => KeyCode::KEY_D,
        ActionKey::E => KeyCode::KEY_E,
        ActionKey::F => KeyCode::KEY_F,
        ActionKey::G => KeyCode::KEY_G,
        ActionKey::H => KeyCode::KEY_H,
        ActionKey::I => KeyCode::KEY_I,
        ActionKey::J => KeyCode::KEY_J,
        ActionKey::K => KeyCode::KEY_K,
        ActionKey::L => KeyCode::KEY_L,
        ActionKey::M => KeyCode::KEY_M,
        ActionKey::N => KeyCode::KEY_N,
        ActionKey::O => KeyCode::KEY_O,
        ActionKey::P => KeyCode::KEY_P,
        ActionKey::Q => KeyCode::KEY_Q,
        ActionKey::R => KeyCode::KEY_R,
        ActionKey::S => KeyCode::KEY_S,
        ActionKey::T => KeyCode::KEY_T,
        ActionKey::U => KeyCode::KEY_U,
        ActionKey::V => KeyCode::KEY_V,
        ActionKey::W => KeyCode::KEY_W,
        ActionKey::X => KeyCode::KEY_X,
        ActionKey::Y => KeyCode::KEY_Y,
        ActionKey::Z => KeyCode::KEY_Z,
        ActionKey::Digit0 => KeyCode::KEY_0,
        ActionKey::Digit1 => KeyCode::KEY_1,
        ActionKey::Digit2 => KeyCode::KEY_2,
        ActionKey::Digit3 => KeyCode::KEY_3,
        ActionKey::Digit4 => KeyCode::KEY_4,
        ActionKey::Digit5 => KeyCode::KEY_5,
        ActionKey::Digit6 => KeyCode::KEY_6,
        ActionKey::Digit7 => KeyCode::KEY_7,
        ActionKey::Digit8 => KeyCode::KEY_8,
        ActionKey::Digit9 => KeyCode::KEY_9,
        ActionKey::F1 => KeyCode::KEY_F1,
        ActionKey::F2 => KeyCode::KEY_F2,
        ActionKey::F3 => KeyCode::KEY_F3,
        ActionKey::F4 => KeyCode::KEY_F4,
        ActionKey::F5 => KeyCode::KEY_F5,
        ActionKey::F6 => KeyCode::KEY_F6,
        ActionKey::F7 => KeyCode::KEY_F7,
        ActionKey::F8 => KeyCode::KEY_F8,
        ActionKey::F9 => KeyCode::KEY_F9,
        ActionKey::F10 => KeyCode::KEY_F10,
        ActionKey::F11 => KeyCode::KEY_F11,
        ActionKey::F12 => KeyCode::KEY_F12,
    }
}

fn map_io(error: io::Error) -> WisprError {
    WisprError::Io(error)
}

fn modifier_press_order(modifiers: &[ModifierKey]) -> Vec<KeyCode> {
    let mut ordered = Vec::new();
    for modifier in [
        ModifierKey::Ctrl,
        ModifierKey::Shift,
        ModifierKey::Alt,
        ModifierKey::Super,
    ] {
        if modifiers.contains(&modifier) {
            ordered.push(map_modifier_key(&modifier));
        }
    }
    ordered
}

fn modifier_release_order(modifiers: &[ModifierKey]) -> Vec<KeyCode> {
    let mut ordered = modifier_press_order(modifiers);
    ordered.reverse();
    ordered
}

fn map_modifier_key(key: &ModifierKey) -> KeyCode {
    match key {
        ModifierKey::Ctrl => KeyCode::KEY_LEFTCTRL,
        ModifierKey::Shift => KeyCode::KEY_LEFTSHIFT,
        ModifierKey::Alt => KeyCode::KEY_LEFTALT,
        ModifierKey::Super => KeyCode::KEY_LEFTMETA,
    }
}

fn supported_keys() -> Vec<KeyCode> {
    vec![
        KeyCode::KEY_A,
        KeyCode::KEY_B,
        KeyCode::KEY_C,
        KeyCode::KEY_D,
        KeyCode::KEY_E,
        KeyCode::KEY_F,
        KeyCode::KEY_G,
        KeyCode::KEY_H,
        KeyCode::KEY_I,
        KeyCode::KEY_J,
        KeyCode::KEY_K,
        KeyCode::KEY_L,
        KeyCode::KEY_M,
        KeyCode::KEY_N,
        KeyCode::KEY_O,
        KeyCode::KEY_P,
        KeyCode::KEY_Q,
        KeyCode::KEY_R,
        KeyCode::KEY_S,
        KeyCode::KEY_T,
        KeyCode::KEY_U,
        KeyCode::KEY_V,
        KeyCode::KEY_W,
        KeyCode::KEY_X,
        KeyCode::KEY_Y,
        KeyCode::KEY_Z,
        KeyCode::KEY_1,
        KeyCode::KEY_2,
        KeyCode::KEY_3,
        KeyCode::KEY_4,
        KeyCode::KEY_5,
        KeyCode::KEY_6,
        KeyCode::KEY_7,
        KeyCode::KEY_8,
        KeyCode::KEY_9,
        KeyCode::KEY_0,
        KeyCode::KEY_SPACE,
        KeyCode::KEY_F1,
        KeyCode::KEY_F2,
        KeyCode::KEY_F3,
        KeyCode::KEY_F4,
        KeyCode::KEY_F5,
        KeyCode::KEY_F6,
        KeyCode::KEY_F7,
        KeyCode::KEY_F8,
        KeyCode::KEY_F9,
        KeyCode::KEY_F10,
        KeyCode::KEY_F11,
        KeyCode::KEY_F12,
        KeyCode::KEY_DOT,
        KeyCode::KEY_COMMA,
        KeyCode::KEY_APOSTROPHE,
        KeyCode::KEY_SLASH,
        KeyCode::KEY_MINUS,
        KeyCode::KEY_EQUAL,
        KeyCode::KEY_SEMICOLON,
        KeyCode::KEY_BACKSPACE,
        KeyCode::KEY_ENTER,
        KeyCode::KEY_TAB,
        KeyCode::KEY_ESC,
        KeyCode::KEY_DELETE,
        KeyCode::KEY_INSERT,
        KeyCode::KEY_LEFT,
        KeyCode::KEY_RIGHT,
        KeyCode::KEY_UP,
        KeyCode::KEY_DOWN,
        KeyCode::KEY_HOME,
        KeyCode::KEY_END,
        KeyCode::KEY_PAGEUP,
        KeyCode::KEY_PAGEDOWN,
        KeyCode::KEY_LEFTSHIFT,
        KeyCode::KEY_LEFTCTRL,
        KeyCode::KEY_LEFTALT,
        KeyCode::KEY_LEFTMETA,
    ]
}

fn build_keymap() -> HashMap<char, KeyStroke> {
    let mut map = HashMap::new();
    let letters = [
        ('a', KeyCode::KEY_A),
        ('b', KeyCode::KEY_B),
        ('c', KeyCode::KEY_C),
        ('d', KeyCode::KEY_D),
        ('e', KeyCode::KEY_E),
        ('f', KeyCode::KEY_F),
        ('g', KeyCode::KEY_G),
        ('h', KeyCode::KEY_H),
        ('i', KeyCode::KEY_I),
        ('j', KeyCode::KEY_J),
        ('k', KeyCode::KEY_K),
        ('l', KeyCode::KEY_L),
        ('m', KeyCode::KEY_M),
        ('n', KeyCode::KEY_N),
        ('o', KeyCode::KEY_O),
        ('p', KeyCode::KEY_P),
        ('q', KeyCode::KEY_Q),
        ('r', KeyCode::KEY_R),
        ('s', KeyCode::KEY_S),
        ('t', KeyCode::KEY_T),
        ('u', KeyCode::KEY_U),
        ('v', KeyCode::KEY_V),
        ('w', KeyCode::KEY_W),
        ('x', KeyCode::KEY_X),
        ('y', KeyCode::KEY_Y),
        ('z', KeyCode::KEY_Z),
    ];

    for (ch, code) in letters {
        map.insert(
            ch,
            KeyStroke {
                key: code,
                shift: false,
            },
        );
        map.insert(
            ch.to_ascii_uppercase(),
            KeyStroke {
                key: code,
                shift: true,
            },
        );
    }

    for (ch, code) in [
        ('1', KeyCode::KEY_1),
        ('2', KeyCode::KEY_2),
        ('3', KeyCode::KEY_3),
        ('4', KeyCode::KEY_4),
        ('5', KeyCode::KEY_5),
        ('6', KeyCode::KEY_6),
        ('7', KeyCode::KEY_7),
        ('8', KeyCode::KEY_8),
        ('9', KeyCode::KEY_9),
        ('0', KeyCode::KEY_0),
        (' ', KeyCode::KEY_SPACE),
        ('.', KeyCode::KEY_DOT),
        (',', KeyCode::KEY_COMMA),
        ('\'', KeyCode::KEY_APOSTROPHE),
        ('/', KeyCode::KEY_SLASH),
        ('-', KeyCode::KEY_MINUS),
        ('=', KeyCode::KEY_EQUAL),
        (';', KeyCode::KEY_SEMICOLON),
        ('\n', KeyCode::KEY_ENTER),
        ('\r', KeyCode::KEY_ENTER),
        ('\t', KeyCode::KEY_TAB),
    ] {
        map.insert(
            ch,
            KeyStroke {
                key: code,
                shift: false,
            },
        );
    }

    for (ch, code) in [
        ('!', KeyCode::KEY_1),
        ('@', KeyCode::KEY_2),
        ('#', KeyCode::KEY_3),
        ('$', KeyCode::KEY_4),
        ('%', KeyCode::KEY_5),
        ('^', KeyCode::KEY_6),
        ('&', KeyCode::KEY_7),
        ('*', KeyCode::KEY_8),
        ('(', KeyCode::KEY_9),
        (')', KeyCode::KEY_0),
        ('?', KeyCode::KEY_SLASH),
        ('_', KeyCode::KEY_MINUS),
        ('+', KeyCode::KEY_EQUAL),
        (':', KeyCode::KEY_SEMICOLON),
        ('"', KeyCode::KEY_APOSTROPHE),
    ] {
        map.insert(
            ch,
            KeyStroke {
                key: code,
                shift: true,
            },
        );
    }

    map
}
