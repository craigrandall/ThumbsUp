//! EPUB data quality analysis tool for digital library auditing.
//!
//! This tool iterates over EPUB files in a directory and produces a
//! structured report on data quality issues, cover metadata, and extraction
//! statistics. It is read-only and does not modify any files.
//!
//! Cover analysis operates on the original cover resource returned by
//! `extract_cover_bytes()`. It therefore reports intrinsic properties of the
//! source cover rather than dimensions of a resized Windows thumbnail.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use clap::Parser;
use serde::Serialize;
use thumbsup_core::{detect_image_format, image_dimensions, image_format_name, CoverPolicy};
use walkdir::WalkDir;

/// Command-line arguments for the analyzer.
#[derive(Parser, Debug)]
#[command(name = "thumbsup-analyze")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory containing EPUB files to analyze.
    #[arg(short, long)]
    input: PathBuf,

    /// Directory to write analysis report.
    #[arg(short, long)]
    output: PathBuf,

    /// Recursively search subdirectories for EPUB files.
    #[arg(short, long, default_value = "false")]
    recursive: bool,

    /// Cover extraction policy: 'strict' or 'fallback'.
    #[arg(short, long, default_value = "strict")]
    policy: String,
}

/// Analysis result for a single EPUB, serialized to JSON for the report.
#[derive(Serialize)]
struct AnalysisResult {
    /// Source EPUB file path (relative to input directory).
    source: String,
    /// EPUB version major (2, 3, or 0 if unknown).
    epub_version: u8,
    /// Whether a cover resource was successfully located and extracted.
    has_cover: bool,
    /// Cover extraction strategy, if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    strategy: Option<String>,
    /// Cover file path inside the archive, if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_path: Option<String>,
    /// Declared cover media type, if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_media_type: Option<String>,
    /// Image format detected from the original cover bytes, if recognizable.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_format: Option<String>,
    /// Original cover width in pixels, if the format is supported for decoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_width: Option<u32>,
    /// Original cover height in pixels, if the format is supported for decoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_height: Option<u32>,
    /// Original cover resource size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_byte_size: Option<usize>,
    /// Extraction error message, if locating or reading the cover failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Extraction error category, if locating or reading the cover failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_category: Option<String>,
    /// Image-analysis error when the original cover was extracted but could
    /// not be decoded by ThumbsUp's supported raster codecs.
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis_error: Option<String>,
}

/// Analyze a single EPUB file and record results.
fn analyze_epub(
    epub_path: &Path,
    input_dir: &Path,
    policy: CoverPolicy,
    counter: &AtomicUsize,
    report_file: &mut fs::File,
) -> Result<(), String> {
    let relative_path = epub_path
        .strip_prefix(input_dir)
        .map_err(|e| format!("source path is outside input directory: {e}"))?;
    let source_name = relative_path.to_string_lossy().replace('\\', "/");

    let epub_bytes = match fs::read(epub_path) {
        Ok(b) => b,
        Err(e) => {
            let result = AnalysisResult {
                source: source_name,
                epub_version: 0,
                has_cover: false,
                strategy: None,
                cover_path: None,
                cover_media_type: None,
                cover_format: None,
                cover_width: None,
                cover_height: None,
                cover_byte_size: None,
                error: Some(format!("Failed to read file: {e}")),
                error_category: Some("io".to_string()),
                analysis_error: None,
            };
            writeln!(report_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
            counter.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
    };

    match thumbsup_core::extract_cover_bytes(&epub_bytes, policy, u64::MAX) {
        Ok((cover_bytes, report)) => {
            let (cover_format, cover_width, cover_height, analysis_error) =
                match detect_image_format(&cover_bytes) {
                    Ok(format) => {
                        let name = image_format_name(format);
                        match image_dimensions(&cover_bytes) {
                            Ok((width, height)) => (Some(name), Some(width), Some(height), None),
                            Err(err) => (Some(name), None, None, Some(err.to_string())),
                        }
                    }
                    Err(err) => (None, None, None, Some(err.to_string())),
                };

            let result = AnalysisResult {
                source: source_name,
                epub_version: report.epub_version_major,
                has_cover: true,
                strategy: Some(report.strategy.to_string()),
                cover_path: report.cover_path.clone(),
                cover_media_type: report.cover_media_type.clone(),
                cover_format,
                cover_width,
                cover_height,
                cover_byte_size: Some(cover_bytes.len()),
                error: None,
                error_category: None,
                analysis_error,
            };
            writeln!(report_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
        }
        Err((err, report)) => {
            let result = AnalysisResult {
                source: source_name,
                epub_version: report.epub_version_major,
                has_cover: false,
                strategy: Some(report.strategy.to_string()),
                cover_path: report.cover_path.clone(),
                cover_media_type: report.cover_media_type.clone(),
                cover_format: None,
                cover_width: None,
                cover_height: None,
                cover_byte_size: None,
                error: Some(err.to_string()),
                error_category: Some(err.category().to_string()),
                analysis_error: None,
            };
            writeln!(report_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
        }
    }

    counter.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn main() -> Result<(), String> {
    let args = Args::parse();

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

    fs::create_dir_all(&args.output)
        .map_err(|e| format!("Failed to create output directory: {e}"))?;

    let report_path = args.output.join("report.jsonl");
    let mut report_file =
        fs::File::create(&report_path).map_err(|e| format!("Failed to create report file: {e}"))?;

    let summary_path = args.output.join("summary.json");
    let mut summary_file = fs::File::create(&summary_path)
        .map_err(|e| format!("Failed to create summary file: {e}"))?;

    let mut walkdir = WalkDir::new(&args.input);
    if !args.recursive {
        walkdir = walkdir.max_depth(1);
    }

    let processed_count = AtomicUsize::new(0);
    let total_start = Instant::now();

    for entry in walkdir {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("epub") {
            analyze_epub(
                path,
                &args.input,
                policy,
                &processed_count,
                &mut report_file,
            )?;
        }
    }

    let total_elapsed = total_start.elapsed();
    let count = processed_count.load(Ordering::SeqCst);

    let summary = serde_json::json!({
        "total_epubs": count,
        "elapsed_seconds": total_elapsed.as_secs_f64(),
        "elapsed_millis": total_elapsed.as_millis(),
    });
    writeln!(summary_file, "{}", serde_json::to_string(&summary).unwrap())
        .map_err(|e| e.to_string())?;

    println!(
        "Analyzed {} EPUBs in {:.2?}. Report written to {}",
        count,
        total_elapsed,
        report_path.display()
    );

    Ok(())
}
