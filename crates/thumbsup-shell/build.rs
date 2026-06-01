// Pass the export-definition file to MSVC's link.exe. Skipped on non-Windows
// where the cdylib produces nothing useful anyway.
fn main() {
    println!("cargo:rerun-if-changed=exports.def");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg=/DEF:{manifest}/exports.def");
    }
}
