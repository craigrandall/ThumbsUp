# Security Threat Model

## Trust boundary

The DLL is loaded into `dllhost.exe` (the COM surrogate Explorer uses for
shell extensions on modern Windows) and asked to parse content that came
from the network — books downloaded by the user from any source.
**Every byte of input is untrusted.**

## Threats considered

### T1 — Memory-safety bugs in ZIP / XML / image parsing

Mitigation: the entire core parsing path is in safe Rust. The `thumbsup-core`
crate is `#![forbid(unsafe_code)]`. The crates we depend on (`zip`,
`quick-xml`, `image`) are all written in safe Rust.

### T2 — Path traversal via cover href

A malicious EPUB could include a manifest item whose `href` is
`../../../etc/passwd` or, on Windows, `..\..\Windows\System32\…`.

Mitigation: `path::resolve_href` normalizes `.` and `..` segments and
returns `EpubError::PathTraversal` for any path that would escape the
archive root. The unit-test suite includes specific cases for
`../../etc/passwd`, `../cover.jpg` at archive root, and Windows-style
backslash separators (`images\\cover.jpg`).

Even if the path *did* somehow escape, we read it via the ZIP archive's
own `by_name` API, which only returns archive members — the OS file
system is never consulted.

### T3 — Zip-bomb amplification

A small EPUB could declare an inner cover with a wildly large
uncompressed size, exhausting Explorer's memory.

Mitigation: two-tiered size cap.
* **Outer** (configurable, default 256 MiB): the entire EPUB read from
  `IStream` is rejected if it exceeds the cap.
* **Inner** (hard, 64 MiB): every individual archive member read is
  bounded by both the declared `file.size()` and a `Read::take` so a
  fraudulently-low declared size cannot trick us into reading more.

### T4 — Image-decoder vulnerability

Image decoders are historically a fertile source of CVEs.

Mitigation: format whitelist. We sniff magic bytes via
`image::guess_format` and refuse anything that isn't JPEG, PNG, or GIF —
even though the underlying crate could decode more formats. SVG is
explicitly excluded because it's an XML format that has been a
recurrent source of XXE-style issues in other ecosystems.

### T5 — XML external-entity / billion-laughs in OPF or container.xml

Mitigation: `quick-xml` is a pull parser that does not resolve external
entities by default. Our parsers ignore entity declarations entirely —
we only look at start/end tags and attributes.

### T6 — Hostile EPUB causing DLL panic and crashing Explorer

Even safe Rust can panic (e.g. `unwrap()` on a malformed input).

Mitigation:
* `panic = "abort"` is set in the release profile so a panic terminates
  `dllhost.exe`, the COM surrogate, rather than corrupting Explorer's
  state.
* The release builds use `lto = "thin"` and `strip = true` to minimize
  the attack surface from accidental leaked debug info.
* All `unwrap`/`expect` calls in the production paths have been audited.
  The error path of `extract_cover` returns `Result` rather than
  panicking; `Mutex::lock()` failures (poisoning) are translated to
  `E_FAIL` rather than propagating.

### T9 — Resource exhaustion via pathological archive structure

The DarkThumbs project's bug tracker documents EPUBs with 5,000–10,000+
internal files crashing Windows Explorer. The original C++ extension's
parsers walked the archive recursively; ours don't, but the central
directory still allocates O(n) on entry count.

Mitigation: `MAX_ARCHIVE_ENTRIES = 50_000`. We refuse to open archives
above this threshold without ever reading their entries. Legitimate
EPUBs have at most a few hundred entries; the cap is two orders of
magnitude of headroom.

### T10 — XHTML cover wrapper as a second untrusted parse surface

Strategy #6 (`guide-cover-xhtml`) opens a *second* file from inside the
archive — the XHTML cover wrapper — and parses it to find the embedded
`<img>` element. This widens the attack surface compared to strategies
1–5, which only ever read the OPF and the cover image itself.

Mitigation:
* The XHTML scanner (`xhtml::first_img_src`) is built on the same
  `quick-xml` pull parser as the OPF parser; no new XML library is
  introduced.
