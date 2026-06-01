//! Path resolution and traversal protection for OCF (EPUB) archives.
//!
//! All paths inside an EPUB are POSIX-style URI references encoded in the
//! OPF and `container.xml`. References from the OPF are *relative to the
//! OPF file's directory* — not to the archive root. We must therefore:
//!
//! 1. Resolve `href` relative to the OPF's directory.
//! 2. Normalize `.` and `..` segments.
//! 3. Reject any path that escapes the archive root, since a hostile EPUB
//!    could otherwise reference arbitrary archive members or — if a future
//!    extension wrote files — host-filesystem locations.
//!
//! The functions in this module are pure and have no I/O dependencies, so
//! they are exhaustively unit-tested.

use crate::error::{EpubError, Result};

/// Returns the directory portion of a POSIX-style path (everything up to and
/// including the final `/`), or an empty string if the path has no slashes.
///
/// Mirrors the EPUB spec's interpretation of "the directory of the OPF":
/// the OPF's full-path determines the base for all relative manifest hrefs.
pub fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..=i], // include trailing '/'
        None    => "",
    }
}

/// Resolve a POSIX-style relative `href` against a base directory and return
/// the normalized archive-root-relative path.
///
/// * Backslashes are converted to forward slashes (some authoring tools emit
///   them on Windows).
/// * Percent-encoded sequences are *not* decoded here; ZIP central directory
///   entries are byte-for-byte and EPUB spec requires hrefs match those bytes
///   after the URI/IRI rules — but in practice cover hrefs are ASCII, so we
///   do a best-effort percent-decode of common cases (space, `%2F`).
/// * `..` segments are resolved; if they would escape the archive root the
///   call returns [`EpubError::PathTraversal`].
pub fn resolve_href(base_dir: &str, href: &str) -> Result<String> {
    let href = href.replace('\\', "/");
    let href = percent_decode_minimal(&href);

    // Absolute paths inside the archive (rare but legal): an OPF href starting
    // with '/' is interpreted as relative to the archive root.
    let combined: String = if href.starts_with('/') {
        href.trim_start_matches('/').to_string()
    } else {
        format!("{base_dir}{href}")
    };

    let mut stack: Vec<&str> = Vec::new();
    for seg in combined.split('/') {
        match seg {
            "" | "." => {} // collapse empty / current-dir segments
            ".." => {
                if stack.pop().is_none() {
                    return Err(EpubError::PathTraversal(href.to_string()));
                }
            }
            other => stack.push(other),
        }
    }

    Ok(stack.join("/"))
}

/// Decode the small set of percent-escapes that show up in real-world covers.
/// We deliberately avoid pulling in a full URL-decoding crate for the DLL.
fn percent_decode_minimal(s: &str) -> String {
    if !s.contains('%') && !s.contains('+') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                hex_digit(bytes[i + 1]),
                hex_digit(bytes[i + 2]),
            ) {
                out.push((hi * 16 + lo) as char);
                i += 3;
                continue;
            }
        }
        out.push(b as char);
        i += 1;
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_of_returns_empty_for_root() {
        assert_eq!(dir_of("content.opf"), "");
    }

    #[test]
    fn dir_of_includes_trailing_slash() {
        assert_eq!(dir_of("OEBPS/content.opf"), "OEBPS/");
        assert_eq!(dir_of("a/b/c/content.opf"), "a/b/c/");
    }

    #[test]
    fn resolve_simple_relative() {
        assert_eq!(resolve_href("OEBPS/", "cover.jpg").unwrap(), "OEBPS/cover.jpg");
    }

    #[test]
    fn resolve_subdirectory() {
        assert_eq!(
            resolve_href("OEBPS/", "images/cover.jpg").unwrap(),
            "OEBPS/images/cover.jpg"
        );
    }

    #[test]
    fn resolve_dot_segment_is_collapsed() {
        assert_eq!(
            resolve_href("OEBPS/", "./images/cover.jpg").unwrap(),
            "OEBPS/images/cover.jpg"
        );
    }

    #[test]
    fn resolve_parent_segment_within_root() {
        // OPF in OEBPS/text/, cover at OEBPS/images/ — legal upward traversal.
        assert_eq!(
            resolve_href("OEBPS/text/", "../images/cover.jpg").unwrap(),
            "OEBPS/images/cover.jpg"
        );
    }

    #[test]
    fn resolve_rejects_escape_above_root() {
        let err = resolve_href("OEBPS/", "../../etc/passwd").unwrap_err();
        assert!(matches!(err, EpubError::PathTraversal(_)));
    }

    #[test]
    fn resolve_rejects_double_dot_at_root() {
        let err = resolve_href("", "../cover.jpg").unwrap_err();
        assert!(matches!(err, EpubError::PathTraversal(_)));
    }

    #[test]
    fn resolve_handles_backslashes() {
        assert_eq!(
            resolve_href("OEBPS/", "images\\cover.jpg").unwrap(),
            "OEBPS/images/cover.jpg"
        );
    }

    #[test]
    fn resolve_handles_absolute_archive_paths() {
        // Some EPUBs use root-relative hrefs in the OPF.
        assert_eq!(
            resolve_href("OEBPS/", "/images/cover.jpg").unwrap(),
            "images/cover.jpg"
        );
    }

    #[test]
    fn resolve_decodes_percent_space() {
        assert_eq!(
            resolve_href("OEBPS/", "my%20cover.jpg").unwrap(),
            "OEBPS/my cover.jpg"
        );
    }

    #[test]
    fn percent_decode_keeps_invalid_escape_intact() {
        // A bare '%' that is not followed by hex digits is preserved.
        assert_eq!(percent_decode_minimal("100%done"), "100%done");
    }
}
