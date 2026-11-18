//! AudioSwitch: a hotkey (Num 1 by default) switches the default playback device and shows a small island.
//!   AudioSwitch.exe                          run in background (single instance)
//!   AudioSwitch.exe setkey                   press a new hotkey; saved to hotkey.txt
//!   AudioSwitch.exe once                     switch once and exit
//!   AudioSwitch.exe list                     write active playback devices to list.txt
//!   AudioSwitch.exe demo [caption title 0|1] preview the island without switching
#![windows_subsystem = "windows"]

mod audio;
mod hotkey;
mod osd;

use hotkey::Hotkey;
use osd::Icon;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Device we are switching to while the (possibly slow, Bluetooth) switch is in progress.
static PENDING: Mutex<Option<String>> = Mutex::new(None);

const HOTKEY_ID: i32 = 1;
const WM_CAPTURED: u32 = WM_APP + 1;

fn on_hotkey() {
    let (all, current) = match audio::devices() {
        Ok(v) => v,
        Err(e) => return osd::show("Ошибка", &e.message(), Icon::Speaker),
    };
    let devices = audio::filter(all);
    let current = PENDING.lock().unwrap().clone().or(current);

    if devices.len() < 2 {
        let (title, hp) = match devices.first() {
            Some(d) => (audio::split_name(&d.name).1, audio::is_headphones(&d.name)),
            None => ("Нет устройств".to_string(), false),
        };
        return osd::show("Переключать не на что", &title, Icon::for_device(hp));
    }
    let idx = devices.iter().position(|d| Some(&d.id) == current.as_ref());
    let next = devices[idx.map_or(0, |i| (i + 1) % devices.len())].clone();

    // show first: switching to Bluetooth can take a couple of seconds
    let (caption, title) = audio::split_name(&next.name);
    osd::show(&caption, &title, Icon::for_device(audio::is_headphones(&next.name)));

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

fn message_loop(mut handle: impl FnMut(&MSG)) {
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            handle(&msg);
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn register(id: i32, h: Hotkey) -> bool {
    unsafe { RegisterHotKey(None, id, HOT_KEY_MODIFIERS(h.mods) | MOD_NOREPEAT, h.vk).is_ok() }
}

fn unregister(id: i32) {
    unsafe {
        let _ = UnregisterHotKey(None, id);
    }
}

// ---------------- background mode ----------------

fn run_background() {
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("AudioSwitch_NumPad1"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return; // already running
        }
    }

    let mut current: Option<Hotkey> = None;
    match hotkey::load() {
        Ok(h) if register(HOTKEY_ID, h) => current = Some(h),
        Ok(h) => unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(format!(
                    "Клавиша {} уже занята другой программой.\n\nВыберите другую: запустите «AudioSwitch.exe setkey».",
                    hotkey::format(h)
                )),
                w!("AudioSwitch"),
                MB_ICONWARNING,
            );
        },
        Err(text) => osd::show("Не понял клавишу в hotkey.txt", &text, Icon::Keyboard),
    }

    // watch hotkey.txt: a new key applies without restarting
    let mut stamp = hotkey::modified();
    unsafe { SetTimer(None, 0, 1500, None) };

    message_loop(|msg| {
        if msg.message == WM_HOTKEY && msg.wParam.0 == HOTKEY_ID as usize {
            on_hotkey();
        } else if msg.message == WM_TIMER && msg.hwnd.0.is_null() {
            let now = hotkey::modified();
            if now == stamp {
                return;
            }
            stamp = now;
            match hotkey::load() {
                Err(text) => osd::show("Не понял клавишу в hotkey.txt", &text, Icon::Keyboard),
                Ok(h) if Some(h) == current => {}
                Ok(h) => {
                    unregister(HOTKEY_ID);
                    if register(HOTKEY_ID, h) {
                        current = Some(h);
                    } else {
                        osd::show("Клавиша занята другой программой", &hotkey::format(h), Icon::Keyboard);
                        if let Some(old) = current {
                            register(HOTKEY_ID, old);
                        }
                    }
                }
            }
        }
    });
}

