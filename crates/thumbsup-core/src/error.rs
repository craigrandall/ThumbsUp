//! Error type for the EPUB cover-extraction pipeline.
//!
//! The error variants are deliberately fine-grained so the diagnostics view
//! in the GUI configuration tool can show the user exactly *why* a thumbnail
//! could not be produced (e.g. "missing OPF" vs "no `cover-image` property"
//! vs "unsupported image format").

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, EpubError>;

/// A categorized error produced while extracting a cover image from an EPUB.
///
/// Each variant maps to a stable string returned by [`EpubError::category`],
/// suitable for logging and for display in the configuration UI's diagnostics
/// log without exposing implementation detail.
#[derive(Debug, Error)]
pub enum EpubError {
    /// The byte stream is not a valid ZIP archive.
    #[error("malformed ZIP archive: {0}")]
    MalformedZip(String),

    /// `META-INF/container.xml` is absent. Required by the OCF specification.
    #[error("missing META-INF/container.xml")]
    MissingContainer,

    /// `container.xml` parsed but contained no usable `rootfile` entry.
    #[error("no rootfile declared in container.xml")]
    NoRootfile,

    /// The OPF package document referenced by `container.xml` is missing.
    #[error("OPF package document not found at '{0}'")]
    MissingOpf(String),

    /// XML parsing failed (either container.xml or the OPF).
    #[error("XML parse error: {0}")]
    XmlParse(String),

    /// The OPF parsed correctly but no cover image could be located by any
    /// of the supported strategies (EPUB 3 `properties="cover-image"`,
    /// EPUB 2 `<meta name="cover">`, conventional `id="cover"`, or first
    /// image fallback when enabled).
    #[error("no cover image declared in OPF")]
    NoCover,

    /// A cover was declared but the referenced file is not in the archive.
    #[error("declared cover file '{0}' is missing from the archive")]
    CoverFileMissing(String),

    /// The cover href escapes the archive root via `..` segments.
    /// Treated as a security error: returned without attempting extraction.
    #[error("cover path '{0}' escapes archive root")]
    PathTraversal(String),

    /// The cover bytes could not be decoded by any supported codec.
    #[error("unsupported or corrupt cover image: {0}")]
    ImageDecode(String),

    /// The EPUB exceeds the configured maximum size and was skipped.
    #[error("EPUB exceeds configured size limit ({size} bytes > {limit} bytes)")]
    TooLarge { size: u64, limit: u64 },

    /// The cooperative deadline elapsed mid-extraction. Reported with
    /// fine-grained timing so the diagnostics view can show the user
    /// which stage of the pipeline overran.
    #[error("thumbnail deadline exceeded: {elapsed_ms} ms > {limit_ms} ms")]
    DeadlineExceeded { elapsed_ms: u64, limit_ms: u64 },

    /// Generic IO error (e.g. failed read from the underlying stream).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl EpubError {
    /// A short, stable category string suitable for logs and the diagnostics
    /// view. Never localized.
    pub fn category(&self) -> &'static str {
        match self {
            EpubError::MalformedZip(_)        => "malformed-zip",
            EpubError::MissingContainer       => "missing-container",
            EpubError::NoRootfile             => "no-rootfile",
            EpubError::MissingOpf(_)          => "missing-opf",
            EpubError::XmlParse(_)            => "xml-parse",
            EpubError::NoCover                => "no-cover",
            EpubError::CoverFileMissing(_)    => "cover-file-missing",
            EpubError::PathTraversal(_)       => "path-traversal",
            EpubError::ImageDecode(_)         => "image-decode",
            EpubError::TooLarge { .. }        => "too-large",
            EpubError::DeadlineExceeded { .. } => "deadline-exceeded",
            EpubError::Io(_)                  => "io",
        }
    }
}

impl From<zip::result::ZipError> for EpubError {
    fn from(value: zip::result::ZipError) -> Self {
        match value {
            zip::result::ZipError::Io(e) => EpubError::Io(e),
            other => EpubError::MalformedZip(other.to_string()),
        }
    }
}
