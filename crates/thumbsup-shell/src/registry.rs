//! Registry register/unregister helpers.
//!
//! Two scopes are supported:
//! * **Per-user** (`HKCU\Software\Classes`) — does not require admin and is
//!   what the GUI tool uses for its one-click Register button.
//! * **Machine-wide** (`HKLM\Software\Classes`) — what the WiX installer
//!   writes during a per-machine install. Approving the extension under
//!   `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved`
//!   is also done at this scope.
//!
//! Both paths share the same key shapes; only the root hive differs.

#![cfg(windows)]

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;

use crate::clsid::{
    CLSID_EPUB_THUMBNAIL_PROVIDER, EPUB_EXTENSION, FRIENDLY_NAME, SHELLEX_THUMBNAIL_PROVIDER_KEY,
};

/// Where to write the registration.
///
/// The DLL's own `DllRegisterServer` entry point only ever calls this
/// with `Scope::Machine` (via the MSI installer's elevated `regsvr32`
/// invocation). `Scope::PerUser` is exposed as a public variant for
/// completeness — callers who load this crate as a library and want to
/// drive registration directly (e.g. an embedder building a portable
/// app) can use it. The dead-code lint would otherwise flag it because
/// nothing inside this crate currently constructs it.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `HKCU\Software\Classes` — no admin required.
    PerUser,
    /// `HKLM\Software\Classes` — admin required, used by the MSI installer.
    Machine,
}

impl Scope {
    fn classes_root(self) -> HKEY {
        match self {
            Scope::PerUser => HKEY_CURRENT_USER,
            Scope::Machine => HKEY_LOCAL_MACHINE,
        }
    }
    fn classes_subkey(self) -> &'static str {
        // Both hives expose Software\Classes with the same shape.
        r"Software\Classes"
    }
}

/// Register the thumbnail provider so File Explorer will load it for
/// `.epub` files.
///
/// `dll_path` must be the absolute path of the deployed shell-extension DLL.
///
/// **Previous-handler memory.** If `.epub` is already associated with a
/// *different* thumbnail handler (some users have CBXShell or DarkThumbs
/// installed concurrently for evaluation), we record that handler's
/// CLSID under our own `\PreviousThumbnailHandler` value so a later
/// [`unregister`] can restore it instead of leaving the user's
/// thumbnails permanently broken. This mirrors a UX feature the Icaros
/// project added in v3.3.0 ("File Explorer settings that have been
/// modified by Icaros is now reverted during uninstall").
///
/// **Verification.** After writing every required key, we read back the
/// thumbnail-handler association to confirm the write actually took
/// effect. Failures here usually mean corrupted ACLs on
/// `HKCR\.epub\ShellEx\` — a common silent-failure mode that the Icaros
/// community traced to corrupted registry permissions
/// ([Discussion #68]). We surface it as a clear error rather than
/// reporting success.
///
/// [Discussion #68]: https://github.com/Xanashi/Icaros/discussions/68
pub fn register(scope: Scope, dll_path: &str) -> std::io::Result<()> {
    let classes = open_create(scope.classes_root(), scope.classes_subkey())?;
    let clsid_guid = format!("{{{:?}}}", CLSID_EPUB_THUMBNAIL_PROVIDER);

    // 1. HKCR\CLSID\{guid} ----------------------------------------------
    let clsid_key = open_create(classes, &format!(r"CLSID\{clsid_guid}"))?;
    set_default_string(clsid_key, FRIENDLY_NAME)?;

    let inproc = open_create(clsid_key, "InprocServer32")?;
    set_default_string(inproc, dll_path)?;
    set_string(inproc, "ThreadingModel", "Apartment")?;
    close(inproc);

    // 2. Capture the existing thumbnail handler (if any) BEFORE we
    // overwrite it, so unregister can restore it.
    let assoc_key_path = format!(r"{EPUB_EXTENSION}\ShellEx\{SHELLEX_THUMBNAIL_PROVIDER_KEY}");
    let previous = read_default_string(classes, &assoc_key_path).unwrap_or_default();
    if !previous.is_empty() && previous != clsid_guid {
        // Save previous handler under our CLSID key so it travels with
        // our registration and is naturally cleaned up on full
        // uninstall.
        let _ = set_string(clsid_key, "PreviousThumbnailHandler", &previous);
    }
    close(clsid_key);

    // 3. HKCR\.epub\ShellEx\{thumbnail-iid} = "{our CLSID}" ------------
    let ext_key = open_create(classes, &assoc_key_path)?;
    set_default_string(ext_key, &clsid_guid)?;
    close(ext_key);

    // 4. Approved shell-extensions list (HKLM only). HKCU is auto-approved.
    if scope == Scope::Machine {
        let approved = open_create(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved",
        )?;
        set_string(approved, &clsid_guid, FRIENDLY_NAME)?;
        close(approved);
    }
    close(classes);

    // 5. Verification. Read back what we wrote; failure to round-trip
    // is the smoking gun for corrupted-ACL silent-failure cases.
    verify_registration(scope, &clsid_guid)?;

    Ok(())
}

