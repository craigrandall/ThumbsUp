//! Constants shared across the DLL: our CLSID, the EPUB extension, and the
//! well-known Windows GUIDs we register against.

#![cfg(windows)]

use windows::core::GUID;

/// The CLSID assigned to *this* thumbnail provider. It is hard-coded — the
/// installer writes it into the registry, the DLL's `DllGetClassObject`
/// matches against it, and the GUI tool re-uses it for register/unregister.
///
/// CLSID is a fresh, project-specific GUID. Do not change without bumping
/// the major version of the installer, since existing registrations will
/// otherwise become orphaned.
pub const CLSID_EPUB_THUMBNAIL_PROVIDER: GUID =
    GUID::from_u128(0x1C4E5C2A_7E51_4F1C_8B6D_AB12CDEF3456);

/// Friendly name shown in the registry "ApprovedShellExtensions" list and
/// returned by `IObjectWithSite::GetSite` etc. Kept short for log display.
pub const FRIENDLY_NAME: &str = "EPUB Thumbnail Provider";

/// File extension we register against. The leading dot is part of the key
/// path under `HKCR`.
pub const EPUB_EXTENSION: &str = ".epub";

/// The well-known Windows shell handler interface ID for thumbnail
/// providers. File extensions register thumbnail providers under
/// `HKCR\<.ext>\ShellEx\{e357fccd-a995-4576-b01f-234630154e96}`.
pub const SHELLEX_THUMBNAIL_PROVIDER_KEY: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

// NOTE: the actual maximum inner-archive-file size is enforced inside
// thumbsup-core (see `cover::MAX_INNER_FILE_BYTES`). It used to be
// re-declared here too; removed to keep a single source of truth.
