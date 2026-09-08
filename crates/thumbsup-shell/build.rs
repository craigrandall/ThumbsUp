// Pass the export-definition file to MSVC's link.exe for the DLL only.
// Skipped on non-Windows where the cdylib is not useful.
fn main() {
    println!("cargo:rerun-if-changed=exports.def");

    let target = std::env::var("TARGET").unwrap_or_default();

    if target.contains("windows-msvc") {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-cdylib=/DEF:{manifest}/exports.def");
    }
}
