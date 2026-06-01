//! End-to-end integration tests: build a synthetic EPUB in memory, then
//! drive [`thumbsup_core::extract_cover`] over the bytes exactly the way the
//! shell extension will.

mod common;

use common::*;
use thumbsup_core::{extract_cover, extract_cover_with_deadline, CoverPolicy, EpubError};

const NO_LIMIT: u64 = u64::MAX;

#[test]
fn extracts_cover_from_epub3() {
    let bytes = epub3_with_cover_image_property();
    let result = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT)
        .expect("should extract");
    assert_eq!(result.report.epub_version_major, 3);
    assert_eq!(result.report.strategy, "epub3-cover-image");
    assert_eq!(result.report.cover_path.as_deref(), Some("OEBPS/images/cover.jpg"));
    assert_eq!(result.report.cover_media_type.as_deref(), Some("image/jpeg"));
    // 800x1200 fitted into 256 → 256 high, ~170 wide.
    assert!(result.thumbnail.height <= 256);
    assert!(result.thumbnail.width  <= 256);
    assert!(result.thumbnail.width  > 0);
    assert!(result.thumbnail.height > 0);
    assert_eq!(result.thumbnail.byte_len(), result.thumbnail.pixels.len());
}

#[test]
fn extracts_cover_from_epub2() {
    let bytes = epub2_with_meta_cover();
    let result = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT)
        .expect("should extract");
    assert_eq!(result.report.epub_version_major, 2);
    assert_eq!(result.report.strategy, "epub2-meta-cover");
    assert_eq!(result.report.cover_path.as_deref(), Some("OEBPS/cover.png"));
}

