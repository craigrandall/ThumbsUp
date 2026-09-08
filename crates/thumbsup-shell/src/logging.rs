//! Lightweight, no-runtime file logging.
//!
//! The DLL runs *inside* `dllhost.exe` (the COM surrogate Explorer uses for
//! shell extensions on modern Windows). Spinning up a real logging
//! framework with channels and worker threads would be both wasteful and
//! risky in that host. Instead, when logging is enabled we simply append
//! a single short line per thumbnail attempt — open file, write, close.
//!
//! The log lines are designed to be parsed by the GUI tool's diagnostics
//! view: one event per line, fields separated by `\t`, ISO-8601 timestamp
//! first.

#![cfg(windows)]

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;

/// Serializes log writes so concurrent thumbnail requests cannot interleave
/// partial lines. Contention is negligible — Explorer rarely fires more
/// than a handful of thumbnail requests in parallel.
static LOG_LOCK: Mutex<()> = Mutex::new(());

/// Append a single tab-delimited record to the log if logging is enabled.
/// All errors are swallowed: a failing log must never fail a thumbnail.
///
/// In addition to the file output, the line is mirrored to
/// `OutputDebugString` so power users can attach Sysinternals
/// **DebugView** and watch thumbnail decisions in real time without
/// restarting Explorer. This is the same diagnostic flow long-time users
/// of DarkThumbs are familiar with.
pub fn log_event(cfg: &Config, file_name: &str, outcome: &str, detail: &str) {
    if !cfg.logging_enabled {
        return;
    }
    let line = format!(
        "{ts}\t{file}\t{outcome}\t{detail}\n",
        ts = iso_timestamp(),
        file = sanitize(file_name),
        outcome = sanitize(outcome),
        detail = sanitize(detail),
    );

    write_to_debugger(&line);
    write_to_file(cfg, &line);
}

fn write_to_file(cfg: &Config, line: &str) {
    let path = resolve_log_path(cfg);
    let _g = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Mirror to `OutputDebugStringW`. The string is tagged with our DLL
/// name so DebugView's "Filter / Highlight" feature can isolate our
/// output among the rest of Explorer's chatter.
fn write_to_debugger(line: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
    let tagged = format!("[ThumbsUp] {line}");
    // Convert to UTF-16 with terminating NUL.
    let mut wide: Vec<u16> = tagged.encode_utf16().collect();
    wide.push(0);
    unsafe {
        OutputDebugStringW(PCWSTR(wide.as_ptr()));
    }
}

fn resolve_log_path(cfg: &Config) -> PathBuf {
    if !cfg.log_path.is_empty() {
        return PathBuf::from(&cfg.log_path);
    }
    // Default: %LOCALAPPDATA%\ThumbsUp\diagnostics.log
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local)
            .join("ThumbsUp")
            .join("diagnostics.log");
    }
    PathBuf::from("thumbsup-shell.log")
}

/// RFC-3339-ish timestamp without pulling in chrono. Resolution: seconds.
fn iso_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Compute Y-M-D H:M:S from secs (UTC). Good enough for diagnostics.
    let (y, mo, d, h, mi, s) = epoch_to_civil(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Convert Unix seconds → (year, month, day, hour, minute, second). Uses
/// Howard Hinnant's date algorithm; valid for any year in [-32767, 32767].
fn epoch_to_civil(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let z = (secs / 86400) as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64 + era * 400) as i64;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    let day_secs = secs % 86400;
    let h = (day_secs / 3600) as u32;
    let mi = ((day_secs % 3600) / 60) as u32;
    let s = (day_secs % 60) as u32;
    (y as i32, m, d, h, mi, s)
}

/// Replace tabs and newlines so they cannot break the line-oriented format.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' => ' ',
            '\n' | '\r' => ' ',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_to_civil_unix_zero() {
        assert_eq!(epoch_to_civil(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn epoch_to_civil_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(epoch_to_civil(1_704_067_200), (2024, 1, 1, 0, 0, 0));
    }

    #[test]
    fn sanitize_removes_separators() {
        assert_eq!(sanitize("hello\tworld\nfoo\rbar"), "hello world foo bar");
    }
}
