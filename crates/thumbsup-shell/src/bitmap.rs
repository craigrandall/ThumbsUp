//! BGRA8 buffer → HBITMAP conversion.
//!
//! `IThumbnailProvider::GetThumbnail` returns ownership of an `HBITMAP` to
//! the shell, which then takes responsibility for `DeleteObject`-ing it.
//! We therefore must not free the bitmap on the success path.

#![cfg(windows)]

use std::ffi::c_void;

use thumbsup_core::Thumbnail;
use windows::core::Result as WResult;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Gdi::*;

/// Build a 32-bit top-down HBITMAP from a [`Thumbnail`]. The returned
/// handle is owned by the caller (Explorer); on the error path we return
/// nothing and the caller drops the thumbnail.
pub fn create_hbitmap(thumb: &Thumbnail) -> WResult<HBITMAP> {
    if thumb.width == 0 || thumb.height == 0 {
        return Err(E_FAIL.into());
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize:        std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth:       thumb.width as i32,
            // Negative height = top-down DIB. Our pixel buffer is also
            // top-down, so the two match without an extra flip.
            biHeight:      -(thumb.height as i32),
            biPlanes:      1,
            biBitCount:    32,
            biCompression: BI_RGB.0 as u32,
            biSizeImage:   0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed:     0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD::default()],
    };

    let mut bits: *mut c_void = std::ptr::null_mut();
    let hbmp = unsafe {
        CreateDIBSection(
            None,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?
    };

    if hbmp.is_invalid() || bits.is_null() {
        return Err(E_FAIL.into());
    }

    // Sanity check: the buffer the GDI gave us must be exactly large enough
    // for our pixels. CreateDIBSection rounds rows up to a 4-byte boundary,
    // but for 32-bpp images each row is already 4-byte aligned by construction.
    let expected = thumb.byte_len();
    unsafe {
        std::ptr::copy_nonoverlapping(thumb.pixels.as_ptr(), bits as *mut u8, expected);
    }

    Ok(hbmp)
}
