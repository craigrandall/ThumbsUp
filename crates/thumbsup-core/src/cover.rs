//! High-level "give me bytes, get a cover" entry point.
//!
//! This module is the seam between the format parsers and the thumbnail
//! handler. It accepts a byte slice (the entire EPUB read from the
//! `IStream` Explorer hands us) and returns either a ready-to-display
//! [`Thumbnail`] or a categorized [`EpubError`].

use std::io::Cursor;
use std::io::Read;
use std::time::{Duration, Instant};

use crate::container::parse_container_xml;
use crate::error::{EpubError, Result};
use crate::image_ops::{prepare_thumbnail, Thumbnail};
use crate::opf::{href_looks_like_image, CoverPolicy, ManifestItem, OpfPackage};
use crate::path::{dir_of, resolve_href};
use crate::xhtml::first_img_src;

/// Hard cap on the size of a single file we will read from inside the
/// archive. This is a defense-in-depth measure against zip-bombs: even if
/// the outer EPUB is small, an inner cover could be wildly oversized.
/// 64 MiB comfortably accommodates legitimate 2560px JPEGs.
const MAX_INNER_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Hard cap on the number of entries in the archive's central directory.
/// Lifted from the DarkThumbs project's [#5] bug, where pathological EPUBs
/// with 5,000–10,000+ internal files crashed Explorer due to deep parser
/// recursion. Our streaming parsers don't recurse, but central-directory
/// allocation still scales linearly with entry count, so we refuse to
/// even open archives above this threshold.
///
/// Real-world legitimate EPUBs have at most a few hundred entries; 50k
/// is two orders of magnitude headroom.
///
/// [#5]: https://github.com/L0garithmic/DarkThumbs/issues/5
const MAX_ARCHIVE_ENTRIES: usize = 50_000;

/// Cooperative deadline for the extraction pipeline.
///
/// `IThumbnailProvider` runs synchronously inside Explorer's COM
/// surrogate; if we take too long we make Explorer feel laggy and risk
/// the surrogate killing us mid-flight (see Icaros [Issue #196] and
/// Icaros [Discussion #106] — both document slow inputs causing
/// cascading Explorer problems). Rather than spawn a worker thread per
/// thumbnail (overhead) or rely on Explorer's own timeout (kills us
/// less politely), we cooperatively check the deadline at strategic
/// stage boundaries and abort with [`EpubError::DeadlineExceeded`].
///
/// [Issue #196]: https://github.com/Xanashi/Icaros/issues/196
/// [Discussion #106]: https://github.com/Xanashi/Icaros/issues/106
#[derive(Debug, Clone, Copy)]
struct Deadline {
    start:  Instant,
    budget: Duration,
}

impl Deadline {
    fn new(budget: Duration) -> Self {
        Self { start: Instant::now(), budget }
    }

    /// "No deadline" — used by the simple `extract_cover` entry point and
    /// by the test suite where deterministic behavior is preferred over
    /// wall-clock semantics.
    fn unlimited() -> Self {
        Self { start: Instant::now(), budget: Duration::from_secs(86_400) }
    }

    fn check(&self) -> std::result::Result<(), EpubError> {
        let elapsed = self.start.elapsed();
        if elapsed > self.budget {
            return Err(EpubError::DeadlineExceeded {
                elapsed_ms: elapsed.as_millis() as u64,
                limit_ms:   self.budget.as_millis() as u64,
            });
        }
        Ok(())
    }
}

/// Diagnostic record produced for every extraction attempt. The shell
/// extension forwards these into the optional log file consumed by the
/// configuration GUI's diagnostics view.
#[derive(Debug, Clone)]
pub struct ExtractionReport {
    /// EPUB version major as parsed from the OPF (2, 3, or 0 if unknown).
    pub epub_version_major: u8,
    /// Which strategy succeeded, or which failed last.
    pub strategy: &'static str,
    /// Path of the cover file inside the archive, if one was located.
    pub cover_path: Option<String>,
    /// Declared media type of the cover (from OPF), if a cover was located.
    pub cover_media_type: Option<String>,
}

/// The successful result of a full extraction: the rendered thumbnail plus
/// a diagnostic report. The report is always populated, including on the
/// successful path.
#[derive(Debug)]
pub struct ExtractedCover {
    pub thumbnail: Thumbnail,
    pub report:    ExtractionReport,
}

