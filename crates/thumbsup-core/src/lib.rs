//! Core EPUB cover-image extraction.
//!
//! This crate is the platform-independent half of the ThumbsUp
//! project. It contains every piece of logic that does *not* require the
//! Windows shell APIs:
//!
//! * `container` — `META-INF/container.xml` parsing.
//! * `opf`       — OPF package-document parsing and cover resolution.
//! * `path`      — POSIX-style path resolution with traversal protection.
//! * `image_ops` — Decoding, resizing, and BGRA8 conversion.
//! * `cover`    — High-level "bytes in, thumbnail out" entry point.
//!
//! The shell-extension DLL (`thumbsup-shell`) and the GUI configuration
//! tool (`thumbsup-config`) both link against this crate so that the same
//! tested code path produces the thumbnail Explorer displays and the
//! preview shown in the diagnostics view.

#![forbid(unsafe_code)]
#![warn(rust_2018_idioms)]
#![deny(missing_docs)]
#![allow(missing_docs)] // Field-level docs are inferred from struct docs.

pub mod container;
pub mod cover;
pub mod error;
pub mod image_ops;
pub mod opf;
pub mod path;
pub mod xhtml;

pub use cover::{extract_cover, extract_cover_with_deadline, ExtractedCover, ExtractionReport};
pub use error::{EpubError, Result};
pub use image_ops::Thumbnail;
pub use opf::CoverPolicy;
