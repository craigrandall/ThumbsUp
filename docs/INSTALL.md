# Installation

## Recommended: signed MSI

1. Download `ThumbsUp.msi` from the releases page.
2. Verify the signature (right-click → Properties → Digital Signatures).
3. Double-click to install. Accept the UAC prompt when asked — the
   installer needs admin to register the DLL machine-wide and to add it
   to the Approved Shell Extensions list under HKLM.
4. Open any folder containing `.epub` files. Thumbnails should appear
   automatically as you scroll.

If thumbnails do not appear:

* Open *ThumbsUp Settings* from the Start Menu.
* Go to the **Thumbnail Cache** tab and click **Clear thumbnail cache**.
* Browse the folder again. Explorer rebuilds thumbnails on demand.

## Sideloading (developers)

```powershell
cargo build --release --target x86_64-pc-windows-msvc -p thumbsup-shell
.\scripts\register.ps1
```

This registers the freshly-built DLL under HKCU (no admin required) and
restarts Explorer so the change takes effect immediately.

To remove:

```powershell
.\scripts\register.ps1 -Unregister
```

## Uninstalling

* MSI install: Settings → Apps → ThumbsUp → Uninstall.
* Sideload: `.\scripts\register.ps1 -Unregister`.

## Troubleshooting

| Symptom                                    | Likely cause                                              | Fix                                                                     |
|--------------------------------------------|-----------------------------------------------------------|-------------------------------------------------------------------------|
| Thumbnails never appear                    | Cache holds the old generic icons.                        | Clear thumbnail cache via the GUI's Cache tab.                          |
| Some EPUBs work, others show generic icon  | Those EPUBs declare no compliant cover.                   | Settings tab → Fallback Strategy → "First image in archive".            |
| Generic icon for very large EPUBs          | File exceeds the configured size cap.                     | Settings tab → raise *Maximum file size to process*.                    |
| Want to know exactly why an EPUB fails     | Logging is off by default.                                | Settings tab → enable Logging, then check the Diagnostics tab.          |
| Installer reports "another version exists" | Older version still present.                              | Uninstall via Settings → Apps first, then reinstall.                    |

## Build troubleshooting

If you're building the project from source and the build fails, check
the following common causes before filing a bug.

### "linker `link.exe` not found"

Visual Studio Build Tools weren't installed before Rust, or the C++
workload wasn't selected. Install the Build Tools with the C++
workload + Windows 11 SDK component, reboot, and re-run
`rustup default stable-x86_64-pc-windows-msvc`.

### `unresolved import windows::Win32::System::SystemServices`

Your local resolution of the `windows` crate is missing the
`Win32_System_SystemServices` feature. The pinned `Cargo.toml` in this
repo sets it; if you've forked or modified the manifest, restore that
feature flag.

### `no IFoo_Impl in Win32::…`

The `windows` crate's `#[implement]` proc-macro generates code that
references `_Impl` traits, but those traits are gated behind the
`implement` feature flag. Make sure `"implement"` appears in the
`features` list of the `windows` dependency in the workspace
`Cargo.toml`.

### `cargo update -p windows-core --precise 0.58.0` is ambiguous

`windows-core` is pulled in twice in this workspace: at version 0.58.0
by the shell extension DLL (pinned), and at a newer version by the
`eframe`/`egui` GUI crate (transitive, unpinned). Disambiguate by
including the version in the spec:

```powershell
cargo update -p windows-core@0.58.0 --precise 0.58.0
```

This is harmless and recommended after a fresh checkout. The two
`windows-core` copies don't interfere with each other; each crate
links against its own.

### `cannot find function RegCreateKeyExW`

Same root cause as the `IFoo_Impl` errors: a missing feature flag on
the `windows` dependency. `RegCreateKeyExW`'s signature references a
type from `Win32_Security`, so that feature must be enabled too. Both
flags are set in the pinned manifest; check it hasn't been edited.

### Tests pass but the DLL build fails or vice-versa

The two halves of the workspace have different feature requirements.
`thumbsup-core` builds anywhere with default features. `thumbsup-shell`
needs the MSVC toolchain, the Windows SDK, and the full `windows`
feature set. Always run with `--target x86_64-pc-windows-msvc`
explicitly when building the DLL; without it cargo may pick the GNU
toolchain on some Rust installations.
