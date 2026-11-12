//! Variable stroke weight: thickens or thins glyph outlines by moving every outline point along
//! its normal by an amount that depends on its position (start -> end of the text or of each
//! letter). Pure geometry; no UI.
//!
//! The outline is flattened, offset with mitred corners, then cleaned with a positive-winding
//! boolean pass so thinning past a stroke's width (which inverts the contour) removes the
//! stroke instead of producing garbage.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
use kurbo::{BezPath, PathEl, Point, Rect, Shape, Vec2};

use super::text_to_paths::EM_SIZE;
use super::warp::flatten_subdivide;
use crate::model::letter::Letter;
use crate::model::params::Params;
use crate::model::scene::LayoutDirection;

const TOLERANCE: f64 = 0.05;
const MAX_SEGMENT: f64 = 2.0;
/// Longest allowed corner extension, as a multiple of the offset distance.
const MITER_LIMIT: f64 = 2.5;

/// True when the params change the outline at all.
pub fn is_active(p: &Params) -> bool {
    p.weight.abs() > 1e-9 || p.weight_contrast.abs() > 1e-9 || p.weight_start.abs() > 1e-9 || p.weight_end.abs() > 1e-9
}

/// Offset distance (scene units; positive = thicker) at `t` in 0..=1 along the ramp.
pub fn offset_at(p: &Params, t: f64) -> f64 {
    (p.weight + p.weight_start + (p.weight_end - p.weight_start) * t.clamp(0.0, 1.0)) * EM_SIZE
}

fn flow_range(r: Rect, dir: LayoutDirection) -> (f64, f64) {
    match dir {
        LayoutDirection::Horizontal => (r.x0, r.x1),
        LayoutDirection::Vertical => (r.y0, r.y1),
    }
}

fn contours(path: &BezPath) -> Vec<Vec<Point>> {
    let flat = flatten_subdivide(path, TOLERANCE, MAX_SEGMENT);
    let mut out: Vec<Vec<Point>> = Vec::new();
    for el in flat.iter() {
        match el {
            PathEl::MoveTo(p) => out.push(vec![p]),
            PathEl::LineTo(p) => {
                if let Some(c) = out.last_mut() {
                    c.push(p);
                }
            }
            _ => {}
        }
    }
    for c in &mut out {
        // Drop a duplicated closing point.
        if c.len() > 1 && c[0].distance(c[c.len() - 1]) < 1e-9 {
            c.pop();
        }
    }
    out.retain(|c| c.len() >= 3);
    out
}

fn signed_area(c: &[Point]) -> f64 {
    let n = c.len();
    (0..n).map(|i| c[i].x * c[(i + 1) % n].y - c[(i + 1) % n].x * c[i].y).sum::<f64>() / 2.0
}

/// Offsets one contour. `outward_is_right` says on which side of the travel direction the
/// outside (away from the fill) lies. `dist(p)` is the signed offset at a point.
fn offset_contour(c: &[Point], outward_is_right: bool, dist: &dyn Fn(Point) -> f64) -> Vec<Point> {
    let n = c.len();
    let normal = |a: Point, b: Point| {
        let d = b - a;
        let len = d.hypot();
        if len < 1e-12 {
            return Vec2::ZERO;
        }
        let t = d / len;
        if outward_is_right { Vec2::new(t.y, -t.x) } else { Vec2::new(-t.y, t.x) }
    };
    (0..n)
        .map(|i| {
            let prev = c[(i + n - 1) % n];
            let next = c[(i + 1) % n];
            let (n1, n2) = (normal(prev, c[i]), normal(c[i], next));
            let sum = n1 + n2;
            let m = if sum.hypot() < 1e-9 { n2 } else { sum / sum.hypot() };
            // Mitre: lengthen so both adjoining edges move by the full distance.
            let cos = m.dot(n2).max(1.0 / MITER_LIMIT);
            c[i] + m * (dist(c[i]) / cos)
        })
        .collect()
}

