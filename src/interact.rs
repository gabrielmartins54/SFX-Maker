//! Pure selection helpers: hit testing and group transforms. No UI dependencies.

use std::collections::BTreeSet;

use kurbo::{Affine, Point, Rect, Shape, Vec2};

use crate::model::scene::Scene;

/// Topmost letter under `pt` (scene space). Prefers exact glyph hits; falls back to the
/// smallest bounding box containing the point so thin or hollow letters stay clickable.
pub fn hit_letter(scene: &Scene, pt: Point) -> Option<usize> {
    for (i, l) in scene.letters.iter().enumerate().rev() {
        if l.transform.determinant().abs() < 1e-12 {
            continue;
        }
        if l.local_path().contains(l.transform.inverse() * pt) {
            return Some(i);
        }
    }
    scene
        .letters
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let b = l.placed_path().bounding_box();
            b.contains(pt).then_some((i, b.area()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Union of placed bounding boxes of the selected letters.
pub fn selection_bounds(scene: &Scene, sel: &BTreeSet<usize>) -> Option<Rect> {
    sel.iter()
        .filter_map(|&i| scene.letters.get(i))
        .map(|l| l.placed_path().bounding_box())
        .reduce(|a, b| a.union(b))
}

pub fn scale_about(pivot: Point, factor: f64) -> Affine {
    Affine::translate(pivot.to_vec2()) * Affine::scale(factor) * Affine::translate(-pivot.to_vec2())
}

pub fn rotate_about(pivot: Point, radians: f64) -> Affine {
    Affine::translate(pivot.to_vec2()) * Affine::rotate(radians) * Affine::translate(-pivot.to_vec2())
}

pub fn translate(d: Vec2) -> Affine {
    Affine::translate(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::letter::Letter;
    use kurbo::BezPath;

    fn square_letter(x: f64) -> Letter {
        Letter::new('x', Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1), Affine::translate((x, 0.0)))
    }

    fn scene() -> Scene {
        Scene { letters: vec![square_letter(0.0), square_letter(20.0)], ..Scene::default() }
    }

    #[test]
    fn hits_correct_letter() {
        let s = scene();
        assert_eq!(hit_letter(&s, Point::new(25.0, 5.0)), Some(1));
        assert_eq!(hit_letter(&s, Point::new(5.0, 5.0)), Some(0));
        assert_eq!(hit_letter(&s, Point::new(15.0, 5.0)), None);
    }

    #[test]
    fn hit_respects_transform() {
        let mut s = scene();
        s.letters[0].transform = Affine::translate((100.0, 100.0));
        assert_eq!(hit_letter(&s, Point::new(5.0, 5.0)), None);
        assert_eq!(hit_letter(&s, Point::new(105.0, 105.0)), Some(0));
    }

    #[test]
    fn bounds_union() {
        let s = scene();
        let sel: BTreeSet<usize> = [0, 1].into_iter().collect();
        let b = selection_bounds(&s, &sel).unwrap();
        assert!((b.x1 - 30.0).abs() < 1e-9 && b.x0.abs() < 1e-9);
        assert!(selection_bounds(&s, &BTreeSet::new()).is_none());
    }

    #[test]
    fn scale_keeps_pivot() {
        let p = Point::new(3.0, 4.0);
        let q = scale_about(p, 2.0) * p;
        assert!((q - p).hypot() < 1e-9);
        let r = rotate_about(p, 1.0) * p;
        assert!((r - p).hypot() < 1e-9);
    }

    #[test]
    fn empty_path_not_hit() {
        let s = Scene { letters: vec![Letter::new(' ', BezPath::new(), Affine::IDENTITY)], ..Scene::default() };
        assert_eq!(hit_letter(&s, Point::new(0.0, 0.0)), None);
    }
}
