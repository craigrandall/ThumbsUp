//! Embeds the ThumbsUp icon into thumbsup-config.exe's PE resources so
//! File Explorer, the taskbar, and Alt-Tab show the correct icon for the
//! binary itself -- independent of the eframe window icon set at runtime
//! in main.rs, and independent of the MSI-level ARPPRODUCTICON/Shortcut
//! icon wiring in installer/Product.wxs.
//!
//! References installer/branding/thumbsup.ico directly rather than a
//! copy, so there is exactly one file to update if the icon ever changes.

fn main() {
    #[cfg(windows)]
    {
        let icon_path = "../../installer/branding/thumbsup.ico";
        println!("cargo:rerun-if-changed={icon_path}");

        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon_path);
        if let Err(e) = res.compile() {
            panic!("failed to embed thumbsup.ico into thumbsup-config.exe: {e}");
        }
    }
}
