//! Registry I/O for the GUI: read existing settings, write user changes,
//! and shell out to `regsvr32` for register/unregister actions.

#![cfg(windows)]

use std::process::Command;

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;

use crate::settings::{FallbackPolicy, Settings};

const SUBKEY: &str = r"Software\ThumbsUp";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn load_settings() -> Settings {
    let mut s = Settings::default();
    let sub = wide(SUBKEY);
    let mut hkey = HKEY::default();
    let r = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), 0, KEY_READ, &mut hkey)
    };
    if r != ERROR_SUCCESS {
        return s;
    }
    if let Some(v) = read_dword(hkey, "Enabled")          { s.enabled = v != 0; }
    if let Some(v) = read_qword(hkey, "MaxFileBytes")     { s.max_file_mb = v / (1024 * 1024); }
    if let Some(v) = read_dword(hkey, "MaxThumbnailMs")   { s.max_thumbnail_ms = v; }
    if let Some(v) = read_dword(hkey, "FallbackPolicy")   { s.fallback_policy = FallbackPolicy::from_dword(v); }
    if let Some(v) = read_dword(hkey, "LoggingEnabled")   { s.logging_enabled = v != 0; }
    if let Some(v) = read_string(hkey, "LogPath")         { s.log_path = v; }
    unsafe { let _ = RegCloseKey(hkey); }
    s
}

pub fn save_settings(s: &Settings) -> std::io::Result<()> {
    let sub = wide(SUBKEY);
    let mut hkey = HKEY::default();
    let mut disp = REG_CREATE_KEY_DISPOSITION::default();
    let r = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut hkey,
            Some(&mut disp),
        )
    };
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    write_dword(hkey, "Enabled",          s.enabled as u32)?;
    write_qword(hkey, "MaxFileBytes",     s.max_file_bytes())?;
    write_dword(hkey, "MaxThumbnailMs",   s.max_thumbnail_ms)?;
    write_dword(hkey, "FallbackPolicy",   s.fallback_policy.as_dword())?;
    write_dword(hkey, "LoggingEnabled",   s.logging_enabled as u32)?;
    write_string(hkey, "LogPath",         &s.log_path)?;
    unsafe { let _ = RegCloseKey(hkey); }
    Ok(())
}

/// Run `regsvr32 /s /i:user <dll>` to register the DLL per-user. Returns
/// the exit code as a string for display in the GUI.
pub fn register_dll(dll_path: &str) -> std::io::Result<String> {
    let status = Command::new("regsvr32")
        .args(["/s", "/n", "/i:user", dll_path])
        .status()?;
    if status.success() {
        Ok("Registration succeeded.".into())
    } else {
        Ok(format!("regsvr32 exited with code {:?}", status.code()))
    }
}

/// Run `regsvr32 /u /s /i:user <dll>` to unregister per-user.
pub fn unregister_dll(dll_path: &str) -> std::io::Result<String> {
    let status = Command::new("regsvr32")
        .args(["/s", "/u", "/n", "/i:user", dll_path])
        .status()?;
    if status.success() {
        Ok("Unregistration succeeded.".into())
    } else {
        Ok(format!("regsvr32 exited with code {:?}", status.code()))
    }
}

fn read_dword(hkey: HKEY, name: &str) -> Option<u32> {
    let n = wide(name);
    let mut v: u32 = 0;
    let mut sz: u32 = 4;
    let mut ty = REG_VALUE_TYPE::default();
    let r = unsafe {
        RegQueryValueExW(hkey, PCWSTR(n.as_ptr()), None, Some(&mut ty),
            Some(&mut v as *mut u32 as *mut u8), Some(&mut sz))
    };
    if r == ERROR_SUCCESS && ty == REG_DWORD { Some(v) } else { None }
}

fn read_qword(hkey: HKEY, name: &str) -> Option<u64> {
    let n = wide(name);
    let mut v: u64 = 0;
    let mut sz: u32 = 8;
    let mut ty = REG_VALUE_TYPE::default();
    let r = unsafe {
        RegQueryValueExW(hkey, PCWSTR(n.as_ptr()), None, Some(&mut ty),
            Some(&mut v as *mut u64 as *mut u8), Some(&mut sz))
    };
    if r == ERROR_SUCCESS && ty == REG_QWORD { Some(v) } else { None }
}

fn read_string(hkey: HKEY, name: &str) -> Option<String> {
    let n = wide(name);
    let mut sz: u32 = 0;
    let mut ty = REG_VALUE_TYPE::default();
    let r = unsafe {
        RegQueryValueExW(hkey, PCWSTR(n.as_ptr()), None, Some(&mut ty), None, Some(&mut sz))
    };
    if r != ERROR_SUCCESS || ty != REG_SZ || sz == 0 { return None; }
    let count = (sz as usize).div_ceil(2);
    let mut buf: Vec<u16> = vec![0; count];
    let mut sz2 = sz;
    let r = unsafe {
        RegQueryValueExW(hkey, PCWSTR(n.as_ptr()), None, Some(&mut ty),
            Some(buf.as_mut_ptr() as *mut u8), Some(&mut sz2))
    };
    if r != ERROR_SUCCESS { return None; }
    while buf.last() == Some(&0) { buf.pop(); }
    String::from_utf16(&buf).ok()
}

fn write_dword(hkey: HKEY, name: &str, value: u32) -> std::io::Result<()> {
    let n = wide(name);
    let bytes = value.to_le_bytes();
    let r = unsafe {
        RegSetValueExW(hkey, PCWSTR(n.as_ptr()), 0, REG_DWORD, Some(&bytes))
    };
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    Ok(())
}

fn write_qword(hkey: HKEY, name: &str, value: u64) -> std::io::Result<()> {
    let n = wide(name);
    let bytes = value.to_le_bytes();
    let r = unsafe {
        RegSetValueExW(hkey, PCWSTR(n.as_ptr()), 0, REG_QWORD, Some(&bytes))
    };
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    Ok(())
}

fn write_string(hkey: HKEY, name: &str, value: &str) -> std::io::Result<()> {
    let n = wide(name);
    let v = wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            v.as_ptr() as *const u8,
            v.len() * std::mem::size_of::<u16>(),
        )
    };
    let r = unsafe {
        RegSetValueExW(hkey, PCWSTR(n.as_ptr()), 0, REG_SZ, Some(bytes))
    };
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    Ok(())
}