* The scanner reads only start-tag events and never resolves entities
  or follows references — it cannot be tricked into XXE-style attacks.
* The scanner soft-fails on any malformed input (returns `NoCover`)
  so a malformed wrapper merely causes us to try the next fallback,
  not propagate an error to Explorer.
* The XHTML file is read through the same `read_archive_file` path as
  every other inner member, inheriting the `MAX_INNER_FILE_BYTES` cap.
* The `<img src>` target is resolved relative to the XHTML's directory
  with the same path-traversal protection as direct cover hrefs.

### T11 — Codec expansion temptation

The DarkThumbs project added support for HEIC/HEIF/AVIF/JXL via the
Windows Imaging Component because users requested non-standard cover
formats. Each new codec is also new attack surface (CVEs in image
decoders are routine).

Mitigation: we deliberately maintain a JPEG/PNG/GIF whitelist sniffed
from magic bytes, and reject every other format with a clean
`ImageDecode` error. Users who want exotic formats can fork; the
default build will not load them.

### T12 — Slow inputs as a denial-of-service against Explorer

Icaros's bug tracker (Issues #196, #106, and several forum threads)
documents a consistent pattern: a thumbnail provider that takes too
long blocks Explorer's whole thumbnail-rendering path, making the UI
feel frozen. A hostile EPUB could be crafted with a maximum-size,
maximally-compressed cover whose decode dominates wall-clock time,
without ever crossing a memory limit.

Mitigation: the cooperative deadline introduced in
`extract_cover_with_deadline`. The DLL passes the user-configured
`MaxThumbnailMs` (default 5 s) and the pipeline checks it at six
strategic boundaries. On timeout we abort with
`EpubError::DeadlineExceeded`, which the COM provider translates to
`WTS_E_FAILEDEXTRACTION`; Explorer falls back to the generic icon
without ever waiting more than `MaxThumbnailMs` per file.

We chose cooperative checkpointing over thread-based preemption
because spawning a worker thread per thumbnail wastes resources in
Explorer's COM surrogate, and a cooperatively-aborted decoder leaves
no half-allocated GDI handles.

### T13 — Non-ASCII filename evasion

A hostile or simply careless EPUB authoring tool can write UTF-8
filenames into the ZIP central directory *without* setting the
General-Purpose Bit 11 "language encoding flag". The `zip` crate's
`by_name` hash lookup is keyed on the decoded filename, so under
this condition our intended cover lookup misses and we'd otherwise
fall back to a default thumbnail — even though the file is right
there in the archive.

This class of bug is documented in Icaros [Issue #212][i212]: the
original symptom is a wrong-or-missing thumbnail, not a security
issue, but the same mismatch could in principle be used to hide a
malicious payload from a path-traversal check (the check sees one
path; the archive holds the file under another byte sequence).

Mitigation: `cover::read_archive_file` falls back to a slow-path
iteration that compares `name_raw()` bytes against our requested
path's UTF-8 bytes directly. This restores the round-trip property —
"if the OPF declares a path X, and X exists in the archive under
*any* encoding interpretation, we read the file at X" — that the
path-traversal protection in `path::resolve_href` depends on.

[i212]: https://github.com/Xanashi/Icaros/issues/212

### T7 — Unauthorized registration

A non-admin user installing the per-machine MSI requires elevation. The
GUI's per-user register/unregister flow uses HKCU only and never touches
HKLM, matching standard Windows user-scope expectations.

### T8 — Code-signing bypass

The DLL must be code-signed for production use. Unsigned shell
extensions face SmartScreen friction and are blocked outright by some
enterprise policies. Signing is out of scope for the build system but
the `scripts\build-installer.ps1` includes the signtool invocation.

See `docs\SIGNING.md` for project code signing strategy details.

## Out of scope

* Defending against a compromised installation directory. If an attacker
  has Program Files write access, they have already won.
* Protecting against malicious *configuration* values in HKCU. The DLL
  treats every setting as untrusted user input (clamped, validated) but
  a user who writes garbage into their own registry can break their own
  thumbnails — the threat surface is contained to that user.
