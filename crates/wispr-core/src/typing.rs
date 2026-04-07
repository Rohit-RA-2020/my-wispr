use std::{thread, time::Duration};

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

#[cfg(target_os = "linux")]
mod platform_impl {
    use std::{collections::HashMap, io};

    use evdev::{AttributeSet, EventType, InputEvent, KeyCode, KeyEvent, uinput::VirtualDevice};

    use super::*;

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
}

#[cfg(target_os = "macos")]
mod platform_impl {
    use super::*;

    use core_graphics::event::{CGEvent, CGEventFlags, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    struct SendableSource(CGEventSource);
    // SAFETY: CGEventSource is a CFType (reference-counted, thread-safe).
    unsafe impl Send for SendableSource {}

    pub struct UInputKeyboard {
        source: SendableSource,
    }

    impl UInputKeyboard {
        pub fn open() -> Result<Self> {
            let source =
                CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| {
                    WisprError::InvalidState(
                        "failed to create CGEventSource — is Accessibility granted for this process?"
                            .to_string(),
                    )
                })?;
            Ok(Self {
                source: SendableSource(source),
            })
        }

        pub fn emit_patch(&mut self, patch: &TextPatch) -> Result<()> {
            for _ in 0..patch.backspaces {
                self.tap_key(51, CGEventFlags::empty())?;
                self.sync_delay();
            }

            for ch in patch.insertion.chars() {
                match ch {
                    '\n' | '\r' => {
                        self.tap_key(36, CGEventFlags::empty())?;
                    }
                    '\t' => {
                        self.tap_key(48, CGEventFlags::empty())?;
                    }
                    _ => {
                        self.type_unicode_char(ch)?;
                    }
                }
                self.sync_delay();
            }
            Ok(())
        }

        pub fn emit_actions(&mut self, actions: &[ActionCommand]) -> Result<()> {
            for action in actions {
                let keycode = map_action_keycode(action.key.as_ref().ok_or_else(|| {
                    WisprError::InvalidState("resolved action is missing a primary key".to_string())
                })?);

                let flags = build_modifier_flags(&action.modifiers);

                for _ in 0..action.repeat.max(1) {
                    self.tap_key(keycode, flags)?;
                    self.sync_delay();
                }
            }
            Ok(())
        }

        pub fn sync_delay(&self) {
            thread::sleep(Duration::from_millis(6));
        }

        fn tap_key(&self, keycode: CGKeyCode, flags: CGEventFlags) -> Result<()> {
            let down = CGEvent::new_keyboard_event(self.source.0.clone(), keycode, true)
                .map_err(|_| {
                    WisprError::InvalidState("failed to create CGEvent key-down".to_string())
                })?;
            let up = CGEvent::new_keyboard_event(self.source.0.clone(), keycode, false)
                .map_err(|_| {
                    WisprError::InvalidState("failed to create CGEvent key-up".to_string())
                })?;

            if !flags.is_empty() {
                down.set_flags(flags);
                up.set_flags(flags);
            }

            down.post(core_graphics::event::CGEventTapLocation::HID);
            up.post(core_graphics::event::CGEventTapLocation::HID);
            Ok(())
        }

        fn type_unicode_char(&self, ch: char) -> Result<()> {
            let down =
                CGEvent::new_keyboard_event(self.source.0.clone(), 0, true).map_err(|_| {
                    WisprError::InvalidState(
                        "failed to create CGEvent for unicode char".to_string(),
                    )
                })?;
            let up =
                CGEvent::new_keyboard_event(self.source.0.clone(), 0, false).map_err(|_| {
                    WisprError::InvalidState(
                        "failed to create CGEvent for unicode char".to_string(),
                    )
                })?;

            let mut buf = [0u16; 2];
            let units: &[u16] = ch.encode_utf16(&mut buf);
            down.set_string_from_utf16_unchecked(units);
            up.set_string_from_utf16_unchecked(units);

            down.post(core_graphics::event::CGEventTapLocation::HID);
            up.post(core_graphics::event::CGEventTapLocation::HID);
            Ok(())
        }
    }

