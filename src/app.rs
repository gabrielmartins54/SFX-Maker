//! egui application: panels, input handling, canvas display.
//! Heavy work lives in `geometry` / `render`; this file only wires UI to them.

mod library_ui;
mod tools;
mod transform;
mod widgets;

use std::collections::BTreeSet;
use std::path::Path;

use egui::{Color32, Sense, Vec2};
use kurbo::{Point, Rect};

use crate::export;
use crate::fonts::FontEntry;
use crate::geometry::build::build_text;
use crate::geometry::text_to_paths::FontData;
use crate::history::History;
use crate::library::{self, Folder, Library};
use crate::settings::Settings;
use crate::interact::hit_letter;
use crate::model::params::{ranges, Params};
use crate::model::scene::{LayoutDirection, Scene};
use crate::render::gpu::{tessellate_scene, GpuScene};

const BUNDLED_FONT: &[u8] = include_bytes!("../assets/fonts/Bangers-Regular.ttf");
const CANVAS_BG: Color32 = Color32::WHITE;
const MIN_ZOOM: f64 = 0.02;
const MAX_ZOOM: f64 = 64.0;
/// Pointer distance (screen points) within which a handle is grabbed.
const HANDLE_HIT: f32 = 9.0;
const ACCENT: Color32 = Color32::from_rgb(0, 140, 255);

#[derive(Clone, Copy)]
enum ViewMode {
    Fit,
    Center,
    Keep,
}

pub struct SfxApp {
    scene: Scene,
    font: FontData,

    fonts: Vec<FontEntry>,
    settings: Settings,
    library: Library,
    folders: Vec<Folder>,

    status: Option<String>,

    /// Scene -> canvas-point transform (pan + zoom).
    pan: Vec2,
    zoom: f64,
    fit_pending: bool,

    warp_rect: Option<Rect>,
    gpu: GpuScene,
    gpu_key: Option<(u64, i32)>,
    center_pending: bool,
    last_canvas: Option<Vec2>,
    library_open: bool,
    scene_version: u64,

    selection: BTreeSet<usize>,
    drag: Option<transform::DragOp>,
    history: History<Scene>,

    notice: Option<String>,
    export_size: u32,
    export_transparent: bool,
    /// Scene snapshot taken at the first change of a slider drag; committed to history on release.
    param_snapshot: Option<Scene>,
    tools: tools::ToolState,
}

impl SfxApp {
    pub fn new() -> Self {
        // The bundled font is a compile-time constant; failure here is a build bug,
        // but we still avoid panicking: fall back to an empty scene + status message.
        let (font, status) = match FontData::new(library_ui::BUNDLED_NAME, BUNDLED_FONT.to_vec(), 0) {
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
            fonts: Vec::new(),
            settings: Settings::load(),
            library: Library::new(library::default_root()),
            folders: Vec::new(),
            status,
            pan: Vec2::ZERO,
            zoom: 1.0,
            fit_pending: true,
            warp_rect: None,
            gpu: GpuScene::default(),
            gpu_key: None,
            center_pending: false,
            last_canvas: None,
            library_open: true,
            scene_version: 0,
            selection: BTreeSet::new(),
            drag: None,
            history: History::default(),
            notice: None,
            export_size: 2048,
            export_transparent: true,
            param_snapshot: None,
            tools: tools::ToolState::new(),
        };
        app.rescan_fonts();
        app.refresh_library();
        app.relayout();
        app
    }

    fn set_error(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
        self.notice = None;
    }

    /// Snapshot the current scene as an undo step. Call BEFORE mutating the scene.
    fn record(&mut self) {
        self.history.record(self.scene.clone());
    }

    fn restore(&mut self, scene: Scene) {
        self.scene = scene;
        let n = self.scene.letters.len();
        self.selection.retain(|&i| i < n);
        self.drag = None;
        self.scene_version += 1;
    }

    fn undo(&mut self) {
        if let Some(s) = self.history.undo(self.scene.clone()) {
            self.restore(s);
        }
    }

