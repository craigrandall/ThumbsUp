//! OPF (Open Packaging Format) package-document parser, focused on locating
//! the cover image. The parser intentionally does **not** build a full DOM —
//! it streams events and extracts only what the thumbnail handler needs:
//!
//! 1. The package version (to dispatch EPUB 2 vs EPUB 3 strategies).
//! 2. Every `<item>` in the manifest (id, href, media-type, properties).
//! 3. The `<meta name="cover" content="…">` declaration used by EPUB 2.
//!
//! Cover resolution then proceeds in priority order:
//!
//! * **EPUB 3**: manifest item whose `properties` attribute contains the
//!   space-separated token `cover-image`.
//! * **EPUB 2**: `<meta name="cover">`'s `content` attribute references a
//!   manifest item by id.
//! * **Fallbacks** (any version): an item whose id is exactly `cover`,
//!   `cover-image`, or `ci`. Many real-world EPUBs use these conventions
//!   without setting the formal property.
//! * **Last resort** (only if [`CoverPolicy::FirstImageFallback`] is set):
//!   the first manifest item whose media-type begins with `image/`.

use crate::error::{EpubError, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

/// One `<item>` from the OPF manifest, retaining only the fields the
/// thumbnail handler cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestItem {
    pub id:         String,
    pub href:       String,
    pub media_type: String,
    /// Space-separated EPUB 3 properties. Empty in EPUB 2.
    pub properties: String,
}

impl ManifestItem {
    /// True if this item declares the EPUB 3 `cover-image` property token.
    pub fn has_cover_image_property(&self) -> bool {
        self.properties.split_whitespace().any(|p| p == "cover-image")
    }

    /// True if the media type indicates a raster image format we can decode.
    pub fn is_supported_image(&self) -> bool {
        matches!(
            self.media_type.to_ascii_lowercase().as_str(),
            "image/jpeg" | "image/jpg" | "image/png" | "image/gif"
        )
    }

    /// True if the media type is *any* image type, used for the fallback
    /// "first image in the manifest" strategy.
    pub fn is_image(&self) -> bool {
        self.media_type.to_ascii_lowercase().starts_with("image/")
    }
}

/// Result of parsing an OPF: the manifest plus EPUB-2 meta cover reference.
#[derive(Debug, Clone, Default)]
pub struct OpfPackage {
    /// Major version: 2, 3, or 0 if the version attribute is missing/unknown.
    pub version_major:  u8,
    pub manifest:       Vec<ManifestItem>,
    /// Value of `<meta name="cover" content="…">` if present.
    pub meta_cover_idref: Option<String>,
    /// `<guide><reference type="cover" href="…"/>` href if present.
    /// Many real-world EPUB 2 files (notably older Adobe-DRM Random House
    /// and Penguin titles) declare their cover only via this mechanism.
    /// The href may point at an XHTML wrapper or directly at an image —
    /// the cover-resolution code disambiguates by looking at the path
    /// extension and falling through to the XHTML scanner if needed.
    pub guide_cover_href: Option<String>,
    /// `<guide><reference type="thumbimagestandard" href="…"/>` href if
    /// present. Used by some publishers (Random House) to point directly
    /// at a low-res cover JPEG. Tried before the regular `type="cover"`
    /// guide reference because it's almost always an image, never an
    /// XHTML wrapper.
    pub guide_thumb_href: Option<String>,
}

/// Whether to fall back to the first image in the manifest when no compliant
/// cover declaration is found. Mirrors the user-configurable policy in the
/// GUI tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverPolicy {
    /// Only return covers declared per the EPUB spec or by conventional id.
    Strict,
    /// If no declared cover, return the first image item in the manifest.
    FirstImageFallback,
}

