//! egui application: panels, input handling, canvas display.
//! Heavy work lives in `geometry` / `render`; this file only wires UI to them.

use std::path::Path;

use egui::{Color32, Sense, TextureHandle, TextureOptions, Vec2};
use kurbo::{Affine, Rect, Shape};

use crate::geometry::text_to_paths::{text_to_paths, FontData};
use crate::model::scene::{LayoutDirection, Scene};
use crate::render::raster::{render_scene, Background};

const BUNDLED_FONT: &[u8] = include_bytes!("../assets/fonts/Bangers-Regular.ttf");
const CANVAS_BG: [u8; 4] = [255, 255, 255, 255];
const MIN_ZOOM: f64 = 0.02;
const MAX_ZOOM: f64 = 64.0;

/// Everything that affects the rendered canvas image; re-render only when it changes.
#[derive(PartialEq, Clone, Copy)]
struct RenderKey {
    size_px: (u32, u32),
    pan: (u32, u32),
    zoom: u64,
    scene_version: u64,
}

pub struct SfxApp {
    scene: Scene,
    font: FontData,

    system_faces: Vec<(String, fontdb::ID)>,
    fontdb: fontdb::Database,

    status: Option<String>,

    /// Scene -> canvas-point transform (pan + zoom).
    pan: Vec2,
    zoom: f64,
    fit_pending: bool,

    texture: Option<TextureHandle>,
    last_key: Option<RenderKey>,
    scene_version: u64,
}

impl SfxApp {
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut system_faces: Vec<(String, fontdb::ID)> = db
            .faces()
            .filter_map(|f| f.families.first().map(|(n, _)| (n.clone(), f.id)))
            .collect();
        system_faces.sort_by_key(|a| a.0.to_lowercase());
        system_faces.dedup_by(|a, b| a.0 == b.0);

        // The bundled font is a compile-time constant; failure here is a build bug,
        // but we still avoid panicking: fall back to an empty scene + status message.
        let (font, status) = match FontData::new("Bangers (bundled)", BUNDLED_FONT.to_vec(), 0) {
            Ok(f) => (Some(f), None),
            Err(e) => (None, Some(e.to_string())),
        };

