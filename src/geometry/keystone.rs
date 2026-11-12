//! Keystone (perspective-like) distortion of the whole text block.
//! `keystone_v` narrows the top (+) or bottom (-); `keystone_h` makes the right (+) or left (-)
//! side taller. Pure: letters in -> letters out. Letters become polylines with an identity transform.

use kurbo::{BezPath, PathEl, Point, Shape};

use super::warp::flatten_subdivide;
use crate::model::letter::Letter;
use crate::model::params::Params;

/// How much the edge shrinks / grows at |keystone| = 1.
const STRENGTH: f64 = 0.4;

pub fn apply_keystone(letters: &mut [Letter], params: &Params) {
    if params.keystone_v.abs() < 1e-6 && params.keystone_h.abs() < 1e-6 {
        return;
    }
    let Some(bounds) = letters
        .iter()
        .map(|l| l.placed_path().bounding_box())
        .reduce(|a, b| a.union(b))
    else {
        return;
    };
    let c = bounds.center();
    let (hw, hh) = ((bounds.width() / 2.0).max(1e-6), (bounds.height() / 2.0).max(1e-6));
    let (kv, kh) = (params.keystone_v, params.keystone_h);
    let map = |p: Point| {
        let (nx, ny) = ((p.x - c.x) / hw, (p.y - c.y) / hh);
        let sx = 1.0 + STRENGTH * kv * ny; // top (ny = -1) narrower for kv > 0
        let sy = 1.0 + STRENGTH * kh * nx; // right (nx = +1) taller for kh > 0
        Point::new(c.x + (p.x - c.x) * sx, c.y + (p.y - c.y) * sy)
    };
    for l in letters.iter_mut() {
        let flat = flatten_subdivide(&l.placed_path(), 0.04, 4.0);
        let mut out = BezPath::new();
        for el in flat.iter() {
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
    use kurbo::{Affine, Rect};

    fn row() -> Vec<Letter> {
        (0..3)
            .map(|i| {
                Letter::new(
                    'x',
                    Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
                    Affine::translate((f64::from(i) * 20.0, 0.0)),
                )
            })
            .collect()
    }

    #[test]
    fn neutral_changes_nothing() {
        let mut l = row();
        apply_keystone(&mut l, &Params::default());
        assert_eq!(l[0].transform, Affine::translate((0.0, 0.0)));
    }

    #[test]
    fn horizontal_keystone_makes_right_side_taller() {
        let mut l = row();
        apply_keystone(&mut l, &Params { keystone_h: 1.0, ..Params::default() });
        let first = l[0].placed_path().bounding_box().height();
        let last = l[2].placed_path().bounding_box().height();
        assert!(last > first * 1.3);
    }

    #[test]
    fn vertical_keystone_narrows_the_top() {
        let mut l = vec![
            Letter::new('a', Rect::new(0.0, 0.0, 40.0, 4.0).to_path(0.1), Affine::IDENTITY),
            Letter::new('b', Rect::new(0.0, 36.0, 40.0, 40.0).to_path(0.1), Affine::IDENTITY),
        ];
        apply_keystone(&mut l, &Params { keystone_v: 1.0, ..Params::default() });
        assert!(l[0].placed_path().bounding_box().width() < l[1].placed_path().bounding_box().width());
    }
}