impl OpfPackage {
    /// Parse an OPF byte slice.
    pub fn parse(xml: &[u8]) -> Result<Self> {
        let mut reader = Reader::from_reader(xml);
        reader.trim_text(true);
        reader.expand_empty_elements(true);

        let mut pkg = OpfPackage::default();
        let mut buf = Vec::new();
        let mut in_manifest = false;
        let mut in_metadata = false;
        let mut in_guide    = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                    b"package" => {
                        for attr in e.attributes().with_checks(false) {
                            let attr = attr.map_err(|err| EpubError::XmlParse(err.to_string()))?;
                            if local_name(attr.key.as_ref()) == b"version" {
                                let v = std::str::from_utf8(&attr.value)
                                    .map_err(|_| EpubError::XmlParse("non-UTF-8 version".into()))?;
                                pkg.version_major = parse_version_major(v);
                            }
                        }
                    }
                    b"manifest" => in_manifest = true,
                    b"metadata" => in_metadata = true,
                    b"guide"    => in_guide    = true,
                    b"item" if in_manifest => {
                        if let Some(item) = parse_manifest_item(&e)? {
                            pkg.manifest.push(item);
                        }
                    }
                    b"reference" if in_guide => {
                        // <reference type="cover|thumbimagestandard|…" href="…"/>
                        let mut ref_type: Option<String> = None;
                        let mut ref_href: Option<String> = None;
                        for attr in e.attributes().with_checks(false) {
                            let attr = attr.map_err(|err| EpubError::XmlParse(err.to_string()))?;
                            let key = local_name(attr.key.as_ref());
                            let val = std::str::from_utf8(&attr.value)
                                .map_err(|_| EpubError::XmlParse("non-UTF-8 attr".into()))?
                                .to_string();
                            match key {
                                b"type" => ref_type = Some(val),
                                b"href" => ref_href = Some(val),
                                _ => {}
                            }
                        }
                        if let (Some(t), Some(h)) = (ref_type, ref_href) {
                            match t.as_str() {
                                "cover" if pkg.guide_cover_href.is_none()
                                    => pkg.guide_cover_href = Some(h),
                                "thumbimagestandard" if pkg.guide_thumb_href.is_none()
                                    => pkg.guide_thumb_href = Some(h),
                                _ => {}
                            }
                        }
                    }
                    b"meta" if in_metadata => {
                        // EPUB 2: <meta name="cover" content="..."/>
                        let mut name: Option<String> = None;
                        let mut content: Option<String> = None;
                        for attr in e.attributes().with_checks(false) {
                            let attr = attr.map_err(|err| EpubError::XmlParse(err.to_string()))?;
                            let key = local_name(attr.key.as_ref());
                            let val = std::str::from_utf8(&attr.value)
                                .map_err(|_| EpubError::XmlParse("non-UTF-8 attr".into()))?
                                .to_string();
                            match key {
                                b"name"    => name    = Some(val),
                                b"content" => content = Some(val),
                                _ => {}
                            }
                        }
                        if name.as_deref() == Some("cover") {
                            if let Some(c) = content {
                                pkg.meta_cover_idref = Some(c);
                            }
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                    b"manifest" => in_manifest = false,
                    b"metadata" => in_metadata = false,
                    b"guide"    => in_guide    = false,
                    _ => {}
                },
                Ok(Event::Eof) => break,
                Err(e) => return Err(EpubError::XmlParse(e.to_string())),
                _ => {}
            }
            buf.clear();
        }

        Ok(pkg)
    }

    /// Look up a manifest item by id.
    pub fn item_by_id(&self, id: &str) -> Option<&ManifestItem> {
        self.manifest.iter().find(|i| i.id == id)
    }

    /// Resolve the cover image manifest item according to the documented
    /// priority order. Returns `Err(EpubError::NoCover)` if nothing matches.
    pub fn resolve_cover(&self, policy: CoverPolicy) -> Result<&ManifestItem> {
        // 1. EPUB 3 properties="cover-image"
        if let Some(item) = self.manifest.iter().find(|i| i.has_cover_image_property()) {
            return Ok(item);
        }

        // 2. EPUB 2 <meta name="cover" content="...">
        if let Some(idref) = &self.meta_cover_idref {
            if let Some(item) = self.item_by_id(idref) {
                return Ok(item);
            }
            // Idref present but dangling — fall through to other strategies
            // rather than fail, so we tolerate slightly broken EPUBs.
            log::debug!("meta cover idref '{idref}' not found in manifest");
        }

        // 3. Conventional ids used by many real-world EPUBs.
        for conventional in ["cover", "cover-image", "ci", "coverimage"] {
            if let Some(item) = self.item_by_id(conventional) {
                if item.is_image() {
                    return Ok(item);
                }
            }
        }

        // 4. Configurable fallback: first image in the manifest.
        if policy == CoverPolicy::FirstImageFallback {
            if let Some(item) = self.manifest.iter().find(|i| i.is_image()) {
                return Ok(item);
            }
        }

        Err(EpubError::NoCover)
    }
}

