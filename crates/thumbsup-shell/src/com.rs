//! COM types: the [`EpubThumbnailProvider`] (which implements both
//! `IInitializeWithStream` and `IThumbnailProvider`) and the
//! [`ClassFactory`] that creates instances of it.
//!
//! Threading model is **Apartment**: Explorer marshals each provider into
//! its own apartment, so we don't need internal locking for COM call
//! ordering. The `Mutex` around the cached bytes is a paranoia measure —
//! it costs almost nothing and rules out the possibility of a future
//! Windows version concurrently dispatching `Initialize` and
//! `GetThumbnail` on the same instance.

#![cfg(windows)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use thumbsup_core::extract_cover_with_deadline;
use windows::core::{implement, IUnknown, Interface, Result as WResult, GUID};
use windows::Win32::Foundation::{
    BOOL, CLASS_E_NOAGGREGATION, E_FAIL, E_NOINTERFACE, E_POINTER,
};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, IStream};
use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithStream;
use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithStream_Impl;
use windows::Win32::UI::Shell::{
    IThumbnailProvider, IThumbnailProvider_Impl, WTSAT_ARGB, WTS_ALPHATYPE,
    WTS_E_FAILEDEXTRACTION,
};

use crate::bitmap::create_hbitmap;
use crate::config::Config;
use crate::logging::log_event;
use crate::stream::read_stream;

/// Live-instance counter consulted by [`crate::DllCanUnloadNow`]. Every
/// outstanding provider or factory increments it; drops decrement.
pub static OBJECT_COUNT: AtomicI32 = AtomicI32::new(0);

/// The thumbnail-provider COM object.
#[implement(IInitializeWithStream, IThumbnailProvider)]
pub struct EpubThumbnailProvider {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Bytes of the EPUB read from the stream during `Initialize`.
    bytes:  Option<Vec<u8>>,
    /// Snapshot of the registry config, captured at `Initialize` time so
    /// the same policy is used for the matching `GetThumbnail`.
    config: Config,
}

impl EpubThumbnailProvider {
    pub fn new() -> Self {
        OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
        EpubThumbnailProvider {
            inner: Mutex::new(Inner {
                bytes:  None,
                config: Config::default(),
            }),
        }
    }
}

impl Drop for EpubThumbnailProvider {
    fn drop(&mut self) {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IInitializeWithStream_Impl for EpubThumbnailProvider_Impl {
    fn Initialize(&self, pstream: Option<&IStream>, _grfmode: u32) -> WResult<()> {
        let stream = pstream.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let cfg = Config::load();
        if !cfg.enabled {
            // Master switch off: refuse cleanly. WTS_E_FAILEDEXTRACTION
            // tells the shell to use the file's generic icon.
            return Err(windows::core::Error::from(WTS_E_FAILEDEXTRACTION));
        }
        let bytes = read_stream(stream, cfg.max_file_bytes)?;
        let mut g = self.inner.lock().map_err(|_| windows::core::Error::from(E_FAIL))?;
        g.bytes  = Some(bytes);
        g.config = cfg;
        Ok(())
    }
}

impl IThumbnailProvider_Impl for EpubThumbnailProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> WResult<()> {
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(windows::core::Error::from(E_POINTER));
        }

        let (bytes, cfg) = {
            let g = self.inner.lock().map_err(|_| windows::core::Error::from(E_FAIL))?;
            let b = g.bytes.clone().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            (b, g.config.clone())
        };

        match extract_cover_with_deadline(
            &bytes, cx, cfg.cover_policy, cfg.max_file_bytes,
            // Map the user-configured millisecond budget into a Duration.
            // Zero means "no timeout"; any positive value enforces it.
            if cfg.max_thumbnail_ms == 0 {
                None
            } else {
                Some(Duration::from_millis(cfg.max_thumbnail_ms as u64))
            },
        ) {
            Ok(extracted) => {
                let hbmp = create_hbitmap(&extracted.thumbnail)?;
                unsafe {
                    *phbmp    = hbmp;
                    *pdwalpha = WTSAT_ARGB;
                }
                log_event(
                    &cfg,
                    "<stream>",
                    "ok",
                    &format!(
                        "v{} strategy={} path={:?} mt={:?}",
                        extracted.report.epub_version_major,
                        extracted.report.strategy,
                        extracted.report.cover_path,
                        extracted.report.cover_media_type,
                    ),
                );
                Ok(())
            }
            Err((err, report)) => {
                log_event(
                    &cfg,
                    "<stream>",
                    err.category(),
                    &format!(
                        "v{} strategy={} err={}",
                        report.epub_version_major, report.strategy, err
                    ),
                );
                // Translate every failure into WTS_E_FAILEDEXTRACTION so
                // Explorer falls back to the generic icon rather than
                // surfacing an error to the user.
                Err(windows::core::Error::from(WTS_E_FAILEDEXTRACTION))
            }
        }
    }
}

/// The class factory Explorer asks for via `DllGetClassObject`. It just
/// vends new [`EpubThumbnailProvider`] instances.
#[implement(IClassFactory)]
pub struct ClassFactory;

impl ClassFactory {
    pub fn new() -> Self {
        OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
        ClassFactory
    }
}

impl Drop for ClassFactory {
    fn drop(&mut self) {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Option<&IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> WResult<()> {
        if punkouter.is_some() {
            // We do not support COM aggregation.
            return Err(windows::core::Error::from(CLASS_E_NOAGGREGATION));
        }
        if ppvobject.is_null() {
            return Err(windows::core::Error::from(E_POINTER));
        }
        unsafe { *ppvobject = std::ptr::null_mut(); }

        // Build the provider, then QueryInterface to whatever Explorer asked for.
        let provider: IInitializeWithStream = EpubThumbnailProvider::new().into();
        let unknown: IUnknown = provider.cast()?;
        let riid = unsafe { *riid };
        unsafe {
            let hr = unknown.query(&riid, ppvobject);
            if hr.is_ok() {
                Ok(())
            } else {
                Err(windows::core::Error::from(E_NOINTERFACE))
            }
        }
    }

    fn LockServer(&self, flock: BOOL) -> WResult<()> {
        // Lock/unlock the server: increment OBJECT_COUNT while locked so the
        // DLL is not unloaded out from under a holder of the factory.
        if flock.as_bool() {
            OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}
