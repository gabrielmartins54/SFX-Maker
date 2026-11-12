//! Giongo-style on-canvas transform gizmo and selection highlight for lettering.
//!
//! Features:
//! - Selected letters display a glowing cyan contour outline along their glyph silhouettes.
//! - Giongo transform Gizmo at selection centre:
//!   - Blue sideways arrow (Move horizontal)
//!   - Yellow up/down arrow (Move vertical)
//!   - White center box & coral quadrant patch (Move free)
//!   - Coral rotate ring (Rotate; Shift snaps 15 deg)
//!   - Silver diagonal handle (Scale uniform)
//! - Bottom-right HUD legend displaying interactive guide:
//!   — Sideways   ↑ Up/down   □ Free   ○ Rotate   ■ Scale

use egui::{Color32, Pos2, Stroke, Vec2};
use kurbo::{Affine, Point, Rect};

use super::SfxApp;
use crate::interact::{hit_letter, rotate_about, scale_about, selection_bounds, translate};

pub const GZ_CYAN: Color32 = Color32::from_rgb(0, 207, 216); // #00CFD8
pub const GZ_X: Color32 = Color32::from_rgb(0x75, 0xD9, 0xFF); // #75D9FF - Sideways (blue)
pub const GZ_Y: Color32 = Color32::from_rgb(0xF2, 0xC9, 0x4C); // #F2C94C - Up/down (yellow)
pub const GZ_ROT: Color32 = Color32::from_rgb(0xFF, 0x8C, 0x73); // #FF8C73 - Rotate (coral)
pub const GZ_SCALE: Color32 = Color32::from_rgb(0xD8, 0xD8, 0xD8); // #D8D8D8 - Scale (silver)
pub const GZ_FREE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF); // #FFFFFF - Free (white)
pub const GZ_DARK: Color32 = Color32::from_rgba_premultiplied(20, 20, 20, 200);
pub const GZ_BORDER: Color32 = Color32::from_rgb(45, 45, 45);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GizmoPart {
    None,
    MoveX,
    MoveY,
    MoveFree,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragKind {
    MoveFree,
    MoveX,
    MoveY,
    Rotate,
    Scale,
    #[allow(dead_code)]
    Skew,
    #[allow(dead_code)]
    Distort,
}

/// An in-progress transform of the selection.
pub struct DragOp {
    pub kind: DragKind,
    pub centre: Point,
    pub start: Point,
    pub orig: Vec<(usize, Affine)>,
    #[allow(dead_code)]
    pub orig_quads: Vec<(usize, [Point; 4])>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub(super) enum Hit {
    Rotate,
    RotateCorner,
    Corner(usize),
    Edge(usize),
}

/// The eight handle positions: corners TL, TR, BR, BL then edge midpoints T, R, B, L.
#[allow(dead_code)]
pub(super) fn handles(b: Rect) -> [Point; 8] {
    let c = b.center();
    [
        Point::new(b.x0, b.y0),
        Point::new(b.x1, b.y0),
        Point::new(b.x1, b.y1),
        Point::new(b.x0, b.y1),
        Point::new(c.x, b.y0),
        Point::new(b.x1, c.y),
        Point::new(c.x, b.y1),
        Point::new(b.x0, c.y),
    ]
}

#[allow(dead_code)]
fn opposite(i: usize) -> usize {
    if i < 4 { (i + 2) % 4 } else { 4 + (i - 4 + 2) % 4 }
}

fn safe_scale(s: f64) -> f64 {
    if s.abs() < 0.01 { 0.01_f64.copysign(if s == 0.0 { 1.0 } else { s }) } else { s.clamp(-100.0, 100.0) }
}

#[allow(dead_code)]
pub(super) fn scale_transform(pivot: Point, handle: Point, cur: Point, sx: bool, sy: bool, proportional: bool) -> Option<Affine> {
    let (dx, dy) = (handle.x - pivot.x, handle.y - pivot.y);
    let fx = if sx && dx.abs() > 1e-9 { (cur.x - pivot.x) / dx } else { 1.0 };
    let fy = if sy && dy.abs() > 1e-9 { (cur.y - pivot.y) / dy } else { 1.0 };
    if (sx && dx.abs() <= 1e-9) || (sy && dy.abs() <= 1e-9) {
        return None;
    }
    let (fx, fy) = if proportional {
        let f = if sx && sy {
            let d0 = (handle - pivot).hypot();
            if d0 < 1e-9 {
                return None;
            }
            (cur - pivot).hypot() / d0
        } else if sx {
            fx
        } else {
            fy
        };
        (f, f)
    } else {
        (fx, fy)
    };
    let (fx, fy) = (safe_scale(fx), safe_scale(fy));
    let p = pivot.to_vec2();
    Some(Affine::translate(p) * Affine::scale_non_uniform(fx, fy) * Affine::translate(-p))
}

#[allow(dead_code)]
pub(super) fn skew_transform(pivot: Point, handle: Point, start: Point, cur: Point, horizontal: bool) -> Option<Affine> {
    let p = pivot.to_vec2();
    let k = if horizontal {
        let d = handle.y - pivot.y;
        if d.abs() < 1e-9 {
            return None;
        }
        (cur.x - start.x) / d
    } else {
        let d = handle.x - pivot.x;
        if d.abs() < 1e-9 {
            return None;
        }
        (cur.y - start.y) / d
    };
    let k = k.clamp(-5.0, 5.0);
    let s = if horizontal { Affine::skew(k, 0.0) } else { Affine::skew(0.0, k) };
    Some(Affine::translate(p) * s * Affine::translate(-p))
}

fn bezpath_to_screen_polylines(
    path: &kurbo::BezPath,
    scene_to_screen: impl Fn(Point) -> Pos2,
    tol: f64,
) -> Vec<Vec<Pos2>> {
    let mut polylines = Vec::new();
    let mut current = Vec::new();
    kurbo::flatten(path.elements().iter().copied(), tol, |el| match el {
        kurbo::PathEl::MoveTo(pt) => {
            if current.len() >= 2 {
                polylines.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            current.push(scene_to_screen(pt));
        }
        kurbo::PathEl::LineTo(pt) => {
            current.push(scene_to_screen(pt));
        }
        kurbo::PathEl::ClosePath if current.len() >= 2 => {
            polylines.push(std::mem::take(&mut current));
        }
        _ => {}
    });
    if current.len() >= 2 {
        polylines.push(current);
    }
    polylines
}

fn draw_hud_legend(painter: &egui::Painter, canvas_rect: egui::Rect) {
    let font_id = egui::FontId::proportional(11.5);
    let items = [
        ("—", GZ_X, "Sideways"),
        ("↑", GZ_Y, "Up/down"),
        ("□", GZ_FREE, "Free"),
        ("○", GZ_ROT, "Rotate"),
        ("■", GZ_SCALE, "Scale"),
    ];
    let text_color = Color32::from_rgb(225, 225, 225);
    let gap = 12.0_f32;
    let sym_gap = 4.0_f32;

    let mut galleys = Vec::new();
    let mut content_w = 0.0_f32;
    for (sym, col, label) in items {
        let sg = painter.layout_no_wrap(sym.into(), font_id.clone(), col);
        let lg = painter.layout_no_wrap(label.into(), font_id.clone(), text_color);
        let pair_w = sg.size().x + sym_gap + lg.size().x;
        content_w += pair_w;
        galleys.push((sg, lg));
    }
    content_w += gap * (galleys.len() as f32 - 1.0);

    let pad_x = 10.0_f32;
    let pill_w = content_w + pad_x * 2.0;
    let pill_h = 24.0_f32;
    let margin = 12.0_f32;

    let pill_rect = egui::Rect::from_min_size(
        egui::Pos2::new(
            canvas_rect.max.x - pill_w - margin,
            canvas_rect.max.y - pill_h - margin,
        ),
        egui::Vec2::new(pill_w, pill_h),
    );

    // Dark translucent background with subtle rounded border
    painter.rect_filled(
        pill_rect,
        4.0,
        Color32::from_rgba_unmultiplied(30, 30, 34, 230),
    );
    painter.rect_stroke(
        pill_rect,
        4.0,
        egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(70, 70, 75, 180)),
        egui::StrokeKind::Outside,
    );

    let mut cur_x = pill_rect.min.x + pad_x;
    let mid_y = pill_rect.center().y;
    for (sg, lg) in galleys {
        let sy = mid_y - sg.size().y * 0.5;
        let ly = mid_y - lg.size().y * 0.5;
        let sw = sg.size().x;
        let lw = lg.size().x;
        painter.galley(egui::Pos2::new(cur_x, sy), sg, Color32::WHITE);
        cur_x += sw + sym_gap;
        painter.galley(egui::Pos2::new(cur_x, ly), lg, Color32::WHITE);
        cur_x += lw + gap;
    }
}

impl SfxApp {
    pub fn gizmo_metrics(&self, rect: egui::Rect) -> Option<(Pos2, Point, f32)> {
        let b = selection_bounds(&self.scene, &self.selection)?;
        let center_scene = b.center();
        let center_screen = self.scene_to_screen(rect, center_scene);
        let scale = (rect.height().min(rect.width()) / 650.0).clamp(0.7, 1.1);
        Some((center_screen, center_scene, scale))
    }

    pub fn hit_gizmo(&self, rect: egui::Rect, pos: Pos2) -> GizmoPart {
        let Some((c, _, scale)) = self.gizmo_metrics(rect) else {
            return GizmoPart::None;
        };
        let d = pos - c;
        let r = d.length();

        // 1. Center free box (size 11, hit padding ~10)
        if d.x.abs() <= 10.0 * scale && d.y.abs() <= 10.0 * scale {
            return GizmoPart::MoveFree;
        }

        // 2. Scale square (at 45 deg down-right: 52 * u * scale)
        let u = std::f32::consts::FRAC_1_SQRT_2;
        let scale_pt = c + Vec2::new(52.0 * u * scale, 52.0 * u * scale);
        if pos.distance(scale_pt) <= 12.0 * scale {
            return GizmoPart::Scale;
        }

        // 3. Sideways arrow (x: 8..70, y: -11..11)
        if d.x >= 7.0 * scale && d.x <= 70.0 * scale && d.y.abs() <= 11.0 * scale {
            return GizmoPart::MoveX;
        }

        // 4. Up/down arrow (-y: 8..70, x: -11..11)
        if -d.y >= 7.0 * scale && -d.y <= 70.0 * scale && d.x.abs() <= 11.0 * scale {
            return GizmoPart::MoveY;
        }

        // 5. Free patch (top-right quadrant at (22*scale, -22*scale))
        let patch_pt = c + Vec2::new(22.0 * scale, -22.0 * scale);
        if pos.distance(patch_pt) <= 10.0 * scale {
            return GizmoPart::MoveFree;
        }

        // 6. Rotate ring (radius 56*scale, hit band +- 9px)
        let ring_r = 56.0 * scale;
        if (r - ring_r).abs() <= 9.0 * scale {
            return GizmoPart::Rotate;
        }

        // 7. Scale line (connecting to scale box)
        if d.x > 5.0 && d.y > 5.0 && r <= 52.0 * u * scale && (d.x - d.y).abs() <= 8.0 * scale {
            return GizmoPart::Scale;
        }

        GizmoPart::None
    }

    pub(super) fn handle_cursor(&self, ctx: &egui::Context, rect: egui::Rect, pos: Pos2, _ctrl: bool) {
        let part = self.hit_gizmo(rect, pos);
        let cursor = match part {
            GizmoPart::MoveX => egui::CursorIcon::ResizeHorizontal,
            GizmoPart::MoveY => egui::CursorIcon::ResizeVertical,
            GizmoPart::MoveFree => egui::CursorIcon::Move,
            GizmoPart::Rotate => egui::CursorIcon::Grab,
            GizmoPart::Scale => egui::CursorIcon::ResizeNwSe,
            GizmoPart::None => {
                let pt = self.screen_to_scene(rect, pos);
                if hit_letter(&self.scene, pt).is_some() {
                    egui::CursorIcon::PointingHand
                } else {
                    return;
                }
            }
        };
        ctx.set_cursor_icon(cursor);
    }

    pub(super) fn begin_drag(&mut self, rect: egui::Rect, pos: Pos2, shift: bool, _ctrl: bool) {
        let part = self.hit_gizmo(rect, pos);
        let start = self.screen_to_scene(rect, pos);
        let letter_hit = hit_letter(&self.scene, start);

        let kind = match part {
            GizmoPart::MoveX => Some(DragKind::MoveX),
            GizmoPart::MoveY => Some(DragKind::MoveY),
            GizmoPart::MoveFree => Some(DragKind::MoveFree),
            GizmoPart::Rotate => Some(DragKind::Rotate),
            GizmoPart::Scale => Some(DragKind::Scale),
            GizmoPart::None => {
                if let Some(i) = letter_hit {
                    if !self.selection.contains(&i) {
                        if !shift {
                            self.selection.clear();
                        }
                        self.selection.insert(i);
                    }
                    Some(DragKind::MoveFree)
                } else {
                    None
                }
            }
        };

        if let Some(kind) = kind {
            let Some(b) = selection_bounds(&self.scene, &self.selection) else {
                return;
            };
            self.record();
            let orig: Vec<(usize, Affine)> = self
                .selection
                .iter()
                .filter_map(|&i| self.scene.letters.get(i).map(|l| (i, l.transform)))
                .collect();
            let orig_quads: Vec<(usize, [Point; 4])> = self
                .selection
                .iter()
                .filter_map(|&i| self.scene.letters.get(i).map(|l| (i, l.scene_corners())))
                .collect();
            self.drag = Some(DragOp {
                kind,
                centre: b.center(),
                start,
                orig,
                orig_quads,
            });
        }
    }

    pub(super) fn update_drag(&mut self, rect: egui::Rect, pos: Pos2, shift: bool, _alt: bool) {
        let cur = self.screen_to_scene(rect, pos);
        let Some(op) = &self.drag else { return };
        let c = op.centre;
        let delta = cur - op.start;

        let g = match op.kind {
            DragKind::MoveFree => translate(delta),
            DragKind::MoveX => translate(kurbo::Vec2::new(delta.x, 0.0)),
            DragKind::MoveY => translate(kurbo::Vec2::new(0.0, delta.y)),
            DragKind::Rotate => {
                let a0 = (op.start - c).atan2();
                let a1 = (cur - c).atan2();
                let mut d = a1 - a0;
                if shift {
                    let step = 15f64.to_radians();
                    d = (d / step).round() * step;
                }
                rotate_about(c, d)
            }
            DragKind::Scale => {
                let d0 = (op.start - c).hypot();
                let d1 = (cur - c).hypot();
                let factor = if d0 > 1e-6 {
                    (d1 / d0).clamp(0.05, 20.0)
                } else {
                    1.0
                };
                scale_about(c, factor)
            }
            DragKind::Skew | DragKind::Distort => translate(delta),
        };

        let updates: Vec<(usize, Affine)> = op.orig.iter().map(|&(i, t)| (i, g * t)).collect();
        for (i, t) in updates {
            if let Some(l) = self.scene.letters.get_mut(i) {
                l.transform = t;
            }
        }
        self.scene_version += 1;
    }

    pub(super) fn draw_selection(&self, painter: &egui::Painter, rect: egui::Rect) {
        if self.selection.is_empty() {
            return;
        }

        // 1. Cyan contour outline around each selected letter's silhouette
        let tol = (0.35 / self.zoom).clamp(0.05, 1.5);
        for &i in &self.selection {
            if let Some(l) = self.scene.letters.get(i) {
                let polylines = bezpath_to_screen_polylines(
                    &l.placed_path(),
                    |p| self.scene_to_screen(rect, p),
                    tol,
                );
                for pts in polylines {
                    if pts.len() >= 2 {
                        // Halo stroke (soft white backing for high contrast on dark/light canvas)
                        painter.add(egui::epaint::PathShape::closed_line(
                            pts.clone(),
                            Stroke::new(3.6, Color32::from_rgba_unmultiplied(255, 255, 255, 190)),
                        ));
                        // Glowing cyan contour stroke
                        painter.add(egui::epaint::PathShape::closed_line(
                            pts,
                            Stroke::new(1.8, GZ_CYAN),
                        ));
                    }
                }
            }
        }

        // 2. Giongo letter transform gizmo at selection centre
        let Some((c, _, scale)) = self.gizmo_metrics(rect) else {
            return;
        };

        let u = std::f32::consts::FRAC_1_SQRT_2;

        // Rotate ring (coral #FF8C73)
        let ring_r = 56.0 * scale;
        painter.circle_stroke(c, ring_r, Stroke::new(5.0, GZ_DARK));
        painter.circle_stroke(c, ring_r, Stroke::new(2.4, GZ_ROT));

        // Scale handle: diagonal line + silver square at 45 deg down-right
        let scale_line_start = c + Vec2::new(8.0 * u * scale, 8.0 * u * scale);
        let scale_line_end = c + Vec2::new(46.0 * u * scale, 46.0 * u * scale);
        painter.line_segment([scale_line_start, scale_line_end], Stroke::new(4.5, GZ_DARK));
        painter.line_segment([scale_line_start, scale_line_end], Stroke::new(2.0, GZ_SCALE));

        let scale_box_c = c + Vec2::new(52.0 * u * scale, 52.0 * u * scale);
        let scale_box_r = egui::Rect::from_center_size(scale_box_c, Vec2::splat(10.0 * scale));
        painter.rect_filled(scale_box_r, 0.0, GZ_SCALE);
        painter.rect_stroke(scale_box_r, 0.0, Stroke::new(1.0, GZ_BORDER), egui::StrokeKind::Outside);

        // Sideways arrow: line + blue arrowhead pointing right along +X
        let x_line_start = c + Vec2::new(8.0 * scale, 0.0);
        let x_line_end = c + Vec2::new(50.0 * scale, 0.0);
        painter.line_segment([x_line_start, x_line_end], Stroke::new(5.0, GZ_DARK));
        painter.line_segment([x_line_start, x_line_end], Stroke::new(2.4, GZ_X));

        let x_arrow_tip = c + Vec2::new(64.0 * scale, 0.0);
        let x_arrow_top = c + Vec2::new(50.0 * scale, -5.5 * scale);
        let x_arrow_bot = c + Vec2::new(50.0 * scale, 5.5 * scale);
        painter.add(egui::Shape::convex_polygon(
            vec![x_arrow_tip, x_arrow_top, x_arrow_bot],
            GZ_X,
            Stroke::new(1.0, GZ_BORDER),
        ));

        // Up/down arrow: line + yellow arrowhead pointing up along -Y
        let y_line_start = c - Vec2::new(0.0, 8.0 * scale);
        let y_line_end = c - Vec2::new(0.0, 50.0 * scale);
        painter.line_segment([y_line_start, y_line_end], Stroke::new(5.0, GZ_DARK));
        painter.line_segment([y_line_start, y_line_end], Stroke::new(2.4, GZ_Y));

        let y_arrow_tip = c - Vec2::new(0.0, 64.0 * scale);
        let y_arrow_left = c + Vec2::new(-5.5 * scale, -50.0 * scale);
        let y_arrow_right = c + Vec2::new(5.5 * scale, -50.0 * scale);
        painter.add(egui::Shape::convex_polygon(
            vec![y_arrow_tip, y_arrow_left, y_arrow_right],
            GZ_Y,
            Stroke::new(1.0, GZ_BORDER),
        ));

        // Free patch in top-right quadrant
        let patch_c = c + Vec2::new(22.0 * scale, -22.0 * scale);
        let patch_r = egui::Rect::from_center_size(patch_c, Vec2::splat(14.0 * scale));
        painter.rect_filled(patch_r, 0.0, Color32::from_rgba_unmultiplied(255, 140, 115, 90));
        painter.rect_stroke(
            patch_r,
            0.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 140, 115, 160)),
            egui::StrokeKind::Outside,
        );

        // Center Free box: white square at center
        let center_box_r = egui::Rect::from_center_size(c, Vec2::splat(11.0 * scale));
        painter.rect_filled(center_box_r, 0.0, GZ_FREE);
        painter.rect_stroke(center_box_r, 0.0, Stroke::new(1.2, GZ_BORDER), egui::StrokeKind::Outside);

        // 3. Bottom-right HUD legend
        draw_hud_legend(painter, rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b() -> Rect {
        Rect::new(0.0, 0.0, 100.0, 50.0)
    }

    #[test]
    fn handle_layout_and_opposites() {
        let h = handles(b());
        assert_eq!(h[0], Point::new(0.0, 0.0));
        assert_eq!(h[4], Point::new(50.0, 0.0));
        assert_eq!(h[5], Point::new(100.0, 25.0));
        assert_eq!(opposite(0), 2);
        assert_eq!(opposite(1), 3);
        assert_eq!(opposite(4), 6);
        assert_eq!(opposite(5), 7);
        assert_eq!(opposite(7), 5);
    }

    #[test]
    fn corner_scale_is_non_uniform_unless_shift() {
        let (p, h) = (Point::new(0.0, 0.0), Point::new(100.0, 50.0));
        let cur = Point::new(200.0, 50.0);
        let free = scale_transform(p, h, cur, true, true, false).unwrap();
        assert_eq!(free * Point::new(100.0, 50.0), Point::new(200.0, 50.0));
        let prop = scale_transform(p, h, cur, true, true, true).unwrap();
        let r = prop * Point::new(100.0, 50.0);
        assert!((r.x / r.y - 2.0).abs() < 1e-9, "aspect kept: {r:?}");
    }

    #[test]
    fn edge_scale_only_moves_one_axis() {
        let g = scale_transform(Point::new(0.0, 25.0), Point::new(100.0, 25.0), Point::new(150.0, 999.0), true, false, false).unwrap();
        assert_eq!(g * Point::new(100.0, 40.0), Point::new(150.0, 40.0));
    }

    #[test]
    fn dragging_past_the_pivot_flips() {
        let g = scale_transform(Point::new(0.0, 0.0), Point::new(100.0, 50.0), Point::new(-100.0, 50.0), true, true, false).unwrap();
        let q = g * Point::new(100.0, 50.0);
        assert!(q.x < 0.0);
    }

    #[test]
    fn skew_top_edge_shifts_top_keeps_bottom() {
        let g = skew_transform(Point::new(50.0, 50.0), Point::new(50.0, 0.0), Point::new(50.0, 0.0), Point::new(60.0, 0.0), true).unwrap();
        let top = g * Point::new(50.0, 0.0);
        let bottom = g * Point::new(50.0, 50.0);
        assert!((top.x - 60.0).abs() < 1e-9, "{top:?}");
        assert_eq!(bottom, Point::new(50.0, 50.0));
    }

    #[test]
    fn degenerate_handles_give_none() {
        assert!(scale_transform(Point::new(5.0, 5.0), Point::new(5.0, 5.0), Point::new(9.0, 9.0), true, true, false).is_none());
    }

    #[test]
    fn gizmo_colors_and_parts() {
        assert_eq!(GizmoPart::None, GizmoPart::None);
        assert_ne!(GizmoPart::MoveX, GizmoPart::MoveY);
        assert_ne!(GizmoPart::Rotate, GizmoPart::Scale);
        assert_eq!(GZ_X, Color32::from_rgb(0x75, 0xD9, 0xFF));
        assert_eq!(GZ_Y, Color32::from_rgb(0xF2, 0xC9, 0x4C));
        assert_eq!(GZ_ROT, Color32::from_rgb(0xFF, 0x8C, 0x73));
        assert_eq!(GZ_SCALE, Color32::from_rgb(0xD8, 0xD8, 0xD8));
        assert_eq!(GZ_FREE, Color32::from_rgb(0xFF, 0xFF, 0xFF));
    }

    #[test]
    fn hit_gizmo_parts_from_center() {
        let mut app = SfxApp::new();
        let mut p = kurbo::BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((100.0, 0.0));
        p.line_to((100.0, 100.0));
        p.close_path();
        app.scene.letters = vec![crate::model::letter::Letter::new('A', p, Affine::IDENTITY)];
        app.selection.insert(0);

        let canvas = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let Some((c, _, scale)) = app.gizmo_metrics(canvas) else {
            panic!("metrics");
        };

        // Center hits MoveFree
        assert_eq!(app.hit_gizmo(canvas, c), GizmoPart::MoveFree);

        // Positive X hits MoveX
        assert_eq!(app.hit_gizmo(canvas, c + Vec2::new(30.0 * scale, 0.0)), GizmoPart::MoveX);

        // Negative Y hits MoveY
        assert_eq!(app.hit_gizmo(canvas, c - Vec2::new(0.0, 30.0 * scale)), GizmoPart::MoveY);

        // Diagonal hits Scale
        let u = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(app.hit_gizmo(canvas, c + Vec2::new(52.0 * u * scale, 52.0 * u * scale)), GizmoPart::Scale);

        // Ring distance hits Rotate
        assert_eq!(app.hit_gizmo(canvas, c + Vec2::new(0.0, 56.0 * scale)), GizmoPart::Rotate);

        // Far away hits None
        assert_eq!(app.hit_gizmo(canvas, c + Vec2::new(300.0, 300.0)), GizmoPart::None);
    }
}
