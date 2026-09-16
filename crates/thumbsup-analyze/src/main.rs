//! EPUB data quality analysis tool for digital library auditing.
//!
//! This tool iterates over EPUB files in a directory and produces a
//! structured report on data quality issues, cover metadata, and extraction
//! statistics. It is read-only and does not modify any files.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use clap::Parser;
use serde::Serialize;
use thumbsup_core::CoverPolicy;
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
    /// Whether a cover was successfully extracted.
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
    /// Cover thumbnail width in pixels (resized to fit max_side), if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_width: Option<u32>,
    /// Cover thumbnail height in pixels (resized to fit max_side), if successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_height: Option<u32>,
    /// Error message, if extraction failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Error category, if extraction failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_category: Option<String>,
}

/// Analyze a single EPUB file and record results.
fn analyze_epub(
    epub_path: &Path,
    output_dir: &Path,
    policy: CoverPolicy,
    counter: &AtomicUsize,
    report_file: &mut fs::File,
) -> Result<(), String> {
    let relative_path = epub_path
        .strip_prefix(output_dir.parent().unwrap_or(Path::new("")))
        .unwrap_or(epub_path);
    let source_name = relative_path.to_string_lossy();

    // Read the EPUB file
    let epub_bytes = match fs::read(epub_path) {
        Ok(b) => b,
        Err(e) => {
            let result = AnalysisResult {
                source: source_name.into_owned(),
                epub_version: 0,
                has_cover: false,
                strategy: None,
                cover_path: None,
                cover_media_type: None,
                cover_width: None,
                cover_height: None,
                error: Some(format!("Failed to read file: {e}")),
                error_category: Some("io".to_string()),
            };
            writeln!(report_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
            counter.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
    };

    // Use a large max_side to minimize resizing; dimensions reflect the fitted image
    // Note: these are thumbnail dimensions, not original cover dimensions
    const MAX_SIDE: u32 = 4096;
    match thumbsup_core::extract_cover(&epub_bytes, MAX_SIDE, policy, u64::MAX) {
        Ok(extracted) => {
            let result = AnalysisResult {
                source: source_name.into_owned(),
                epub_version: extracted.report.epub_version_major,
                has_cover: true,
                strategy: Some(extracted.report.strategy.to_string()),
                cover_path: extracted.report.cover_path.clone(),
                cover_media_type: extracted.report.cover_media_type.clone(),
                cover_width: Some(extracted.thumbnail.width),
                cover_height: Some(extracted.thumbnail.height),
                error: None,
                error_category: None,
            };
            writeln!(report_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
        }
        Err((err, report)) => {
            let result = AnalysisResult {
                source: source_name.into_owned(),
                epub_version: report.epub_version_major,
                has_cover: false,
                strategy: Some(report.strategy.to_string()),
                cover_path: None,
                cover_media_type: None,
                cover_width: None,
                cover_height: None,
                error: Some(err.to_string()),
                error_category: Some(err.category().to_string()),
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

    // Open report file
    let report_path = args.output.join("report.jsonl");
    let mut report_file =
        fs::File::create(&report_path).map_err(|e| format!("Failed to create report file: {e}"))?;

    // Open summary file
    let summary_path = args.output.join("summary.json");
    let mut summary_file = fs::File::create(&summary_path)
        .map_err(|e| format!("Failed to create summary file: {e}"))?;

    // Set up walkdir options
    let mut walkdir = WalkDir::new(&args.input);
    if !args.recursive {
        walkdir = walkdir.max_depth(1);
    }

    let processed_count = AtomicUsize::new(0);
    // let success_count = AtomicUsize::new(0);
    // let failure_count = AtomicUsize::new(0);
    let total_start = Instant::now();

    // Collect strategy counts
    use std::collections::HashMap;
    use std::sync::Mutex;
    // let strategy_counts: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
    // let error_counts: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());

    // Process each EPUB file
    for entry in walkdir {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("epub") {
            analyze_epub(
                path,
                &args.output,
                policy,
                &processed_count,
                &mut report_file,
            )?;

            // Update summary counts would go here in a more complete implementation
        }
    }

    let total_elapsed = total_start.elapsed();
    let count = processed_count.load(Ordering::SeqCst);

    // Write summary
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
