//! Configuration loaded from the registry on every thumbnail request.
//!
//! Reads are infrequent (Explorer only invokes us when a thumbnail is
//! actually needed) and the registry path is in the per-user hive, so
//! caching would buy us almost nothing while making the GUI tool's
//! settings take effect only after Explorer restart. We re-read every
//! time, accepting the negligible overhead.
//!
//! All values live under `HKCU\Software\ThumbsUp`. The GUI writes
//! them; the DLL reads them.

#![cfg(windows)]

use thumbsup_core::CoverPolicy;
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;

/// Registry path (relative to HKCU) where settings live.
pub const REG_SUBKEY: &str = r"Software\ThumbsUp";

/// Every knob the user can tweak from the GUI.
#[derive(Debug, Clone)]
pub struct Config {
    /// Master switch. When false, `GetThumbnail` returns `WTS_E_FAILEDEXTRACTION`
    /// immediately, and Explorer falls back to the generic icon.
    pub enabled: bool,
    /// Refuse to process EPUBs larger than this many bytes.
    pub max_file_bytes: u64,
    /// Maximum wall-clock time (ms) we'll spend on a single thumbnail before
    /// giving up. Zero disables the timeout.
    pub max_thumbnail_ms: u32,
    /// What to do when no spec-compliant cover declaration is found.
    pub cover_policy: CoverPolicy,
    /// Whether to write a diagnostic log line for every thumbnail attempt.
    pub logging_enabled: bool,
    /// Where to write the log. Empty string = pick a default under LOCALAPPDATA.
    pub log_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled:          true,
            max_file_bytes:   256 * 1024 * 1024, // 256 MiB
            max_thumbnail_ms: 5_000,
            cover_policy:     CoverPolicy::Strict,
            logging_enabled:  false,
            log_path:         String::new(),
        }
    }
}

impl Config {
    /// Load from `HKCU\Software\ThumbsUp`, returning defaults for
    /// any missing or unreadable values. The DLL never panics on bad
    /// registry data — it just falls back to defaults so File Explorer
    /// keeps working.
    pub fn load() -> Self {
        let mut cfg = Config::default();

        let subkey = wide(REG_SUBKEY);
        let mut hkey = HKEY::default();
        let open = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            )
        };
        if open != ERROR_SUCCESS {
            return cfg;
        }

        if let Some(v) = read_dword(hkey, "Enabled")          { cfg.enabled = v != 0; }
        if let Some(v) = read_qword(hkey, "MaxFileBytes")     { cfg.max_file_bytes = v; }
        if let Some(v) = read_dword(hkey, "MaxThumbnailMs")   { cfg.max_thumbnail_ms = v; }
        if let Some(v) = read_dword(hkey, "FallbackPolicy") {
            cfg.cover_policy = match v {
                1 => CoverPolicy::FirstImageFallback,
                _ => CoverPolicy::Strict,
            };
        }
        if let Some(v) = read_dword(hkey, "LoggingEnabled")   { cfg.logging_enabled = v != 0; }
        if let Some(s) = read_string(hkey, "LogPath")         { cfg.log_path = s; }

        unsafe { let _ = RegCloseKey(hkey); }
        cfg
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn read_dword(hkey: HKEY, name: &str) -> Option<u32> {
    let name = wide(name);
    let mut value: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let mut ty = REG_VALUE_TYPE::default();
    let res = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        )
    };
    if res == ERROR_SUCCESS && ty == REG_DWORD {
        Some(value)
    } else {
        None
    }
}

fn read_qword(hkey: HKEY, name: &str) -> Option<u64> {
    let name = wide(name);
    let mut value: u64 = 0;
    let mut size: u32 = std::mem::size_of::<u64>() as u32;
    let mut ty = REG_VALUE_TYPE::default();
    let res = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            Some(&mut value as *mut u64 as *mut u8),
            Some(&mut size),
        )
    };
    if res == ERROR_SUCCESS && ty == REG_QWORD {
        Some(value)
    } else {
        None
    }
}

fn read_string(hkey: HKEY, name: &str) -> Option<String> {
    let name_w = wide(name);
    // First call: get required size in bytes.
    let mut size: u32 = 0;
    let mut ty = REG_VALUE_TYPE::default();
    let r1 = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut ty),
            None,
            Some(&mut size),
        )
    };
    if r1 != ERROR_SUCCESS || ty != REG_SZ || size == 0 {
        return None;
    }
    let count = (size as usize).div_ceil(2);
    let mut buf: Vec<u16> = vec![0; count];
    let mut size_inout = size;
    let r2 = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size_inout),
        )
    };
    if r2 != ERROR_SUCCESS {
        return None;
    }
    // Trim trailing NULs.
    while buf.last() == Some(&0) { buf.pop(); }
    String::from_utf16(&buf).ok()
}
