//! Overlap prevention: after jitter / size effects, neighbouring letters are pushed apart along
//! the flow axis until they no longer overlap. The baseline (pre-effect) gap is respected, so a
//! tight or negative tracking the user chose on purpose is kept.

use kurbo::{flatten, Affine, BezPath, PathEl, Point, Shape};

use super::text_to_paths::EM_SIZE;
use crate::model::letter::Letter;
use crate::model::scene::LayoutDirection;

const ROWS: usize = 48;
/// Smallest gap enforced between letters that would otherwise touch / overlap.
const MIN_GAP_EM: f64 = 0.015;

/// Flattened outline as edges in (flow, cross) coordinates.
fn edges(path: &BezPath, dir: LayoutDirection) -> Vec<[f64; 4]> {
    let uv = |p: Point| match dir {
        LayoutDirection::Horizontal => (p.x, p.y),
        LayoutDirection::Vertical => (p.y, p.x),
    };
    let mut out = Vec::new();
    let (mut start, mut last) = (Point::ZERO, Point::ZERO);
    flatten(path.iter(), 0.2, |el| match el {
        PathEl::MoveTo(p) => {
            start = p;
            last = p;
        }
        PathEl::LineTo(p) => {
            let (a, b) = (uv(last), uv(p));
            out.push([a.0, a.1, b.0, b.1]);
            last = p;
        }
        PathEl::ClosePath => {
            let (a, b) = (uv(last), uv(start));
            out.push([a.0, a.1, b.0, b.1]);
            last = start;
        }
        _ => {}
    });
    out
}

fn extent_at(e: &[[f64; 4]], v: f64) -> Option<(f64, f64)> {
    let mut r: Option<(f64, f64)> = None;
    for &[u0, v0, u1, v1] in e {
        if (v0 <= v) != (v1 <= v) {
            let u = u0 + (v - v0) / (v1 - v0) * (u1 - u0);
            r = Some(r.map_or((u, u), |(lo, hi)| (lo.min(u), hi.max(u))));
        }
    }
    r
}

/// Smallest distance along the flow axis from letter `a`'s trailing edge to letter `b`'s leading
/// edge, over the cross-axis range both occupy. Negative = overlapping. `None` if they never
/// share a row.
pub fn flow_gap(a: &BezPath, b: &BezPath, dir: LayoutDirection) -> Option<f64> {
    let (ea, eb) = (edges(a, dir), edges(b, dir));
    let cross = |p: &BezPath| {
        let r = p.bounding_box();
        match dir {
            LayoutDirection::Horizontal => (r.y0, r.y1),
            LayoutDirection::Vertical => (r.x0, r.x1),
        }
    };
    let ((a0, a1), (b0, b1)) = (cross(a), cross(b));
    let (lo, hi) = (a0.max(b0), a1.min(b1));
    if hi <= lo {
        return None;
    }
    let mut gap: Option<f64> = None;
    for i in 0..=ROWS {
        let v = lo + (hi - lo) * (i as f64 + 0.5) / (ROWS as f64 + 1.0);
        if let (Some((_, a_hi)), Some((b_lo, _))) = (extent_at(&ea, v), extent_at(&eb, v)) {
            let g = b_lo - a_hi;
            gap = Some(gap.map_or(g, |x| x.min(g)));
        }
    }
    gap
}

fn flow_centre(l: &Letter, dir: LayoutDirection) -> f64 {
    let c = l.placed_path().bounding_box().center();
    match dir {
        LayoutDirection::Horizontal => c.x,
        LayoutDirection::Vertical => c.y,
    }
}

/// `baseline` = the letters straight after layout (before effects / weight); `letters` = the
/// final shapes. Pushes letters (and everything after them on the same line) along the flow axis.
pub fn separate_letters(baseline: &[Letter], letters: &mut [Letter], dir: LayoutDirection) {
    if baseline.len() != letters.len() {
        return;
    }
    let mut carry = 0.0;
    let mut prev_path: Option<BezPath> = None;
    for i in 0..letters.len() {
        let new_line = i > 0
            && flow_centre(&baseline[i], dir) < flow_centre(&baseline[i - 1], dir) - 0.25 * EM_SIZE;
        if new_line {
            carry = 0.0;
            prev_path = None;
        }
        let shift = |d: f64| match dir {
            LayoutDirection::Horizontal => Affine::translate((d, 0.0)),
            LayoutDirection::Vertical => Affine::translate((0.0, d)),
        };
        if carry != 0.0 {
            letters[i].transform = shift(carry) * letters[i].transform;
        }
        let cur = letters[i].placed_path();
        if let Some(prev) = &prev_path {
            let g0 = flow_gap(&baseline[i - 1].placed_path(), &baseline[i].placed_path(), dir);
            let g1 = flow_gap(prev, &cur, dir);
            if let (Some(g0), Some(g1)) = (g0, g1) {
                let target = g0.min(MIN_GAP_EM * EM_SIZE);
                if g1 < target {
                    let push = target - g1;
                    carry += push;
                    letters[i].transform = shift(push) * letters[i].transform;
                }
            }
        }
        prev_path = Some(letters[i].placed_path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect;

    fn sq(x: f64) -> Letter {
        Letter::new('a', Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1), Affine::translate((x, 0.0)))
    }

    #[test]
    fn gap_of_two_boxes() {
        let g = flow_gap(&sq(0.0).placed_path(), &sq(15.0).placed_path(), LayoutDirection::Horizontal).unwrap();
        assert!((g - 5.0).abs() < 1e-6);
        let o = flow_gap(&sq(0.0).placed_path(), &sq(7.0).placed_path(), LayoutDirection::Horizontal).unwrap();
        assert!((o + 3.0).abs() < 1e-6);
    }

    #[test]
    fn overlap_created_by_effects_is_removed_and_followers_move_along() {
        let base = vec![sq(0.0), sq(20.0), sq(40.0)];
        let mut l = vec![sq(0.0), sq(5.0), sq(40.0)]; // second letter drifted onto the first
        separate_letters(&base, &mut l, LayoutDirection::Horizontal);
        let g = flow_gap(&l[0].placed_path(), &l[1].placed_path(), LayoutDirection::Horizontal).unwrap();
        assert!(g >= MIN_GAP_EM * EM_SIZE - 1e-6);
        assert!(l[2].placed_path().bounding_box().x0 > 40.0, "later letters shift too");
    }

    #[test]
    fn deliberate_overlap_in_the_baseline_is_kept() {
        let base = vec![sq(0.0), sq(7.0)];
        let mut l = vec![sq(0.0), sq(7.0)];
        separate_letters(&base, &mut l, LayoutDirection::Horizontal);
        assert_eq!(l[1].transform, Affine::translate((7.0, 0.0)));
    }
}
