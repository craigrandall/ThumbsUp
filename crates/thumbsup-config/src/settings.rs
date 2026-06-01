//! Configuration model used by the GUI tool. Mirrors the registry layout
//! the DLL reads at runtime; the GUI is the only writer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FallbackPolicy {
    /// Only return covers declared per the EPUB spec or by conventional id.
    Strict,
    /// If no declared cover, return the first image item in the manifest.
    FirstImage,
}

impl FallbackPolicy {
    pub fn as_dword(self) -> u32 {
        match self {
            FallbackPolicy::Strict     => 0,
            FallbackPolicy::FirstImage => 1,
        }
    }
    pub fn from_dword(v: u32) -> Self {
        match v {
            1 => FallbackPolicy::FirstImage,
            _ => FallbackPolicy::Strict,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FallbackPolicy::Strict     => "Strict (spec-compliant only)",
            FallbackPolicy::FirstImage => "First image in archive",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
    pub max_file_mb: u64,
    pub max_thumbnail_ms: u32,
    pub fallback_policy: FallbackPolicy,
    pub logging_enabled: bool,
    pub log_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled:          true,
            max_file_mb:      256,
            max_thumbnail_ms: 5_000,
            fallback_policy:  FallbackPolicy::Strict,
            logging_enabled:  false,
            log_path:         String::new(),
        }
    }
}

impl Settings {
    pub fn max_file_bytes(&self) -> u64 {
        self.max_file_mb.saturating_mul(1024 * 1024)
    }
}
