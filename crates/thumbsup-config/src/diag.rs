//! Read the DLL's diagnostics log (the tab-delimited file the DLL writes
//! when logging is enabled) and present the latest entries in the GUI.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DiagEntry {
    pub timestamp: String,
    pub file: String,
    pub outcome: String,
    pub detail: String,
}

/// Default log path when the user hasn't specified one. Mirrors the DLL's
/// default exactly.
pub fn default_log_path() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    Some(
        PathBuf::from(local)
            .join("ThumbsUp")
            .join("diagnostics.log"),
    )
}

/// Read the last `max_entries` lines of the log. Returns most-recent first.
pub fn read_recent(path: &std::path::Path, max_entries: usize) -> std::io::Result<Vec<DiagEntry>> {
    let content = std::fs::read_to_string(path)?;
    let mut entries: Vec<DiagEntry> = content.lines().filter_map(parse_line).collect();
    if entries.len() > max_entries {
        let drop = entries.len() - max_entries;
        entries.drain(0..drop);
    }
    entries.reverse();
    Ok(entries)
}

fn parse_line(line: &str) -> Option<DiagEntry> {
    let mut parts = line.splitn(4, '\t');
    let ts = parts.next()?.to_string();
    let file = parts.next()?.to_string();
    let outcome = parts.next()?.to_string();
    let detail = parts.next().unwrap_or("").to_string();
    Some(DiagEntry {
        timestamp: ts,
        file,
        outcome,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_line() {
        let entry =
            parse_line("2026-01-15T12:34:56Z\tbook.epub\tok\tv3 strategy=epub3-cover-image")
                .unwrap();
        assert_eq!(entry.timestamp, "2026-01-15T12:34:56Z");
        assert_eq!(entry.file, "book.epub");
        assert_eq!(entry.outcome, "ok");
        assert_eq!(entry.detail, "v3 strategy=epub3-cover-image");
    }

    #[test]
    fn parses_short_line_with_empty_detail() {
        let entry = parse_line("2026-01-15T12:34:56Z\tx.epub\tno-cover").unwrap();
        assert_eq!(entry.detail, "");
    }

    #[test]
    fn rejects_garbage_line() {
        assert!(parse_line("garbage").is_none());
    }
}
