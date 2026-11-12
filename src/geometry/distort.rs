//! Per-letter distort (Photoshop Free Transform, Ctrl + corner): the four corners of a
//! letter's local bounding box can be moved freely; the outline is mapped bilinearly.
//! Offsets are stored as fractions of the bounding box size so they survive layout changes.

use kurbo::{BezPath, PathEl, Point, Rect, Shape};

use super::warp::flatten_subdivide;

/// Offsets for the corners TL, TR, BR, BL, as fractions of (width, height).
pub type Offsets = [[f64; 2]; 4];

const TOLERANCE: f64 = 0.04;
const MAX_SEGMENT: f64 = 1.5;

pub fn is_zero(o: &Offsets) -> bool {
    o.iter().all(|c| c[0].abs() < 1e-12 && c[1].abs() < 1e-12)
}

/// Corner positions TL, TR, BR, BL of `bbox` moved by `o`.
pub fn corners(bbox: Rect, o: &Offsets) -> [Point; 4] {
    let base = [
        Point::new(bbox.x0, bbox.y0),
        Point::new(bbox.x1, bbox.y0),
        Point::new(bbox.x1, bbox.y1),
        Point::new(bbox.x0, bbox.y1),
    ];
    std::array::from_fn(|i| Point::new(base[i].x + o[i][0] * bbox.width(), base[i].y + o[i][1] * bbox.height()))
}

/// Bilinear point of the quad `q` (TL, TR, BR, BL) at (u, v); (0..1, 0..1) is inside.
pub fn bilinear(q: &[Point; 4], u: f64, v: f64) -> Point {
    let top = q[0].lerp(q[1], u);
    let bottom = q[3].lerp(q[2], u);
    top.lerp(bottom, v)
}

/// Maps `p` (relative to `rect`) onto the quad `q`.
pub fn quad_map(rect: Rect, q: &[Point; 4], p: Point) -> Point {
    let u = (p.x - rect.x0) / rect.width().max(1e-12);
    let v = (p.y - rect.y0) / rect.height().max(1e-12);
    bilinear(q, u, v)
}

/// The outline distorted by `o` (flattened to polylines; unchanged if `o` is zero).
pub fn distort_path(path: &BezPath, o: &Offsets) -> BezPath {
    let bbox = path.bounding_box();
    if is_zero(o) || bbox.width() < 1e-9 || bbox.height() < 1e-9 {
        return path.clone();
    }
    let q = corners(bbox, o);
    let flat = flatten_subdivide(path, TOLERANCE, MAX_SEGMENT);
    let mut out = BezPath::new();
    for el in flat.iter() {
        match el {
            PathEl::MoveTo(p) => out.move_to(quad_map(bbox, &q, p)),
            PathEl::LineTo(p) => out.line_to(quad_map(bbox, &q, p)),
            PathEl::ClosePath => out.close_path(),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> BezPath {
        Rect::new(0.0, 0.0, 100.0, 50.0).to_path(0.1)
    }

    #[test]
    fn zero_offsets_leave_the_path_alone() {
        assert_eq!(distort_path(&square(), &[[0.0; 2]; 4]), square());
    }

    #[test]
    fn moving_one_corner_moves_only_that_corner() {
        let mut o = [[0.0; 2]; 4];
        o[1] = [0.5, -0.4]; // TR: +50, -20
        let b = distort_path(&square(), &o).bounding_box();
        assert!((b.x1 - 150.0).abs() < 1e-6 && (b.y0 + 20.0).abs() < 1e-6);
        assert!(b.x0.abs() < 1e-6 && (b.y1 - 50.0).abs() < 1e-6);
    }

    #[test]
    fn bilinear_hits_the_corners_and_centre() {
        let q = corners(Rect::new(0.0, 0.0, 10.0, 10.0), &[[0.0; 2]; 4]);
        assert_eq!(bilinear(&q, 0.0, 0.0), q[0]);
        assert_eq!(bilinear(&q, 1.0, 1.0), q[2]);
        assert_eq!(bilinear(&q, 0.5, 0.5), Point::new(5.0, 5.0));
    }
}