/// The path thickened (`dist > 0`) or thinned by a per-point distance function.
pub fn offset_path(path: &BezPath, dist: &dyn Fn(Point) -> f64) -> BezPath {
    let cs = contours(path);
    if cs.is_empty() {
        return BezPath::new();
    }
    // The largest contour is an outer boundary; its winding tells which side the fill is on.
    let reference = cs
        .iter()
        .map(|c| signed_area(c))
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(1.0);
    // Normalise so outer contours have positive area, then outside is on the right of travel.
    let flip = reference < 0.0;
    let polys: Vec<Vec<[f64; 2]>> = cs
        .iter()
        .map(|c| {
            let mut c = c.clone();
            if flip {
                c.reverse();
            }
            let off = offset_contour(&c, true, dist);
            off.iter().map(|p| [p.x, p.y]).collect()
        })
        .collect();

    // With y-down screen coordinates a positive shoelace area is clockwise on screen, which is
    // what `Positive` keeps as filled.
    let shapes = polys.simplify_shape(FillRule::Positive);
    let mut out = BezPath::new();
    for shape in shapes {
        for contour in shape {
            let mut it = contour.iter();
            if let Some(f) = it.next() {
                out.move_to((f[0], f[1]));
                for p in it {
                    out.line_to((p[0], p[1]));
                }
                out.close_path();
            }
        }
    }
    out
}

/// Exact offset of a path with a proper polygon offsetter (no per-vertex normal guessing).
/// `dx` is the distance vertical stems move, `dy` the distance horizontal strokes move
/// (positive = thicker). Unequal values are an elliptical offset, done exactly by scaling y so
/// the ellipse becomes a circle, offsetting, and scaling back. Mixed signs / near-zero values on
/// one axis are clamped to a small same-sign amount (an offset ellipse needs both radii).
pub fn offset_xy(path: &BezPath, dx: f64, dy: f64, round: bool) -> BezPath {
    if dx.abs() < 1e-9 && dy.abs() < 1e-9 {
        return path.clone();
    }
    let (big, small) = if dx.abs() >= dy.abs() { (dx, dy) } else { (dy, dx) };
    let small = if small * big <= 0.0 || small.abs() < 0.05 * big.abs() { 0.05 * big } else { small };
    let (a, b) = if dx.abs() >= dy.abs() { (big, small) } else { (small, big) };
    // y scale that turns radii (a, b) into (a, a).
    let sy = a / b;
    let polys: Vec<Vec<[f64; 2]>> = contours(path)
        .iter()
        .map(|c| c.iter().map(|p| [p.x, p.y * sy]).collect())
        .collect();
    if polys.is_empty() {
        return BezPath::new();
    }
    let clean: Vec<Vec<Vec<[f64; 2]>>> = polys.simplify_shape(FillRule::NonZero);
    let join = if round { LineJoin::Round(0.15) } else { LineJoin::Miter(0.2 * std::f64::consts::PI) };
    let style = OutlineStyle::new(a).line_join(join);
    let mut out = BezPath::new();
    for shape in clean {
        let shapes: Vec<Vec<Vec<[f64; 2]>>> = shape.outline(&style);
        for sh in shapes {
            for contour in sh {
                let mut it = contour.iter();
                if let Some(f) = it.next() {
                    out.move_to((f[0], f[1] / sy));
                    for p in it {
                        out.line_to((p[0], p[1] / sy));
                    }
                    out.close_path();
                }
            }
        }
    }
    out
}

/// Equal offset on both axes.
#[cfg(test)]
pub fn uniform_offset(path: &BezPath, d: f64) -> BezPath {
    offset_xy(path, d, d, false)
}