/// Extract a cover from in-memory EPUB bytes and produce a thumbnail
/// suitable for handing to `IThumbnailProvider`.
///
/// This is the simple, no-deadline entry point; it never aborts on
/// time. Production callers (the shell-extension DLL) should use
/// [`extract_cover_with_deadline`] instead so a malformed or
/// pathologically slow EPUB cannot stall Windows Explorer.
///
/// * `bytes`     — the full EPUB contents.
/// * `max_side`  — longest side (in pixels) the resulting thumbnail
///                 should occupy. The shell typically requests 32, 96,
///                 256, or 1024.
/// * `policy`    — whether to fall back to "first image in manifest"
///                 when no compliant cover declaration is found.
/// * `size_limit` — refuse to process EPUBs larger than this. Pass
///                 `u64::MAX` to disable.
pub fn extract_cover(
    bytes: &[u8],
    max_side: u32,
    policy: CoverPolicy,
    size_limit: u64,
) -> std::result::Result<ExtractedCover, (EpubError, ExtractionReport)> {
    extract_cover_with_deadline(bytes, max_side, policy, size_limit, None)
}

/// Same as [`extract_cover`] but enforces a soft cooperative deadline.
///
/// `max_duration` is the wall-clock budget, measured from the moment
/// this function is called. Once exceeded, the next deadline checkpoint
/// returns [`EpubError::DeadlineExceeded`]. Pass `None` for no limit
/// (equivalent to [`extract_cover`]).
pub fn extract_cover_with_deadline(
    bytes: &[u8],
    max_side: u32,
    policy: CoverPolicy,
    size_limit: u64,
    max_duration: Option<Duration>,
) -> std::result::Result<ExtractedCover, (EpubError, ExtractionReport)> {
    let deadline = match max_duration {
        Some(d) => Deadline::new(d),
        None    => Deadline::unlimited(),
    };
    extract_inner(bytes, max_side, policy, size_limit, deadline)
}