fn parse_manifest_item(e: &quick_xml::events::BytesStart<'_>) -> Result<Option<ManifestItem>> {
    let mut id = String::new();
    let mut href = String::new();
    let mut media_type = String::new();
    let mut properties = String::new();
    for attr in e.attributes().with_checks(false) {
        let attr = attr.map_err(|err| EpubError::XmlParse(err.to_string()))?;
        let key = local_name(attr.key.as_ref());
        let val = std::str::from_utf8(&attr.value)
            .map_err(|_| EpubError::XmlParse("non-UTF-8 attr".into()))?
            .to_string();
        match key {
            b"id"         => id = val,
            b"href"       => href = val,
            b"media-type" => media_type = val,
            b"properties" => properties = val,
            _ => {}
        }
    }
    // An item with no href is meaningless; skip silently rather than fail
    // the whole document over one bad row.
    if href.is_empty() {
        return Ok(None);
    }
    Ok(Some(ManifestItem { id, href, media_type, properties }))
}

fn local_name(qname: &[u8]) -> &[u8] {
    match qname.iter().rposition(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None    => qname,
    }
}

fn parse_version_major(v: &str) -> u8 {
    v.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Heuristic: does this href point at an image based purely on its
/// extension? Used by the guide-cover fallback to choose between using
/// the href directly versus following it into an XHTML wrapper.
///
/// Conservative — only the formats we actually decode are recognized.
pub fn href_looks_like_image(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    // Strip query/fragment if any (rare in EPUBs but defensive).
    let path_only = lower.split(['?', '#']).next().unwrap_or(&lower);
    matches!(
        path_only.rsplit('.').next(),
        Some("jpg") | Some("jpeg") | Some("png") | Some("gif")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, href: &str, mt: &str, props: &str) -> ManifestItem {
        ManifestItem {
            id: id.into(), href: href.into(),
            media_type: mt.into(), properties: props.into(),
        }
    }

    #[test]
    fn parses_epub3_with_cover_image_property() {
        let xml = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata/>
  <manifest>
    <item id="c"  href="cover.jpg"  media-type="image/jpeg" properties="cover-image"/>
    <item id="t"  href="title.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.version_major, 3);
        assert_eq!(pkg.manifest.len(), 2);
        assert_eq!(
            pkg.resolve_cover(CoverPolicy::Strict).unwrap(),
            &item("c", "cover.jpg", "image/jpeg", "cover-image")
        );
    }

    #[test]
    fn parses_epub2_with_meta_cover() {
        let xml = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata>
    <meta name="cover" content="my-cover-id"/>
  </metadata>
  <manifest>
    <item id="my-cover-id" href="images/cover.jpg" media-type="image/jpeg"/>
    <item id="ch1"          href="ch1.html"        media-type="application/xhtml+xml"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.version_major, 2);
        assert_eq!(pkg.meta_cover_idref.as_deref(), Some("my-cover-id"));
        let resolved = pkg.resolve_cover(CoverPolicy::Strict).unwrap();
        assert_eq!(resolved.href, "images/cover.jpg");
    }

    #[test]
    fn epub3_property_takes_precedence_over_meta_cover() {
        // Pathological but possible: both declarations present, pointing
        // at different items. EPUB 3 property wins.
        let xml = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata>
    <meta name="cover" content="legacy-cover"/>
  </metadata>
  <manifest>
    <item id="legacy-cover" href="old.jpg" media-type="image/jpeg"/>
    <item id="new-cover"    href="new.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.resolve_cover(CoverPolicy::Strict).unwrap().href, "new.jpg");
    }

    #[test]
    fn properties_token_match_is_word_not_substring() {
        // "not-cover-image" must not match the "cover-image" token.
        let xml = br#"<?xml version="1.0"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata/>
  <manifest>
    <item id="x" href="x.jpg" media-type="image/jpeg" properties="not-cover-image-decoy"/>
    <item id="y" href="y.jpg" media-type="image/jpeg" properties="nav cover-image scripted"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.resolve_cover(CoverPolicy::Strict).unwrap().href, "y.jpg");
    }

    #[test]
    fn falls_back_to_conventional_cover_id() {
        let xml = br#"<?xml version="1.0"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata/>
  <manifest>
    <item id="cover" href="cover.png" media-type="image/png"/>
    <item id="ch1"   href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.resolve_cover(CoverPolicy::Strict).unwrap().href, "cover.png");
    }

    #[test]
    fn first_image_fallback_only_with_policy() {
        let xml = br#"<?xml version="1.0"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata/>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="img1" href="figure.png" media-type="image/png"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert!(matches!(pkg.resolve_cover(CoverPolicy::Strict), Err(EpubError::NoCover)));
        assert_eq!(
            pkg.resolve_cover(CoverPolicy::FirstImageFallback).unwrap().href,
            "figure.png"
        );
    }

    #[test]
    fn meta_cover_with_dangling_idref_falls_through() {
        // EPUB 2 declares meta cover but the id doesn't exist; we should
        // continue to the conventional-id fallback rather than fail.
        let xml = br#"<?xml version="1.0"?>
<package version="2.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata><meta name="cover" content="ghost"/></metadata>
  <manifest>
    <item id="cover" href="real.jpg" media-type="image/jpeg"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.resolve_cover(CoverPolicy::Strict).unwrap().href, "real.jpg");
    }

    #[test]
    fn no_cover_anywhere_returns_no_cover() {
        let xml = br#"<?xml version="1.0"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata/>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert!(matches!(pkg.resolve_cover(CoverPolicy::Strict), Err(EpubError::NoCover)));
        assert!(matches!(
            pkg.resolve_cover(CoverPolicy::FirstImageFallback),
            Err(EpubError::NoCover)
        ));
    }

    #[test]
    fn ignores_items_without_href() {
        let xml = br#"<?xml version="1.0"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf">
  <metadata/>
  <manifest>
    <item id="broken" media-type="image/jpeg"/>
    <item id="cover" href="c.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.manifest.len(), 1);
        assert_eq!(pkg.resolve_cover(CoverPolicy::Strict).unwrap().href, "c.jpg");
    }

    #[test]
    fn version_parsing_handles_minor_versions() {
        assert_eq!(parse_version_major("3.2"), 3);
        assert_eq!(parse_version_major("3.0"), 3);
        assert_eq!(parse_version_major("2.0.1"), 2);
        assert_eq!(parse_version_major(""), 0);
        assert_eq!(parse_version_major("garbage"), 0);
    }

    #[test]
    fn supported_image_types() {
        assert!(item("a", "x.jpg", "image/jpeg", "").is_supported_image());
        assert!(item("a", "x.JPG", "IMAGE/JPEG", "").is_supported_image());
        assert!(item("a", "x.png", "image/png",  "").is_supported_image());
        assert!(item("a", "x.gif", "image/gif",  "").is_supported_image());
        assert!(!item("a", "x.svg", "image/svg+xml", "").is_supported_image());
        assert!(!item("a", "x.html", "application/xhtml+xml", "").is_supported_image());
    }

    #[test]
    fn malformed_opf_returns_xml_parse_error() {
        let xml = b"<package version=\"3.0\"><manifest><item id=";
        assert!(matches!(OpfPackage::parse(xml), Err(EpubError::XmlParse(_))));
    }

    // ---------- guide element parsing (DarkThumbs Issue #9 lessons) ----------

    #[test]
    fn captures_guide_cover_reference() {
        let xml = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata/>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <guide>
    <reference href="cover.xhtml" type="cover" title="Cover"/>
    <reference href="ch1.xhtml" type="text"/>
  </guide>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.guide_cover_href.as_deref(), Some("cover.xhtml"));
        assert!(pkg.guide_thumb_href.is_none());
    }

    #[test]
    fn captures_thumbimagestandard_separately() {
        // The Random House EPUB pattern from DarkThumbs#9 Case 2: both a
        // type="cover" XHTML wrapper and a type="thumbimagestandard"
        // direct image reference.
        let xml = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata/>
  <manifest/>
  <guide>
    <reference href="OEBPS/cvi.htm" type="cover"/>
    <reference href="OEBPS/images/cvt.jpg" type="thumbimagestandard"/>
  </guide>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert_eq!(pkg.guide_cover_href.as_deref(), Some("OEBPS/cvi.htm"));
        assert_eq!(pkg.guide_thumb_href.as_deref(), Some("OEBPS/images/cvt.jpg"));
    }

    #[test]
    fn ignores_guide_outside_guide_element() {
        // A <reference> that appears outside <guide> must be ignored.
        let xml = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata>
    <reference href="trick.jpg" type="cover"/>
  </metadata>
  <manifest/>
</package>"#;
        let pkg = OpfPackage::parse(xml).unwrap();
        assert!(pkg.guide_cover_href.is_none());
    }

    #[test]
    fn href_image_extension_detection() {
        assert!(href_looks_like_image("cover.jpg"));
        assert!(href_looks_like_image("path/to/cover.JPEG"));
        assert!(href_looks_like_image("a.png"));
        assert!(href_looks_like_image("a.gif"));
        assert!(!href_looks_like_image("cover.xhtml"));
        assert!(!href_looks_like_image("cover.htm"));
        assert!(!href_looks_like_image("cover.html"));
        assert!(!href_looks_like_image("cover"));
        assert!(!href_looks_like_image(""));
        assert!(!href_looks_like_image("cover.svg"));  // we don't decode SVG
        assert!(!href_looks_like_image("cover.webp")); // not in whitelist
    }
}
