//! `thumbsup-shell` — Windows shell extension DLL implementing
//! `IThumbnailProvider` for EPUB files.
//!
//! Entry-point exports (in DLL terminology):
//!
//! | Export | Purpose |
//! |---|---|
//! | `DllMain`              | Process/thread attach/detach. |
//! | `DllGetClassObject`    | Hand out our `IClassFactory`. |
//! | `DllCanUnloadNow`      | Tell COM whether it's safe to unload. |
//! | `DllRegisterServer`    | `regsvr32 /i` install hook (per-machine). |
//! | `DllUnregisterServer`  | `regsvr32 /u` uninstall hook. |
//!
//! On non-Windows targets the crate compiles to an empty `cdylib` so the
//! workspace can be `cargo check`-ed on Linux/macOS CI.

#![cfg(windows)]
#![allow(non_snake_case)]
#![forbid(unsafe_op_in_unsafe_fn)]

mod bitmap;
mod clsid;
mod com;
mod config;
mod logging;
mod registry;
mod stream;

use std::ffi::c_void;
use std::sync::atomic::Ordering;

use windows::core::{Interface, GUID};
use windows::Win32::Foundation::{
    BOOL, CLASS_E_CLASSNOTAVAILABLE, E_POINTER, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

use crate::clsid::CLSID_EPUB_THUMBNAIL_PROVIDER;
use crate::com::{ClassFactory, OBJECT_COUNT};
use crate::registry::Scope;

/// Module handle stashed at `DLL_PROCESS_ATTACH` so we can later resolve
/// our own on-disk path from `DllRegisterServer` without depending on the
/// caller passing it in.
static mut MODULE_HANDLE: HMODULE = HMODULE(std::ptr::null_mut());

#[no_mangle]
pub extern "system" fn DllMain(
    hinstance: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // Suppress `DLL_THREAD_ATTACH`/`DETACH` callbacks; we don't need
            // them and they add measurable overhead in long-running hosts
            // like Explorer.
            unsafe {
                MODULE_HANDLE = HMODULE(hinstance.0);
                let _ = DisableThreadLibraryCalls(HMODULE(hinstance.0));
            }
        }
        DLL_PROCESS_DETACH => {}
        _ => {}
    }
    BOOL(1)
}

/// COM entry point: hand out an `IClassFactory` for our CLSID.
#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid:   *const GUID,
    ppv:    *mut *mut c_void,
) -> windows::core::HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    let requested = unsafe { *rclsid };
    if requested != CLSID_EPUB_THUMBNAIL_PROVIDER {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    unsafe { *ppv = std::ptr::null_mut(); }

    let factory: IClassFactory = ClassFactory::new().into();
    let riid = unsafe { *riid };
    unsafe { factory.query(&riid, ppv) }
}

/// COM entry point: report whether all our objects have been released.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> windows::core::HRESULT {
    if OBJECT_COUNT.load(Ordering::SeqCst) == 0 { S_OK } else { S_FALSE }
}

/// `regsvr32 /i` hook: register the thumbnail provider per-machine.
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> windows::core::HRESULT {
    let Some(path) = self_module_path() else {
        return windows::core::HRESULT(0x80070002u32 as i32); // ERROR_FILE_NOT_FOUND
    };
    match registry::register(Scope::Machine, &path) {
        Ok(()) => S_OK,
        Err(e) => windows::core::HRESULT(
            0x80070000u32 as i32 | e.raw_os_error().unwrap_or(1),
        ),
    }
}

/// `regsvr32 /u` hook: remove every key written by `DllRegisterServer`.
#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> windows::core::HRESULT {
    match registry::unregister(Scope::Machine) {
        Ok(()) => S_OK,
        Err(e) => windows::core::HRESULT(
            0x80070000u32 as i32 | e.raw_os_error().unwrap_or(1),
        ),
    }
}

fn self_module_path() -> Option<String> {
    let mut buf = [0u16; 1024];
    let handle = unsafe { MODULE_HANDLE };
    let len = unsafe { GetModuleFileNameW(handle, &mut buf) };
    if len == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}
