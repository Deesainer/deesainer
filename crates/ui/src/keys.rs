use egui::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub key: Key,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

pub fn key_from_token(token: &str) -> Option<Key> {
    match token {
        "[" => Some(Key::OpenBracket),
        "]" => Some(Key::CloseBracket),
        "Delete" => Some(Key::Delete),
        "Backspace" => Some(Key::Backspace),
        "Escape" => Some(Key::Escape),
        "Enter" => Some(Key::Enter),
        "+" | "=" => Some(Key::Equals),
        "-" => Some(Key::Minus),
        "0" => Some(Key::Num0),
        "1" => Some(Key::Num1),
        "2" => Some(Key::Num2),
        other => Key::from_name(other),
    }
}

pub fn parse(s: &str) -> Option<Shortcut> {
    let mut sc = Shortcut {
        key: Key::Escape,
        command: false,
        shift: false,
        alt: false,
        ctrl: false,
    };
    let mut key = None;
    for part in s.split('+') {
        match part {
            "Cmd" => sc.command = true,
            "Shift" => sc.shift = true,
            "Alt" => sc.alt = true,
            "Ctrl" => sc.ctrl = true,
            "" => key = Some(Key::Equals),
            token => key = key_from_token(token),
        }
    }
    sc.key = key?;
    Some(sc)
}

impl Shortcut {
    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        let ctrl_only = m.ctrl && !m.mac_cmd && cfg!(target_os = "macos");
        self.key == key
            && self.command == m.command
            && self.shift == m.shift
            && self.alt == m.alt
            && self.ctrl == ctrl_only
    }

    pub fn label(&self) -> String {
        let mac = cfg!(target_os = "macos");
        let mut s = String::new();
        if self.ctrl {
            s.push_str(if mac { "⌃" } else { "Ctrl+" });
        }
        if self.alt {
            s.push_str(if mac { "⌥" } else { "Alt+" });
        }
        if self.shift {
            s.push_str(if mac { "⇧" } else { "Shift+" });
        }
        if self.command {
            s.push_str(if mac { "⌘" } else { "Ctrl+" });
        }
        s.push_str(match self.key {
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::Equals => "+",
            Key::Minus => "-",
            Key::Num0 => "0",
            Key::Num1 => "1",
            Key::Num2 => "2",
            k => k.name(),
        });
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_registry_shortcuts() {
        let s = parse("Shift+Cmd+Z").unwrap();
        assert!(s.shift && s.command && s.key == Key::Z);
        assert_eq!(parse("Alt+Cmd+]").unwrap().key, Key::CloseBracket);
        assert_eq!(parse("R").unwrap().key, Key::R);
        assert_eq!(parse("Delete").unwrap().key, Key::Delete);
    }
}
