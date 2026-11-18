//! Hotkey: parse/format names like "Num1", "Ctrl+Alt+H", "F9"; stored in hotkey.txt next to the exe.

use std::time::SystemTime;

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CTRL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hotkey {
    pub mods: u32,
    pub vk: u32,
}

pub const DEFAULT: Hotkey = Hotkey { mods: 0, vk: 0x61 }; // Num1

fn key_names() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = Vec::new();
    for i in 0..10u32 {
        v.push((format!("Num{i}"), 0x60 + i));
    }
    for (n, k) in [("NumMul", 0x6A), ("NumPlus", 0x6B), ("NumMinus", 0x6D), ("NumDot", 0x6E), ("NumDiv", 0x6F)] {
        v.push((n.into(), k));
    }
    for i in 1..=24u32 {
        v.push((format!("F{i}"), 0x6F + i));
    }
    for c in b'A'..=b'Z' {
        v.push(((c as char).to_string(), c as u32));
    }
    for c in b'0'..=b'9' {
        v.push(((c as char).to_string(), c as u32));
    }
    for (n, k) in [
        ("Space", 0x20), ("Enter", 0x0D), ("Tab", 0x09), ("Esc", 0x1B), ("Backspace", 0x08),
        ("Insert", 0x2D), ("Delete", 0x2E), ("Home", 0x24), ("End", 0x23), ("PageUp", 0x21), ("PageDown", 0x22),
        ("Left", 0x25), ("Up", 0x26), ("Right", 0x27), ("Down", 0x28),
        ("Pause", 0x13), ("ScrollLock", 0x91), ("PrintScreen", 0x2C), ("Menu", 0x5D),
        ("VolumeMute", 0xAD), ("VolumeDown", 0xAE), ("VolumeUp", 0xAF),
        ("MediaNext", 0xB0), ("MediaPrev", 0xB1), ("MediaStop", 0xB2), ("MediaPlay", 0xB3),
        ("`", 0xC0), ("-", 0xBD), ("=", 0xBB), ("[", 0xDB), ("]", 0xDD), ("\\", 0xDC),
        (";", 0xBA), ("'", 0xDE), (",", 0xBC), (".", 0xBE), ("/", 0xBF),
    ] {
        v.push((n.into(), k));
    }
    v
}

pub fn key_name(vk: u32) -> Option<String> {
    key_names().into_iter().find(|(_, k)| *k == vk).map(|(n, _)| n)
}

pub fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

pub fn parse(text: &str) -> Option<Hotkey> {
    let mut mods = 0;
    let mut vk = None;
    for part in text.split('+') {
        let p: String = part.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
        if p.is_empty() {
            // "Ctrl++" -> the plus key itself
            continue;
        }
        match p.as_str() {
            "ctrl" | "control" | "ctl" => mods |= MOD_CTRL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "windows" | "super" => mods |= MOD_WIN,
            _ => {
                let p = p.replace("numpad", "num");
                let p = match p.as_str() {
                    "escape" => "esc".to_string(),
                    "return" => "enter".to_string(),
                    "del" => "delete".to_string(),
                    "ins" => "insert".to_string(),
                    "pgup" => "pageup".to_string(),
                    "pgdn" => "pagedown".to_string(),
                    _ => p,
                };
                vk = Some(key_names().into_iter().find(|(n, _)| n.to_lowercase() == p)?.1);
            }
        }
    }
    vk.map(|vk| Hotkey { mods, vk })
}

pub fn format(h: Hotkey) -> String {
    let mut s = String::new();
    for (m, n) in [(MOD_CTRL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")] {
        if h.mods & m != 0 {
            s += n;
            s += "+";
        }
    }
    let name = key_name(h.vk).unwrap_or_else(|| format!("0x{:X}", h.vk));
    // "Num1" reads nicer as "Num 1" on screen
    s + &match name.strip_prefix("Num") {
        Some(rest) if rest.len() == 1 => format!("Num {rest}"),
        _ => name,
    }
}

pub fn path() -> std::path::PathBuf {
    crate::audio::exe_dir_file("hotkey.txt")
}

pub fn modified() -> Option<SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

/// Ok(hotkey) from hotkey.txt (DEFAULT if the file is missing), Err(text) if the text can't be parsed.
pub fn load() -> Result<Hotkey, String> {
    let Ok(text) = std::fs::read_to_string(path()) else { return Ok(DEFAULT) };
    let line = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'));
    match line {
        None => Ok(DEFAULT),
        Some(l) => parse(l).ok_or_else(|| l.to_string()),
    }
}

pub fn save(h: Hotkey) -> std::io::Result<()> {
    let text = format!(
        "\u{feff}# Клавиша для переключения звука. Примеры: Num1, F9, Pause, Ctrl+Alt+H, Win+Shift+S\r\n\
         # Проще всего поменять командой: AudioSwitch.exe setkey\r\n{}\r\n",
        format(h).replace("Num ", "Num")
    );
    std::fs::write(path(), text)
}
