//! Batch EPUB cover image extractor for digital library analysis.
//!
//! This tool iterates over EPUB files in a directory, extracts the original
//! cover image bytes (without decoding, resizing, or color conversion), and
//! writes them to an output directory with a manifest for subsequent analysis.
//!
//! The input-to-output mapping is deterministic: the source path is always
//! relative to `--input`, and recursive extraction preserves that relative
//! directory structure below `--output`. Only image formats that ThumbsUp's
//! core renderer explicitly supports (JPEG, PNG, GIF) are written. The
//! extension is derived from the actual bytes, never from the OPF declaration.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use serde::Serialize;
use thumbsup_core::{detect_image_format, image_format_name, CoverPolicy, ImageFormat};
use walkdir::WalkDir;

/// Sanitize a string for use as a Windows filename by replacing invalid characters.
fn sanitize_filename(name: &str) -> String {
    const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\\', '/'];
    name.chars()
        .map(|c| if INVALID_CHARS.contains(&c) { '_' } else { c })
        .collect()
}

/// Return the output extension supported by the batch extractor for the
/// actual image format found in the cover bytes.
fn supported_extension(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Jpeg => Some("jpg"),
        ImageFormat::Png => Some("png"),
        ImageFormat::Gif => Some("gif"),
        _ => None,
    }
}

/// Build the deterministic output path corresponding to an EPUB path.
///
/// The path is relative to `input_dir`, so recursive input such as
/// `A/Book.epub` maps to `output/A/Book.jpg` rather than relying on the
/// output directory's location or on traversal order.
fn output_path_for(
    epub_path: &Path,
    input_dir: &Path,
    output_dir: &Path,
    extension: &str,
) -> Result<PathBuf, String> {
    let relative = epub_path
        .strip_prefix(input_dir)
        .map_err(|e| format!("source path is outside input directory: {e}"))?;

    let stem = relative
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let filename = format!("{}.{}", sanitize_filename(stem), extension);

    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    Ok(output_dir.join(parent).join(filename))
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
    /// Cover media type declared by the EPUB, if a cover was located.
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
    /// Image format detected from the extracted bytes, if recognizable.
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    /// Error message, if extraction failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Error category, if extraction failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_category: Option<String>,
}

