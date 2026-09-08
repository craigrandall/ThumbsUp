//! The egui application: a tabbed window with Settings, Register/Unregister,
//! Cache, and Diagnostics panes.

use eframe::egui;

use crate::settings::{FallbackPolicy, Settings};

use crate::diag;
#[cfg(windows)]
use crate::{cache, regio};

#[derive(PartialEq, Eq)]
enum Tab {
    Settings,
    Registration,
    Cache,
    Diagnostics,
    About,
}

pub struct ConfigApp {
    settings: Settings,
    /// Settings as last loaded from / saved to the registry. Used to detect
    /// unsaved changes and offer a Save button.
    saved: Settings,
    tab: Tab,
    /// Path of the deployed DLL. Pre-populated from the install location
    /// when running from the installer's Start Menu shortcut.
    dll_path: String,
    status: String,
    diagnostics: Vec<diag::DiagEntry>,
}

impl ConfigApp {
    pub fn new() -> Self {
        let settings = load_settings();
        ConfigApp {
            saved: settings.clone(),
            settings,
            tab: Tab::Settings,
            dll_path: default_dll_path(),
            status: String::new(),
            diagnostics: Vec::new(),
        }
    }
}

#[cfg(windows)]
fn load_settings() -> Settings {
    regio::load_settings()
}
#[cfg(not(windows))]
fn load_settings() -> Settings {
    Settings::default()
}

fn default_dll_path() -> String {
    // Best guess: alongside our exe.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir
                .join("thumbsup_shell.dll")
                .to_string_lossy()
                .into_owned();
        }
    }
    "thumbsup_shell.dll".into()
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("ThumbsUp");
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
                ui.selectable_value(&mut self.tab, Tab::Registration, "Registration");
                ui.selectable_value(&mut self.tab, Tab::Cache, "Thumbnail Cache");
                ui.selectable_value(&mut self.tab, Tab::Diagnostics, "Diagnostics");
                ui.selectable_value(&mut self.tab, Tab::About, "About");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Settings => self.draw_settings(ui),
            Tab::Registration => self.draw_registration(ui),
            Tab::Cache => self.draw_cache(ui),
            Tab::Diagnostics => self.draw_diagnostics(ui),
            Tab::About => self.draw_about(ui),
        });

        if !self.status.is_empty() {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.label(&self.status);
            });
        }
    }
}

impl ConfigApp {
    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Behaviour");
        ui.checkbox(&mut self.settings.enabled, "Enable thumbnail handler")
            .on_hover_text(
                "When unchecked, Explorer will use the generic icon for .epub files. \
               Useful for temporarily disabling without uninstalling.",
            );

        ui.add_space(8.0);
        ui.label("Maximum file size to process (MB):");
        ui.add(
            egui::DragValue::new(&mut self.settings.max_file_mb)
                .clamp_range(1u64..=4096u64)
                .speed(1.0),
        )
        .on_hover_text("EPUBs larger than this are skipped to keep Explorer responsive.");

        ui.add_space(8.0);
        ui.label("Per-thumbnail timeout (ms):");
        ui.add(
            egui::DragValue::new(&mut self.settings.max_thumbnail_ms)
                .clamp_range(0u32..=60_000u32)
                .speed(100.0),
        )
        .on_hover_text(
            "Maximum time the handler will spend on a single thumbnail. \
             Zero disables the timeout.",
        );

        ui.add_space(12.0);
        ui.heading("Fallback strategy");
        ui.label("When no compliant cover is declared in the OPF:");
        for policy in [FallbackPolicy::Strict, FallbackPolicy::FirstImage] {
            ui.radio_value(&mut self.settings.fallback_policy, policy, policy.label());
        }

