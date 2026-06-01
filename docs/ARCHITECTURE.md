# Architecture

The project is split into three crates that communicate only through
well-defined data types — no shared globals, no leaking implementation
detail across boundaries.

```
              ┌────────────────────────┐
   bytes ───▶ │       thumbsup-core        │ ──▶ Thumbnail (BGRA8)
              │  (cross-platform)      │ ──▶ ExtractionReport
              └────────────────────────┘
                       ▲
                       │ links
                       │
       ┌───────────────┴───────────────┐
       │                               │
┌──────────────┐               ┌─────────────────┐
│ epub-thumb…  │               │ epub-thumb-     │
│ -nailer      │               │ config          │
│ (Windows)    │               │ (Windows)       │
│              │               │                 │
│ DLL inside   │               │ exe with egui   │
│ Explorer     │               │ UI for users    │
└──────────────┘               └─────────────────┘
```

## `thumbsup-core`

The only crate that knows the EPUB format. Built with
`#![forbid(unsafe_code)]`. Contains:

* `container.rs` — streaming SAX-style parse of `META-INF/container.xml`.
  Returns the OPF path, picking the first `<rootfile>` whose `media-type`
  is the OPF MIME, or the first rootfile if none are typed.
* `opf.rs` — streaming parse of the OPF package document. Captures
  enough state to resolve a cover image. The OPF parser captures the
  manifest, the EPUB-2 `<meta name="cover">` idref, and any
  `<guide><reference type="cover|thumbimagestandard">` hrefs.
* `xhtml.rs` — minimal XHTML scanner that returns the first
  `<img src="…">` value. Used only by the guide-cover-via-XHTML strategy
  (priority #6). Soft-fails on malformed input so the caller can fall
  through to the next strategy.
* `path.rs` — POSIX path resolution. `dir_of` + `resolve_href` together
  combine an OPF directory with a manifest item's `href`, normalize
  `.`/`..`/percent-escapes, and reject any path that would escape the
  archive root.
* `image_ops.rs` — Decode (whitelisted to JPEG/PNG/GIF), fit-into-box
  resize with Lanczos3, convert to top-down BGRA8 ready for
  `CreateDIBSection`.
* `cover.rs` — High-level pipeline: ZIP open → entry-count cap →
  container parse → OPF parse → cover lookup (7-tier priority below) →
  archive read → image decode → BGRA8. Returns a rich `ExtractionReport`
  even on success so the GUI's diagnostics view can show *which*
  strategy hit.

### Cover-resolution priority

The pipeline tries strategies in the order below; the first hit wins.
Strategy numbers are stable identifiers used in log lines and the
diagnostics view.

| # | Identifier              | Mechanism                                                  |
|---|-------------------------|------------------------------------------------------------|
| 1 | `epub3-cover-image`     | Manifest item with `properties="cover-image"`              |
| 2 | `epub2-meta-cover`      | `<meta name="cover" content="X">` → manifest item id="X"   |
| 3 | `conventional-id`       | Manifest item with id `cover`, `cover-image`, `ci`, …      |
| 4 | `guide-thumb`           | `<guide><reference type="thumbimagestandard" href="…"/>`   |
| 5 | `guide-cover-image`     | `<guide><reference type="cover" href="…"/>` (image href)   |
| 6 | `guide-cover-xhtml`     | `<guide><reference type="cover" href="…"/>` (XHTML wrapper)|
| 7 | `first-image-fallback`  | First image in manifest (only with `FirstImageFallback`)   |

Strategies 4–6 were added after analysis of the
[DarkThumbs](https://github.com/fire-eggs/DarkThumbs) project's bug
tracker (notably its
[Issue #9](https://github.com/fire-eggs/DarkThumbs/issues/9)) showed
real-world EPUBs whose covers are reachable only via the `<guide>`
element. See `docs/SECURITY.md` for the threat-model implications of
strategy #6, which is the only strategy that opens a *second* file
inside the archive (the XHTML wrapper).

### Defense-in-depth caps

The cover pipeline applies three independent size limits:

* `max_file_bytes` (configurable, default 256 MiB) on the outer EPUB.
* `MAX_ARCHIVE_ENTRIES = 50_000` on the central-directory entry count.
  Pathological EPUBs with hundreds of thousands of entries — a known
  failure mode in DarkThumbs's predecessor — are refused before the
  ZIP is opened. Real-world EPUBs have at most a few hundred entries.
* `MAX_INNER_FILE_BYTES = 64 MiB` (hard) on every individual archive
  member read, defending against zip-bomb amplification.

## `thumbsup-shell`

The COM in-process server. The smallest possible amount of unsafe code,
all isolated to ABI shims:

* `lib.rs` — DLL exports (`DllMain`, `DllGetClassObject`,
  `DllCanUnloadNow`, `DllRegisterServer`, `DllUnregisterServer`).
* `com.rs` — `EpubThumbnailProvider` (implements `IInitializeWithStream`
  and `IThumbnailProvider`) and `ClassFactory`.
* `stream.rs` — Reads the entire `IStream` Explorer hands us into a
  `Vec<u8>` with a configurable byte cap.
* `bitmap.rs` — Creates a top-down 32-bit DIB section and copies our
  BGRA pixels into it.
* `config.rs` — Reads `HKCU\Software\ThumbsUp` on every
  `Initialize`. Caching would buy nothing and would make the GUI tool's
  setting changes invisible until Explorer restart.
* `logging.rs` — Single-line tab-delimited log writer. No background
  threads, no logging framework — Explorer hosts are not the place to
  spawn anything.
* `registry.rs` — Same register/unregister logic used by both the GUI
  and the DLL's `DllRegisterServer` entry point.

## `thumbsup-config`

A small `eframe`/`egui` GUI. Five tabs:

1. **Settings** — every knob, with tooltips explaining what each does.
2. **Registration** — one-click register/unregister of the DLL.
3. **Thumbnail Cache** — clear `thumbcache_*.db` files.
4. **Diagnostics** — tail of the diagnostics log, color-coded by outcome.
5. **About**.

Communicates with the DLL strictly through the registry and the
diagnostics log file. There is no IPC, no shared memory, no extension
point that would let a hostile config corrupt the DLL.

## Threading

Per the COM Apartment model that thumbnail providers register under,
each provider instance is called only from a single thread. The DLL
relies on this for correctness but still wraps internal state in a
`Mutex` as a safety belt. `OBJECT_COUNT` is `AtomicI32` because
`DllCanUnloadNow` may be called from any thread.

## Why no `Send`/`Sync` panics?

`EpubThumbnailProvider` and `ClassFactory` contain only `Mutex<…>`,
`Vec<u8>`, and primitive counters — all `Send + Sync`. The `windows`
crate's `#[implement]` macro requires this. We never store a raw
`IStream`, only the bytes we read out of it.
