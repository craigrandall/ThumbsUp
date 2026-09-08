//! Find the first `<img src="…">` reference in an XHTML cover page.
//!
//! Why this exists: many real-world EPUB 2 files (especially Adobe-DRM-era
//! Random House and Penguin titles) declare their cover via
//! `<guide><reference type="cover" href="cover.xhtml"/>` — pointing at an
//! XHTML page rather than directly at an image. The page itself contains
//! a single `<img>` element wrapping the cover bitmap.
//!
//! The DarkThumbs project ([fire-eggs/DarkThumbs#9]) showed that ignoring
//! this pattern causes the thumbnail handler to either fail entirely or
//! pick a wrong image. We implement the lookup with the same XML pull-
//! parser we already use for the OPF — no new dependency, no full HTML
//! parser, just a scan for the first `<img>` tag and its `src` attribute.
//!
//! XHTML is well-formed XML by definition, so `quick-xml` parses it
//! correctly. If a publisher ships malformed (non-XHTML) HTML the parser
//! will return a soft error and the caller proceeds to the next fallback.
//!
//! [fire-eggs/DarkThumbs#9]: https://github.com/fire-eggs/DarkThumbs/issues/9

use crate::error::{EpubError, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Scan XHTML bytes and return the value of the `src` attribute of the
/// first `<img>` element encountered. Returns `Err(EpubError::NoCover)`
/// if no `<img>` tag is found.
pub fn first_img_src(xhtml: &[u8]) -> Result<String> {
    let mut reader = Reader::from_reader(xhtml);
    reader.trim_text(true);
    reader.expand_empty_elements(true);
    // XHTML cover pages frequently contain unresolvable entity refs
    // (e.g. &nbsp;) that quick-xml would otherwise flag. We skip
    // entity-resolution because we never look at text content here.
    reader.check_end_names(false);

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if local_name(e.name().as_ref()).eq_ignore_ascii_case(b"img") {
                    for attr in e.attributes().with_checks(false) {
                        let attr = attr.map_err(|err| EpubError::XmlParse(err.to_string()))?;
                        if local_name(attr.key.as_ref()).eq_ignore_ascii_case(b"src") {
                            let v = std::str::from_utf8(&attr.value)
                                .map_err(|_| EpubError::XmlParse("non-UTF-8 src".into()))?
                                .to_string();
                            if !v.is_empty() {
                                return Ok(v);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            // Soft-fail on malformed XHTML — let the caller try other
            // fallbacks rather than failing the whole thumbnail.
            Err(_) => return Err(EpubError::NoCover),
            _ => {}
        }
        buf.clear();
    }
    Err(EpubError::NoCover)
}

fn local_name(qname: &[u8]) -> &[u8] {
    match qname.iter().rposition(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_simple_img_src() {
        let html = br#"<html><body>
            <div class="cover">
              <img alt="" src="images/cover.jpg"/>
            </div>
        </body></html>"#;
        assert_eq!(first_img_src(html).unwrap(), "images/cover.jpg");
    }

    #[test]
    fn case_insensitive_tag_and_attr() {
        // XHTML mandates lowercase, but real-world cover pages have shipped
        // with quirky casing. Be lenient.
        let html = br#"<html><body><IMG SRC="cover.png"/></body></html>"#;
        assert_eq!(first_img_src(html).unwrap(), "cover.png");
    }

    #[test]
    fn picks_first_img_when_multiple() {
        // A guide-referenced cover page should contain only one img, but
        // some publishers add decorative imagery; we use the first.
        let html = br#"<html><body>
            <img src="logo.png"/>
            <img src="cover.jpg"/>
        </body></html>"#;
        assert_eq!(first_img_src(html).unwrap(), "logo.png");
    }

    #[test]
    fn handles_xhtml_namespaces() {
        let html = br#"<html xmlns="http://www.w3.org/1999/xhtml"
                             xmlns:epub="http://www.idpf.org/2007/ops">
            <body epub:type="cover">
              <img src="../images/cover.jpg"/>
            </body>
        </html>"#;
        assert_eq!(first_img_src(html).unwrap(), "../images/cover.jpg");
    }

    #[test]
    fn returns_no_cover_when_no_img_tag() {
        let html = br#"<html><body><p>No image here.</p></body></html>"#;
        assert!(matches!(first_img_src(html), Err(EpubError::NoCover)));
    }

    #[test]
    fn returns_no_cover_when_img_has_no_src() {
        let html = br#"<html><body><img alt="cover"/></body></html>"#;
        assert!(matches!(first_img_src(html), Err(EpubError::NoCover)));
    }

    #[test]
    fn malformed_html_returns_no_cover_softly() {
        // Garbled input must not panic or propagate XmlParse — it just
        // means "this fallback didn't work, try the next one".
        let html = b"<<<<not really xhtml>>>>";
        assert!(matches!(first_img_src(html), Err(EpubError::NoCover)));
    }
}
