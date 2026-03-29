use std::{collections::HashMap, io, thread, time::Duration};

use evdev::{AttributeSet, EventType, InputEvent, KeyCode, KeyEvent, uinput::VirtualDevice};

use crate::error::{Result, WisprError};

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

    pub fn sync_delay(&self) {
        thread::sleep(Duration::from_millis(6));
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

fn map_io(error: io::Error) -> WisprError {
    WisprError::Io(error)
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
        KeyCode::KEY_DOT,
        KeyCode::KEY_COMMA,
        KeyCode::KEY_APOSTROPHE,
        KeyCode::KEY_SLASH,
        KeyCode::KEY_MINUS,
        KeyCode::KEY_EQUAL,
        KeyCode::KEY_SEMICOLON,
        KeyCode::KEY_BACKSPACE,
        KeyCode::KEY_LEFTSHIFT,
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

#[cfg(test)]
mod tests {
    use super::diff_patch;

    #[test]
    fn patch_keeps_common_prefix() {
        let patch = diff_patch("hello wor", "hello world");
        assert_eq!(patch.backspaces, 0);
        assert_eq!(patch.insertion, "ld");
    }

    #[test]
    fn patch_replaces_changed_suffix() {
        let patch = diff_patch("hello ward", "hello world");
        assert_eq!(patch.backspaces, 3);
        assert_eq!(patch.insertion, "rld");
    }
}
