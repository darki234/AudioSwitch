//! Playback devices: enumerate, read the default one, switch the default (undocumented IPolicyConfig).
#![allow(non_snake_case, dead_code)]

use std::ffi::c_void;
use windows::core::{interface, IUnknown, IUnknown_Vtbl, Result, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

#[interface("F8679F50-850A-41CF-9C72-430F290290C8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, a: PCWSTR, b: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, a: PCWSTR, b: i32, c: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, a: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, a: PCWSTR, b: *mut c_void, c: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, a: PCWSTR, b: i32, c: *mut i64, d: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, a: PCWSTR, b: *mut i64) -> HRESULT;
    fn GetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, a: PCWSTR, b: *const c_void, c: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, a: PCWSTR, b: *const c_void, c: *const c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
}

pub fn com_init(sta: bool) {
    let mode = if sta { COINIT_APARTMENTTHREADED } else { COINIT_MULTITHREADED };
    unsafe {
        let _ = CoInitializeEx(None, mode);
    }
}

unsafe fn take(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

/// Active playback devices and the id of the current default one.
pub fn devices() -> Result<(Vec<Device>, Option<String>)> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let col = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut list = Vec::new();
        for i in 0..col.GetCount()? {
            let d = col.Item(i)?;
            let id = take(d.GetId()?);
            let name = d
                .OpenPropertyStore(STGM_READ)
                .and_then(|s| s.GetValue(&PKEY_Device_FriendlyName))
                .map(|v| v.to_string())
                .unwrap_or_else(|_| "?".into());
            list.push(Device { id, name });
        }
        let current = en
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .and_then(|d| d.GetId())
            .ok()
            .map(|p| take(p));
        Ok((list, current))
    }
}

pub fn set_default(id: &str) -> Result<()> {
    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?;
        for role in [eConsole, eMultimedia, eCommunications] {
            policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role).ok()?;
        }
    }
    Ok(())
}

fn config_path(file: &str) -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(file)))
        .unwrap_or_else(|| file.into())
}

pub fn exe_dir_file(file: &str) -> std::path::PathBuf {
    config_path(file)
}

/// devices.txt: name fragments to include (one per line), "!fragment" to exclude, "#" comments.
pub fn filter(all: Vec<Device>) -> Vec<Device> {
    let Ok(text) = std::fs::read_to_string(config_path("devices.txt")) else { return all };
    let (mut include, mut exclude) = (Vec::new(), Vec::new());
    for line in text.trim_start_matches('\u{feff}').lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        match l.strip_prefix('!') {
            Some(x) => exclude.push(x.trim().to_lowercase()),
            None => include.push(l.to_lowercase()),
        }
    }
    let has = |d: &Device, f: &String| d.name.to_lowercase().contains(f.as_str());
    let mut picked: Vec<Device> = Vec::new();
    if include.is_empty() {
        picked = all;
    } else {
        for f in &include {
            for d in &all {
                if has(d, f) && !picked.contains(d) {
                    picked.push(d.clone());
                }
            }
        }
    }
    picked.retain(|d| !exclude.iter().any(|f| has(d, f)));
    picked
}

/// "Headphones (Galaxy Buds+ (A01C))" -> ("Наушники", "Galaxy Buds+ (A01C)")
pub fn split_name(name: &str) -> (String, String) {
    let (caption, title) = match name.find(" (") {
        Some(i) if name.ends_with(')') => (&name[..i], &name[i + 2..name.len() - 1]),
        _ => ("Вывод звука", name),
    };
    let caption = match caption {
        "Headphones" | "Headset" => "Наушники",
        "Speakers" => "Динамики",
        c => c,
    };
    (caption.to_string(), title.to_string())
}

pub fn is_headphones(name: &str) -> bool {
    let n = name.to_lowercase();
    ["headphone", "headset", "buds", "airpods", "наушник", "гарнитур"]
        .iter()
        .any(|w| n.contains(w))
}
