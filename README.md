# ThumbsUp 👍

A Windows 11 File Explorer thumbnail handler for `.epub` files. Reads the
cover image declared in the OPF package document and renders it as the
file's thumbnail in Explorer, the file picker, and any other shell host.

Implemented in Rust to avoid the entire memory-safety class of bugs in
code that runs inside Explorer and parses untrusted ZIP / XML / image
input.

## What it covers

* **EPUB 3** — manifest items with `properties="cover-image"`.
* **EPUB 2** — `<meta name="cover" content="…">` references.
* **Conventional fallbacks** — items with `id="cover"`, `id="cover-image"`,
  etc., for real-world EPUBs that don't quite follow the spec.
* **`<guide>` element fallbacks** — both `type="thumbimagestandard"`
  (direct image) and `type="cover"` (image *or* XHTML wrapper containing
  an `<img>`). Catches Random House and Penguin EPUB 2 titles whose
  cover is reachable only through this older mechanism.
* **Optional first-image fallback** — configurable via the GUI.
* **Path-traversal hardened** — covers whose href escapes the archive root
  are rejected before the file is read.
* **Size-capped** — both the outer EPUB and any inner archive entry have
  configurable / hard caps to neutralize zip-bomb amplification.
* **Entry-count capped** — archives with more than 50,000 internal
  entries are refused outright (defense against the file-count DoS class
  documented in the DarkThumbs project's bug tracker).

## Cover resolution priority

The pipeline tries strategies in this order; the first hit wins.

| # | Strategy                                         | Source                            |
|---|--------------------------------------------------|-----------------------------------|
| 1 | EPUB 3 `properties="cover-image"`                | OPF manifest                      |
| 2 | EPUB 2 `<meta name="cover" content="X">`         | OPF metadata + manifest by id     |
| 3 | Conventional ids: `cover`, `cover-image`, `ci`   | OPF manifest                      |
| 4 | `<guide><reference type="thumbimagestandard">`   | OPF guide (always direct image)   |
| 5 | `<guide><reference type="cover">` (image href)   | OPF guide                         |
| 6 | `<guide><reference type="cover">` (XHTML wrapper)| Read XHTML, find first `<img src>`|
| 7 | First image item in manifest                     | Only when policy allows           |

## Layout

```
thumbsup-shell/
├── Cargo.toml                       # Workspace root
├── crates/
│   ├── thumbsup-core/                   # Cross-platform parsing & image work
│   │   ├── src/
│   │   │   ├── container.rs         # META-INF/container.xml parser
│   │   │   ├── opf.rs               # OPF parser + cover resolution
│   │   │   ├── path.rs              # Path resolution + traversal protection
│   │   │   ├── image_ops.rs         # Decode → resize → BGRA8
│   │   │   ├── cover.rs             # Top-level "bytes in, thumbnail out"
│   │   │   ├── error.rs
│   │   │   └── lib.rs
│   │   └── tests/                   # Integration tests + fixture builder
│   ├── thumbsup-shell/            # The shell extension DLL (Windows-only)
│   │   ├── src/
│   │   │   ├── lib.rs               # DllMain, DllGetClassObject, …
│   │   │   ├── com.rs               # IThumbnailProvider + IClassFactory
│   │   │   ├── bitmap.rs            # BGRA → HBITMAP
│   │   │   ├── stream.rs            # IStream → Vec<u8>
│   │   │   ├── registry.rs          # Register / unregister helpers
│   │   │   ├── config.rs            # Registry-backed runtime config
│   │   │   ├── logging.rs           # File logging (no runtime, no threads)
│   │   │   └── clsid.rs             # CLSID + well-known constants
│   │   ├── exports.def
│   │   └── build.rs
│   └── thumbsup-config/           # GUI configuration tool (eframe/egui)
│       └── src/
│           ├── main.rs
│           ├── app.rs               # Tabbed UI
│           ├── settings.rs
│           ├── regio.rs             # Registry I/O
│           ├── cache.rs             # Thumbnail-cache clear
│           └── diag.rs              # Diagnostics log reader
├── installer/
│   ├── Product.wxs                  # WiX 3 authoring
│   ├── License.rtf
│   └── en-us.wxl
├── scripts/
│   ├── build-installer.ps1          # End-to-end MSI build
│   └── register.ps1                 # Sideload helper
└── docs/
    ├── ARCHITECTURE.md
    ├── INSTALL.md
    └── SECURITY.md
```

## Building

### Prerequisites

* Rust 1.74+ (stable channel, **MSVC** toolchain — not GNU).
* Visual Studio 2022 Build Tools with the C++ workload and Windows 11
  SDK 22621 (or later). The MSVC linker is required for the DLL.
* WiX Toolset 3.11 or 3.14 (for the installer; not needed for the DLL
  alone). Don't use WiX 4 — the authoring is incompatible.

If you're setting up fresh, see `docs/INSTALL.md` for the step-by-step.

### Build & test the parsing core (any OS)

```bash
cargo test -p thumbsup-core
```

This runs **79 tests** (51 unit, 28 integration) covering both EPUB
versions, malformed inputs, path traversal, image-format whitelisting,
guide-element fallbacks, non-ASCII filenames, deadline enforcement,
and end-to-end extraction.

### Build the DLL and GUI (Windows)

```powershell
cargo build --release --target x86_64-pc-windows-msvc `
  -p thumbsup-shell -p thumbsup-config
```

Outputs:

* `target\x86_64-pc-windows-msvc\release\thumbsup_shell.dll`
* `target\x86_64-pc-windows-msvc\release\thumbsup-config.exe`

The first build takes 3–5 minutes because the `windows`,
`windows-implement`, `windows-core`, and `image` crates compile from
source. Subsequent builds are incremental.

### Build the MSI

```powershell
.\scripts\build-installer.ps1
```

Produces `ThumbsUp.msi`. Code-sign before distribution:

```powershell
signtool.exe sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /a ThumbsUp.msi
```

## Installing

* **End users**: run the signed MSI. The installer registers the DLL
  per-machine and adds it to the Approved Shell Extensions list. A Start
  Menu shortcut to the configuration GUI is created.
* **Developers**: `scripts\register.ps1` registers the freshly-built DLL
  per-user without admin rights and restarts Explorer so the change takes
  effect immediately.

## Configuration

The configuration GUI (`thumbsup-config.exe`) writes to
`HKCU\Software\ThumbsUp`. The DLL re-reads these values on every
thumbnail request, so changes take effect on the next folder browse —
no Explorer restart required.

| Value             | Type   | Meaning                                       |
|-------------------|--------|-----------------------------------------------|
| `Enabled`         | DWORD  | Master kill-switch. 0 = pretend handler gone. |
| `MaxFileBytes`    | QWORD  | Skip EPUBs larger than this.                  |
| `MaxThumbnailMs`  | DWORD  | Per-thumbnail timeout, in milliseconds.       |
| `FallbackPolicy`  | DWORD  | 0 = strict, 1 = first-image fallback.         |
| `LoggingEnabled`  | DWORD  | 0/1 — write diagnostics log file.             |
| `LogPath`         | string | Override default log path.                    |

## Security model

* DLL parses untrusted ZIP, XML, and image data. All three are handled
  through memory-safe Rust crates (`zip`, `quick-xml`, `image`).
* The crate is `#![forbid(unsafe_code)]` for `thumbsup-core`. Unsafe code
  exists only in the shell-extension DLL where the COM ABI requires it,
  and only at the boundaries (`IStream` reading, `HBITMAP` creation).
* Path traversal in cover hrefs is detected and rejected before any read.
* Outer EPUB and inner archive members have hard size caps to prevent
  zip-bomb DoS against Explorer.
* Threading model is Apartment; concurrent calls into a single provider
  instance are not possible by Windows contract, but internal state is
  still mutex-guarded as defense in depth.

See `docs/SECURITY.md` for the full threat model.

## License

Dual-licensed under MIT or Apache-2.0, at your option. See
`LICENSE-MIT` and `LICENSE-APACHE`.
