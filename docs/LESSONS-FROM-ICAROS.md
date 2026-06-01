# Lessons learned from prior art: Icaros Shell Extensions

This document records the analysis of the [Xanashi/Icaros][icaros]
project — a closed-source but mature (1.5k★) Windows Explorer
thumbnail extension for video and audio formats — and the decisions
that analysis drove in this codebase.

Although Icaros doesn't handle EPUB, it is one of the longest-running
and most-used Windows shell extensions in the wild, and its public
issue tracker is therefore a uniquely good source of operational
gotchas that apply to *any* in-process thumbnail provider — especially
ones that read ZIP-derived formats. The lessons below come from the
issue tracker, the project's release notes, and several external
discussion threads that link back to it.

[icaros]: https://github.com/Xanashi/Icaros

## Summary

| # | Source                     | Lesson                                                     | Decision in this project        |
|---|----------------------------|------------------------------------------------------------|---------------------------------|
| 1 | [Issue #212][i212]         | Non-ASCII filenames in ZIPs                                | **Implemented** — slow-path raw byte fallback in `read_archive_file` |
| 2 | [Issue #196][i196] + others| Slow inputs hang Explorer                                  | **Implemented** — cooperative deadline, `extract_cover_with_deadline` |
| 3 | [3.3.0 release notes][r33] | Restore previous handler on uninstall                      | **Implemented** — `PreviousThumbnailHandler` value |
| 4 | [Discussion #68][d68]      | Corrupted ACLs on `HKCR\.ext\ShellEx` are common           | **Implemented** — registration verification with clear error |
| 5 | [Issue #211][i211]         | Configurable max file size                                 | Already in initial implementation |
| 6 | [TC forum][tcf]            | Non-square thumbnail sizes                                 | Already correct (we use `cx` only); locked in by tests |
| 7 | Internal Cache concept     | Windows aggressively deletes its thumbnail cache           | **Documented** as out-of-scope (see below) |
| 8 | Codec expansion            | Adding HEIC/AVIF/JXL                                       | Same answer as DarkThumbs lesson 5 — rejected for security |

[i212]: https://github.com/Xanashi/Icaros/issues/212
[i196]: https://github.com/Xanashi/Icaros/issues/196
[i211]: https://github.com/Xanashi/Icaros/issues/211
[d68]: https://github.com/Xanashi/Icaros/discussions/68
[r33]: https://github.com/Xanashi/Icaros/discussions/39
[tcf]: https://www.ghisler.ch/board/viewtopic.php?t=76960&start=15

## Detailed analysis

### 1 — Non-ASCII filenames in the ZIP central directory

Icaros [Issue #212][i212] is concrete: a CBZ archive whose entries
have Japanese hiragana in their names sometimes fails to thumbnail or
picks the wrong file. The root cause is the ZIP central directory's
filename-encoding ambiguity:

* The ZIP spec originally said filenames are CP437.
* General-Purpose Bit 11 ("language encoding flag") signals UTF-8.
* Many ZIP authoring tools write UTF-8 *without setting the flag*.

The Rust `zip` crate at v0.6 keys its internal hash table on the
*decoded* filename, using CP437 when the flag isn't set. A UTF-8
filename written without the flag therefore appears in the hash table
under a mojibake key, and our `archive.by_name(utf8_path)` lookup
misses.

**Fix**: a slow-path fallback in `cover::read_archive_file`. If
`by_name` returns `FileNotFound`, iterate the central directory and
compare each entry's `name_raw()` bytes against our path's UTF-8 bytes
directly. The slow path is O(n) over entries, but only runs after a
fast-path miss, and `MAX_ARCHIVE_ENTRIES = 50_000` bounds it.

The integration test
`icaros_issue212_non_ascii_filename_in_zip_central_directory` builds
an EPUB whose cover lives at `OEBPS/images/封面.jpg` (Chinese for
"cover.jpg") and verifies extraction succeeds.

### 2 — Slow inputs hang Explorer

Icaros [Issue #196][i196] reports Windows hanging when reading video
files with PCM audio streams. [Discussion #106][d106] reports
slowness across whole folders. Various forum threads document
"Explorer hangs and the green bar takes forever" symptoms with slow
or corrupt files. The pattern is consistent: a thumbnail provider
that takes too long blocks Explorer's thumbnail-rendering path,
making the whole UI feel frozen.

[d106]: https://github.com/Xanashi/Icaros/issues/106

We had a `MaxThumbnailMs` registry value and a GUI knob for it from
the start, but the DLL didn't actually enforce it. **Fix**: the new
`extract_cover_with_deadline` function takes a `max_duration` and
checks it at each major pipeline boundary:

| Checkpoint               | Strategy id                |
|--------------------------|----------------------------|
| Before opening the ZIP   | `deadline-pre-zip`         |
| After reading container.xml | `deadline-post-container` |
| After reading the OPF    | `deadline-post-opf-read`   |
| After parsing the OPF    | `deadline-post-opf-parse`  |
| After reading the cover  | `deadline-post-cover-read` |
| After image decode       | `deadline-post-decode`     |

On timeout, the function returns `EpubError::DeadlineExceeded`, which
the COM provider translates to `WTS_E_FAILEDEXTRACTION`, and Explorer
falls back to the generic icon.

We deliberately chose **cooperative checkpointing** over thread-based
preemption. Spawning a worker per thumbnail would burn threads in
Explorer's surrogate, and a cooperatively-aborted decoder leaves no
half-allocated GDI handles. The downside is that a checkpoint must
exist between the start and the slowest single operation in the
pipeline; the image decode is the longest, so a `MaxThumbnailMs` of
e.g. 500 ms might still be exceeded by a 600 ms decode of a single
huge JPEG. In practice the EPUB cover sizes (1600–2560 px) decode in
tens of ms, so a 5-second default is a comfortable ceiling.

### 3 — Restore previous file-association handler on uninstall

Icaros [v3.3.0 release notes][r33] include "File Explorer settings
that have been modified by Icaros is now reverted during uninstall."
Translated: when Icaros registers itself for `.mkv`, it remembers
whatever shell extension was previously associated with `.mkv`, and
on uninstall it restores that association rather than leaving `.mkv`
files with no thumbnail handler.

This matters more for a niche format like EPUB than it might first
seem. Many users evaluate multiple thumbnail handlers (we saw this
firsthand in DarkThumbs's bug tracker — users running CBXShell *and*
DarkThumbs concurrently). A clean uninstall that doesn't strand the
user is good citizenship.

**Fix** in `registry.rs::register`:

1. Before writing our CLSID into
   `HKCR\.epub\ShellEx\{thumbnail-iid}\(default)`, read the existing
   value.
2. If non-empty and not already our CLSID, save it under our own
   `HKCR\CLSID\{ours}\PreviousThumbnailHandler` value.

In `unregister`:

1. Read `PreviousThumbnailHandler` before deleting our CLSID key.
2. If present, restore it to the `.epub` association.
3. Otherwise, just delete the association.

### 4 — Registration silently fails on corrupted ACLs

Icaros [Discussion #68][d68] and several forum threads identify a
recurring symptom: a user installs Icaros, the installer claims
success, but no thumbnails appear — and the registry shows the
`HKCR\.ext\ShellEx` keys aren't actually present despite no error.
Root cause: corrupted ACLs on the parent registry key. `RegSetValueEx`
returns success but the write doesn't persist on the affected key.

**Fix**: `registry::register` now performs a verification step at the
end. After writing every required key, it reads back the
`HKCR\.epub\ShellEx\{thumbnail-iid}\(default)` value. If the read
either fails or returns a value other than our CLSID, we return an
`io::Error` of kind `PermissionDenied` with a message that explicitly
mentions "corrupted ACLs" so the user can google it and find existing
fixes. Better a loud-and-clear error than a silent fail.

### 5 — Maximum file size

Icaros [Issue #211][i211] requested a configurable file-size cap;
we already had `MaxFileBytes` from the start. Already-correct
findings are useful too — they confirm our defaults aren't out of
line with what users expect.

### 6 — Non-square thumbnail dimensions

[Total Commander forum][tcf] documents an Icaros bug where TC's
non-square `cx`/`cy` thumbnail request triggered an aspect-ratio
mismatch. `IThumbnailProvider::GetThumbnail` actually receives only a
single `cx` parameter — there is no `cy`. The shell fits whatever
bitmap we return into a square box of side `cx`, preserving the
bitmap's intrinsic aspect ratio.

Our pipeline already passes `cx` as `max_side` to
`image_ops::fit_into`, which preserves aspect ratio under a single-
side cap. The integration test `varying_thumbnail_sizes_all_succeed`
verifies that `cx ∈ {32, 64, 96, 128, 256, 512, 1024}` all return a
bitmap with both dimensions ≤ `cx`.

### 7 — Internal cache (Icaros Cache)

Icaros invested significant engineering in an *internal* thumbnail
cache, motivated by Windows aggressively deleting its own
`thumbcache_*.db` files. Three cache modes (Disabled, Static,
Dynamic), max size, min free space, exclusion list, location override,
and a "Cache Indexer" that pre-thumbnails entire folders. Users on
the issue tracker care about this feature deeply.

**Decision**: out of scope for v0.x of this project. Reasons:

* It would dwarf the rest of the project in code volume.
* It introduces a new on-disk format (`*.icdb`-style files) we'd
  have to maintain.
* It introduces filesystem-permission concerns of its own (Icaros's
  Discussion #34 documents AD/Domain user impersonation issues with
  cache-folder access).
* The current Windows behavior is improving — Win11 has more stable
  thumbnail caching than Win7/8 — so the user-pain motivation may
  recede.
* We provide a *Clear thumbnail cache* button in the GUI as a partial
  mitigation (forces Explorer to rebuild from us on next browse).

If users report repeated cache loss, this is the obvious next
feature to consider. The out-of-scope decision is recorded here so
it's not silently re-litigated.

### 8 — Codec expansion (HEIC, AVIF, JXL, etc.)

Same answer as the DarkThumbs analysis: rejected for security. See
`docs/SECURITY.md` T11.

## Architectural validations (no code change)

* Icaros's `IcarosConfig.exe` is a separate GUI app, not bundled into
  the DLL. Confirms our `thumbsup-config` design.
* Icaros uses `OutputDebugString` plus a debug-mode dialog for
  diagnostics. We adopt the former (added in the DarkThumbs
  iteration) but skip the latter — modal dialogs from a shell
  extension are user-hostile.
* Icaros has built up significant logic around per-user vs.
  per-machine registration, including impersonation handling. Our
  WiX installer uses per-machine HKLM, the GUI uses per-user HKCU
  via `regsvr32 /n /i:user`. Same split, simpler scope.

## Tests added under this analysis

| Test                                                      | Demonstrates                                       |
|-----------------------------------------------------------|----------------------------------------------------|
| `icaros_issue212_non_ascii_filename_in_zip_central_directory` | Cover at `OEBPS/images/封面.jpg` extracts cleanly |
| `icaros_issue196_deadline_check_aborts_slow_extraction`   | Zero-duration deadline trips at first checkpoint   |
| `icaros_issue196_generous_deadline_does_not_interfere`    | 60-second deadline never trips on a tiny synthetic |
| `icaros_issue196_no_deadline_param_is_unlimited`          | `extract_cover` (no-deadline entry) still works    |

Plus a new `EpubError::DeadlineExceeded` variant covered by the
deadline tests above.

Total test count: **78** (51 unit + 27 integration), up from 74
before this iteration.
