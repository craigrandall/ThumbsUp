//! Image-side of the pipeline: decode the cover bytes, fit-into-box resize
//! preserving aspect ratio, and produce a top-down BGRA8 buffer ready to
//! hand to `CreateDIBSection` on Windows.
//!
//! Splitting this out lets us unit-test the whole pipeline on Linux without
//! pulling in any GDI types.

use crate::error::{EpubError, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};

/// A decoded, resized cover ready to be uploaded to a Windows DIB.
///
/// * `pixels` is in **BGRA8 top-down** order — one row at a time, top row
///   first, with each pixel encoded as `[B, G, R, A]`. This is the layout
///   `IThumbnailProvider` expects when paired with `WTSAT_ARGB`.
/// * `width` and `height` are the dimensions of the decoded image, *not*
///   the requested thumbnail size — Explorer is tolerant of either, and
///   centring is performed by the shell.
#[derive(Clone, Debug)]
pub struct Thumbnail {
    pub width:  u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Thumbnail {
    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// Total byte length of the pixel buffer (4 bytes per pixel, BGRA8).
    pub fn byte_len(&self) -> usize {
        self.pixel_count() * 4
    }
}

/// Decode arbitrary cover bytes (JPEG / PNG / GIF) into a `DynamicImage`,
/// rejecting formats we don't support.
pub fn decode_cover(bytes: &[u8]) -> Result<DynamicImage> {
    // Sniff the format from the magic bytes rather than trusting the
    // declared media-type, since some EPUBs lie about it.
    let format = image::guess_format(bytes)
        .map_err(|e| EpubError::ImageDecode(e.to_string()))?;
    match format {
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif => {}
        other => {
            return Err(EpubError::ImageDecode(format!(
                "unsupported image format: {other:?}"
            )));
        }
    }
    image::load_from_memory_with_format(bytes, format)
        .map_err(|e| EpubError::ImageDecode(e.to_string()))
}

/// Resize so that the longer side is at most `max_side`, preserving aspect
/// ratio. If the image is already smaller it is returned unchanged.
///
/// The resampling filter is `Lanczos3`; `CatmullRom` would be faster but
/// produces visibly worse covers at small sizes (Explorer commonly asks
/// for 96 or 256 pixels).
pub fn fit_into(img: DynamicImage, max_side: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    let longest = w.max(h);
    if longest <= max_side || max_side == 0 {
        return img;
    }
    // `image::DynamicImage::resize` already preserves aspect ratio when
    // given equal max-width and max-height.
    img.resize(max_side, max_side, FilterType::Lanczos3)
}

/// Convert a `DynamicImage` into the BGRA8 top-down buffer required by
/// `CreateDIBSection`. Source images without an alpha channel are given
/// fully opaque alpha.
pub fn to_bgra8(img: DynamicImage) -> Thumbnail {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for px in rgba.pixels() {
        // Source: RGBA. Target: BGRA.
        pixels.push(px[2]); // B
        pixels.push(px[1]); // G
        pixels.push(px[0]); // R
        pixels.push(px[3]); // A
    }
    Thumbnail { width, height, pixels }
}

/// Convenience: decode → fit → convert in one call. This is what the
/// shell extension actually uses.
pub fn prepare_thumbnail(bytes: &[u8], max_side: u32) -> Result<Thumbnail> {
    let img    = decode_cover(bytes)?;
    let fitted = fit_into(img, max_side);
    Ok(to_bgra8(fitted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    /// Build a tiny PNG in-memory for testing without committing fixtures.
    fn synth_png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, _> =
            ImageBuffer::from_fn(w, h, |_, _| Rgba(color));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn synth_jpeg(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
        let img: ImageBuffer<image::Rgb<u8>, _> =
            ImageBuffer::from_fn(w, h, |_, _| image::Rgb(color));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Jpeg)
            .unwrap();
        out
    }

    #[test]
    fn decode_png_succeeds() {
        let bytes = synth_png(10, 20, [255, 0, 0, 255]);
        let img = decode_cover(&bytes).unwrap();
        assert_eq!(img.dimensions(), (10, 20));
    }

    #[test]
    fn decode_jpeg_succeeds() {
        let bytes = synth_jpeg(40, 60, [10, 20, 30]);
        let img = decode_cover(&bytes).unwrap();
        assert_eq!(img.dimensions(), (40, 60));
    }

    #[test]
    fn decode_rejects_garbage() {
        let bytes = b"not an image at all";
        assert!(matches!(decode_cover(bytes), Err(EpubError::ImageDecode(_))));
    }

    #[test]
    fn decode_rejects_bmp_even_if_image_crate_supports_it() {
        // We deliberately whitelist JPEG/PNG/GIF; BMP must be rejected with
        // a clean ImageDecode error rather than crashing the host.
        // 14-byte BMP header + 40-byte DIB header + 4 bytes pixel data.
        let mut bmp = vec![0u8; 58];
        bmp[0..2].copy_from_slice(b"BM");
        let res = decode_cover(&bmp);
        assert!(matches!(res, Err(EpubError::ImageDecode(_))));
    }

    #[test]
    fn fit_into_preserves_aspect_ratio() {
        let img = DynamicImage::new_rgba8(2000, 1000);
        let resized = fit_into(img, 256);
        let (w, h) = resized.dimensions();
        assert_eq!(w, 256);
        assert_eq!(h, 128); // 2:1 ratio preserved
    }

    #[test]
    fn fit_into_does_not_upscale() {
        let img = DynamicImage::new_rgba8(50, 80);
        let resized = fit_into(img, 256);
        assert_eq!(resized.dimensions(), (50, 80));
    }

    #[test]
    fn fit_into_zero_max_is_passthrough() {
        let img = DynamicImage::new_rgba8(50, 80);
        let resized = fit_into(img, 0);
        assert_eq!(resized.dimensions(), (50, 80));
    }

    #[test]
    fn to_bgra8_layout_is_bgra_top_down() {
        // Build a 2x1 image with known RGBA pixels and verify byte order.
        let mut img = ImageBuffer::<Rgba<u8>, _>::new(2, 1);
        img.put_pixel(0, 0, Rgba([0xAA, 0xBB, 0xCC, 0xDD])); // R,G,B,A
        img.put_pixel(1, 0, Rgba([0x11, 0x22, 0x33, 0x44]));
        let thumb = to_bgra8(DynamicImage::ImageRgba8(img));
        assert_eq!(thumb.width, 2);
        assert_eq!(thumb.height, 1);
        // Pixel 0: BGRA = CC BB AA DD
        assert_eq!(&thumb.pixels[0..4], &[0xCC, 0xBB, 0xAA, 0xDD]);
        // Pixel 1: BGRA = 33 22 11 44
        assert_eq!(&thumb.pixels[4..8], &[0x33, 0x22, 0x11, 0x44]);
    }

    #[test]
    fn to_bgra8_jpeg_gets_opaque_alpha() {
        let bytes = synth_jpeg(2, 1, [10, 20, 30]);
        let thumb = prepare_thumbnail(&bytes, 256).unwrap();
        // Every alpha byte must be 0xFF (opaque).
        for chunk in thumb.pixels.chunks_exact(4) {
            assert_eq!(chunk[3], 0xFF);
        }
    }

    #[test]
    fn prepare_thumbnail_resizes_large_input() {
        let bytes = synth_png(1024, 2048, [50, 50, 50, 255]);
        let thumb = prepare_thumbnail(&bytes, 256).unwrap();
        assert!(thumb.width <= 256 && thumb.height <= 256);
        assert!(thumb.width == 256 || thumb.height == 256);
        assert_eq!(thumb.byte_len(), thumb.pixels.len());
    }
}