/// Remove every registry key written by [`register`], restoring any
/// previously-recorded thumbnail handler instead of leaving the
/// association unset.
pub fn unregister(scope: Scope) -> std::io::Result<()> {
    let classes_root = scope.classes_root();
    let classes_path = scope.classes_subkey();
    let clsid_guid = format!("{{{:?}}}", CLSID_EPUB_THUMBNAIL_PROVIDER);

    // Read PreviousThumbnailHandler before we delete the CLSID key.
    let our_clsid_path = format!(r"{classes_path}\CLSID\{clsid_guid}");
    let previous =
        read_value(classes_root, &our_clsid_path, "PreviousThumbnailHandler").unwrap_or_default();

    let assoc_full =
        format!(r"{classes_path}\{EPUB_EXTENSION}\ShellEx\{SHELLEX_THUMBNAIL_PROVIDER_KEY}");

    if !previous.is_empty() {
        // Restore the original handler.
        if let Ok(k) = open_create(
            classes_root,
            assoc_full.trim_start_matches(&format!("{classes_path}\\")),
        ) {
            let _ = set_default_string(k, &previous);
            close(k);
        }
    } else {
        // No previous handler recorded — just remove the association.
        let _ = delete_tree(classes_root, &assoc_full);
    }

    // Delete our CLSID key tree.
    let _ = delete_tree(classes_root, &our_clsid_path);

    if scope == Scope::Machine {
        let _ = delete_value(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved",
            &clsid_guid,
        );
    }
    Ok(())
}

fn verify_registration(scope: Scope, expected_clsid: &str) -> std::io::Result<()> {
    let path = format!(
        r"{}\{EPUB_EXTENSION}\ShellEx\{SHELLEX_THUMBNAIL_PROVIDER_KEY}",
        scope.classes_subkey()
    );
    let got = read_default_string(scope.classes_root(), &path).unwrap_or_default();
    if got != expected_clsid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "Registration verification failed: expected {expected_clsid} \
                 at {path}, got {got:?}. This usually indicates corrupted \
                 ACLs on the parent registry key — fix permissions on \
                 HKCR\\{EPUB_EXTENSION}\\ShellEx and try again."
            ),
        ));
    }
    Ok(())
}

fn read_default_string(parent: HKEY, sub: &str) -> Option<String> {
    read_value(parent, sub, "")
}

fn read_value(parent: HKEY, sub: &str, name: &str) -> Option<String> {
    let sub_w = wide(sub);
    let mut hkey = HKEY::default();
    let r = unsafe { RegOpenKeyExW(parent, PCWSTR(sub_w.as_ptr()), 0, KEY_READ, &mut hkey) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let name_w = wide(name);
    let mut size: u32 = 0;
    let mut ty = REG_VALUE_TYPE::default();
    let r = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut ty),
            None,
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS || ty != REG_SZ || size == 0 {
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        return None;
    }
    let count = (size as usize).div_ceil(2);
    let mut buf: Vec<u16> = vec![0; count];
    let mut size_inout = size;
    let r = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size_inout),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if r != ERROR_SUCCESS {
        return None;
    }
    while buf.last() == Some(&0) {
        buf.pop();
    }
    String::from_utf16(&buf).ok()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_create(parent: HKEY, sub: &str) -> std::io::Result<HKEY> {
    let w = wide(sub);
    let mut hkey = HKEY::default();
    let mut disp: REG_CREATE_KEY_DISPOSITION = Default::default();
    let r = unsafe {
        RegCreateKeyExW(
            parent,
            PCWSTR(w.as_ptr()),
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
    Ok(hkey)
}

fn close(hkey: HKEY) {
    unsafe {
        let _ = RegCloseKey(hkey);
    }
}

fn set_default_string(hkey: HKEY, value: &str) -> std::io::Result<()> {
    set_string_inner(hkey, "", value)
}

fn set_string(hkey: HKEY, name: &str, value: &str) -> std::io::Result<()> {
    set_string_inner(hkey, name, value)
}

fn set_string_inner(hkey: HKEY, name: &str, value: &str) -> std::io::Result<()> {
    let name_w = wide(name);
    let value_w = wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            value_w.as_ptr() as *const u8,
            value_w.len() * std::mem::size_of::<u16>(),
        )
    };
    let r = unsafe { RegSetValueExW(hkey, PCWSTR(name_w.as_ptr()), 0, REG_SZ, Some(bytes)) };
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    Ok(())
}

fn delete_tree(parent: HKEY, sub: &str) -> std::io::Result<()> {
    let w = wide(sub);
    let r = unsafe { RegDeleteTreeW(parent, PCWSTR(w.as_ptr())) };
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    Ok(())
}

fn delete_value(parent: HKEY, sub: &str, name: &str) -> std::io::Result<()> {
    let sub_w = wide(sub);
    let mut hkey = HKEY::default();
    let r = unsafe { RegOpenKeyExW(parent, PCWSTR(sub_w.as_ptr()), 0, KEY_SET_VALUE, &mut hkey) };
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    let name_w = wide(name);
    let r = unsafe { RegDeleteValueW(hkey, PCWSTR(name_w.as_ptr())) };
    close(hkey);
    if r != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    Ok(())
}
