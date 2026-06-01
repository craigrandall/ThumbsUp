//! Read an `IStream` into a `Vec<u8>` with a hard byte cap.
//!
//! Explorer hands us the EPUB via an `IStream`. We slurp the whole thing
//! into memory because the `zip` crate needs `Read + Seek`. For a
//! reasonably-sized EPUB (< 100 MiB) this is fine; if the stream exceeds
//! the configured cap we abort early and let Explorer fall back to its
//! generic icon.

#![cfg(windows)]

use windows::core::Result as WResult;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::IStream;

/// Read every byte the stream will give us, up to `cap` bytes inclusive.
/// If the stream produces more than `cap` bytes the call returns `E_FAIL`.
pub fn read_stream(stream: &IStream, cap: u64) -> WResult<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut chunk = [0u8; 64 * 1024];

    loop {
        let mut got: u32 = 0;
        // IStream::Read may return S_FALSE when fewer bytes are available
        // than requested; that is *not* an error and is handled by the
        // returned `got` count.
        let hr = unsafe {
            stream.Read(
                chunk.as_mut_ptr() as *mut _,
                chunk.len() as u32,
                Some(&mut got as *mut u32),
            )
        };
        // S_OK = 0, S_FALSE = 1; both have a clear sign bit so .ok() succeeds.
        hr.ok()?;
        if got == 0 {
            break;
        }
        if (out.len() as u64).saturating_add(got as u64) > cap {
            return Err(E_FAIL.into());
        }
        out.extend_from_slice(&chunk[..got as usize]);
    }

    Ok(out)
}
