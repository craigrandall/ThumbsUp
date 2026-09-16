//! Batch EPUB cover image extractor for digital library analysis.
//!
//! This tool iterates over EPUB files in a directory, extracts the original
//! cover image bytes (without decoding, resizing, or color conversion), and
//! writes them to an output directory with a manifest for subsequent analysis.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use serde::Serialize;
use thumbsup_core::CoverPolicy;
use walkdir::WalkDir;

/// Sanitize a string for use as a Windows filename by replacing invalid characters.
fn sanitize_filename(name: &str) -> String {
    const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\\', '/'];
    name.chars()
        .map(|c| if INVALID_CHARS.contains(&c) { '_' } else { c })
        .collect()
}

/// Command-line arguments for the extractor.
#[derive(Parser, Debug)]
#[command(name = "thumbsup-extract")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory containing EPUB files to process.
    #[arg(short, long)]
    input: PathBuf,

    /// Directory to write extracted cover images.
    #[arg(short, long)]
    output: PathBuf,

    /// Recursively search subdirectories for EPUB files.
    #[arg(short, long, default_value = "false")]
    recursive: bool,

    /// Cover extraction policy: 'strict' or 'fallback'.
    #[arg(short, long, default_value = "strict")]
    policy: String,
}

/// Extraction result for a single EPUB, serialized to JSON for the manifest.
#[derive(Serialize)]
struct ExtractionResult {
    /// Source EPUB file path (relative to input directory).
    source: String,
    /// Output cover file path (relative to output directory), if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    /// Extraction strategy used, if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    strategy: Option<String>,
    /// EPUB version major, if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    epub_version: Option<u8>,
    /// Cover media type, if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
    /// Error message, if extraction failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Error category, if extraction failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_category: Option<String>,
}

/// Determine the file extension from the actual image bytes using magic number detection.
fn detect_image_extension(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 2 {
        match &bytes[0..2] {
            b"\xFF\xD8" => return "jpg", // JPEG
            b"\x89P" => return "png",    // PNG
            b"GIF" => return "gif",      // GIF
            _ => {}
        }
    }
    // Fallback: use the declared media type or default to jpg
    "jpg"
}

/// Process a single EPUB file and write its cover to the output directory.
fn process_epub(
    epub_path: &Path,
    output_dir: &Path,
    policy: CoverPolicy,
    counter: &AtomicUsize,
    manifest_file: &mut fs::File,
) -> Result<(), String> {
    let relative_path = epub_path
        .strip_prefix(output_dir.parent().unwrap_or(Path::new("")))
        .unwrap_or(epub_path);
    let source_name = relative_path.to_string_lossy();

    // Read the EPUB file
    let epub_bytes = match fs::read(epub_path) {
        Ok(b) => b,
        Err(e) => {
            let result = ExtractionResult {
                source: source_name.into_owned(),
                output: None,
                strategy: None,
                epub_version: None,
                media_type: None,
                error: Some(format!("Failed to read file: {e}")),
                error_category: Some("io".to_string()),
            };
            writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
            counter.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
    };

    // Extract cover bytes
    match thumbsup_core::extract_cover_bytes(&epub_bytes, policy, u64::MAX) {
        Ok((cover_bytes, report)) => {
            // Determine output filename
            let basename = sanitize_filename(
                &epub_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown"),
            );
            let ext = detect_image_extension(&cover_bytes);
            let output_filename = format!("{}.{}", basename, ext);
            let output_path = output_dir.join(&output_filename);

            // Handle filename collisions
            let mut final_path = output_path.clone();
            let mut collision_count = 0;
            while final_path.exists() {
                collision_count += 1;
                let new_filename = format!("{} ({}).{}", basename, collision_count, ext);
                final_path = output_dir.join(new_filename);
            }

            // Write the cover bytes
            if let Err(e) = fs::write(&final_path, &cover_bytes) {
                let result = ExtractionResult {
                    source: source_name.into_owned(),
                    output: None,
                    strategy: None,
                    epub_version: None,
                    media_type: None,
                    error: Some(format!("Failed to write cover: {e}")),
                    error_category: Some("io".to_string()),
                };
                writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }

            // Record success in manifest
            let result = ExtractionResult {
                source: source_name.into_owned(),
                output: Some(final_path.to_string_lossy().into_owned()),
                strategy: Some(report.strategy.to_string()),
                epub_version: Some(report.epub_version_major),
                media_type: report.cover_media_type.clone(),
                error: None,
                error_category: None,
            };
            writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
        }
        Err((err, report)) => {
            // Record failure in manifest
            let result = ExtractionResult {
                source: source_name.into_owned(),
                output: None,
                strategy: Some(report.strategy.to_string()),
                epub_version: Some(report.epub_version_major),
                media_type: None,
                error: Some(err.to_string()),
                error_category: Some(err.category().to_string()),
            };
            writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
        }
    }

    counter.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn main() -> Result<(), String> {
    let args = Args::parse();

    // Validate and parse policy
    let policy = match args.policy.as_str() {
        "strict" => CoverPolicy::Strict,
        "fallback" => CoverPolicy::FirstImageFallback,
        _ => {
            return Err(format!(
                "Invalid policy: {}. Use 'strict' or 'fallback'.",
                args.policy
            ))
        }
    };

    // Create output directory if it doesn't exist
    fs::create_dir_all(&args.output)
        .map_err(|e| format!("Failed to create output directory: {e}"))?;

    // Open manifest file
    let manifest_path = args.output.join("manifest.jsonl");
    let mut manifest_file = fs::File::create(&manifest_path)
        .map_err(|e| format!("Failed to create manifest file: {e}"))?;

    // Set up walkdir options
    let mut walkdir = WalkDir::new(&args.input);
    if !args.recursive {
        walkdir = walkdir.max_depth(1);
    }

    let processed_count = AtomicUsize::new(0);
    let total_start = std::time::Instant::now();

    // Process each EPUB file
    for entry in walkdir {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("epub") {
            process_epub(
                path,
                &args.output,
                policy,
                &processed_count,
                &mut manifest_file,
            )?;
        }
    }

    let total_elapsed = total_start.elapsed();
    let count = processed_count.load(Ordering::SeqCst);

    println!(
        "Processed {} EPUBs in {:.2?}. Manifest written to {}",
        count,
        total_elapsed,
        manifest_path.display()
    );

    Ok(())
}