// ---------------- setkey mode: press the new key ----------------

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
static CAPTURING: AtomicBool = AtomicBool::new(true);

fn held_modifiers() -> u32 {
    let down = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(vk.0 as i32) } < 0;
    let mut m = 0;
    if down(VK_CONTROL) { m |= hotkey::MOD_CTRL; }
    if down(VK_MENU) { m |= hotkey::MOD_ALT; }
    if down(VK_SHIFT) { m |= hotkey::MOD_SHIFT; }
    if down(VK_LWIN) || down(VK_RWIN) { m |= hotkey::MOD_WIN; }
    m
}

unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && CAPTURING.load(Ordering::SeqCst) {
        let k = &*(l.0 as *const KBDLLHOOKSTRUCT);
        if !hotkey::is_modifier(k.vkCode) {
            let msg = w.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let _ = PostThreadMessageW(
                    MAIN_THREAD.load(Ordering::SeqCst),
                    WM_CAPTURED,
                    WPARAM(k.vkCode as usize),
                    LPARAM(held_modifiers() as isize),
                );
            }
            return LRESULT(1); // swallow: the key shouldn't reach other apps while choosing
        }
    }
    CallNextHookEx(None, code, w, l)
}

fn run_setkey() {
    let current = hotkey::load().unwrap_or(hotkey::DEFAULT);
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) };
    if hook.is_err() {
        return osd_and_wait("Не удалось перехватить клавиатуру", "Попробуйте ещё раз", Icon::Keyboard);
    }

    let prompt_title = format!("Сейчас: {} · Esc — отмена", hotkey::format(current));
    osd::quit_when_done();
    osd::show_for("Нажмите новую клавишу", &prompt_title, Icon::Keyboard, 20.0);

    message_loop(|msg| {
        if msg.message != WM_CAPTURED || !CAPTURING.load(Ordering::SeqCst) {
            return;
        }
        let h = Hotkey { vk: msg.wParam.0 as u32, mods: msg.lParam.0 as u32 };
        if h.vk == 0x1B && h.mods == 0 {
            CAPTURING.store(false, Ordering::SeqCst);
            return osd::show("Клавиша не изменилась", &hotkey::format(current), Icon::Keyboard);
        }
        if hotkey::key_name(h.vk).is_none() {
            return osd::show_for("Эту клавишу нельзя назначить", "Нажмите другую · Esc — отмена", Icon::Keyboard, 20.0);
        }
        // the running AudioSwitch holds the current key, so only test a different one
        if h != current {
            if !register(99, h) {
                let t = format!("{} · нажмите другую", hotkey::format(h));
                return osd::show_for("Клавиша занята другой программой", &t, Icon::Keyboard, 20.0);
            }
            unregister(99);
        }
        CAPTURING.store(false, Ordering::SeqCst);
        match hotkey::save(h) {
            Ok(()) => osd::show("Новая клавиша", &hotkey::format(h), Icon::Keyboard),
            Err(e) => osd::show("Не удалось сохранить", &e.to_string(), Icon::Keyboard),
        }
    });

    if let Ok(hook) = hook {
        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
    }
}

fn osd_and_wait(caption: &str, title: &str, icon: Icon) {
    osd::quit_when_done();
    osd::show(caption, title, icon);
    message_loop(|_| {});
}

// ---------------- entry ----------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    audio::com_init(true);

    match mode {
        "setkey" => run_setkey(),
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
            let icon = match args.get(3).map(String::as_str) {
                Some("0") => Icon::Speaker,
                Some("k") => Icon::Keyboard,
                _ => Icon::Headphones,
            };
            osd_and_wait(caption, title, icon);
        }
        _ => run_background(),
    }
}
