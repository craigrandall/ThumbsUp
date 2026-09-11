//! Binary entry point for the GUI configuration tool.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
#[cfg(windows)]
mod cache;
mod diag;
#[cfg(windows)]
mod regio;
mod settings;

/// Decodes the embedded window icon into the raw RGBA buffer eframe/winit expects.
///
/// This controls the title bar / taskbar / Alt-Tab icon while the app is
/// running. It is independent of (and does not replace) the icon embedded
/// in the .exe's own PE resources -- see build.rs for that.
fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../assets/icon-128.png");
    let image = image::load_from_memory(bytes)
        .expect("embedded icon-128.png must decode")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 560.0])
            .with_min_inner_size([520.0, 360.0])
            .with_title("ThumbsUp Configuration")
            .with_icon(load_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "ThumbsUp",
        options,
        Box::new(|_cc| Box::new(app::ConfigApp::new())),
    )
}