fn extract_inner(
    bytes: &[u8],
    max_side: u32,
    policy: CoverPolicy,
    size_limit: u64,
    deadline: Deadline,
) -> std::result::Result<ExtractedCover, (EpubError, ExtractionReport)> {
    let report_err = |err, strategy| {
        (
            err,
            ExtractionReport {
                epub_version_major: 0,
                strategy,
                cover_path: None,
                cover_media_type: None,
            },
        )
    };

    if (bytes.len() as u64) > size_limit {
        return Err(report_err(
            EpubError::TooLarge { size: bytes.len() as u64, limit: size_limit },
            "size-limit",
        ));
    }

    deadline.check().map_err(|e| report_err(e, "deadline-pre-zip"))?;

    // 1. Open ZIP.
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| report_err(EpubError::from(e), "open-zip"))?;

    // Defense in depth: refuse pathological archives outright. See the
    // MAX_ARCHIVE_ENTRIES doc-comment for the lesson behind this cap.
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(report_err(
            EpubError::TooLarge {
                size: archive.len() as u64,
                limit: MAX_ARCHIVE_ENTRIES as u64,
            },
            "too-many-entries",
        ));
    }

    // 2. Locate and parse META-INF/container.xml.
    let container_bytes = read_archive_file(&mut archive, "META-INF/container.xml")
        .map_err(|e| {
            let strategy = if matches!(e, EpubError::CoverFileMissing(_)) {
                "container-missing"
            } else {
                "container-read"
            };
            // Translate "file not in archive" into the more specific
            // MissingContainer error category.
            let err = match e {
                EpubError::CoverFileMissing(_) => EpubError::MissingContainer,
                other => other,
            };
            report_err(err, strategy)
        })?;
    deadline.check().map_err(|e| report_err(e, "deadline-post-container"))?;
    let opf_path = parse_container_xml(&container_bytes)
        .map_err(|e| report_err(e, "container-parse"))?;

    // 3. Read and parse the OPF.
    let opf_bytes = read_archive_file(&mut archive, &opf_path)
        .map_err(|e| {
            let err = match e {
                EpubError::CoverFileMissing(_) => EpubError::MissingOpf(opf_path.clone()),
                other => other,
            };
            report_err(err, "opf-read")
        })?;
    deadline.check().map_err(|e| report_err(e, "deadline-post-opf-read"))?;
    let pkg = OpfPackage::parse(&opf_bytes)
        .map_err(|e| report_err(e, "opf-parse"))?;
    deadline.check().map_err(|e| report_err(e, "deadline-post-opf-parse"))?;

    // ------------------------------------------------------------------
    // Cover-resolution priority. Order matters and is documented here so
    // it stays in sync with the table in docs/ARCHITECTURE.md.
    //
    //   1. EPUB 3 manifest item with properties="cover-image"
    //   2. EPUB 2 <meta name="cover" content="…"> manifest reference
    //   3. Conventional manifest id ("cover", "cover-image", "ci")
    //   4. <guide><reference type="thumbimagestandard" href="…"/>  (image)
    //   5. <guide><reference type="cover" href="…"/>                (image)
    //   6. <guide><reference type="cover" href="…"/>                (XHTML wrapper → first <img>)
    //   7. First image item in manifest          (only with FirstImageFallback)
    //
    // 1–3 produce a ManifestItem; 4–6 produce only an href (no
    // ManifestItem since the guide doesn't carry media-type); 7 produces
    // a ManifestItem again. We funnel everything through a single
    // (archive_path, declared_media_type, strategy_name) tuple.
    // ------------------------------------------------------------------
    let opf_dir = dir_of(&opf_path).to_string();
    let mut found: Option<(String, String, &'static str)> = None;

    // --- 1–3: manifest-based, spec / conventional. ---
    if let Ok(item) = pkg.resolve_cover(CoverPolicy::Strict) {
        let path = match resolve_href(&opf_dir, &item.href) {
            Ok(p) => p,
            Err(e) => return Err((
                e,
                ExtractionReport {
                    epub_version_major: pkg.version_major,
                    strategy: "path-traversal",
                    cover_path: Some(item.href.clone()),
                    cover_media_type: Some(item.media_type.clone()),
                },
            )),
        };
        found = Some((path, item.media_type.clone(), classify_strategy(&pkg, item)));
    }

    // --- 4: guide thumbimagestandard (always a direct image). ---
    if found.is_none() {
        if let Some(href) = pkg.guide_thumb_href.clone() {
            if href_looks_like_image(&href) {
                if let Ok(path) = resolve_href(&opf_dir, &href) {
                    found = Some((path, String::new(), "guide-thumb"));
                }
            }
        }
    }

    // --- 5–6: guide cover. Image directly, or XHTML wrapper we follow. ---
    if found.is_none() {
        if let Some(href) = pkg.guide_cover_href.clone() {
            if href_looks_like_image(&href) {
                if let Ok(path) = resolve_href(&opf_dir, &href) {
                    found = Some((path, String::new(), "guide-cover-image"));
                }
            } else {
                // Assume XHTML wrapper. Read it, find the first <img src>,
                // resolve that src relative to the XHTML's directory.
                if let Ok(xhtml_path) = resolve_href(&opf_dir, &href) {
                    if let Ok(xhtml_bytes) = read_archive_file(&mut archive, &xhtml_path) {
                        if let Ok(img_src) = first_img_src(&xhtml_bytes) {
                            let xhtml_dir = dir_of(&xhtml_path).to_string();
                            if let Ok(path) = resolve_href(&xhtml_dir, &img_src) {
                                found = Some((path, String::new(), "guide-cover-xhtml"));
                            }
                        }
                    }
                }
            }
        }
    }

    // --- 7: first-image fallback (policy-gated). ---
    if found.is_none() && policy == CoverPolicy::FirstImageFallback {
        if let Some(item) = pkg.manifest.iter().find(|i| i.is_image()) {
            if let Ok(path) = resolve_href(&opf_dir, &item.href) {
                found = Some((path, item.media_type.clone(), "first-image-fallback"));
            }
        }
    }

    let (cover_path, cover_media_type, strategy) = match found {
        Some(t) => t,
        None => {
            return Err((
                EpubError::NoCover,
                ExtractionReport {
                    epub_version_major: pkg.version_major,
                    strategy: "no-cover",
                    cover_path: None,
                    cover_media_type: None,
                },
            ));
        }
    };

    let cover_bytes = read_archive_file(&mut archive, &cover_path).map_err(|e| {
        (
            e,
            ExtractionReport {
                epub_version_major: pkg.version_major,
                strategy: "cover-read",
                cover_path: Some(cover_path.clone()),
                cover_media_type: Some(cover_media_type.clone()),
            },
        )
    })?;
    deadline.check().map_err(|e| {
        (
            e,
            ExtractionReport {
                epub_version_major: pkg.version_major,
                strategy: "deadline-post-cover-read",
                cover_path: Some(cover_path.clone()),
                cover_media_type: Some(cover_media_type.clone()),
            },
        )
    })?;

    // 6. Decode → fit → BGRA.
    let thumbnail = prepare_thumbnail(&cover_bytes, max_side).map_err(|e| {
        (
            e,
            ExtractionReport {
                epub_version_major: pkg.version_major,
                strategy: "image-decode",
                cover_path: Some(cover_path.clone()),
                cover_media_type: Some(cover_media_type.clone()),
            },
        )
    })?;
    deadline.check().map_err(|e| {
        (
            e,
            ExtractionReport {
                epub_version_major: pkg.version_major,
                strategy: "deadline-post-decode",
                cover_path: Some(cover_path.clone()),
                cover_media_type: Some(cover_media_type.clone()),
            },
        )
    })?;

    Ok(ExtractedCover {
        thumbnail,
        report: ExtractionReport {
            epub_version_major: pkg.version_major,
            strategy,
            cover_path: Some(cover_path),
            cover_media_type: Some(cover_media_type),
        },
    })
}

