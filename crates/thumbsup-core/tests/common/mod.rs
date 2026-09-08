//! Test helpers for building synthetic EPUB byte streams in memory.
//!
//! Real EPUBs are too large and too licensing-encumbered to commit. This
//! builder produces minimal but spec-compliant archives covering every
//! variant the thumbnail handler must support: EPUB 2 with `<meta name=
//! "cover">`, EPUB 3 with `properties="cover-image"`, conventional `id=
//! "cover"`, no-cover, and various malformed forms.

use std::io::{Cursor, Write};

use image::{ImageBuffer, ImageFormat, Rgba};
use zip::write::FileOptions;
use zip::CompressionMethod;
use zip::ZipWriter;

/// Builder for a single in-memory EPUB. Methods are chainable.
pub struct EpubBuilder {
    container_xml: Option<Vec<u8>>,
    opf_path: String,
    opf_xml: Option<Vec<u8>>,
    files: Vec<(String, Vec<u8>)>,
    skip_mimetype: bool,
}

// Not every fixture-builder method is exercised by every test. Suppress
// the dead-code lint at the impl level rather than on each method, so
// future additions don't reintroduce the warning.
#[allow(dead_code)]
impl EpubBuilder {
    pub fn new() -> Self {
        Self {
            container_xml: None,
            opf_path: "OEBPS/content.opf".into(),
            opf_xml: None,
            files: Vec::new(),
            skip_mimetype: false,
        }
    }

    /// Path of the OPF inside the archive. Default: `OEBPS/content.opf`.
    pub fn opf_path(mut self, path: &str) -> Self {
        self.opf_path = path.into();
        self
    }

    pub fn opf_xml(mut self, xml: impl Into<Vec<u8>>) -> Self {
        self.opf_xml = Some(xml.into());
        self
    }

    pub fn container_xml(mut self, xml: impl Into<Vec<u8>>) -> Self {
        self.container_xml = Some(xml.into());
        self
    }

    pub fn add_file(mut self, path: &str, data: impl Into<Vec<u8>>) -> Self {
        self.files.push((path.into(), data.into()));
        self
    }

    /// Omit the `mimetype` entry (which is technically required by OCF
    /// but irrelevant to cover extraction).
    pub fn skip_mimetype(mut self) -> Self {
        self.skip_mimetype = true;
        self
    }

    pub fn build(self) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = FileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated = FileOptions::default().compression_method(CompressionMethod::Deflated);

            if !self.skip_mimetype {
                // Per OCF, mimetype must be the first entry, uncompressed,
                // with the literal contents below.
                zw.start_file("mimetype", stored).unwrap();
                zw.write_all(b"application/epub+zip").unwrap();
            }

            if let Some(c) = self.container_xml {
                zw.start_file("META-INF/container.xml", deflated).unwrap();
                zw.write_all(&c).unwrap();
            }

            if let Some(o) = self.opf_xml {
                zw.start_file(&self.opf_path, deflated).unwrap();
                zw.write_all(&o).unwrap();
            }

            for (path, data) in self.files {
                zw.start_file(&path, deflated).unwrap();
                zw.write_all(&data).unwrap();
            }

            zw.finish().unwrap();
        }
        buf
    }
}

/// A canonical container.xml pointing at `OEBPS/content.opf`.
pub fn standard_container() -> Vec<u8> {
    br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
        .to_vec()
}

/// A small synthetic PNG of the requested dimensions and solid color.
pub fn solid_png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_fn(w, h, |_, _| Rgba(color));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .unwrap();
    out
}

/// A small synthetic JPEG of the requested dimensions and solid color.
pub fn solid_jpeg(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
    let img: ImageBuffer<image::Rgb<u8>, _> = ImageBuffer::from_fn(w, h, |_, _| image::Rgb(color));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .unwrap();
    out
}

/// Convenience: a complete EPUB 3 with a `properties="cover-image"` cover.
pub fn epub3_with_cover_image_property() -> Vec<u8> {
    let cover = solid_jpeg(800, 1200, [200, 50, 50]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-id</dc:identifier>
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="cover-img" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="nav"       href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
  </manifest>
  <spine><itemref idref="nav"/></spine>
</package>"#;
    EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/images/cover.jpg", cover)
        .add_file("OEBPS/nav.xhtml", b"<html/>".to_vec())
        .build()
}

/// Convenience: a complete EPUB 2 with `<meta name="cover">`.
pub fn epub2_with_meta_cover() -> Vec<u8> {
    let cover = solid_png(600, 900, [50, 200, 50, 255]);
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="bookid">test-id</dc:identifier>
    <dc:title>Old Book</dc:title>
    <dc:language>en</dc:language>
    <meta name="cover" content="my-cover-id"/>
  </metadata>
  <manifest>
    <item id="my-cover-id" href="cover.png" media-type="image/png"/>
    <item id="ncx"          href="toc.ncx"  media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx"/>
</package>"#;
    EpubBuilder::new()
        .container_xml(standard_container())
        .opf_xml(opf.to_vec())
        .add_file("OEBPS/cover.png", cover)
        .add_file("OEBPS/toc.ncx", b"<ncx/>".to_vec())
        .build()
}
