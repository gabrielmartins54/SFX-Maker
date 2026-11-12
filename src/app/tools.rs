//! Tool state and UI for presets / Roll / Mutate, guide editing, brush lettering,
//! project files, ZIP export and keyboard nudging. Lives under `app` so it can use
//! `SfxApp`'s private fields; geometry itself stays in `geometry/`.

use egui::{Color32, PointerButton, Pos2, Vec2};
use kurbo::{Affine, Point};

use super::{widgets, SfxApp, ACCENT, HANDLE_HIT};
use crate::export;
use crate::geometry::brush::stroke_to_path;
use crate::geometry::build::build_text_letters;
use crate::geometry::free_warp::{self, FreeWarp, WarpStyle};
use crate::geometry::warp::catmull_rom;
use crate::interact::translate;
use crate::model::letter::Letter;
use crate::model::params::Params;
use crate::model::preset::{self, Preset};
use crate::random::gacha;

const GUIDE_COLOR: Color32 = Color32::from_rgb(255, 150, 0);

pub struct ToolState {
    presets: Vec<Preset>,
    user_presets: Vec<Preset>,
    preset_name: String,
    mutate_amount: f64,
    brush_mode: bool,
    brush_width: f64,
    brush_taper: f64,
    stroke: Vec<Point>,
    guide_edit: bool,
    guide_drag: Option<usize>,
    warp_edit: bool,
    warp_drag: Option<usize>,
    warp_style: usize,
    warp_bend: f64,
    dict_category: usize,
    /// Scene before the first change of a warp-slider drag; committed to history on release.
    warp_snapshot: Option<crate::model::scene::Scene>,
}

impl ToolState {
    pub fn new() -> Self {
        Self {
            presets: preset::builtin(),
            user_presets: preset::load_user_presets(),
            preset_name: String::new(),
            mutate_amount: 0.15,
            brush_mode: false,
            brush_width: 16.0,
            brush_taper: 0.3,
            stroke: Vec::new(),
            guide_edit: false,
            guide_drag: None,
            warp_edit: false,
            warp_drag: None,
            warp_style: 0,
            warp_bend: 0.5,
            dict_category: 0,
            warp_snapshot: None,
        }
    }
}

impl SfxApp {
    // ---------------------------------------------------------------- presets / roll / mutate

    /// Replaces the params (and optionally the seed) as one undo step.
    fn apply_params(&mut self, params: Params, seed: Option<u64>) {
        self.record();
        self.scene.params = params.clamped();
        if let Some(s) = seed {
            self.scene.seed = s;
        }
        self.relayout_center();
    }