    fn redo(&mut self) {
        if let Some(s) = self.history.redo(self.scene.clone()) {
            self.restore(s);
        }
    }

    /// Rebuilds letters from text + font (resets manual transforms and selection).
    /// Errors are shown, never panic.
    /// Brush strokes (the tail of `letters`) survive and keep their transforms.
    /// Re-fits the view (text / direction / font changes).
    fn relayout(&mut self) {
        self.relayout_impl(ViewMode::Fit);
    }

    /// Re-lays out and re-centres the content, keeping zoom when it still fits
    /// (slider / preset changes).
    fn relayout_center(&mut self) {
        self.relayout_impl(ViewMode::Center);
    }

    /// Re-lays out without touching the view (guide point dragging: the point under the
    /// cursor must not slide away).
    fn relayout_static(&mut self) {
        self.relayout_impl(ViewMode::Keep);
    }

    fn relayout_impl(&mut self, view: ViewMode) {
        // Only a text / direction / font change throws manual edits away; anything else
        // (sliders, presets, guide / warp editing) keeps moved, scaled and rotated letters.
        self.relayout_with(view, !matches!(view, ViewMode::Fit));
    }

    fn relayout_with(&mut self, view: ViewMode, keep_manual: bool) {
        let keep = self.scene.text_letter_count.min(self.scene.letters.len());
        let brush = self.scene.letters.split_off(keep);
        let old_text = std::mem::take(&mut self.scene.letters);
        let manual: Vec<_> = if keep_manual {
            old_text.iter().map(crate::model::letter::Letter::manual_delta).collect()
        } else {
            Vec::new()
        };
        match build_text(&self.font, &self.scene) {
            Ok(built) => {
                let mut letters = built.letters;
                if manual.len() == letters.len() {
                    for (l, m) in letters.iter_mut().zip(manual) {
                        if let Some(m) = m {
                            l.apply_manual(m);
                        }
                    }
                } else {
                    // Letter count changed (text edit): selection indices no longer mean anything.
                    self.selection.clear();
                }
                self.warp_rect = built.warp_rect;
                self.scene.text_letter_count = letters.len();
                self.scene.letters = letters;
                self.status = None;
            }
            Err(e) => {
                self.warp_rect = None;
                self.selection.clear();
                self.scene.text_letter_count = 0;
                self.scene.letters.clear();
                self.set_error(e.to_string());
            }
        }
        if !keep_manual {
            self.selection.clear();
        }
        if !keep_manual {
            self.drag = None;
        }
        self.scene.letters.extend(brush);
        self.scene_version += 1;
        match view {
            ViewMode::Fit => self.fit_pending = true,
            ViewMode::Center => self.center_pending = true,
            ViewMode::Keep => {}
        }
    }