        ui.add_space(12.0);
        ui.heading("Logging");
        ui.checkbox(&mut self.settings.logging_enabled, "Write diagnostics log");
        ui.horizontal(|ui| {
            ui.label("Log file:");
            ui.text_edit_singleline(&mut self.settings.log_path);
            if ui.button("Default").clicked() {
                self.settings.log_path.clear();
            }
        });

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let dirty = !settings_eq(&self.settings, &self.saved);
            if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
                self.save_settings();
            }
            if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                self.settings = self.saved.clone();
                self.status = "Reverted unsaved changes.".into();
            }
        });
    }

    fn save_settings(&mut self) {
        #[cfg(windows)]
        match regio::save_settings(&self.settings) {
            Ok(_) => {
                self.saved = self.settings.clone();
                self.status =
                    "Settings saved. They take effect on the next thumbnail request.".into();
            }
            Err(e) => self.status = format!("Failed to save settings: {e}"),
        }
        #[cfg(not(windows))]
        {
            self.saved = self.settings.clone();
            self.status = "Settings saved (no-op on non-Windows build).".into();
        }
    }

    fn draw_registration(&mut self, ui: &mut egui::Ui) {
        ui.heading("Shell extension registration");
        ui.label(
            "Registering the DLL tells File Explorer to load it for .epub files. \
             You only need to do this if you installed manually (e.g. by copying \
             the DLL out of a zip). The MSI installer registers automatically.",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("DLL path:");
            ui.text_edit_singleline(&mut self.dll_path);
            #[cfg(windows)]
            if ui.button("Browse…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("DLL", &["dll"])
                    .pick_file()
                {
                    self.dll_path = path.to_string_lossy().into_owned();
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            #[cfg(windows)]
            {
                if ui.button("Register (per-user)").clicked() {
                    self.status = match regio::register_dll(&self.dll_path) {
                        Ok(s) => s,
                        Err(e) => format!("Error: {e}"),
                    };
                }
                if ui.button("Unregister").clicked() {
                    self.status = match regio::unregister_dll(&self.dll_path) {
                        Ok(s) => s,
                        Err(e) => format!("Error: {e}"),
                    };
                }
            }
            #[cfg(not(windows))]
            ui.label("(Registration is a Windows-only operation.)");
        });
    }

    fn draw_cache(&mut self, ui: &mut egui::Ui) {
        ui.heading("Thumbnail cache");
        ui.label(
            "Windows aggressively caches thumbnails. When you change settings here, \
             previously-viewed .epub files keep their old thumbnails until the cache \
             is cleared. Click below to delete the per-user thumbcache_*.db files; \
             Explorer rebuilds them automatically as you browse.",
        );
        ui.add_space(8.0);
        #[cfg(windows)]
        if ui.button("Clear thumbnail cache").clicked() {
            match cache::clear_thumbnail_cache() {
                Ok(n) => self.status = format!("Removed {n} cache file(s)."),
                Err(e) => self.status = format!("Error clearing cache: {e}"),
            }
        }
        #[cfg(not(windows))]
        ui.label("(Available on Windows only.)");
    }

    fn draw_diagnostics(&mut self, ui: &mut egui::Ui) {
        ui.heading("Recent thumbnail attempts");
        if !self.settings.logging_enabled {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Logging is disabled — enable it on the Settings tab to populate this view.",
            );
        }
        ui.label(
            "Tip: when logging is enabled, every line is also written to \
             OutputDebugString and is visible live in Sysinternals \
             DebugView (filter on \"[ThumbsUp]\"). Useful for \
             diagnosing thumbnail issues without restarting Explorer.",
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_diagnostics();
            }
            if ui.button("Open log folder").clicked() {
                #[cfg(windows)]
                if let Some(p) =
                    diag::default_log_path().and_then(|p| p.parent().map(|p| p.to_path_buf()))
                {
                    let _ = std::process::Command::new("explorer").arg(p).spawn();
                }
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("diag-grid").striped(true).show(ui, |ui| {
                ui.strong("Time");
                ui.strong("File");
                ui.strong("Outcome");
                ui.strong("Detail");
                ui.end_row();
                for entry in &self.diagnostics {
                    ui.label(&entry.timestamp);
                    ui.label(&entry.file);
                    let color = if entry.outcome == "ok" {
                        egui::Color32::LIGHT_GREEN
                    } else {
                        egui::Color32::LIGHT_RED
                    };
                    ui.colored_label(color, &entry.outcome);
                    ui.label(&entry.detail);
                    ui.end_row();
                }
            });
        });
    }

    fn refresh_diagnostics(&mut self) {
        let path = if self.settings.log_path.is_empty() {
            diag::default_log_path()
        } else {
            Some(std::path::PathBuf::from(&self.settings.log_path))
        };
        match path {
            Some(p) => match diag::read_recent(&p, 200) {
                Ok(entries) => {
                    let n = entries.len();
                    self.diagnostics = entries;
                    self.status = format!("Loaded {n} entries from {}.", p.display());
                }
                Err(e) => self.status = format!("Could not read log: {e}"),
            },
            None => self.status = "No log path resolved.".into(),
        }
    }

    fn draw_about(&mut self, ui: &mut egui::Ui) {
        ui.heading("About");
        ui.label(format!("ThumbsUp v{}", env!("CARGO_PKG_VERSION")));
        ui.label("Windows 11 File Explorer thumbnail handler for EPUB files.");
        ui.add_space(8.0);
        ui.label(
            "Reads the cover image declared in the OPF package document \
             (EPUB 2 <meta name=\"cover\"> or EPUB 3 properties=\"cover-image\") \
             and renders it as the file's thumbnail.",
        );
    }
}

fn settings_eq(a: &Settings, b: &Settings) -> bool {
    a.enabled == b.enabled
        && a.max_file_mb == b.max_file_mb
        && a.max_thumbnail_ms == b.max_thumbnail_ms
        && a.fallback_policy == b.fallback_policy
        && a.logging_enabled == b.logging_enabled
        && a.log_path == b.log_path
}
