//! Guide-path warp: bends glyph outlines so the text baseline follows a guide curve.
//! Pure: letters + guide polyline in -> letters out.

use kurbo::{flatten, BezPath, PathEl, Point, Shape, Vec2};

use super::text_to_paths::EM_SIZE;
use crate::model::letter::Letter;
use crate::model::params::Params;
use crate::model::scene::LayoutDirection;

/// Longest straight segment kept before warping; longer ones are subdivided so they can bend.
const MAX_SEGMENT: f64 = 1.5;
const FLATTEN_TOLERANCE: f64 = 0.04;

/// A polyline with cumulative arc lengths, sampled by distance.
pub struct Guide {
    pts: Vec<Point>,
    cum: Vec<f64>,
    /// Unit tangent at each vertex (average of the adjoining segments), interpolated when
    /// sampling so strongly bent guides do not show a facet per segment.
    tangents: Vec<Vec2>,
}

impl Guide {
    /// `None` if there are fewer than two distinct points.
    pub fn new(points: &[Point]) -> Option<Self> {
        let mut pts: Vec<Point> = Vec::with_capacity(points.len());
        for &p in points {
            if pts.last().is_none_or(|q| q.distance(p) > 1e-9) {
                pts.push(p);
            }
        }
        if pts.len() < 2 {
            return None;
        }
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            cum.push(cum[cum.len() - 1] + w[0].distance(w[1]));
        }
        let n = pts.len();
        let seg_dir = |i: usize| {
            let d = pts[i + 1] - pts[i];
            d / d.hypot().max(1e-12)
        };
        let tangents = (0..n)
            .map(|i| {
                let d = match i {
                    0 => seg_dir(0),
                    i if i == n - 1 => seg_dir(n - 2),
                    i => seg_dir(i - 1) + seg_dir(i),
                };
                let len = d.hypot();
                if len > 1e-9 { d / len } else { seg_dir(i.min(n - 2)) }
            })
            .collect();
        Some(Self { pts, cum, tangents })
    }

    pub fn length(&self) -> f64 {
        self.cum[self.cum.len() - 1]
    }

    /// Point and unit tangent at arc length `s` (clamped to the guide).
    pub fn sample(&self, s: f64) -> (Point, Vec2) {
        let s = s.clamp(0.0, self.length());
        let i = match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) | Err(i) => i.clamp(1, self.pts.len() - 1),
        };
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let seg = self.cum[i] - self.cum[i - 1];
        let t = if seg > 0.0 { (s - self.cum[i - 1]) / seg } else { 0.0 };
        let dir = self.tangents[i - 1].lerp(self.tangents[i], t);
        let len = dir.hypot();
        let dir = if len > 1e-9 { dir / len } else { (b - a) / seg.max(1e-12) };
        (a.lerp(b, t), dir)
    }
}

/// Smooth Catmull-Rom curve through `ctrl`, as a dense polyline.
pub fn catmull_rom(ctrl: &[Point], samples_per_seg: usize) -> Vec<Point> {
    if ctrl.len() < 2 {
        return ctrl.to_vec();
    }
    let n = ctrl.len();
    let at = |i: isize| ctrl[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::new();
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
        for k in 0..samples_per_seg {
            let t = k as f64 / samples_per_seg as f64;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push(Point::new(f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y)));
        }
    }
    out.push(ctrl[n - 1]);
    out
}

/// Procedural guide from `bend` / `wave` params across `[u0, u1]` of the flow axis.
/// Horizontal text flows along x on the line y = 0; vertical along y on x = 0.
/// Returns `None` when both are zero (no warp needed).
pub fn procedural_guide(u0: f64, u1: f64, params: &Params, direction: LayoutDirection) -> Option<Vec<Point>> {
    if params.bend.abs() < 1e-6 && params.wave.abs() < 1e-6 {
        return None;
    }
    let len = (u1 - u0).max(1.0);
    let n = 400;
    let pts = (0..=n)
        .map(|i| {
            let t = f64::from(i) / f64::from(n);
            let u = u0 + len * t;
            // Parabolic arc: sagitta = bend * len * 0.5, peaking mid-text.
            let arc = -params.bend * len * 0.5 * 4.0 * t * (1.0 - t);
            let wave = params.wave * EM_SIZE * (std::f64::consts::TAU * params.wave_cycles * t).sin();
            let off = arc + wave;
            match direction {
                LayoutDirection::Horizontal => Point::new(u, off),
                LayoutDirection::Vertical => Point::new(off, u),
            }
        })
        .collect();
    Some(pts)
}

