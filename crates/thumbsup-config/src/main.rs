//! Binary entry point for the GUI configuration tool.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
#[cfg(windows)]
mod cache;
mod diag;
#[cfg(windows)]
mod regio;
mod settings;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 560.0])
            .with_min_inner_size([520.0, 360.0])
            .with_title("ThumbsUp Configuration"),
        ..Default::default()
    };
    eframe::run_native(
        "ThumbsUp",
        options,
        Box::new(|_cc| Box::new(app::ConfigApp::new())),
    )
}
