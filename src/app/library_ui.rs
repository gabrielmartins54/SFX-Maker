//! Font picker (user-supplied folders only) and the SFX library panel.

use std::path::Path;

use super::SfxApp;
use crate::fonts::{self, FontEntry, FontSource};
use crate::geometry::text_to_paths::FontData;
use crate::library::{Library, sanitize};

pub const BUNDLED_NAME: &str = "Bangers";

impl SfxApp {
    /// Bundled default font first, then everything found in the user's font folders.
    pub(super) fn rescan_fonts(&mut self) {
        let mut list = vec![FontEntry { name: BUNDLED_NAME.to_owned(), source: FontSource::Bundled }];
        for dir in &self.settings.font_dirs {
            list.extend(fonts::scan_dir(dir));
        }
        self.fonts = list;
    }

    fn font_from_source(&self, entry: &FontEntry) -> Result<FontData, String> {
        match &entry.source {
            FontSource::Bundled => FontData::new(&entry.name, super::BUNDLED_FONT.to_vec(), 0).map_err(|e| e.to_string()),
            FontSource::File(p) => {
                let bytes = std::fs::read(p).map_err(|e| format!("Cannot read {}: {e}", p.display()))?;
                FontData::new(&entry.name, bytes, 0).map_err(|e| format!("{}: {e}", p.display()))
            }
        }
    }

    fn select_font(&mut self, idx: usize) {
        let Some(entry) = self.fonts.get(idx).cloned() else {
            return;
        };
        match self.font_from_source(&entry) {
            Ok(f) => self.set_font(f),
            Err(e) => self.set_error(e),
        }
    }

    fn persist_settings(&mut self) {
        if let Err(e) = self.settings.save() {
            self.set_error(format!("Saving settings failed: {e:#}"));
        }
    }

    pub(super) fn font_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Font");
        let mut chosen = None;
        egui::ComboBox::from_id_salt("font_combo")
            .selected_text(self.font.name.clone())
            .width(ui.available_width() - 8.0)
            .height(400.0)
            .show_ui(ui, |ui| {
                for (i, f) in self.fonts.iter().enumerate() {
                    let label = if f.source == FontSource::Bundled { format!("{} (default)", f.name) } else { f.name.clone() };
                    if ui.selectable_label(f.name == self.font.name, label).clicked() {
                        chosen = Some(i);
                    }
                }
            });
        if let Some(i) = chosen {
            self.select_font(i);
        }

        ui.horizontal(|ui| {
            if ui.button("Add font folder…").clicked()
                && let Some(dir) = rfd::FileDialog::new().set_title("Choose a folder with fonts").pick_folder()
                && self.settings.add_font_dir(dir)
            {
                self.persist_settings();
                self.rescan_fonts();
            }
            if ui.button("Open font file…").clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("Fonts", &["ttf", "otf", "ttc"]).pick_file()
            {
                self.load_font_file(&path);
            }
        });

        if self.settings.font_dirs.is_empty() {
            ui.weak("No font folders yet: add one to use your own fonts.");
        } else {
            let mut remove = None;
            egui::CollapsingHeader::new(format!("Font folders ({})", self.settings.font_dirs.len())).show(ui, |ui| {
                for (i, d) in self.settings.font_dirs.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("✕").on_hover_text("Remove folder").clicked() {
                            remove = Some(i);
                        }
                        ui.label(d.display().to_string());
                    });
                }
            });
            if let Some(i) = remove {
                self.settings.font_dirs.remove(i);
                self.persist_settings();
                self.rescan_fonts();
            }
        }
    }

    // ---------------------------------------------------------------- library

    pub(super) fn refresh_library(&mut self) {
        self.folders = self.library.scan();
    }

    fn save_to_library(&mut self) {
        match self.library.save(&self.font.name, &self.scene) {
            Ok(path) => {
                self.refresh_library();
                let name = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().replace(".sfx", ""));
                self.status = None;
                self.notice = Some(format!("Saved \"{name}\" to library / {}", self.font.name));
            }
            Err(e) => self.set_error(format!("Library save failed: {e:#}")),
        }
    }

    fn load_from_library(&mut self, path: &Path) {
        match Library::load(path) {
            Ok((font_name, scene)) => {
                self.record();
                let have_font = self.fonts.iter().position(|f| f.name == font_name);
                self.scene = scene;
                self.selection.clear();
                self.drag = None;
                self.tools_reset();
                self.scene_version += 1;
                self.fit_pending = true;
                let mut msg = format!("Loaded \"{}\"", self.scene.text.replace('\n', " "));
                match have_font {
                    Some(i) => {
                        if let Some(entry) = self.fonts.get(i).cloned()
                            && let Ok(f) = self.font_from_source(&entry)
                        {
                            self.font = f;
                        }
                    }
                    None => msg.push_str(&format!(" (font \"{font_name}\" is not in your fonts: editing text uses {})", self.font.name)),
                }
                self.status = None;
                self.notice = Some(msg);
            }
            Err(e) => self.set_error(format!("Library load failed: {e:#}")),
        }
    }

    pub(super) fn library_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Library");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").on_hover_text("Hide library (Ctrl+L)").clicked() {
                    self.library_open = false;
                }
            });
        });
        let preview = sanitize(&self.scene.text, "SFX");
        if ui
            .add_sized([ui.available_width(), 28.0], egui::Button::new("💾 Save to library"))
            .on_hover_text(format!("Saves as \"{preview}\" in the \"{}\" folder", self.font.name))
            .clicked()
        {
            self.save_to_library();
        }
        ui.separator();

        if self.folders.is_empty() {
            ui.weak("Empty. Saved SFX appear here, grouped by font.");
            return;
        }
        let mut load = None;
        let mut delete = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for folder in &self.folders {
                egui::CollapsingHeader::new(format!("📁 {}", folder.name))
                    .default_open(folder.name == sanitize(&self.font.name, ""))
                    .show(ui, |ui| {
                        for e in &folder.entries {
                            ui.horizontal(|ui| {
                                if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                    delete = Some(e.path.clone());
                                }
                                if ui.selectable_label(false, &e.name).on_hover_text("Click to open").clicked() {
                                    load = Some(e.path.clone());
                                }
                            });
                        }
                    });
            }
        });
        if let Some(p) = load {
            self.load_from_library(&p);
        }
        if let Some(p) = delete {
            match Library::delete(&p) {
                Ok(()) => self.refresh_library(),
                Err(e) => self.set_error(format!("{e:#}")),
            }
        }
    }
}