fn format_name(format: ImageFormat) -> String {
    match format {
        ImageFormat::Jpeg => "jpeg".to_string(),
        ImageFormat::Png => "png".to_string(),
        ImageFormat::Gif => "gif".to_string(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

/// Process a single EPUB file and write its cover to the output directory.
fn process_epub(
    epub_path: &Path,
    input_dir: &Path,
    output_dir: &Path,
    policy: CoverPolicy,
    counter: &AtomicUsize,
    manifest_file: &mut fs::File,
) -> Result<(), String> {
    let relative_path = epub_path
        .strip_prefix(input_dir)
        .map_err(|e| format!("source path is outside input directory: {e}"))?;
    let source_name = relative_path.to_string_lossy().replace('\\', "/");

    let epub_bytes = match fs::read(epub_path) {
        Ok(b) => b,
        Err(e) => {
            let result = ExtractionResult {
                source: source_name,
                output: None,
                strategy: None,
                epub_version: None,
                media_type: None,
                format: None,
                error: Some(format!("Failed to read file: {e}")),
                error_category: Some("io".to_string()),
            };
            writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
            counter.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
    };

    match thumbsup_core::extract_cover_bytes(&epub_bytes, policy, u64::MAX) {
        Ok((cover_bytes, report)) => {
            let format = match detect_image_format(&cover_bytes) {
                Ok(format) => format,
                Err(err) => {
                    let result = ExtractionResult {
                        source: source_name,
                        output: None,
                        strategy: Some(report.strategy.to_string()),
                        epub_version: Some(report.epub_version_major),
                        media_type: report.cover_media_type.clone(),
                        format: None,
                        error: Some(format!("Cover format could not be identified: {err}")),
                        error_category: Some("unknown-cover-format".to_string()),
                    };
                    writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                        .map_err(|e| e.to_string())?;
                    counter.fetch_add(1, Ordering::SeqCst);
                    return Ok(());
                }
            };

            let format_label = image_format_name(format);
            let extension = match supported_extension(format) {
                Some(ext) => ext,
                None => {
                    let result = ExtractionResult {
                        source: source_name,
                        output: None,
                        strategy: Some(report.strategy.to_string()),
                        epub_version: Some(report.epub_version_major),
                        media_type: report.cover_media_type.clone(),
                        format: Some(format_label.clone()),
                        error: Some(format!(
                            "Cover format {format_label} is not supported for extraction output"
                        )),
                        error_category: Some("unsupported-cover-format".to_string()),
                    };
                    writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                        .map_err(|e| e.to_string())?;
                    counter.fetch_add(1, Ordering::SeqCst);
                    return Ok(());
                }
            };

            let final_path = output_path_for(epub_path, input_dir, output_dir, extension)?;
            if let Some(parent) = final_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create output directory: {e}"))?;
            }

            if let Err(e) = fs::write(&final_path, &cover_bytes) {
                let result = ExtractionResult {
                    source: source_name,
                    output: None,
                    strategy: Some(report.strategy.to_string()),
                    epub_version: Some(report.epub_version_major),
                    media_type: report.cover_media_type.clone(),
                    format: Some(format_label.clone()),
                    error: Some(format!("Failed to write cover: {e}")),
                    error_category: Some("io".to_string()),
                };
                writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                    .map_err(|e| e.to_string())?;
                counter.fetch_add(1, Ordering::SeqCst);
                return Ok(());
            }

            let output_name = final_path
                .strip_prefix(output_dir)
                .unwrap_or(&final_path)
                .to_string_lossy()
                .replace('\\', "/");
            let result = ExtractionResult {
                source: source_name,
                output: Some(output_name),
                strategy: Some(report.strategy.to_string()),
                epub_version: Some(report.epub_version_major),
                media_type: report.cover_media_type.clone(),
                format: Some(format_label),
                error: None,
                error_category: None,
            };
            writeln!(manifest_file, "{}", serde_json::to_string(&result).unwrap())
                .map_err(|e| e.to_string())?;
        }
        Err((err, report)) => {
            let result = ExtractionResult {
                source: source_name,
                output: None,
                strategy: Some(report.strategy.to_string()),
                epub_version: Some(report.epub_version_major),
                media_type: report.cover_media_type.clone(),
                format: None,
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

    let manifest_path = args.output.join("manifest.jsonl");
    let mut manifest_file = fs::File::create(&manifest_path)
        .map_err(|e| format!("Failed to create manifest file: {e}"))?;

    let mut walkdir = WalkDir::new(&args.input);
    if !args.recursive {
        walkdir = walkdir.max_depth(1);
    }

    let processed_count = AtomicUsize::new(0);
    let total_start = std::time::Instant::now();

    for entry in walkdir {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("epub") {
            process_epub(
                path,
                &args.input,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_match_actual_formats() {
        assert_eq!(supported_extension(ImageFormat::Jpeg), Some("jpg"));
        assert_eq!(supported_extension(ImageFormat::Png), Some("png"));
        assert_eq!(supported_extension(ImageFormat::Gif), Some("gif"));
    }

    #[test]
    fn unsupported_formats_have_no_output_extension() {
        assert_eq!(supported_extension(ImageFormat::Bmp), None);
    }

    #[test]
    fn recursive_output_path_preserves_relative_directory() {
        let path = Path::new("/books/A/Book.epub");
        let output = output_path_for(
            path,
            Path::new("/books"),
            Path::new("/analysis/covers"),
            "jpg",
        )
        .unwrap();
        assert_eq!(output, Path::new("/analysis/covers/A/Book.jpg"));
    }

    #[test]
    fn source_path_is_relative_to_input_not_output() {
        let path = Path::new("/books/A/Book.epub");
        let relative = path.strip_prefix(Path::new("/books")).unwrap();
        assert_eq!(relative, Path::new("A/Book.epub"));
    }
}