        let mut app = Self {
            scene: Scene::default(),
            font: font.unwrap_or_else(|| FontData {
                name: "(none)".into(),
                bytes: std::sync::Arc::new(Vec::new()),
                index: 0,
            }),
            system_faces,
            fontdb: db,
            status,
            pan: Vec2::ZERO,
            zoom: 1.0,
            fit_pending: true,
            texture: None,
            last_key: None,
            scene_version: 0,
        };
        app.relayout();
        app
    }

    fn set_error(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
    }

    /// Rebuilds letters from text + font. Errors are shown, never panic.
    fn relayout(&mut self) {
        match text_to_paths(&self.font, &self.scene.text, self.scene.direction) {
            Ok(letters) => {
                self.scene.letters = letters;
                self.status = None;
            }
            Err(e) => {
                self.scene.letters.clear();
                self.set_error(e.to_string());
            }
        }
        self.scene_version += 1;
        self.fit_pending = true;
    }

    fn load_font_file(&mut self, path: &Path) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return self.set_error(format!("Cannot read {}: {e}", path.display())),
        };
        let name = path
            .file_stem()
            .map_or_else(|| "font".to_owned(), |s| s.to_string_lossy().into_owned());
        match FontData::new(name, bytes, 0) {
            Ok(f) => {
                self.font = f;
                self.relayout();
            }
            Err(e) => self.set_error(format!("{}: {e}", path.display())),
        }
    }

    fn load_system_face(&mut self, id: fontdb::ID, name: &str) {
        let loaded = self
            .fontdb
            .with_face_data(id, |data, index| (data.to_vec(), index));
        match loaded {
            Some((bytes, index)) => match FontData::new(name, bytes, index) {
                Ok(f) => {
                    self.font = f;
                    self.relayout();
                }
                Err(e) => self.set_error(format!("{name}: {e}")),
            },
            None => self.set_error(format!("Could not load system font {name}")),
        }
    }

    fn scene_bounds(&self) -> Option<Rect> {
        self.scene
            .letters
            .iter()
            .map(|l| l.placed_path().bounding_box())
            .reduce(|a, b| a.union(b))
            .filter(|r| r.width() > 0.0 && r.height() > 0.0)
    }

    fn fit_to(&mut self, canvas: egui::Rect) {
        if let Some(b) = self.scene_bounds() {
            let margin = 0.85;
            let zx = f64::from(canvas.width()) * margin / b.width();
            let zy = f64::from(canvas.height()) * margin / b.height();
            self.zoom = zx.min(zy).clamp(MIN_ZOOM, MAX_ZOOM);
            let c = b.center();
            self.pan = Vec2::new(
                (f64::from(canvas.width()) / 2.0 - c.x * self.zoom) as f32,
                (f64::from(canvas.height()) / 2.0 - c.y * self.zoom) as f32,
            );
        } else {
            self.zoom = 1.0;
            self.pan = canvas.size() / 2.0;
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            self.load_font_file(f.path());
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Manga SFX Maker");
        ui.separator();

        ui.label("Text");
        if ui
            .add(egui::TextEdit::multiline(&mut self.scene.text).desired_rows(3).desired_width(f32::INFINITY))
            .changed()
        {
            self.relayout();
        }

        ui.horizontal(|ui| {
            let mut dir = self.scene.direction;
            ui.radio_value(&mut dir, LayoutDirection::Horizontal, "Horizontal");
            ui.radio_value(&mut dir, LayoutDirection::Vertical, "Vertical");
            if dir != self.scene.direction {
                self.scene.direction = dir;
                self.relayout();
            }
        });

        ui.separator();
        ui.label(format!("Font: {}", self.font.name));
        if ui.button("Open font file…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Fonts", &["ttf", "otf", "ttc"])
                .pick_file()
        {
            self.load_font_file(&path);
        }
        ui.weak("…or drop a .ttf / .otf onto the window.");

        let mut chosen: Option<(fontdb::ID, String)> = None;
        egui::ComboBox::from_label("System font")
            .selected_text("Choose…")
            .height(400.0)
            .show_ui(ui, |ui| {
                for (name, id) in &self.system_faces {
                    if ui.selectable_label(false, name).clicked() {
                        chosen = Some((*id, name.clone()));
                    }
                }
            });
        if let Some((id, name)) = chosen {
            self.load_system_face(id, &name);
        }

        ui.separator();
        if ui.button("Fit view").clicked() {
            self.fit_pending = true;
        }
        ui.weak("Drag: pan · Wheel: zoom");
        ui.label(format!("{} letters · zoom {:.0}%", self.scene.letters.len(), self.zoom * 100.0));
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, Color32::from_gray(40));

        if self.fit_pending && rect.width() > 1.0 && rect.height() > 1.0 {
            self.fit_to(rect);
            self.fit_pending = false;
        }

        // Pan
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        // Zoom around the cursor.
        if response.hovered() {
            let (scroll, pinch, hover) = ui.ctx().input(|i| {
                (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos())
            });
            let factor = f64::from(pinch) * (f64::from(scroll) * 0.0015).exp();
            if (factor - 1.0).abs() > 1e-6 {
                let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
                let k = new_zoom / self.zoom;
                if let Some(h) = hover {
                    let c = h - rect.min; // cursor in canvas-local points
                    self.pan = c - (c - self.pan) * k as f32;
                }
                self.zoom = new_zoom;
            }
        }

        // Render at physical pixel resolution.
        let ppp = ui.ctx().pixels_per_point();
        let w = (rect.width() * ppp).round() as u32;
        let h = (rect.height() * ppp).round() as u32;
        let key = RenderKey {
            size_px: (w, h),
            pan: (self.pan.x.to_bits(), self.pan.y.to_bits()),
            zoom: self.zoom.to_bits(),
            scene_version: self.scene_version,
        };
        if self.last_key != Some(key) || self.texture.is_none() {
            let view = Affine::scale(f64::from(ppp))
                * Affine::translate((f64::from(self.pan.x), f64::from(self.pan.y)))
                * Affine::scale(self.zoom);
            if let Some(pm) = render_scene(&self.scene, view, w, h, Background::Solid(CANVAS_BG)) {
                let img = egui::ColorImage::from_rgba_premultiplied(
                    [w as usize, h as usize],
                    pm.data(),
                );
                match &mut self.texture {
                    Some(t) => t.set(img, TextureOptions::LINEAR),
                    None => {
                        self.texture =
                            Some(ui.ctx().load_texture("canvas", img, TextureOptions::LINEAR));
                    }
                }
            }
            self.last_key = Some(key);
        }
        if let Some(tex) = &self.texture {
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

impl eframe::App for SfxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_dropped_files(&ctx);

        egui::Panel::left("controls")
            .default_size(280.0)
            .show(ui, |ui| self.side_panel(ui));

        egui::Panel::bottom("status").show(ui, |ui| match &self.status {
            Some(msg) => {
                ui.colored_label(Color32::LIGHT_RED, msg);
            }
            None => {
                ui.weak("Ready");
            }
        });

        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
    }
}
