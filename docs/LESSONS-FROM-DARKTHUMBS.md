# Lessons learned from prior art: DarkThumbs

This document records the analysis of the [fire-eggs/DarkThumbs][dt]
project (a popular C++ shell-extension that adds thumbnail previews for
EPUB and several other ebook/archive formats) and the decisions that
analysis drove in this codebase.

DarkThumbs has been deployed in the wild for years (~630 GitHub stars
at archive time) and inherits its EPUB code through several forks
back to T800G's original CBXShell. Its bug tracker is therefore an
unusually rich source of real-world EPUB malformation patterns.

[dt]: https://github.com/fire-eggs/DarkThumbs

## Summary

| # | Finding (from DarkThumbs)                                       | Decision in this project        |
|---|------------------------------------------------------------------|---------------------------------|
| 1 | `<guide><reference type="cover">` is a real-world cover declaration | **Implemented** as strategies 4–6 |
| 2 | OPF path is non-standard in some EPUBs                          | **Already correct**, locked in by test |
| 3 | Stack overflow on archives with many internal files             | **Mitigated** by `MAX_ARCHIVE_ENTRIES` |
| 4 | `OutputDebugString` for live diagnostics                        | **Implemented** in `logging.rs` |
| 5 | Codec expansion (HEIC/HEIF/AVIF/JXL via WIC)                    | **Rejected** for security reasons |

## Detailed analysis

### Finding 1 — `<guide><reference type="cover">` is missing from naive parsers

DarkThumbs [Issue #9 "Case 2"][i9] documents a real Random House EPUB
with this OPF shape:

```xml
<metadata>
  <meta name="cover" content="cover-image"/>   <!-- DANGLING idref -->
</metadata>
<manifest>
  <item id="fcvi" href="OEBPS/images/Mich_..._cvt_r1.jpg" media-type="image/jpeg"/>
  <!-- no item has id="cover-image" -->
</manifest>
<guide>
  <reference href="OEBPS/Mich_..._cvi_r1.htm" type="cover"/>
  <reference href="OEBPS/images/Mich_..._cvt_r1.jpg" type="thumbimagestandard"/>
</guide>
```

Why the standard strategies fail on this file:

* EPUB 3 `properties="cover-image"` — not present (this is EPUB 2).
* EPUB 2 `<meta name="cover">` — present but the idref `"cover-image"`
  doesn't match any manifest item.
* Conventional id (`id="cover"`, `id="cover-image"`) — not present.

Both `<guide>` references provide a way through:

* `type="thumbimagestandard"` points directly at a JPEG.
* `type="cover"` points at an XHTML wrapper, which contains
  `<img src="images/..._cvt_r1.jpg"/>`.

DarkThumbs's response was a fall-back to "first image in the manifest",
which often picks the wrong image (a chapter illustration). Our
response is to add three new strategies (4, 5, 6 in the priority
table) that exhaust the legal `<guide>` paths before resorting to
first-image.

[i9]: https://github.com/fire-eggs/DarkThumbs/issues/9

### Finding 2 — Don't hard-code `OEBPS/content.opf`

The same Random House example uses `Mich_9780307790361_epub_opf_r1.opf`
at the archive root rather than the conventional `OEBPS/content.opf`.
Per the OCF spec the OPF location is whatever `META-INF/container.xml`
says — never assume.

Our implementation has always honored container.xml correctly. The
finding drove the addition of an explicit regression test
(`darkthumbs_issue9_case2_non_standard_opf_filename`) that uses an
OPF at archive root with an unusual name; a future refactor can't
silently regress this.

### Finding 3 — File-count DoS

DarkThumbs [Issue #5 (L0garithmic fork)][i5] traces Explorer crashes
to EPUBs with thousands or tens of thousands of internal files.
Debugging notes mention a stack overflow during parse.

Our streaming parsers don't recurse, so we're immune to the specific
crash class. But `zip::ZipArchive::new` allocates a hash map
proportional to entry count. We added `MAX_ARCHIVE_ENTRIES = 50_000`
as a pre-flight check: archives above this size are refused before
the central directory is examined. Legitimate EPUBs have at most a
few hundred entries, so the cap is two orders of magnitude of
headroom and triggers only on adversarial input.

[i5]: https://github.com/L0garithmic/DarkThumbs/issues/5

### Finding 4 — `OutputDebugString` is the Windows-shell-extension diagnostic norm

DarkThumbs's standard troubleshooting workflow ships a debug build
and asks users to attach Sysinternals **DebugView** to capture
`OutputDebugString` output. This is a long-established pattern for
Explorer-loaded extensions because attaching a real debugger to
Explorer is invasive and risky.

Our `logging.rs` already wrote a tab-delimited file log. We extended
it to *also* call `OutputDebugStringW` on every event, prefixed with
`[ThumbsUp]` for easy filtering in DebugView. The file log
remains for postmortem analysis; the OutputDebugString stream is for
live debugging without Explorer restart. The Diagnostics tab in the
GUI advertises this to users.

### Finding 5 — Codec expansion is a security trade-off

DarkThumbs V2.x added HEIC, HEIF, AVIF, and JXL support by routing
cover decoding through Windows Imaging Component. This is convenient
for users with covers in those formats, but every additional codec
is an additional attack surface — image-decoder CVEs are common, and
Windows ships several with extension installers from third parties.

We deliberately kept the JPEG/PNG/GIF whitelist sniffed from magic
bytes. The trade-off is documented in `docs/SECURITY.md` (T11).
Future codec support, if added, should go through the same
content-sniffing whitelist rather than blanket WIC dispatch.

## Architectural validations (no code change)

These DarkThumbs observations confirmed existing decisions in this
project rather than driving new code:

* DarkThumbs ships a separate **CBXManager** GUI for configuration —
  validates our `thumbsup-config` decision over baking config into
  the DLL.
* Recurring memory and GDI handle leaks across years of C++ commits
  — validates choosing Rust over C++ for the in-process shell
  extension.
* DarkThumbs's "first image" fallback being widely useful but
  occasionally wrong — validates making it a *user-configurable
  policy* (Strict vs FirstImageFallback) rather than always-on.

## Tests added under this analysis

All eight reside in `crates/thumbsup-core/tests/integration.rs` with names
beginning `darkthumbs_`:

| Test                                                      | Demonstrates                                       |
|-----------------------------------------------------------|----------------------------------------------------|
| `darkthumbs_issue9_case2_random_house_guide_xhtml_wrapper` | End-to-end: guide → XHTML → img src → bytes        |
| `darkthumbs_issue9_case2_non_standard_opf_filename`        | OPF at archive root with non-standard name         |
| `darkthumbs_thumbimagestandard_direct_image`              | `type="thumbimagestandard"` direct image           |
| `darkthumbs_guide_cover_direct_image`                      | `type="cover"` pointing at an image, not XHTML     |
| `darkthumbs_priority_manifest_beats_guide`                 | Spec-compliant manifest item wins over guide       |
| `darkthumbs_priority_guide_beats_first_image`              | Guide wins over first-image even with permissive policy |
| `darkthumbs_xhtml_wrapper_with_relative_path`              | `<img src>` resolved relative to XHTML, not OPF    |
| `darkthumbs_xhtml_wrapper_with_no_img_falls_through`       | Pathological wrapper falls through cleanly         |

Plus 4 OPF-parser unit tests in `opf::tests` (capture guide cover and
thumbnail hrefs, ignore guide outside `<guide>`, image-extension
heuristic) and 7 XHTML-scanner unit tests in `xhtml::tests`. Total:
+19 tests, bringing the suite from 55 to 74.