    pub(super) fn presets_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Presets");
        let mut chosen: Option<Params> = None;
        let mut delete: Option<usize> = None;
        egui::ComboBox::from_label("Preset")
            .selected_text("Choose…")
            .height(300.0)
            .show_ui(ui, |ui| {
                for p in &self.tools.presets {
                    if ui.selectable_label(false, &p.name).clicked() {
                        chosen = Some(p.params);
                    }
                }
                if !self.tools.user_presets.is_empty() {
                    ui.separator();
                }
                for (i, p) in self.tools.user_presets.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(false, format!("★ {}", p.name)).clicked() {
                            chosen = Some(p.params);
                        }
                        if ui.small_button("🗑").on_hover_text("Delete preset").clicked() {
                            delete = Some(i);
                        }
                    });
                }
            });
        if let Some(p) = chosen {
            self.apply_params(p, None);
        }
        if let Some(i) = delete {
            self.tools.user_presets.remove(i);
            self.persist_presets();
        }

        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.tools.preset_name).hint_text("preset name").desired_width(120.0));
            let name = self.tools.preset_name.trim().to_owned();
            if ui.add_enabled(!name.is_empty(), egui::Button::new("Save preset")).clicked() {
                let p = Preset { name: name.clone(), params: self.scene.params };
                match self.tools.user_presets.iter_mut().find(|u| u.name == name) {
                    Some(existing) => *existing = p,
                    None => self.tools.user_presets.push(p),
                }
                self.persist_presets();
            }
        });

        ui.horizontal(|ui| {
            if ui.button("🎲 Roll").on_hover_text("Random look + new seed").clicked() {
                let seed = u64::from(rand::random::<u32>());
                let p = gacha::roll(&mut gacha::rng_from_seed(seed), &self.scene.params);
                self.apply_params(p, Some(seed));
            }
            if ui.button("Mutate").on_hover_text("Small random tweak of the current look").clicked() {
                let seed = u64::from(rand::random::<u32>());
                let p = gacha::mutate(&self.scene.params, &mut gacha::rng_from_seed(seed), self.tools.mutate_amount);
                self.apply_params(p, None);
            }
            widgets::slider(ui, &mut self.tools.mutate_amount, 0.02..=0.6, "amount");
        });
    }

    fn persist_presets(&mut self) {
        match preset::save_user_presets(&self.tools.user_presets) {
            Ok(()) => self.status = None,
            Err(e) => self.set_error(format!("Saving presets failed: {e:#}")),
        }
    }

    /// Called after the scene is replaced wholesale (library / project open).
    pub(super) fn tools_reset(&mut self) {
        self.tools.guide_edit = false;
        self.tools.warp_edit = false;
        self.tools.warp_drag = None;
        self.tools.guide_drag = None;
        self.tools.stroke.clear();
    }

    // ---------------------------------------------------------------- guide

    pub(super) fn guide_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let has = self.scene.guide.is_some();
            if !has && ui.button("Draw custom guide").clicked() {
                self.create_guide();
            }
            if has {
                ui.checkbox(&mut self.tools.guide_edit, "Edit guide points");
                if ui.button("Clear guide").clicked() {
                    self.record();
                    self.scene.guide = None;
                    self.tools.guide_edit = false;
                    self.relayout_center();
                }
            }
        });
        if self.scene.guide.is_some() && self.tools.guide_edit {
            ui.weak("Click: add point · drag point: move · right-click point: remove");
        }
    }

    /// Straight guide along the unwarped baseline; dragging its points bends the text.
    fn create_guide(&mut self) {
        let mut probe = self.scene.clone();
        probe.guide = None;
        probe.params.bend = 0.0;
        probe.params.wave = 0.0;
        let Ok(letters) = build_text_letters(&self.font, &probe) else {
            return self.set_error("Cannot create a guide: text layout failed");
        };
        probe.letters = letters;
        let Some(b) = probe.bounds() else {
            return self.set_error("Type some text first");
        };
        self.record();
        let pts = (0..4)
            .map(|i| {
                let t = f64::from(i) / 3.0;
                match self.scene.direction {
                    crate::model::scene::LayoutDirection::Horizontal => Point::new(b.x0 + b.width() * t, 0.0),
                    crate::model::scene::LayoutDirection::Vertical => Point::new(0.0, b.y0 + b.height() * t),
                }
            })
            .collect();
        self.scene.guide = Some(pts);
        self.tools.guide_edit = true;
        self.relayout_center();
    }

    fn guide_hit(&self, rect: egui::Rect, pos: Pos2) -> Option<usize> {
        let g = self.scene.guide.as_ref()?;
        g.iter()
            .enumerate()
            .map(|(i, p)| (i, self.scene_to_screen(rect, *p).distance(pos)))
            .filter(|(_, d)| *d <= HANDLE_HIT * 1.5)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    // ---------------------------------------------------------------- transform (Giongo style: None | Trapezoid | Grid)

    pub(super) fn transform_ui(&mut self, ui: &mut egui::Ui) {
        use crate::model::params::TransformMode;
        ui.separator();
        ui.strong("Transform (Giongo style)");

        let current_mode = self.scene.params.transform_mode;
        ui.horizontal(|ui| {
            for mode in TransformMode::ALL {
                if ui.selectable_label(current_mode == mode, mode.label()).clicked() {
                    self.record();
                    self.scene.params.transform_mode = mode;
                    match mode {
                        TransformMode::None => {
                            self.tools.warp_edit = false;
                            self.scene.params.trap_h = 0.0;
                            self.scene.params.trap_v = 0.0;
                            self.scene.params.keystone_h = 0.0;
                            self.scene.params.keystone_v = 0.0;
                            self.scene.free_warp = FreeWarp::default();
                        }
                        TransformMode::Trapezoid => {
                            self.tools.warp_edit = false;
                            self.scene.free_warp = FreeWarp::default();
                        }
                        TransformMode::Grid => {
                            self.tools.warp_edit = true;
                            self.scene.params.trap_h = 0.0;
                            self.scene.params.trap_v = 0.0;
                            self.scene.params.keystone_h = 0.0;
                            self.scene.params.keystone_v = 0.0;
                            if self.scene.free_warp.is_identity() {
                                self.scene.free_warp = WarpStyle::ALL[self.tools.warp_style].build(self.tools.warp_bend);
                            }
                        }
                    }
                    self.relayout_center();
                }
            }
        });

        match self.scene.params.transform_mode {
            TransformMode::None => {
                ui.weak("No perspective or mesh warp applied.");
            }
            TransformMode::Trapezoid => {
                let mut changed = false;
                changed |= widgets::slider(ui, &mut self.scene.params.trap_h, -1.0..=1.0, "Horizontal trapezoid skew");
                changed |= widgets::slider(ui, &mut self.scene.params.trap_v, -1.0..=1.0, "Vertical trapezoid skew");
                if changed {
                    self.scene.params.keystone_h = self.scene.params.trap_h;
                    self.scene.params.keystone_v = self.scene.params.trap_v;
                    self.relayout_center();
                }
                if ui.button("↺ Reset Trapezoid").clicked() {
                    self.record();
                    self.scene.params.trap_h = 0.0;
                    self.scene.params.trap_v = 0.0;
                    self.scene.params.keystone_h = 0.0;
                    self.scene.params.keystone_v = 0.0;
                    self.relayout_center();
                }
            }
            TransformMode::Grid => {
                let mut changed = false;
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("grid_preset")
                        .selected_text(WarpStyle::ALL[self.tools.warp_style].label())
                        .show_ui(ui, |ui| {
                            for (i, s) in WarpStyle::ALL.iter().enumerate() {
                                if ui.selectable_value(&mut self.tools.warp_style, i, s.label()).changed() {
                                    changed = true;
                                }
                            }
                        });
                    if ui.button("🎲 Roll warp").on_hover_text("Generate random mesh deformation").clicked() {
                        self.record();
                        let r_seed = self.scene.seed ^ 0x9E3779B97F4A7C15;
                        self.scene.free_warp = FreeWarp::roll(r_seed, 0.5);
                        self.tools.warp_edit = true;
                        self.relayout_center();
                    }
                });
                changed |= widgets::slider(ui, &mut self.tools.warp_bend, -1.0..=1.0, "Bend");
                if changed {
                    if self.tools.warp_snapshot.is_none() {
                        self.tools.warp_snapshot = Some(self.scene.clone());
                    }
                    self.scene.free_warp = WarpStyle::ALL[self.tools.warp_style].build(self.tools.warp_bend);
                    self.tools.warp_edit = true;
                    self.relayout_center();
                }
                if self.tools.warp_snapshot.is_some()
                    && !ui.ctx().input(|i| i.pointer.any_down())
                    && let Some(before) = self.tools.warp_snapshot.take()
                {
                    self.history.record(before);
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.tools.warp_edit, "Show points on canvas");
                    if ui
                        .add_enabled(!self.scene.free_warp.is_identity(), egui::Button::new("↺ Reset Grid"))
                        .clicked()
                    {
                        self.record();
                        self.scene.free_warp = FreeWarp::default();
                        self.relayout_center();
                    }
                });
            }
        }
    }

    // ---------------------------------------------------------------- SFX Dictionary (Phase 3)

    pub(super) fn dictionary_ui(&mut self, ui: &mut egui::Ui) {
        use crate::model::dictionary::{auto_pick_sfx, ENGLISH_DICTIONARY};
        ui.separator();
        let header = egui::CollapsingHeader::new("📖 English SFX Dictionary")
            .default_open(false);
        header.show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, cat) in ENGLISH_DICTIONARY.iter().enumerate() {
                    let sel = self.tools.dict_category == i;
                    if ui.selectable_label(sel, cat.genre.short_label()).clicked() {
                        self.tools.dict_category = i;
                    }
                }
            });
            let cat = &ENGLISH_DICTIONARY[self.tools.dict_category];
            ui.weak(cat.description);
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                for &word in cat.words {
                    if ui.button(word).on_hover_text("Click to use sound effect & auto-pick preset style").clicked() {
                        self.record();
                        self.scene.text = word.to_string();
                        let pick = auto_pick_sfx(&self.scene.text);
                        self.apply_auto_pick(&pick);
                        self.relayout_center();
                    }
                }
            });
        });
    }

    pub(super) fn apply_auto_pick(&mut self, pick: &crate::model::dictionary::AutoPickResult) {
        self.scene.params.set_power(pick.power);
        self.scene.params.set_roughness(pick.roughness);
        self.scene.params.set_momentum(pick.momentum);

        // Apply matching genre preset if found
        let target_name = match pick.genre {
            crate::model::dictionary::SfxGenre::Impact => "Impact",
            crate::model::dictionary::SfxGenre::Crash => "Crash",
            crate::model::dictionary::SfxGenre::Speed => "Slash",
            crate::model::dictionary::SfxGenre::Rumble => "Rumble",
            crate::model::dictionary::SfxGenre::Water => "Splash",
            crate::model::dictionary::SfxGenre::Sparkle => "Sparkle",
            crate::model::dictionary::SfxGenre::Small => "Small",
            crate::model::dictionary::SfxGenre::Shock => "Shock",
        };

        if let Some(p) = self.tools.presets.iter().find(|p| p.name.eq_ignore_ascii_case(target_name)) {
            let wave = self.scene.params.wave;
            let wave_cycles = self.scene.params.wave_cycles;
            self.scene.params = p.params;
            self.scene.params.wave = wave;
            self.scene.params.wave_cycles = wave_cycles;
            self.scene.params.set_power(pick.power);
            self.scene.params.set_roughness(pick.roughness);
            self.scene.params.set_momentum(pick.momentum);
        }

        self.notice = Some(format!(
            "✨ Auto-picked: {} (Power: {:.0}, Roughness: {:.0}, Momentum: {:.0})",
            pick.genre.label(),
            pick.power,
            pick.roughness,
            pick.momentum
        ));
    }

    fn warp_hit(&self, rect: egui::Rect, pos: Pos2) -> Option<usize> {
        let wr = self.warp_rect?;
        let cp = self.scene.free_warp.control_points(wr);
        cp.iter()
            .enumerate()
            .map(|(i, p)| (i, self.scene_to_screen(rect, *p).distance(pos)))
            .filter(|(_, d)| *d <= HANDLE_HIT * 1.5)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    // ---------------------------------------------------------------- brush

    pub(super) fn brush_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Brush lettering");
        ui.checkbox(&mut self.tools.brush_mode, "Brush mode (drag on canvas to draw)");
        widgets::slider(ui, &mut self.tools.brush_width, 2.0..=80.0, "width");
        widgets::slider(ui, &mut self.tools.brush_taper, 0.0..=0.5, "taper");
        let strokes = self.brush_selection();
        if ui.add_enabled(!strokes.is_empty(), egui::Button::new("Delete selected strokes (Del)")).clicked() {
            self.delete_selected_strokes();
        }
        if self.tools.brush_mode {
            ui.weak("Middle-drag pans · wheel zooms");
        }
    }

    fn brush_selection(&self) -> Vec<usize> {
        self.selection
            .iter()
            .copied()
            .filter(|&i| i >= self.scene.text_letter_count && i < self.scene.letters.len())
            .collect()
    }

    fn commit_stroke(&mut self) {
        let pts = std::mem::take(&mut self.tools.stroke);
        let Some(path) = stroke_to_path(&pts, self.tools.brush_width, self.tools.brush_taper) else {
            return;
        };
        self.record();
        self.scene.letters.push(Letter::new('~', path, Affine::IDENTITY));
        self.selection.clear();
        self.selection.insert(self.scene.letters.len() - 1);
        self.scene_version += 1;
    }

    fn delete_selected_strokes(&mut self) {
        let mut idx = self.brush_selection();
        if idx.is_empty() {
            return;
        }
        self.record();
        idx.sort_unstable_by(|a, b| b.cmp(a));
        for i in idx {
            self.scene.letters.remove(i);
        }
        self.selection.clear();
        self.scene_version += 1;
    }

    // ---------------------------------------------------------------- files

    pub(super) fn file_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Export ZIP…").on_hover_text("PNG + white PNG + SVGs + project").clicked() {
                self.export_zip();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Save project…").clicked() {
                self.save_project();
            }
            if ui.button("Open project…").clicked() {
                self.open_project();
            }
        });
    }

    fn notify(&mut self, msg: String) {
        self.status = None;
        self.notice = Some(msg);
    }

    fn export_zip(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("ZIP archive", &["zip"]).set_file_name("sfx.zip").save_file()
        else {
            return;
        };
        match export::bundle::export_zip(&self.scene, &path, self.export_size) {
            Ok(()) => self.notify(format!("Saved {}", path.display())),
            Err(e) => self.set_error(format!("ZIP export failed: {e:#}")),
        }
    }

    pub(super) fn copy_png(&mut self) {
        let result = export::png::render_export(&self.scene, self.export_size.min(4096), self.export_transparent)
            .and_then(|pm| {
                let img = arboard::ImageData {
                    width: pm.width() as usize,
                    height: pm.height() as usize,
                    bytes: std::borrow::Cow::Owned(export::png::pixmap_to_rgba(&pm)),
                };
                arboard::Clipboard::new()
                    .and_then(|mut c| c.set_image(img))
                    .map_err(|e| anyhow::anyhow!("{e}"))
            });
        match result {
            Ok(()) => self.notify("Copied PNG to clipboard".into()),
            Err(e) => self.set_error(format!("Copy failed: {e:#}")),
        }
    }

    fn save_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SFX project", &["json"])
            .set_file_name("project.sfx.json")
            .save_file()
        else {
            return;
        };
        match export::project::save_project(&self.scene, &path) {
            Ok(()) => self.notify(format!("Saved {}", path.display())),
            Err(e) => self.set_error(format!("Save failed: {e:#}")),
        }
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("SFX project", &["json"]).pick_file() else {
            return;
        };
        match export::project::load_project(&path) {
            Ok(scene) => {
                self.record();
                self.scene = scene;
                self.selection.clear();
                self.drag = None;
                self.tools.guide_edit = false;
                self.scene_version += 1;
                self.fit_pending = true;
                self.notify(format!("Opened {}", path.display()));
            }
            Err(e) => self.set_error(format!("Open failed: {e:#}")),
        }
    }

    // ---------------------------------------------------------------- canvas input

    /// Handles brush drawing and guide editing. Returns `true` when the input was consumed
    /// and normal select / move / pan handling must be skipped.
    pub(super) fn canvas_tools(&mut self, response: &egui::Response, rect: egui::Rect) -> bool {
        let pos = response.interact_pointer_pos();

        if self.tools.brush_mode {
            if response.drag_started_by(PointerButton::Primary)
                && let Some(p) = pos
            {
                self.tools.stroke = vec![self.screen_to_scene(rect, p)];
            }
            if response.dragged_by(PointerButton::Primary)
                && !self.tools.stroke.is_empty()
                && let Some(p) = pos
            {
                let q = self.screen_to_scene(rect, p);
                self.tools.stroke.push(q);
            }
            if response.drag_stopped_by(PointerButton::Primary) {
                self.commit_stroke();
            }
            if response.clicked_by(PointerButton::Primary)
                && let Some(p) = pos
            {
                self.tools.stroke = vec![self.screen_to_scene(rect, p)];
                self.commit_stroke();
            }
            if response.dragged_by(PointerButton::Middle) {
                self.pan += response.drag_delta();
            }
            return true;
        }

        if self.tools.warp_edit && self.warp_rect.is_some() {
            if response.drag_started_by(PointerButton::Primary)
                && let Some(p) = pos
                && let Some(i) = self.warp_hit(rect, p)
            {
                self.record();
                self.tools.warp_drag = Some(i);
            }
            if let Some(i) = self.tools.warp_drag {
                if response.dragged()
                    && let Some(p) = pos
                    && let Some(wr) = self.warp_rect
                {
                    let q = self.screen_to_scene(rect, p);
                    self.scene.free_warp.set_point(wr, i, q);
                    self.relayout_static();
                }
                if response.drag_stopped() {
                    self.tools.warp_drag = None;
                }
                return true;
            }
        } else {
            self.tools.warp_drag = None;
        }

        if !(self.tools.guide_edit && self.scene.guide.is_some()) {
            self.tools.guide_drag = None;
            return false;
        }

        if response.drag_started_by(PointerButton::Primary)
            && let Some(p) = pos
            && let Some(i) = self.guide_hit(rect, p)
        {
            self.record();
            self.tools.guide_drag = Some(i);
        }
        if let Some(i) = self.tools.guide_drag {
            if response.dragged()
                && let Some(p) = pos
            {
                let q = self.screen_to_scene(rect, p);
                if let Some(pt) = self.scene.guide.as_mut().and_then(|g| g.get_mut(i)) {
                    *pt = q;
                }
                self.relayout_static();
            }
            if response.drag_stopped() {
                self.tools.guide_drag = None;
            }
            return true;
        }

        if response.clicked_by(PointerButton::Primary)
            && let Some(p) = pos
            && self.guide_hit(rect, p).is_none()
        {
            let q = self.screen_to_scene(rect, p);
            self.record();
            if let Some(g) = self.scene.guide.as_mut() {
                let at = nearest_segment(g, q) + 1;
                g.insert(at, q);
            }
            self.relayout_static();
            return true;
        }
        if response.secondary_clicked()
            && let Some(p) = pos
            && let Some(i) = self.guide_hit(rect, p)
            && self.scene.guide.as_ref().is_some_and(|g| g.len() > 2)
        {
            self.record();
            if let Some(g) = self.scene.guide.as_mut() {
                g.remove(i);
            }
            self.relayout_static();
            return true;
        }
        false
    }

    pub(super) fn draw_tool_overlay(&self, painter: &egui::Painter, rect: egui::Rect) {
        if let Some(g) = &self.scene.guide {
            let line: Vec<Pos2> = catmull_rom(g, 12).iter().map(|p| self.scene_to_screen(rect, *p)).collect();
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, GUIDE_COLOR.gamma_multiply(0.8))));
            if self.tools.guide_edit {
                for p in g {
                    let c = self.scene_to_screen(rect, *p);
                    painter.circle_filled(c, 5.0, GUIDE_COLOR);
                    painter.circle_stroke(c, 5.0, egui::Stroke::new(1.0, Color32::WHITE));
                }
            }
        }
        if self.tools.warp_edit
            && let Some(wr) = self.warp_rect
        {
            self.draw_warp_grid(painter, rect, wr);
        }
        if self.tools.stroke.len() > 1 {
            let pts: Vec<Pos2> = self.tools.stroke.iter().map(|p| self.scene_to_screen(rect, *p)).collect();
            let w = (self.tools.brush_width * self.zoom * 0.6).max(1.0) as f32;
            painter.add(egui::Shape::line(pts, egui::Stroke::new(w, ACCENT.gamma_multiply(0.7))));
        }
    }

    fn draw_warp_grid(&self, painter: &egui::Painter, rect: egui::Rect, wr: kurbo::Rect) {
        const WARP_COLOR: Color32 = Color32::from_rgb(230, 60, 200);
        let cp = self.scene.free_warp.control_points(wr);
        let to_screen = |p: Point| self.scene_to_screen(rect, p);
        // Control cage (faint) ...
        let faint = egui::Stroke::new(1.0, WARP_COLOR.gamma_multiply(0.35));
        for j in 0..free_warp::GRID {
            for i in 0..free_warp::GRID {
                if i + 1 < free_warp::GRID {
                    painter.line_segment([to_screen(cp[free_warp::idx(i, j)]), to_screen(cp[free_warp::idx(i + 1, j)])], faint);
                }
                if j + 1 < free_warp::GRID {
                    painter.line_segment([to_screen(cp[free_warp::idx(i, j)]), to_screen(cp[free_warp::idx(i, j + 1)])], faint);
                }
            }
        }
        // ... and the actual warped surface lines.
        let strong = egui::Stroke::new(1.2, WARP_COLOR.gamma_multiply(0.85));
        let steps = 40;
        for k in 0..free_warp::GRID {
            let t = k as f64 / (free_warp::GRID - 1) as f64;
            let row: Vec<Pos2> = (0..=steps)
                .map(|s| to_screen(FreeWarp::surface(&cp, f64::from(s) / f64::from(steps), t)))
                .collect();
            let col: Vec<Pos2> = (0..=steps)
                .map(|s| to_screen(FreeWarp::surface(&cp, t, f64::from(s) / f64::from(steps))))
                .collect();
            painter.add(egui::Shape::line(row, strong));
            painter.add(egui::Shape::line(col, strong));
        }
        for p in cp {
            let c = to_screen(p);
            painter.circle_filled(c, 5.0, Color32::WHITE);
            painter.circle_stroke(c, 5.0, egui::Stroke::new(1.5, WARP_COLOR));
        }
    }

    // ---------------------------------------------------------------- keyboard

    /// Delete removes selected brush strokes; arrows nudge the selection (Shift = 10 units).
    pub(super) fn tool_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::{Key, Modifiers};
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
            self.copy_png();
        }
        let (del, bs, nudges) = ctx.input_mut(|i| {
            let mut n = Vec::new();
            for (key, dir) in [
                (Key::ArrowLeft, Vec2::new(-1.0, 0.0)),
                (Key::ArrowRight, Vec2::new(1.0, 0.0)),
                (Key::ArrowUp, Vec2::new(0.0, -1.0)),
                (Key::ArrowDown, Vec2::new(0.0, 1.0)),
            ] {
                if i.consume_key(Modifiers::SHIFT, key) {
                    n.push(dir * 10.0);
                } else if i.consume_key(Modifiers::NONE, key) {
                    n.push(dir);
                }
            }
            (i.consume_key(Modifiers::NONE, Key::Delete), i.consume_key(Modifiers::NONE, Key::Backspace), n)
        });
        if del || bs {
            self.delete_selected_strokes();
        }
        if self.selection.is_empty() || nudges.is_empty() {
            return;
        }
        self.record();
        let total: Vec2 = nudges.into_iter().fold(Vec2::ZERO, |a, b| a + b);
        let d = kurbo::Vec2::new(f64::from(total.x), f64::from(total.y));
        for &i in &self.selection {
            if let Some(l) = self.scene.letters.get_mut(i) {
                l.transform = translate(d) * l.transform;
            }
        }
        self.scene_version += 1;
    }
}

/// Index of the guide segment (i, i+1) closest to `p`.
fn nearest_segment(g: &[Point], p: Point) -> usize {
    use kurbo::{Line, ParamCurveNearest};
    g.windows(2)
        .enumerate()
        .map(|(i, w)| (i, Line::new(w[0], w[1]).nearest(p, 1e-3).distance_sq))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_segment_picks_closest() {
        let g = [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0)];
        assert_eq!(nearest_segment(&g, Point::new(5.0, 1.0)), 0);
        assert_eq!(nearest_segment(&g, Point::new(11.0, 7.0)), 1);
    }
}