/// Flattens curves to `tolerance` and splits long lines into pieces of at most `max_segment`,
/// so the result can be bent point by point without visible facets. Output has only
/// move / line / close elements.
pub fn flatten_subdivide(path: &BezPath, tolerance: f64, max_segment: f64) -> BezPath {
    let mut flat = Vec::new();
    flatten(path.iter(), tolerance, |el| flat.push(el));
    let mut out = BezPath::new();
    let mut last = Point::ZERO;
    for el in flat {
        match el {
            PathEl::MoveTo(p) => {
                out.move_to(p);
                last = p;
            }
            PathEl::LineTo(p) => {
                let n = (last.distance(p) / max_segment).ceil().max(1.0) as usize;
                for k in 1..=n {
                    out.line_to(last.lerp(p, k as f64 / n as f64));
                }
                last = p;
            }
            PathEl::ClosePath => out.close_path(),
            // flatten only emits Move/Line/Close
            _ => {}
        }
    }
    out
}

fn subdivide_lines(path: &BezPath) -> BezPath {
    flatten_subdivide(path, FLATTEN_TOLERANCE, MAX_SEGMENT)
}
/// Places every letter rigidly along `guide`: shapes are untouched, each letter is moved so its
/// centre sits on the guide (text scaled uniformly to the guide length) and rotated by `lean` times the
/// guide direction there. Offsets from the baseline follow the guide normal.
pub fn place_letters(letters: &mut [Letter], guide: &Guide, direction: LayoutDirection, lean: f64) {
    let Some(bounds) = letters
        .iter()
        .map(|l| l.placed_path().bounding_box())
        .reduce(|a, b| a.union(b))
    else {
        return;
    };
    let (u0, u1) = match direction {
        LayoutDirection::Horizontal => (bounds.x0, bounds.x1),
        LayoutDirection::Vertical => (bounds.y0, bounds.y1),
    };
    let k = guide.length() / (u1 - u0).max(1e-6);
    let lean = lean.clamp(0.0, 1.0);
    for l in letters.iter_mut() {
        let c = l.placed_path().bounding_box().center();
        let (cu, cd) = match direction {
            LayoutDirection::Horizontal => (c.x, c.y),
            LayoutDirection::Vertical => (c.y, -c.x),
        };
        let (q, t) = guide.sample((cu - u0) * k);
        // The whole text is scaled by `k` so letters fill the guide (longer guide = bigger letters).
        let target = q + Vec2::new(-t.y, t.x) * (cd * k);
        let mut theta = t.y.atan2(t.x);
        if direction == LayoutDirection::Vertical {
            theta -= std::f64::consts::FRAC_PI_2;
        }
        l.transform = kurbo::Affine::translate(target.to_vec2())
            * kurbo::Affine::rotate(theta * lean)
            * kurbo::Affine::scale(k)
            * kurbo::Affine::translate(-c.to_vec2())
            * l.transform;
    }
}