    /// Drops all manual moves / scales / rotations / skews of the text letters.
    fn reset_manual_edits(&mut self) {
        self.record();
        self.relayout_with(ViewMode::Center, false);
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
                self.record();
                self.font = f;
                self.relayout();
            }
            Err(e) => self.set_error(format!("{}: {e}", path.display())),
        }
    }

    /// Switches font as one undo step and re-lays out the text.
    fn set_font(&mut self, f: FontData) {
        self.record();
        self.font = f;
        self.relayout();
    }

    fn screen_to_scene(&self, rect: egui::Rect, pos: egui::Pos2) -> Point {
        let local = pos - rect.min - self.pan;
        Point::new(f64::from(local.x) / self.zoom, f64::from(local.y) / self.zoom)
    }

    fn scene_to_screen(&self, rect: egui::Rect, p: Point) -> egui::Pos2 {
        rect.min
            + self.pan
            + Vec2::new((p.x * self.zoom) as f32, (p.y * self.zoom) as f32)
    }

    /// Centres the content in the canvas, keeping zoom unless it no longer fits.
    fn center_on_scene(&mut self, canvas: egui::Rect) {
        let Some(b) = self.scene.bounds() else {
            return self.fit_to(canvas);
        };
        let fits = b.width() * self.zoom <= f64::from(canvas.width()) * 0.95
            && b.height() * self.zoom <= f64::from(canvas.height()) * 0.95;
        if fits {
            let c = b.center();
            self.pan = Vec2::new(
                (f64::from(canvas.width()) / 2.0 - c.x * self.zoom) as f32,
                (f64::from(canvas.height()) / 2.0 - c.y * self.zoom) as f32,
            );
        } else {
            self.fit_to(canvas);
        }
    }

    fn fit_to(&mut self, canvas: egui::Rect) {
        if let Some(b) = self.scene.bounds() {
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
        if ui
            .add_sized([ui.available_width(), 32.0], egui::Button::new("📋 Copy PNG (Ctrl+C)"))
            .on_hover_text("Copies the result to the clipboard, ready to paste into Photoshop")
            .clicked()
        {
            self.copy_png();
        }
        ui.separator();

        ui.label("Text");
        let old_text = self.scene.text.clone();
        if ui
            .add(egui::TextEdit::multiline(&mut self.scene.text).desired_rows(3).desired_width(f32::INFINITY))
            .changed()
        {
            // The scene still holds the old layout here, so this is a valid "before" state.
            let mut before = self.scene.clone();
            before.text = old_text;
            self.history.record(before);
            self.relayout();
        }

        ui.horizontal(|ui| {
            if ui
                .button("✨ Auto Pick")
                .on_hover_text("Phonetic analysis: pick sound style & tune intensity macros")
                .clicked()
            {
                self.record();
                let pick = crate::model::dictionary::auto_pick_sfx(&self.scene.text);
                self.apply_auto_pick(&pick);
                self.relayout_center();
            }

            let mut dir = self.scene.direction;
            ui.radio_value(&mut dir, LayoutDirection::Horizontal, "Horizontal");
            ui.radio_value(&mut dir, LayoutDirection::Vertical, "Vertical");
            if dir != self.scene.direction {
                self.record();
                self.scene.direction = dir;
                self.relayout();
            }
        });

        self.dictionary_ui(ui);
        self.font_ui(ui);
        self.params_ui(ui);
        self.guide_ui(ui);
        self.transform_ui(ui);
        self.presets_ui(ui);
        self.brush_ui(ui);

        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(self.history.can_undo(), egui::Button::new("↶ Undo")).clicked() {
                self.undo();
            }
            if ui.add_enabled(self.history.can_redo(), egui::Button::new("↷ Redo")).clicked() {
                self.redo();
            }
        });
        if ui
            .button("Reset moved letters")
            .on_hover_text("Undo manual move / scale / rotate / skew of the text letters")
            .clicked()
        {
            self.reset_manual_edits();
        }
        if ui.button("Fit view").clicked() {
            self.fit_pending = true;
        }
        ui.weak("Click letter: select (Shift: add) · drag: move");
        ui.weak("Transform: Trapezoid perspective & 4x4 Grid preset");
        ui.weak("Empty space drag: pan · Wheel: zoom");
        ui.weak("Ctrl+A select all · Esc deselect · Ctrl+Z / Ctrl+Y");
        ui.label(format!(
            "{} letters ({} selected) · zoom {:.0}%",
            self.scene.letters.len(),
            self.selection.len(),
            self.zoom * 100.0
        ));

        ui.separator();
        ui.label("Export");
        widgets::slider_u32(ui, &mut self.export_size, 256..=8192, "longest side px");
        ui.checkbox(&mut self.export_transparent, "Transparent background");
        ui.horizontal(|ui| {
            if ui.button("Export PNG…").clicked() {
                self.export_png();
            }
            if ui.button("Export SVG…").clicked() {
                self.export_svg();
            }
        });
        self.file_ui(ui);
    }

    /// Layout + effect sliders. Any change re-lays out the scene (manual transforms reset);
    /// a whole drag collapses into one undo step.
    fn params_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let mut p = self.scene.params;
        let mut seed = self.scene.seed;
        let mut changed = false;

        ui.strong("Intensity");
        ui.weak("Each moves several sliders below at once; fine-tune them afterwards.");
        let mut nv = p.power;
        if widgets::slider(ui, &mut nv, ranges::MACRO, "Power (weight + size progression)") {
            p.set_power(nv);
            changed = true;
        }
        let mut nv = p.roughness;
        if widgets::slider(ui, &mut nv, ranges::MACRO, "Roughness (variation + wobble)") {
            p.set_roughness(nv);
            changed = true;
        }
        let mut nv = p.momentum;
        if widgets::slider(ui, &mut nv, ranges::MACRO, "Momentum (lean + slant + rise)") {
            p.set_momentum(nv);
            changed = true;
        }

        ui.separator();
        ui.strong("Layout");
        let slider = |ui: &mut egui::Ui, v: &mut f64, range: std::ops::RangeInclusive<f64>, label: &str| {
            widgets::slider(ui, v, range, label)
        };
        changed |= slider(ui, &mut p.start_size, ranges::SIZE, "Start size");
        changed |= slider(ui, &mut p.end_size, ranges::SIZE, "End size");
        changed |= slider(ui, &mut p.tracking, ranges::TRACKING, "Letter spacing (em)");
        changed |= slider(ui, &mut p.line_spacing, ranges::LINE_SPACING, "Line spacing");
        changed |= slider(ui, &mut p.lean, ranges::LEAN, "Lean (0 upright · 1 follows path)");
        changed |= slider(ui, &mut p.slant, ranges::SLANT, "Slant °");
        changed |= slider(ui, &mut p.rise, ranges::RISE, "Rise ° (horizontal strokes up)");
        changed |= slider(ui, &mut p.rotation_jitter, ranges::ROTATION_JITTER, "Angle variation °");
        changed |= slider(ui, &mut p.scale_jitter, ranges::SCALE_JITTER, "Size variation");
        changed |= slider(ui, &mut p.baseline_wobble, ranges::BASELINE_WOBBLE, "Up/down variation (em)");
        changed |= slider(ui, &mut p.offset_jitter, ranges::OFFSET_JITTER, "Offset jitter (em)");
        changed |= slider(ui, &mut p.spacing_by_size, ranges::SPACING_BY_SIZE, "Spacing by size");

        ui.separator();
        ui.strong("Weight: Simple (TransType / Fontographer algorithm)");
        changed |= slider(ui, &mut p.weight, ranges::WEIGHT, "Weight change (em)");
        changed |= slider(ui, &mut p.weight_stem_ratio, ranges::STEM_RATIO, "Stem contrast ratio (H/V)");
        changed |= ui
            .checkbox(&mut p.weight_keep_v, "Don't change vertical size")
            .on_hover_text("Fontographer constraint: keeps original height and baseline")
            .changed();
        changed |= ui
            .checkbox(&mut p.weight_keep_h, "Don't change horizontal size")
            .on_hover_text("Fontographer constraint: keeps original character width")
            .changed();
        changed |= ui.checkbox(&mut p.weight_round, "Round corners when changing weight").changed();
        changed |= slider(ui, &mut p.weight_start, ranges::WEIGHT_RAMP, "Weight ramp at start");
        changed |= slider(ui, &mut p.weight_end, ranges::WEIGHT_RAMP, "Weight ramp at end");
        changed |= ui
            .checkbox(&mut p.weight_per_letter, "Weight ramp per letter")
            .on_hover_text("Off: start to end of the whole text. On: start to end of each letter.")
            .changed();

        ui.separator();
        ui.strong("Procedural guide & curves");
        changed |= slider(ui, &mut p.bend, ranges::BEND, "Bend (arc)");
        changed |= slider(ui, &mut p.wave, ranges::WAVE, "Wave (em)");
        changed |= slider(ui, &mut p.wave_cycles, ranges::WAVE_CYCLES, "Wave cycles");
        changed |= ui.checkbox(&mut p.kerning, "Kerning (horizontal)").changed();
        changed |= ui
            .checkbox(&mut p.no_overlap, "Keep letters from overlapping")
            .on_hover_text("Jitter / size changes push neighbours apart instead of overlapping. Deliberate tight spacing is kept.")
            .changed();
        changed |= ui
            .checkbox(&mut p.bend_glyphs, "Bend letter shapes along the path")
            .on_hover_text("Off: letters keep their shape and are only moved / rotated along the path.")
            .changed();
        ui.horizontal(|ui| {
            ui.label("Seed");
            changed |= ui.add(egui::DragValue::new(&mut seed)).changed();
            if ui.button("??").on_hover_text("New random seed").clicked() {
                seed = rand::random::<u32>().into();
                changed = true;
            }
            if ui.button("Reset").clicked() {
                p = Params::default();
                changed = true;
            }
        });
        if changed {
            if self.param_snapshot.is_none() {
                self.param_snapshot = Some(self.scene.clone());
            }
            self.scene.params = p;
            self.scene.seed = seed;
            self.relayout_center();
        }
        if self.param_snapshot.is_some()
            && !ui.ctx().input(|i| i.pointer.any_down())
            && let Some(before) = self.param_snapshot.take()
        {
            self.history.record(before);
        }
    }

    fn export_png(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name("sfx.png")
            .save_file()
        else {
            return;
        };
        match export::png::export_png(&self.scene, &path, self.export_size, self.export_transparent) {
            Ok(()) => {
                self.status = None;
                self.notice = Some(format!("Saved {}", path.display()));
            }
            Err(e) => self.set_error(format!("PNG export failed: {e:#}")),
        }
    }

    fn export_svg(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG image", &["svg"])
            .set_file_name("sfx.svg")
            .save_file()
        else {
            return;
        };
        match export::svg::export_svg(&self.scene, &path, !self.export_transparent) {
            Ok(()) => {
                self.status = None;
                self.notice = Some(format!("Saved {}", path.display()));
            }
            Err(e) => self.set_error(format!("SVG export failed: {e:#}")),
        }
    }

    fn click_select(&mut self, rect: egui::Rect, pos: egui::Pos2, shift: bool) {
        if self.hit_gizmo(rect, pos) != transform::GizmoPart::None {
            return;
        }
        let pt = self.screen_to_scene(rect, pos);
        match hit_letter(&self.scene, pt) {
            Some(i) => {
                if shift {
                    if !self.selection.remove(&i) {
                        self.selection.insert(i);
                    }
                } else {
                    self.selection.clear();
                    self.selection.insert(i);
                }
            }
            None if !shift => self.selection.clear(),
            None => {}
        }
    }

    fn double_click_reset(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let part = self.hit_gizmo(rect, pos);
        let pt = self.screen_to_scene(rect, pos);
        let hit = hit_letter(&self.scene, pt);

        if part != transform::GizmoPart::None || hit.is_some() || !self.selection.is_empty() {
            self.record();
            for &i in &self.selection {
                if let Some(l) = self.scene.letters.get_mut(i) {
                    l.transform = l.base.unwrap_or(kurbo::Affine::IDENTITY);
                    l.distort = [[0.0; 2]; 4];
                }
            }
            self.scene_version += 1;
            self.notice = Some("Reset letter transform".into());
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::{Key, Modifiers};
        let (redo, undo, redo_y, all, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::Y),
                i.consume_key(Modifiers::COMMAND, Key::A),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if redo || redo_y {
            self.redo();
        } else if undo {
            self.undo();
        }
        if all {
            self.selection = (0..self.scene.letters.len()).collect();
        }
        if esc {
            self.selection.clear();
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, Color32::from_gray(40));

        // Keep the content centred when the canvas is resized (e.g. library panel toggled).
        if let Some(old) = self.last_canvas
            && old != rect.size()
            && !self.fit_pending
        {
            self.pan += (rect.size() - old) / 2.0;
        }
        self.last_canvas = Some(rect.size());

        if rect.width() > 1.0 && rect.height() > 1.0 {
            if self.fit_pending {
                self.fit_to(rect);
            } else if self.center_pending {
                self.center_on_scene(rect);
            }
            self.fit_pending = false;
            self.center_pending = false;
        }

        // Brush / guide tools take priority, then selection handles; otherwise dragging pans.
        let (shift, alt, ctrl) = ui.ctx().input(|i| (i.modifiers.shift, i.modifiers.alt, i.modifiers.command));
        let tool_input = self.canvas_tools(&response, rect);
        if !tool_input
            && self.drag.is_none()
            && let Some(hover) = response.hover_pos()
        {
            self.handle_cursor(ui.ctx(), rect, hover, ctrl);
        }
        if !tool_input && response.drag_started()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.begin_drag(rect, pos, shift, ctrl);
        }
        if !tool_input && response.dragged() {
            if self.drag.is_some() {
                if let Some(pos) = response.interact_pointer_pos() {
                    self.update_drag(rect, pos, shift, alt);
                }
            } else {
                self.pan += response.drag_delta();
            }
        }
        if response.drag_stopped() {
            self.drag = None;
        }
        if !tool_input
            && response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.double_click_reset(rect, pos);
        } else if !tool_input
            && response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.click_select(rect, pos, shift);
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

        // GPU path: re-tessellate only when the scene or the zoom bucket changes; pan/zoom are
        // uniforms. The bucket keeps curve detail at least as fine as ~0.25 px.
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let bucket = (self.zoom * ppp).max(1e-6).log2().ceil() as i32;
        let key = (self.scene_version, bucket);
        if self.gpu_key != Some(key) {
            let tol = 0.25 / 2f64.powi(bucket);
            self.gpu.set_mesh(tessellate_scene(&self.scene, tol as f32));
            self.gpu_key = Some(key);
        }
        painter.rect_filled(rect, 0.0, CANVAS_BG);
        painter.add(self.gpu.callback(rect, self.pan, self.zoom));
        if let Some(e) = self.gpu.take_error() {
            self.set_error(format!("OpenGL canvas error: {e}"));
        }        // Overlay is drawn by egui, not the rasterizer, so it never appears in exports.
        self.draw_selection(&painter, rect);
        self.draw_tool_overlay(&painter, rect);
    }
}

