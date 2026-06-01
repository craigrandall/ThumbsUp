# Changelog

All notable changes to this project are recorded here. Versions follow
[Semantic Versioning](https://semver.org/). Unreleased changes are
recorded under the next planned version.

## [0.1.0] — Unreleased

Initial public-shaped release. The project went through multiple
analysis-driven iterations before stabilising on this set of features
and tests; the entries below summarise the deltas from the original
55-test cut.

### Renamed

- **Project name: "EPUB Thumbnailer" → "ThumbsUp".** The shorter name
  better fits Start Menu, Apps list, and window-title space, and
  doesn't lock the project to EPUB-only future scope.
- **Crate names** (the build-system identifier; not user-visible):
  - `epub-core` → `thumbsup-core`
  - `epub-thumbnailer` → `thumbsup-shell`
  - `epub-thumb-config` → `thumbsup-config`
- **Build artefacts**:
  - `epub_thumbnailer.dll` → `thumbsup_shell.dll`
  - `epub-thumb-config.exe` → `thumbsup-config.exe`
  - `EpubThumbnailer.msi` → `ThumbsUp.msi`
- **Per-user state locations**:
  - Registry: `HKCU\Software\EpubThumbnailer` → `HKCU\Software\ThumbsUp`
  - Logs: `%LOCALAPPDATA%\EpubThumbnailer\` → `%LOCALAPPDATA%\ThumbsUp\`
  - DebugView filter: `[EpubThumbnailer]` → `[ThumbsUp]`
- The COM `FRIENDLY_NAME` ("EPUB Thumbnail Provider") is unchanged —
  it's the function description shown in the Approved Shell Extensions
  list, not the product name, and accurately describes what the DLL
  does for that audience.
- The CLSID is unchanged. (CLSIDs are allocated once and never altered;
  changing them would orphan existing installations.)

### Fixed

#### Installer
- **WiX `ICE77` validation failure.** The `NotifyShellAssocChanged`
  custom action was sequenced `After="InstallFinalize"`, which placed
  it outside the install transaction; ICE77 requires in-script custom
  actions to be sequenced between `InstallInitialize` and
  `InstallFinalize`. Changed to `Before="InstallFinalize"`, the
  conventional last-slot inside the transaction. The cache-refresh
  custom action now runs after every file is committed but before the
  install transaction commits — exactly when the cache nudge is most
  useful.
- **Build script no longer claims success on native-tool failure.**
  PowerShell's `$ErrorActionPreference = "Stop"` does not apply to
  native-exe non-zero exit codes (they flow into `$LASTEXITCODE`
  rather than the error stream). The previous build script printed
  `==> Done` and exited 0 even when `light.exe` failed ICE
  validation. The script now wraps every native invocation in an
  `Invoke-Native` helper that throws on non-zero exit, making future
  build failures impossible to miss.

#### Build correctness
- **Pinned `windows` and `windows-core` to `=0.58.0`** in the workspace
  manifest. The `#[implement]` macro changes shape between minor
  versions; allowing semver-compatible upgrades broke the build with
  `_Impl` → `_Vtbl` rename errors.
- **Added `implement` feature** to the `windows` dependency. Without
  it, the `IFoo_Impl` traits the `#[implement]` macro generates code
  against don't exist in the compiled crate.
- **Added `Win32_System_SystemServices`** for `DLL_PROCESS_ATTACH`.
- **Added `Win32_Security`** for the security-attributes types
  referenced in `RegCreateKeyExW`'s signature; without it the function
  is gated out and resolves as undefined.
- Removed a stale duplicate `HARD_INNER_FILE_LIMIT` constant in
  `clsid.rs`. The single source of truth is now
  `cover::MAX_INNER_FILE_BYTES` in the core crate.
- Suppressed the dead-code lint on `Scope::PerUser` (kept as a public
  API affordance) and on the `EpubBuilder` test-fixture impl.

### Added

#### EPUB parsing
- **EPUB 3 `properties="cover-image"` cover detection** as the highest-
  priority strategy (matches OCF spec).
- **EPUB 2 `<meta name="cover">` cover detection** with manifest-id
  resolution.
- **Conventional cover ids** (`cover`, `cover-image`, `ci`,
  `coverimage`) for EPUBs that omit the formal declaration.
- **`<guide><reference type="cover">` parsing** (added after analysis
  of [DarkThumbs Issue #9][dt9]).
- **`<guide><reference type="thumbimagestandard">` parsing** for
  publishers (notably Random House) who use this Adobe-DRM-era
  declaration.
- **XHTML cover-wrapper traversal**: when the guide's `type="cover"`
  href points at an XHTML page, scan it for the first `<img src>` and
  follow that, resolving relative to the XHTML's directory.
- **First-image-in-manifest fallback**, gated behind a user-configurable
  `CoverPolicy::FirstImageFallback`.
- **Cooperative deadline enforcement** via
  `extract_cover_with_deadline`. The DLL passes the user-configured
  `MaxThumbnailMs` (default 5 s) and the pipeline checks the deadline
  at six strategic boundaries (added after [Icaros Issue #196][i196]
  showed slow inputs hanging Explorer).
- **Non-ASCII filename slow-path**: when `ZipArchive::by_name` misses
  due to the General-Purpose Bit 11 encoding-flag ambiguity, we fall
  back to a raw-byte central-directory scan. (Added after
  [Icaros Issue #212][i212].)
- **OCF tolerance**: archives that omit the spec-required `mimetype`
  entry are still extractable, since format identification is driven
  by `META-INF/container.xml` and the OPF (test #79).

#### Defense in depth
- **Outer EPUB size cap** (configurable, default 256 MiB).
- **Inner archive entry size cap** (hard, 64 MiB).
- **Archive entry-count cap** of 50 000 (defends against the file-count
  DoS class documented in [DarkThumbs Issue #5][dt5]).
- **Path-traversal protection** rejecting `../`-escaping cover hrefs
  before any read.
- **Image-format whitelist** (JPEG/PNG/GIF, sniffed from magic bytes)
  rejects exotic formats even if the underlying decoder could handle
  them.

#### Windows shell extension
- COM in-process server implementing `IInitializeWithStream` and
  `IThumbnailProvider` via the `windows` crate's `#[implement]` macro.
- Five DLL exports: `DllMain`, `DllGetClassObject`, `DllCanUnloadNow`,
  `DllRegisterServer`, `DllUnregisterServer`.
- 32-bit BGRA8 top-down DIB section creation for `IThumbnailProvider`.
- `OutputDebugStringW` log mirroring (every event is visible live in
  Sysinternals DebugView with the `[ThumbsUp]` filter).
- File-based diagnostics log at
  `%LOCALAPPDATA%\ThumbsUp\diagnostics.log`.
- **Previous-handler save/restore**: `register` records any pre-existing
  `.epub` thumbnail-handler CLSID under a `PreviousThumbnailHandler`
  registry value; `unregister` restores it. (Added after Icaros's
  [v3.3.0 release notes][r33] showed the value of clean-uninstall
  manners.)
- **Registration verification round-trip**: `register` reads back what
  it wrote and returns a clear `PermissionDenied` error citing
  "corrupted ACLs" if the round-trip fails. (Added after
  [Icaros Discussion #68][d68] traced the most common silent-failure
  mode to corrupted registry permissions.)

#### Configuration GUI
- `eframe`/`egui` tabbed app with Settings, Registration,
  Thumbnail Cache, Diagnostics, About.
- Registry-backed configuration writer matching the DLL's reader.
- Diagnostics log tail viewer with color-coded outcomes.
- Thumbnail-cache clearing helper.
- DebugView tip in the Diagnostics tab.

#### Installer & tooling
- WiX 3 authoring (`installer/Product.wxs`) with explicit registry
  entries (no `SelfReg`), Approved Shell Extensions list write, and a
  Start Menu shortcut.
- PowerShell sideloading helper (`scripts/register.ps1`).
- End-to-end MSI build script (`scripts/build-installer.ps1`) with
  hardened native-error handling.
- GitHub Actions workflow running core tests on Linux and a release
  build on Windows.

#### Documentation
- `README.md`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`,
  `docs/INSTALL.md`, `docs/LESSONS-FROM-DARKTHUMBS.md`,
  `docs/LESSONS-FROM-ICAROS.md`, this file.
- 79-test suite with descriptive test names, including 12 named
  after the prior-art issues they reproduce or guard against.

### Threat-model entries

The following threats are documented in `docs/SECURITY.md`:

- T1: Memory-safety bugs in ZIP/XML/image parsing.
- T2: Path traversal via cover href.
- T3: Zip-bomb amplification.
- T4: Image-decoder vulnerability.
- T5: XML external-entity / billion-laughs.
- T6: Hostile EPUB causing DLL panic.
- T7: Unauthorized registration.
- T8: Code-signing bypass.
- T9: Resource exhaustion via pathological archive structure.
- T10: XHTML cover wrapper as a second untrusted parse surface.
- T11: Codec expansion temptation.
- T12: Slow inputs as a denial-of-service against Explorer.
- T13: Non-ASCII filename evasion.

[dt9]:  https://github.com/fire-eggs/DarkThumbs/issues/9
[dt5]:  https://github.com/L0garithmic/DarkThumbs/issues/5
[i196]: https://github.com/Xanashi/Icaros/issues/196
[i212]: https://github.com/Xanashi/Icaros/issues/212
[d68]:  https://github.com/Xanashi/Icaros/discussions/68
[r33]:  https://github.com/Xanashi/Icaros/discussions/39