/// Warps every letter along `guide`. The flow-axis extent of the letters is stretched to the
/// guide's length; the offset from the baseline (y = 0, or x = 0 when vertical) becomes the
/// distance along the guide's normal. Warped letters get an identity transform.
pub fn warp_letters(letters: &mut [Letter], guide: &Guide, direction: LayoutDirection, lean: f64) {
    let Some(bounds) = letters
        .iter()
        .map(|l| l.placed_path().bounding_box())
        .reduce(|a, b| a.union(b))
    else {
        return;
    };
    let (u0, u1) = match direction {
        LayoutDirection::Horizontal => (bounds.x0, bounds.x1),
        LayoutDirection::Vertical => (bounds.y0, bounds.y1),
    };
    let span = (u1 - u0).max(1e-6);
    let k = guide.length() / span;

    let lean = lean.clamp(0.0, 1.0);
    for l in letters.iter_mut() {
        let placed = subdivide_lines(&l.placed_path());
        // Rigid (upright) placement: the letter keeps its shape and sits on the guide at its
        // own centre. `lean` blends between this and the fully bent mapping.
        let lb = placed.bounding_box();
        let cu = match direction {
            LayoutDirection::Horizontal => lb.center().x,
            LayoutDirection::Vertical => lb.center().y,
        };
        let (cq, _) = guide.sample((cu - u0) * k);
        let mut out = BezPath::new();
        for el in placed.iter() {
            let map = |p: Point| {
                let (u, d) = match direction {
                    LayoutDirection::Horizontal => (p.x, p.y),
                    LayoutDirection::Vertical => (p.y, -p.x),
                };
                let (q, t) = guide.sample((u - u0) * k);
                // y-down normal: tangent rotated +90 degrees (points "below" a rightward path).
                let bent = q + Vec2::new(-t.y, t.x) * d;
                if lean >= 1.0 {
                    return bent;
                }
                let rigid = match direction {
                    LayoutDirection::Horizontal => cq + Vec2::new(u - cu, d),
                    LayoutDirection::Vertical => cq + Vec2::new(-d, u - cu),
                };
                bent.lerp(rigid, 1.0 - lean)
            };
            match el {
                PathEl::MoveTo(p) => out.move_to(map(p)),
                PathEl::LineTo(p) => out.line_to(map(p)),
                PathEl::ClosePath => out.close_path(),
                _ => {}
            }
        }
        l.glyph_path = out;
        l.transform = kurbo::Affine::IDENTITY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect;

    fn square() -> Letter {
        Letter::new(
            'x',
            Rect::new(0.0, -10.0, 10.0, 0.0).to_path(0.1),
            kurbo::Affine::IDENTITY,
        )
    }

    #[test]
    fn guide_samples_by_arc_length() {
        let g = Guide::new(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0)]).unwrap();
        assert!((g.length() - 20.0).abs() < 1e-9);
        let (p, t) = g.sample(15.0);
        assert!((p.x - 10.0).abs() < 1e-9 && (p.y - 5.0).abs() < 1e-9);
        // Tangents are smoothed across the corner: between the diagonal and straight down.
        assert!(t.y > 0.85 && t.y < 1.0 && t.x > 0.0);
        let (_, at_corner) = g.sample(10.0);
        assert!((at_corner.x - at_corner.y).abs() < 1e-9, "diagonal at the corner");
    }

    #[test]
    fn rigid_placement_keeps_letter_shapes() {
        let mut l = vec![square(), {
            let mut s = square();
            s.transform = kurbo::Affine::translate((20.0, 0.0));
            s
        }];
        let before = l[1].placed_path().area().abs();
        let g = Guide::new(&[Point::new(0.0, 0.0), Point::new(30.0, -30.0)]).unwrap();
        place_letters(&mut l, &g, LayoutDirection::Horizontal, 1.0);
        let k = g.length() / 30.0;
        assert!((l[1].placed_path().area().abs() / before - k * k).abs() < 1e-6, "uniform scale only");
        assert!(l[1].placed_path().bounding_box().center().y < l[0].placed_path().bounding_box().center().y - 5.0);
    }

    #[test]
    fn degenerate_guide_is_none() {
        assert!(Guide::new(&[Point::new(1.0, 1.0), Point::new(1.0, 1.0)]).is_none());
    }

    #[test]
    fn straight_horizontal_guide_is_identity_like() {
        let mut l = vec![square()];
        let g = Guide::new(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0)]).unwrap();
        warp_letters(&mut l, &g, LayoutDirection::Horizontal, 1.0);
        let b = l[0].placed_path().bounding_box();
        assert!((b.x0).abs() < 1e-6 && (b.x1 - 10.0).abs() < 1e-6);
        assert!((b.y0 + 10.0).abs() < 1e-6 && b.y1.abs() < 1e-6);
    }

    #[test]
    fn rising_guide_lifts_the_letter() {
        let mut l = vec![square(), {
            let mut s = square();
            s.transform = kurbo::Affine::translate((20.0, 0.0));
            s
        }];
        let g = Guide::new(&[Point::new(0.0, 0.0), Point::new(30.0, -30.0)]).unwrap();
        warp_letters(&mut l, &g, LayoutDirection::Horizontal, 1.0);
        let a = l[0].placed_path().bounding_box();
        let b = l[1].placed_path().bounding_box();
        assert!(b.center().y < a.center().y - 5.0);
    }

    #[test]
    fn procedural_guide_none_when_neutral() {
        assert!(procedural_guide(0.0, 100.0, &Params::default(), LayoutDirection::Horizontal).is_none());
        let p = Params { bend: 0.5, ..Params::default() };
        assert!(procedural_guide(0.0, 100.0, &p, LayoutDirection::Horizontal).is_some());
    }

    #[test]
    fn catmull_rom_passes_through_controls() {
        let c = [Point::new(0.0, 0.0), Point::new(10.0, 5.0), Point::new(20.0, 0.0)];
        let s = catmull_rom(&c, 8);
        assert!(s.contains(&c[1]));
        assert_eq!(*s.last().unwrap(), c[2]);
    }
}

