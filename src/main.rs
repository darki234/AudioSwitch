//! AudioSwitch: Num 1 switches the default playback device and shows a small island.
//!   AudioSwitch.exe                         run in background (single instance)
//!   AudioSwitch.exe once                    switch once and exit
//!   AudioSwitch.exe list                    write active playback devices to list.txt
//!   AudioSwitch.exe demo [caption title 0|1] preview the island without switching
#![windows_subsystem = "windows"]

mod audio;
mod osd;

use std::sync::Mutex;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_NOREPEAT, VK_NUMPAD1};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Device we are switching to while the (possibly slow, Bluetooth) switch is in progress.
static PENDING: Mutex<Option<String>> = Mutex::new(None);

fn on_hotkey() {
    let (all, current) = match audio::devices() {
        Ok(v) => v,
        Err(e) => return osd::show("Ошибка", &e.message(), false),
    };
    let devices = audio::filter(all);
    let current = PENDING.lock().unwrap().clone().or(current);

    if devices.len() < 2 {
        let (title, hp) = match devices.first() {
            Some(d) => (audio::split_name(&d.name).1, audio::is_headphones(&d.name)),
            None => ("Нет устройств".to_string(), false),
        };
        return osd::show("Переключать не на что", &title, hp);
    }
    let idx = devices.iter().position(|d| Some(&d.id) == current.as_ref());
    let next = devices[idx.map_or(0, |i| (i + 1) % devices.len())].clone();

    // show first: switching to Bluetooth can take a couple of seconds
    let (caption, title) = audio::split_name(&next.name);
    osd::show(&caption, &title, audio::is_headphones(&next.name));

    *PENDING.lock().unwrap() = Some(next.id.clone());
    std::thread::spawn(move || {
        audio::com_init(false);
        let _ = audio::set_default(&next.id);
        let mut p = PENDING.lock().unwrap();
        if p.as_deref() == Some(next.id.as_str()) {
            *p = None;
        }
    });
}

fn message_loop() {
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                on_hotkey();
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    audio::com_init(true);

    match mode {
        "list" => {
            let mut out = String::from("\u{feff}");
            if let Ok((all, cur)) = audio::devices() {
                for d in all {
                    out += if Some(&d.id) == cur.as_ref() { "* " } else { "  " };
                    out += &d.name;
                    out += "\r\n";
                }
            }
            let _ = std::fs::write(audio::exe_dir_file("list.txt"), out);
        }
        "once" => {
            if let Ok((all, cur)) = audio::devices() {
                let devices = audio::filter(all);
                if devices.len() >= 2 {
                    let idx = devices.iter().position(|d| Some(&d.id) == cur.as_ref());
                    let _ = audio::set_default(&devices[idx.map_or(0, |i| (i + 1) % devices.len())].id);
                }
            }
        }
        "demo" => {
            let caption = args.get(1).map(String::as_str).unwrap_or("Наушники");
            let title = args.get(2).map(String::as_str).unwrap_or("Galaxy Buds+ (A01C)");
            let hp = args.get(3).map_or(true, |v| v == "1");
            osd::quit_when_done();
            osd::show(caption, title, hp);
            message_loop();
        }
        _ => unsafe {
            let _mutex = CreateMutexW(None, true, w!("AudioSwitch_NumPad1"));
            if GetLastError() == ERROR_ALREADY_EXISTS {
                return; // already running
            }
            if RegisterHotKey(None, 1, MOD_NOREPEAT, VK_NUMPAD1.0 as u32).is_err() {
                MessageBoxW(None, &HSTRING::from("Клавиша Num 1 уже занята другой программой"), w!("AudioSwitch"), MB_ICONWARNING);
                return;
            }
            message_loop();
        },
    }
}
