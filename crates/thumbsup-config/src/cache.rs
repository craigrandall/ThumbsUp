//! Clear the Windows thumbnail cache so freshly-changed thumbnails appear
//! immediately in Explorer. The cleanest, most portable way to do this is
//! to shell out to the built-in `cleanmgr.exe` or to delete the cache
//! files directly. We use the direct-delete approach so the user sees an
//! immediate result.

#![cfg(windows)]

use std::path::PathBuf;

/// Returns the per-user thumbnail-cache directory.
fn cache_dir() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    Some(PathBuf::from(local).join(r"Microsoft\Windows\Explorer"))
}

/// Delete every `thumbcache_*.db` file in the per-user cache directory.
/// Returns the number of files removed.
///
/// File Explorer keeps a few of these open at any time; the in-use files
/// are simply skipped. The user can run this again after restarting
/// Explorer to clear them.
pub fn clear_thumbnail_cache() -> std::io::Result<usize> {
    let dir = cache_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "LOCALAPPDATA not set"))?;
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("thumbcache_") && name.ends_with(".db")
                && std::fs::remove_file(entry.path()).is_ok() {
                    removed += 1;
                }
                // Files in use will fail to delete; that's fine.			
           if name.starts_with("iconcache_") && name.ends_with(".db") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(removed)
}