impl eframe::App for SfxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_dropped_files(&ctx);
        self.handle_shortcuts(&ctx);
        self.tool_shortcuts(&ctx);
        if !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::L))
        {
            self.library_open = !self.library_open;
        }

        egui::Panel::left("controls")
            .default_size(280.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.side_panel(ui));
            });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                match (&self.status, &self.notice) {
                    (Some(msg), _) => ui.colored_label(Color32::LIGHT_RED, msg),
                    (None, Some(msg)) => ui.colored_label(Color32::LIGHT_GREEN, msg),
                    (None, None) => ui.weak("Ready"),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.library_open { "📚 Library ▶" } else { "📚 Library ◀" };
                    if ui.button(label).on_hover_text("Show / hide the library (Ctrl+L)").clicked() {
                        self.library_open = !self.library_open;
                    }
                });
            });
        });

        if self.library_open {
            egui::Panel::right("library")
                .default_size(220.0)
                .show(ui, |ui| self.library_panel(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the real side panel headlessly: hover each row and scroll the wheel.
    #[test]
    fn wheel_changes_params_sliders_in_the_real_side_panel() {
        let mut app = SfxApp::new();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 3000.0));
        let mut time = 0.0;
        let mut frame = |app: &mut SfxApp, events: Vec<egui::Event>| {
            time += 0.016;
            let raw = egui::RawInput { screen_rect: Some(screen), time: Some(time), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::Panel::left("controls").default_size(280.0).show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| app.side_panel(ui));
                });
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        let mut changed_rows = 0;
        let mut y = 5.0;
        while y < 1500.0 {
            let before = (app.scene.params, app.export_size);
            frame(&mut app, vec![egui::Event::PointerMoved(egui::pos2(60.0, y))]);
            frame(
                &mut app,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, 1.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            for _ in 0..4 {
                frame(&mut app, vec![]);
            }
            if (app.scene.params, app.export_size) != before {
                changed_rows += 1;
            }
            y += 6.0;
        }
        assert!(changed_rows >= 8, "only {changed_rows} rows reacted to the wheel");
    }
}