    fn build_modifier_flags(modifiers: &[ModifierKey]) -> CGEventFlags {
        let mut flags = CGEventFlags::empty();
        for m in modifiers {
            flags |= match m {
                ModifierKey::Ctrl => CGEventFlags::CGEventFlagControl,
                ModifierKey::Shift => CGEventFlags::CGEventFlagShift,
                ModifierKey::Alt => CGEventFlags::CGEventFlagAlternate,
                ModifierKey::Super => CGEventFlags::CGEventFlagCommand,
            };
        }
        flags
    }

    fn map_action_keycode(key: &ActionKey) -> CGKeyCode {
        match key {
            ActionKey::Space => 49,
            ActionKey::Enter => 36,
            ActionKey::Tab => 48,
            ActionKey::Escape => 53,
            ActionKey::Backspace => 51,
            ActionKey::Delete => 117,
            ActionKey::Insert => 114,
            ActionKey::Left => 123,
            ActionKey::Right => 124,
            ActionKey::Up => 126,
            ActionKey::Down => 125,
            ActionKey::Home => 115,
            ActionKey::End => 119,
            ActionKey::PageUp => 116,
            ActionKey::PageDown => 121,
            ActionKey::A => 0,
            ActionKey::B => 11,
            ActionKey::C => 8,
            ActionKey::D => 2,
            ActionKey::E => 14,
            ActionKey::F => 3,
            ActionKey::G => 5,
            ActionKey::H => 4,
            ActionKey::I => 34,
            ActionKey::J => 38,
            ActionKey::K => 40,
            ActionKey::L => 37,
            ActionKey::M => 46,
            ActionKey::N => 45,
            ActionKey::O => 31,
            ActionKey::P => 35,
            ActionKey::Q => 12,
            ActionKey::R => 15,
            ActionKey::S => 1,
            ActionKey::T => 17,
            ActionKey::U => 32,
            ActionKey::V => 9,
            ActionKey::W => 13,
            ActionKey::X => 7,
            ActionKey::Y => 16,
            ActionKey::Z => 6,
            ActionKey::Digit0 => 29,
            ActionKey::Digit1 => 18,
            ActionKey::Digit2 => 19,
            ActionKey::Digit3 => 20,
            ActionKey::Digit4 => 21,
            ActionKey::Digit5 => 23,
            ActionKey::Digit6 => 22,
            ActionKey::Digit7 => 26,
            ActionKey::Digit8 => 28,
            ActionKey::Digit9 => 25,
            ActionKey::F1 => 122,
            ActionKey::F2 => 120,
            ActionKey::F3 => 99,
            ActionKey::F4 => 118,
            ActionKey::F5 => 96,
            ActionKey::F6 => 97,
            ActionKey::F7 => 98,
            ActionKey::F8 => 100,
            ActionKey::F9 => 101,
            ActionKey::F10 => 109,
            ActionKey::F11 => 103,
            ActionKey::F12 => 111,
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform_impl {
    use super::*;

    pub struct UInputKeyboard;

    impl UInputKeyboard {
        pub fn open() -> Result<Self> {
            Err(WisprError::InvalidState(
                "typing backend is not implemented for this operating system".to_string(),
            ))
        }

        pub fn emit_patch(&mut self, _patch: &TextPatch) -> Result<()> {
            Err(WisprError::InvalidState(
                "typing backend is not implemented for this operating system".to_string(),
            ))
        }

        pub fn emit_actions(&mut self, _actions: &[ActionCommand]) -> Result<()> {
            Err(WisprError::InvalidState(
                "typing backend is not implemented for this operating system".to_string(),
            ))
        }

        pub fn sync_delay(&self) {
            thread::sleep(Duration::from_millis(6));
        }
    }
}

pub use platform_impl::UInputKeyboard;