fn classify_strategy(pkg: &OpfPackage, item: &ManifestItem) -> &'static str {
    if item.has_cover_image_property() {
        "epub3-cover-image"
    } else if pkg.meta_cover_idref.as_deref() == Some(item.id.as_str()) {
        "epub2-meta-cover"
    } else if matches!(item.id.as_str(), "cover" | "cover-image" | "ci" | "coverimage") {
        "conventional-id"
    } else {
        "first-image-fallback"
    }
}

/// Read a file from the archive into memory, capping its size to guard
/// against zip-bomb-style amplification.
///
/// Two-tier lookup:
///
/// 1. **Fast path:** `ZipArchive::by_name` does an O(1) hash lookup
///    keyed by the *decoded* filename (which the `zip` crate decodes as
///    UTF-8 only when the General-Purpose Bit 11 "language encoding"
///    flag is set, otherwise as CP437).
///
/// 2. **Slow path:** if the fast lookup returns `FileNotFound`, iterate
///    every entry and compare its `name_raw()` bytes against our path's
///    UTF-8 bytes directly. This catches the case — documented in
///    Icaros [Issue #212] — where an EPUB authoring tool wrote UTF-8
///    filenames *without* setting the encoding flag, causing the `zip`
///    crate's hash key to be mis-decoded.
///
/// Cost: the slow path is O(n) over central-directory entries, but the
/// `MAX_ARCHIVE_ENTRIES` cap (50 000) bounds it, and it only runs after
/// a fast-path miss — i.e. only for the small minority of archives that
/// trip the bug.
///
/// [Issue #212]: https://github.com/Xanashi/Icaros/issues/212
fn read_archive_file<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    // --- Fast path: by_name. ---
    match archive.by_name(name) {
        Ok(file) => return read_zip_file(file),
        Err(zip::result::ZipError::FileNotFound) => { /* fall through */ }
        Err(e) => return Err(EpubError::from(e)),
    }

    // --- Slow path: iterate and compare raw bytes. ---
    let target = name.as_bytes();
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        if entry.name_raw() == target {
            return read_zip_file(entry);
        }
    }
    Err(EpubError::CoverFileMissing(name.to_string()))
}

fn read_zip_file(mut file: zip::read::ZipFile<'_>) -> Result<Vec<u8>> {
    let declared = file.size();
    if declared > MAX_INNER_FILE_BYTES {
        return Err(EpubError::TooLarge {
            size: declared,
            limit: MAX_INNER_FILE_BYTES,
        });
    }

    // Use take() so even a fraudulently-declared compressed-size cannot
    // make us read more than MAX_INNER_FILE_BYTES.
    let mut buf = Vec::with_capacity(declared as usize);
    file.by_ref()
        .take(MAX_INNER_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    if (buf.len() as u64) > MAX_INNER_FILE_BYTES {
        return Err(EpubError::TooLarge {
            size: buf.len() as u64,
            limit: MAX_INNER_FILE_BYTES,
        });
    }
    Ok(buf)
}