/// Applies the weight params to freshly laid-out letters (before guide / free warp).
/// Affected letters get their outline replaced by the offset outline (in scene space) and an
/// identity transform.
pub fn apply_weight(letters: &mut [Letter], params: &Params, direction: LayoutDirection) {
    if !is_active(params) {
        return;
    }
    let Some(text_rect) = letters
        .iter()
        .map(|l| l.placed_path().bounding_box())
        .reduce(|a, b| a.union(b))
    else {
        return;
    };
    for l in letters.iter_mut() {
        let placed = l.placed_path();
        let rect = if params.weight_per_letter { placed.bounding_box() } else { text_rect };
        let (lo, hi) = flow_range(rect, direction);
        let span = (hi - lo).max(1e-9);
        let dist = |p: Point| {
            let u = match direction {
                LayoutDirection::Horizontal => p.x,
                LayoutDirection::Vertical => p.y,
            };
            offset_at(params, (u - lo) / span)
        };
        // TransType / Fontographer Simple algorithm:
        // dx = vertical stems offset (full weight)
        // dy = horizontal strokes offset (scaled by stem contrast ratio) + weight_contrast
        let dx = params.weight * EM_SIZE;
        let dy = dx * params.weight_stem_ratio + params.weight_contrast * EM_SIZE;
        let uniform = offset_xy(&placed, dx, dy, params.weight_round);
        let mut modified = if params.weight_start.abs() > 1e-9 || params.weight_end.abs() > 1e-9 {
            let ramp = |p: Point| dist(p) - params.weight * EM_SIZE;
            offset_path(&uniform, &ramp)
        } else {
            uniform
        };

        // Fontographer constraints: Keep vertical / horizontal dimensions
        let orig_bbox = placed.bounding_box();
        if (params.weight_keep_v || params.weight_keep_h) && orig_bbox.width() > 1e-6 && orig_bbox.height() > 1e-6 {
            let new_bbox = modified.bounding_box();
            if new_bbox.width() > 1e-6 && new_bbox.height() > 1e-6 {
                let sx = if params.weight_keep_h { orig_bbox.width() / new_bbox.width() } else { 1.0 };
                let sy = if params.weight_keep_v { orig_bbox.height() / new_bbox.height() } else { 1.0 };
                let anchor_x = orig_bbox.center().x;
                let anchor_y = orig_bbox.y1; // anchor baseline / bottom
                let t = kurbo::Affine::translate((anchor_x, anchor_y))
                    * kurbo::Affine::scale_non_uniform(sx, sy)
                    * kurbo::Affine::translate((-new_bbox.center().x, -new_bbox.y1));
                modified = t * modified;
            }
        }

        l.glyph_path = modified;
        l.transform = kurbo::Affine::IDENTITY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        Rect::new(x0, y0, x1, y1).to_path(0.1)
    }

    #[test]
    fn uniform_thicken_grows_every_side() {
        let p = offset_path(&rect_path(0.0, 0.0, 100.0, 50.0), &|_| 5.0);
        let b = p.bounding_box();
        assert!((b.x0 + 5.0).abs() < 1e-3 && (b.y0 + 5.0).abs() < 1e-3, "{b:?}");
        assert!((b.x1 - 105.0).abs() < 1e-3 && (b.y1 - 55.0).abs() < 1e-3, "{b:?}");
    }

    #[test]
    fn uniform_thin_shrinks_and_works_for_either_winding() {
        for rev in [false, true] {
            let mut path = rect_path(0.0, 0.0, 100.0, 50.0);
            if rev {
                path = path.reverse_subpaths();
            }
            let b = offset_path(&path, &|_| -5.0).bounding_box();
            assert!((b.x0 - 5.0).abs() < 1e-3 && (b.y1 - 45.0).abs() < 1e-3, "rev={rev} {b:?}");
        }
    }

    #[test]
    fn exact_offset_keeps_straight_edges_straight_and_moves_them_evenly() {
        let thin = uniform_offset(&rect_path(0.0, 0.0, 100.0, 50.0), -4.0).bounding_box();
        assert!((thin.x0 - 4.0).abs() < 1e-2 && (thin.x1 - 96.0).abs() < 1e-2, "{thin:?}");
        assert!((thin.y0 - 4.0).abs() < 1e-2 && (thin.y1 - 46.0).abs() < 1e-2, "{thin:?}");
        let thick = uniform_offset(&rect_path(0.0, 0.0, 100.0, 50.0), 4.0).bounding_box();
        assert!((thick.x0 + 4.0).abs() < 1e-2 && (thick.y1 - 54.0).abs() < 1e-2, "{thick:?}");
    }

    #[test]
    fn exact_offset_moves_holes_the_right_way() {
        let mut p = rect_path(0.0, 0.0, 100.0, 100.0);
        p.extend(rect_path(40.0, 40.0, 60.0, 60.0).reverse_subpaths().iter());
        let thick = uniform_offset(&p, 4.0);
        assert!(thick.contains(Point::new(38.0, 50.0)) && !thick.contains(Point::new(50.0, 50.0)));
        let thin = uniform_offset(&p, -4.0);
        assert!(!thin.contains(Point::new(38.0, 50.0)) && !thin.contains(Point::new(50.0, 50.0)));
        assert!(thin.contains(Point::new(20.0, 50.0)));
    }

    #[test]
    fn directional_weight_changes_horizontal_and_vertical_strokes_differently() {
        let b = offset_xy(&rect_path(0.0, 0.0, 100.0, 50.0), 2.0, 8.0, false).bounding_box();
        assert!((b.x0 + 2.0).abs() < 0.05 && (b.x1 - 102.0).abs() < 0.05, "{b:?}");
        assert!((b.y0 + 8.0).abs() < 0.05 && (b.y1 - 58.0).abs() < 0.05, "{b:?}");
    }

    #[test]
    fn exact_offset_removes_thin_strokes() {
        let out = uniform_offset(&rect_path(0.0, 0.0, 100.0, 6.0), -10.0);
        assert!(out.elements().is_empty() || out.bounding_box().area() < 1e-6);
    }

    #[test]
    fn holes_move_the_opposite_way() {
        // Outer square with a square hole (opposite winding): thickening shrinks the hole.
        let mut p = rect_path(0.0, 0.0, 100.0, 100.0);
        let hole = rect_path(40.0, 40.0, 60.0, 60.0).reverse_subpaths();
        p.extend(hole.iter());
        let out = offset_path(&p, &|_| 4.0);
        assert!(!out.contains(Point::new(50.0, 50.0)), "centre is still a hole");
        assert!(out.contains(Point::new(38.0, 50.0)), "hole edge grew inward");
        assert!(!out.contains(Point::new(50.0, 50.0)));
        let b = out.bounding_box();
        assert!((b.x0 + 4.0).abs() < 1e-3);
    }

    #[test]
    fn thinning_past_the_stroke_removes_it_cleanly() {
        let out = offset_path(&rect_path(0.0, 0.0, 100.0, 6.0), &|_| -10.0);
        assert!(out.bounding_box().area() < 1e-6 || out.elements().is_empty(), "{:?}", out.bounding_box());
    }

    #[test]
    fn ramp_makes_the_end_thicker_than_the_start() {
        let p = Params { weight_start: -0.02, weight_end: 0.04, ..Params::default() };
        assert!(offset_at(&p, 1.0) > offset_at(&p, 0.0));
        assert!((offset_at(&p, 0.5) - 0.01 * EM_SIZE).abs() < 1e-9);
        assert!(!is_active(&Params::default()) && is_active(&p));
    }

    #[test]
    fn apply_weight_ramps_across_the_text() {
        let mut l = vec![
            Letter::new('a', rect_path(0.0, 0.0, 20.0, 20.0), kurbo::Affine::IDENTITY),
            Letter::new('b', rect_path(0.0, 0.0, 20.0, 20.0), kurbo::Affine::translate((200.0, 0.0))),
        ];
        let p = Params { weight_start: 0.0, weight_end: 0.05, ..Params::default() };
        apply_weight(&mut l, &p, LayoutDirection::Horizontal);
        let (a, b) = (l[0].placed_path().bounding_box(), l[1].placed_path().bounding_box());
        assert!(b.height() > a.height() + 5.0, "first {} last {}", a.height(), b.height());
        assert_eq!(l[0].transform, kurbo::Affine::IDENTITY);
    }

    #[test]
    fn per_letter_scope_gives_each_letter_its_own_ramp() {
        let mk = |x| Letter::new('a', rect_path(0.0, 0.0, 20.0, 20.0), kurbo::Affine::translate((x, 0.0)));
        let mut l = vec![mk(0.0), mk(200.0)];
        let p = Params { weight_start: 0.0, weight_end: 0.05, weight_per_letter: true, ..Params::default() };
        apply_weight(&mut l, &p, LayoutDirection::Horizontal);
        let (a, b) = (l[0].placed_path().bounding_box(), l[1].placed_path().bounding_box());
        assert!((a.width() - b.width()).abs() < 1e-6);
        assert!(a.width() > 20.0 + 5.0);
    }

    #[test]
    fn keep_vertical_and_horizontal_dimensions_constrain_bounds() {
        let mk = || Letter::new('a', rect_path(0.0, 0.0, 40.0, 60.0), kurbo::Affine::IDENTITY);
        let mut l_keep_v = vec![mk()];
        let p_v = Params {
            weight: 0.03,
            weight_keep_v: true,
            ..Params::default()
        };
        apply_weight(&mut l_keep_v, &p_v, LayoutDirection::Horizontal);
        let bv = l_keep_v[0].placed_path().bounding_box();
        assert!((bv.height() - 60.0).abs() < 1e-3, "height should stay 60: {}", bv.height());
        assert!(bv.width() > 40.0, "width should grow: {}", bv.width());

        let mut l_keep_h = vec![mk()];
        let p_h = Params {
            weight: 0.03,
            weight_keep_h: true,
            ..Params::default()
        };
        apply_weight(&mut l_keep_h, &p_h, LayoutDirection::Horizontal);
        let bh = l_keep_h[0].placed_path().bounding_box();
        assert!((bh.width() - 40.0).abs() < 1e-3, "width should stay 40: {}", bh.width());
        assert!(bh.height() > 60.0, "height should grow: {}", bh.height());
    }
}