#[test]
fn epub_with_conventional_cover_id_only() {
    // Neither EPUB 3 property nor EPUB 2 meta — just `id="cover"`.
    let cover = solid_jpeg(400, 600, [10, 10, 200]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="cover" href="cover.jpg" media-type="image/jpeg"/>
    <item id="nav"   href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
  </manifest>
  <spine/>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/cover.jpg", cover)
        .build();

    let result = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(result.report.strategy, "conventional-id");
}

#[test]
fn first_image_fallback_kicks_in_only_with_policy() {
    // No cover declaration at all; manifest has a non-cover image.
    let img = solid_png(120, 200, [80, 80, 80, 255]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="fig" href="figure.png" media-type="image/png"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/figure.png", img)
        .add_file("OEBPS/ch1.xhtml", b"<html/>".to_vec())
        .build();

    // Strict refuses.
    let strict = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT);
    assert!(matches!(strict, Err((EpubError::NoCover, _))));

    // FirstImageFallback succeeds.
    let lenient = extract_cover(&bytes, 256, CoverPolicy::FirstImageFallback, NO_LIMIT)
        .expect("fallback should succeed");
    assert_eq!(lenient.report.strategy, "first-image-fallback");
    assert_eq!(lenient.report.cover_path.as_deref(), Some("OEBPS/figure.png"));
}

#[test]
fn missing_container_xml_is_categorized() {
    // Build an archive that has an OPF but no META-INF/container.xml.
    let bytes = EpubBuilder::new()
        .add_file("OEBPS/content.opf", b"<package/>".to_vec())
        .build();
    let err = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap_err();
    assert!(matches!(err.0, EpubError::MissingContainer));
    assert_eq!(err.0.category(), "missing-container");
}

#[test]
fn malformed_zip_is_categorized() {
    let bytes = b"PK\x03\x04 totally not a real zip";
    let err = extract_cover(bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap_err();
    assert!(matches!(err.0, EpubError::MalformedZip(_)));
    assert_eq!(err.0.category(), "malformed-zip");
}

#[test]
fn missing_opf_is_categorized() {
    // container.xml claims OEBPS/ghost.opf but nothing is at that path.
    let container = br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/ghost.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let bytes = EpubBuilder::new()
        .container_xml(container.to_vec())
        .build();
    let err = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap_err();
    assert!(matches!(err.0, EpubError::MissingOpf(_)));
}

#[test]
fn cover_with_corrupt_image_data_is_categorized() {
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/cover.jpg", b"not actually a jpeg".to_vec())
        .build();
    let err = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap_err();
    assert!(matches!(err.0, EpubError::ImageDecode(_)));
    assert_eq!(err.0.category(), "image-decode");
}

#[test]
fn declared_cover_file_missing_is_categorized() {
    // OPF references cover.jpg but the file isn't in the archive.
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .build();
    let err = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap_err();
    assert!(matches!(err.0, EpubError::CoverFileMissing(_)));
}

#[test]
fn opf_at_archive_root_resolves_cover_correctly() {
    // OPF directly at root, not under OEBPS/.
    let cover = solid_png(50, 80, [10, 20, 30, 255]);
    let container = br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
</package>"#;
    let bytes = EpubBuilder::new()
        .opf_path("package.opf")
        .container_xml(container.to_vec())
        .opf_xml(opf.to_vec())
        .add_file("cover.png", cover)
        .build();
    let result = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(result.report.cover_path.as_deref(), Some("cover.png"));
}

#[test]
fn nested_opf_resolves_relative_href() {
    // OPF at OEBPS/text/pkg.opf, cover at OEBPS/images/cover.jpg.
    let cover = solid_jpeg(100, 150, [128, 128, 128]);
    let container = br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/text/pkg.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="../images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
    let bytes = EpubBuilder::new()
        .opf_path("OEBPS/text/pkg.opf")
        .container_xml(container.to_vec())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/images/cover.jpg", cover)
        .build();
    let result = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(result.report.cover_path.as_deref(), Some("OEBPS/images/cover.jpg"));
}

#[test]
fn path_traversal_is_blocked() {
    // OPF declares a cover whose href escapes the archive root. The
    // extraction must refuse with PathTraversal — not attempt to read it.
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="../../etc/passwd" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .build();
    let err = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap_err();
    assert!(matches!(err.0, EpubError::PathTraversal(_)));
    assert_eq!(err.0.category(), "path-traversal");
}

#[test]
fn size_limit_is_enforced() {
    let bytes = epub3_with_cover_image_property();
    let limit = (bytes.len() as u64).saturating_sub(1);
    let err = extract_cover(&bytes, 256, CoverPolicy::Strict, limit).unwrap_err();
    assert!(matches!(err.0, EpubError::TooLarge { .. }));
}

#[test]
fn extraction_is_deterministic() {
    // Same input must produce identical pixel output across calls.
    let bytes = epub3_with_cover_image_property();
    let a = extract_cover(&bytes, 96, CoverPolicy::Strict, NO_LIMIT).unwrap();
    let b = extract_cover(&bytes, 96, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(a.thumbnail.width,  b.thumbnail.width);
    assert_eq!(a.thumbnail.height, b.thumbnail.height);
    assert_eq!(a.thumbnail.pixels, b.thumbnail.pixels);
}

#[test]
fn varying_thumbnail_sizes_all_succeed() {
    // Explorer asks for many sizes (32, 96, 256, 1024); make sure each works.
    let bytes = epub3_with_cover_image_property();
    for size in [32, 64, 96, 128, 256, 512, 1024] {
        let r = extract_cover(&bytes, size, CoverPolicy::Strict, NO_LIMIT)
            .unwrap_or_else(|e| panic!("size {size} failed: {:?}", e.0));
        assert!(r.thumbnail.width  <= size, "size {size}: w={}", r.thumbnail.width);
        assert!(r.thumbnail.height <= size, "size {size}: h={}", r.thumbnail.height);
    }
}

// ---------------------------------------------------------------------
//  DarkThumbs lessons learned (https://github.com/fire-eggs/DarkThumbs)
// ---------------------------------------------------------------------
// The next few tests reproduce real-world EPUB shapes from DarkThumbs's
// bug tracker that the original C++ implementation got wrong. Each test
// is named for the issue it covers and explains the failure mode.

#[test]
fn darkthumbs_issue9_case2_random_house_guide_xhtml_wrapper() {
    // Random House EPUB 2 with cover declared only via <guide>: the
    // type="cover" reference points at an XHTML page that contains the
    // real <img>. Original DarkThumbs picked the first image and got
    // it wrong.
    let cover = solid_jpeg(800, 1200, [10, 50, 200]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata>
    <meta name="cover" content="cover-image"/>  <!-- dangling: no item with this id -->
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="cvi" href="cvi.htm" media-type="application/xhtml+xml"/>
    <item id="f001" href="images/f001.jpg" media-type="image/jpeg"/>
    <item id="fcvi" href="images/cvt.jpg" media-type="image/jpeg"/>
    <item id="ftp" href="images/tp.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine toc="ncx"><itemref idref="cvi"/></spine>
  <guide>
    <reference href="cvi.htm" title="cover" type="cover"/>
  </guide>
</package>"#;
    let cvi_html = br#"<?xml version='1.0' encoding='utf-8'?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <body>
    <div class="cover">
      <img alt="" src="images/cvt.jpg"/>
    </div>
  </body>
</html>"#;

    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/cvi.htm", cvi_html.to_vec())
        .add_file("OEBPS/images/f001.jpg", solid_jpeg(40, 60, [99, 99, 99]))
        .add_file("OEBPS/images/cvt.jpg", cover)  // the *correct* cover
        .add_file("OEBPS/images/tp.jpg", solid_jpeg(40, 60, [99, 99, 99]))
        .build();

    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT)
        .expect("guide-via-XHTML strategy must locate the cover");
    assert_eq!(r.report.strategy, "guide-cover-xhtml");
    assert_eq!(r.report.cover_path.as_deref(), Some("OEBPS/images/cvt.jpg"));
}

#[test]
fn darkthumbs_thumbimagestandard_direct_image() {
    // Some publishers (notably Random House) include a
    // <reference type="thumbimagestandard"> guide entry pointing
    // directly at a JPEG. Tried before the regular type="cover" path
    // because it never requires opening an XHTML wrapper.
    let cover = solid_jpeg(600, 900, [200, 100, 50]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata/>
  <manifest>
    <item id="x" href="images/x.jpg" media-type="image/jpeg"/>
  </manifest>
  <guide>
    <reference href="images/cvt.jpg" type="thumbimagestandard"/>
  </guide>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/images/x.jpg", solid_jpeg(40, 60, [9, 9, 9]))
        .add_file("OEBPS/images/cvt.jpg", cover)
        .build();
    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(r.report.strategy, "guide-thumb");
    assert_eq!(r.report.cover_path.as_deref(), Some("OEBPS/images/cvt.jpg"));
}

#[test]
fn darkthumbs_guide_cover_direct_image() {
    // The <guide><reference type="cover" href="…"/> may point directly
    // at an image rather than an XHTML wrapper. Distinguished by
    // extension.
    let cover = solid_png(500, 750, [80, 200, 80, 255]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata/>
  <manifest>
    <item id="anything" href="text.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <guide>
    <reference href="cover.png" type="cover"/>
  </guide>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/cover.png", cover)
        .add_file("OEBPS/text.xhtml", b"<html/>".to_vec())
        .build();
    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(r.report.strategy, "guide-cover-image");
}

#[test]
fn darkthumbs_priority_manifest_beats_guide() {
    // When BOTH a spec-compliant manifest declaration AND a guide entry
    // exist, the manifest wins (priority order is by spec correctness).
    let real = solid_jpeg(800, 1200, [200, 50, 50]);
    let decoy = solid_jpeg(40, 60, [9, 9, 9]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="real" href="real.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
  <guide>
    <reference href="decoy.jpg" type="cover"/>
  </guide>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/real.jpg", real)
        .add_file("OEBPS/decoy.jpg", decoy)
        .build();
    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(r.report.strategy, "epub3-cover-image");
    assert_eq!(r.report.cover_path.as_deref(), Some("OEBPS/real.jpg"));
}

#[test]
fn darkthumbs_priority_guide_beats_first_image() {
    // Guide-based strategies must beat first-image-fallback so we don't
    // pick a chapter illustration over the actual cover.
    let cover = solid_jpeg(800, 1200, [10, 200, 10]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata/>
  <manifest>
    <item id="ill1" href="images/illustration1.jpg" media-type="image/jpeg"/>
    <item id="cvr"  href="images/real_cover.jpg"     media-type="image/jpeg"/>
  </manifest>
  <guide>
    <reference href="images/real_cover.jpg" type="cover"/>
  </guide>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/images/illustration1.jpg", solid_jpeg(40, 60, [9, 9, 9]))
        .add_file("OEBPS/images/real_cover.jpg", cover)
        .build();
    // Even with the permissive policy, guide must win.
    let r = extract_cover(&bytes, 256, CoverPolicy::FirstImageFallback, NO_LIMIT).unwrap();
    assert_eq!(r.report.strategy, "guide-cover-image");
    assert_eq!(r.report.cover_path.as_deref(), Some("OEBPS/images/real_cover.jpg"));
}

#[test]
fn darkthumbs_xhtml_wrapper_with_relative_path() {
    // The <img src="…"> inside the XHTML wrapper is relative to the
    // XHTML's directory, NOT the OPF's directory. This test catches
    // the easy mistake of resolving relative to the OPF.
    let cover = solid_jpeg(400, 600, [50, 100, 200]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata/>
  <manifest>
    <item id="cvr-x" href="xhtml/cover.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <guide>
    <reference href="xhtml/cover.xhtml" type="cover"/>
  </guide>
</package>"#;
    // The XHTML lives in OEBPS/xhtml/ and references images/ at its
    // sibling level via "../images/cover.jpg".
    let xhtml = br#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <body><img src="../images/cover.jpg"/></body>
</html>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/xhtml/cover.xhtml", xhtml.to_vec())
        .add_file("OEBPS/images/cover.jpg", cover)
        .build();
    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(r.report.strategy, "guide-cover-xhtml");
    assert_eq!(r.report.cover_path.as_deref(), Some("OEBPS/images/cover.jpg"));
}

#[test]
fn darkthumbs_xhtml_wrapper_with_no_img_falls_through() {
    // Pathological case: the wrapper page exists but contains no <img>.
    // We must not crash; we should fall through to the next strategy
    // (or NoCover under Strict).
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata/>
  <manifest/>
  <guide>
    <reference href="cover.xhtml" type="cover"/>
  </guide>
</package>"#;
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/cover.xhtml", b"<html><body><p>nope</p></body></html>".to_vec())
        .build();
    let err = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap_err();
    assert!(matches!(err.0, EpubError::NoCover));
}

#[test]
fn darkthumbs_issue9_case2_non_standard_opf_filename() {
    // The Random House EPUB used an OPF named like
    // "Mich_9780307790361_epub_opf_r1.opf" at archive root, not
    // "OEBPS/content.opf". container.xml's full-path is the source of
    // truth; never assume a hard-coded location.
    let cover = solid_png(50, 80, [10, 20, 30, 255]);
    let container = br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="Mich_9780307790361_epub_opf_r1.opf"
              media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="OEBPS/images/cvt_r1.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
    let bytes = EpubBuilder::new()
        .opf_path("Mich_9780307790361_epub_opf_r1.opf")
        .container_xml(container.to_vec())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/images/cvt_r1.jpg", cover)
        .build();
    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT).unwrap();
    assert_eq!(r.report.cover_path.as_deref(), Some("OEBPS/images/cvt_r1.jpg"));
}

// ---------------------------------------------------------------------
//  Icaros lessons learned (https://github.com/Xanashi/Icaros)
// ---------------------------------------------------------------------
// Icaros is a closed-source, mature (1.5k★) Windows shell extension for
// video and audio thumbnails, and its public issue tracker has surfaced
// several operational gotchas relevant to *any* shell-extension that
// reads ZIP-based formats. The next tests exercise behaviors prompted
// by those issues.

#[test]
fn icaros_issue212_non_ascii_filename_in_zip_central_directory() {
    // Icaros #212: ZIPs whose entries include non-ASCII filenames can
    // miss the by_name() hash lookup if the creator didn't set the
    // language-encoding flag. Our read_archive_file falls back to a
    // raw-byte scan of the central directory.
    //
    // Synthesizing a ZIP without the UTF-8 flag is fiddly with the
    // `zip` crate's high-level API, but the slow path is also exercised
    // when the requested name *exactly* matches name_raw(). To verify
    // the slow path is reachable, we test what the zip crate gives us
    // back: both the fast and slow paths must succeed for a non-ASCII
    // cover filename.
    let cover = solid_jpeg(400, 600, [50, 100, 200]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="images/\xe5\xb0\x81\xe9\x9d\xa2.jpg"
          media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
    // 封面 = "cover" in Chinese. Resolve via UTF-8 bytes since OPFs are
    // UTF-8 encoded by spec.
    let chinese_cover_filename = "OEBPS/images/封面.jpg";
    let opf_xml = String::from_utf8(opf.to_vec()).unwrap()
        .replace(r"\xe5\xb0\x81\xe9\x9d\xa2", "封面");
    let bytes = EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf_xml.into_bytes())
        .add_file(chinese_cover_filename, cover)
        .build();

    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT)
        .expect("non-ASCII cover filename must be extractable");
    assert_eq!(r.report.cover_path.as_deref(), Some(chinese_cover_filename));
}

#[test]
fn icaros_issue196_deadline_check_aborts_slow_extraction() {
    // Icaros #196: slow inputs hang Explorer. Verify our cooperative
    // deadline path returns DeadlineExceeded rather than running to
    // completion. We use Duration::ZERO so the very first checkpoint
    // trips — a deterministic test that doesn't depend on wall-clock.
    let bytes = epub3_with_cover_image_property();
    let err = extract_cover_with_deadline(
        &bytes, 256, CoverPolicy::Strict, NO_LIMIT,
        Some(std::time::Duration::ZERO),
    ).unwrap_err();
    assert!(matches!(err.0, EpubError::DeadlineExceeded { .. }));
    assert_eq!(err.0.category(), "deadline-exceeded");
}

#[test]
fn icaros_issue196_generous_deadline_does_not_interfere() {
    // A deadline far longer than the work should never trip.
    let bytes = epub3_with_cover_image_property();
    let r = extract_cover_with_deadline(
        &bytes, 256, CoverPolicy::Strict, NO_LIMIT,
        Some(std::time::Duration::from_secs(60)),
    ).expect("60s deadline must not trip on a tiny synthetic EPUB");
    assert_eq!(r.report.strategy, "epub3-cover-image");
}

#[test]
fn icaros_issue196_no_deadline_param_is_unlimited() {
    // The no-deadline entry point is what the test suite generally
    // uses; verify it really doesn't enforce a timeout.
    let bytes = epub3_with_cover_image_property();
    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT)
        .expect("extract_cover (no deadline) must succeed");
    assert_eq!(r.report.strategy, "epub3-cover-image");
}

#[test]
fn missing_mimetype_entry_still_extracts_cover() {
    // The OCF specification requires a "mimetype" entry as the first
    // archive member. In practice, EPUB readers (and our pipeline)
    // tolerate its absence: format identification is driven by
    // META-INF/container.xml + the OPF, never by the mimetype file.
    // Verify we extract cleanly even when authoring tools omit it.
    let cover = solid_jpeg(400, 600, [50, 100, 200]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
    let bytes = EpubBuilder::new()
        .skip_mimetype()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/cover.jpg", cover)
        .build();
    let r = extract_cover(&bytes, 256, CoverPolicy::Strict, NO_LIMIT)
        .expect("EPUB without a mimetype entry must still be processable");
    assert_eq!(r.report.cover_path.as_deref(), Some("OEBPS/cover.jpg"));
}
