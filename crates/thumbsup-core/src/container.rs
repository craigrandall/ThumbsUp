//! `META-INF/container.xml` parsing.
//!
//! Per the OCF specification, every EPUB *must* contain
//! `META-INF/container.xml` whose root element `<container>` declares one
//! or more `<rootfile>` elements pointing at the OPF package documents.
//! We use the first rootfile whose media type is the OPF MIME type, or —
//! if no media-type is declared — the first rootfile.

use crate::error::{EpubError, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

const OPF_MIME: &str = "application/oebps-package+xml";

/// Parse a `container.xml` byte slice and return the archive-relative path
/// of the OPF package document.
pub fn parse_container_xml(xml: &[u8]) -> Result<String> {
    let mut reader = Reader::from_reader(xml);
    reader.trim_text(true);
    reader.expand_empty_elements(true); // emit Start for self-closing tags

    let mut buf = Vec::new();
    let mut first_rootfile: Option<String> = None;
    let mut opf_rootfile: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if local_name(e.name().as_ref()) == b"rootfile" {
                    let mut full_path: Option<String> = None;
                    let mut media_type: Option<String> = None;
                    for attr in e.attributes().with_checks(false) {
                        let attr = attr.map_err(|err| EpubError::XmlParse(err.to_string()))?;
                        let key = local_name(attr.key.as_ref());
                        let val = std::str::from_utf8(&attr.value)
                            .map_err(|_| EpubError::XmlParse("non-UTF-8 attribute".into()))?
                            .to_string();
                        match key {
                            b"full-path" => full_path = Some(val),
                            b"media-type" => media_type = Some(val),
                            _ => {}
                        }
                    }
                    if let Some(path) = full_path {
                        if first_rootfile.is_none() {
                            first_rootfile = Some(path.clone());
                        }
                        if media_type.as_deref() == Some(OPF_MIME) && opf_rootfile.is_none() {
                            opf_rootfile = Some(path);
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(EpubError::XmlParse(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    opf_rootfile.or(first_rootfile).ok_or(EpubError::NoRootfile)
}

/// Strip XML-namespace prefix (`ns:tag` → `tag`).
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
    fn parses_canonical_container() {
        let xml = br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;
        assert_eq!(parse_container_xml(xml).unwrap(), "OEBPS/content.opf");
    }

    #[test]
    fn picks_opf_rootfile_when_multiple_declared() {
        // Some archives include alternate rootfiles (e.g. for renditions);
        // we pick the one with the OPF media type, not necessarily the first.
        let xml = br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="alt/extra.xml" media-type="application/xml"/>
    <rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;
        assert_eq!(parse_container_xml(xml).unwrap(), "OEBPS/package.opf");
    }

    #[test]
    fn falls_back_to_first_rootfile_without_media_type() {
        let xml = br#"<?xml version="1.0"?>
<container><rootfiles>
    <rootfile full-path="content.opf"/>
</rootfiles></container>"#;
        assert_eq!(parse_container_xml(xml).unwrap(), "content.opf");
    }

    #[test]
    fn handles_namespaced_elements() {
        let xml = br#"<?xml version="1.0"?>
<ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container">
  <ocf:rootfiles>
    <ocf:rootfile full-path="pkg.opf" media-type="application/oebps-package+xml"/>
  </ocf:rootfiles>
</ocf:container>"#;
        assert_eq!(parse_container_xml(xml).unwrap(), "pkg.opf");
    }

    #[test]
    fn errors_on_no_rootfile() {
        let xml = br#"<?xml version="1.0"?><container><rootfiles/></container>"#;
        assert!(matches!(
            parse_container_xml(xml),
            Err(EpubError::NoRootfile)
        ));
    }

    #[test]
    fn errors_on_malformed_xml() {
        let xml = b"<container><rootfile full-path=";
        assert!(matches!(
            parse_container_xml(xml),
            Err(EpubError::XmlParse(_))
        ));
    }
}
