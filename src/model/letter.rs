use kurbo::{Affine, BezPath, Point, Shape, Vec2};

use crate::geometry::distort::{self, distort_path, Offsets};
use serde::{Deserialize, Serialize};

/// One glyph of the lettering. `glyph_path` lives in the letter's local space
/// (pen origin at 0,0, y-down); `transform` places it in scene space.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Letter {
    pub ch: char,
    pub glyph_path: BezPath,
    pub transform: Affine,
    /// The `transform` the layout pipeline produced, before any manual move/scale/rotate/skew.
    /// `None` for brush strokes and files saved before this field existed (then `transform`
    /// itself is the base).
    #[serde(default)]
    pub base: Option<Affine>,
    /// Free-transform distort of the glyph's local bounding box corners (TL, TR, BR, BL), as
    /// fractions of its size. Applied before `transform`.
    #[serde(default)]
    pub distort: Offsets,
}

/// A manual edit of a letter, stored independently of the layout so it survives a relayout.
/// It is the scene-space delta `transform * base^-1`, expressed about the letter's own centre:
/// a translation `d` plus a linear part `a` (scale / rotate / skew) applied about the centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ManualDelta {
    pub linear: [f64; 4],
    pub d: Vec2,
    pub distort: Offsets,
}

impl Letter {
    pub fn new(ch: char, glyph_path: BezPath, transform: Affine) -> Self {
        Self {
            ch,
            glyph_path,
            transform,
            base: None,
            distort: [[0.0; 2]; 4],
        }
    }

    /// Outline in the letter's local space with the distort applied.
    pub fn local_path(&self) -> BezPath {
        distort_path(&self.glyph_path, &self.distort)
    }

    /// Path in scene space.
    pub fn placed_path(&self) -> BezPath {
        self.transform * self.local_path()
    }

    /// The four distorted corners (TL, TR, BR, BL) in scene space.
    pub fn scene_corners(&self) -> [Point; 4] {
        distort::corners(self.glyph_path.bounding_box(), &self.distort).map(|p| self.transform * p)
    }

    /// Sets the distort so the corners land on `corners` (scene space, TL, TR, BR, BL).
    #[allow(dead_code)]
    pub fn set_scene_corners(&mut self, corners: [Point; 4]) {
        let bbox = self.glyph_path.bounding_box();
        if self.transform.determinant().abs() < 1e-12 || bbox.width() < 1e-9 || bbox.height() < 1e-9 {
            return;
        }
        let inv = self.transform.inverse();
        let base = distort::corners(bbox, &[[0.0; 2]; 4]);
        for (i, c) in corners.iter().enumerate() {
            let local = inv * *c;
            self.distort[i] = [(local.x - base[i].x) / bbox.width(), (local.y - base[i].y) / bbox.height()];
        }
    }

    fn base_transform(&self) -> Affine {
        self.base.unwrap_or(self.transform)
    }

    fn base_centre(&self) -> Point {
        (self.base_transform() * self.glyph_path.clone()).bounding_box().center()
    }

    /// Marks the current transform as the layout result (call right after layout).
    pub fn commit_base(&mut self) {
        self.base = Some(self.transform);
    }

    /// The manual edit applied on top of the layout, if the letter is still invertible.
    pub fn manual_delta(&self) -> Option<ManualDelta> {
        let base = self.base_transform();
        if base.determinant().abs() < 1e-12 {
            return None;
        }
        let g = self.transform * base.inverse();
        let c = self.base_centre();
        let moved = g * c;
        let k = g.as_coeffs();
        Some(ManualDelta {
            linear: [k[0], k[1], k[2], k[3]],
            d: moved - c,
            distort: self.distort,
        })
    }

    /// Re-applies a manual edit on top of this (freshly laid out) letter. Expects `base` to be set.
    pub fn apply_manual(&mut self, m: ManualDelta) {
        let base = self.base_transform();
        let c = self.base_centre().to_vec2();
        let lin = Affine::new([m.linear[0], m.linear[1], m.linear[2], m.linear[3], 0.0, 0.0]);
        self.distort = m.distort;
        self.transform = Affine::translate(m.d) * Affine::translate(c) * lin * Affine::translate(-c) * base;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect;

    fn letter_at(x: f64) -> Letter {
        let mut l = Letter::new(
            'x',
            Rect::new(0.0, 0.0, 10.0, 20.0).to_path(0.1),
            Affine::translate((x, 0.0)),
        );
        l.commit_base();
        l
    }

    #[test]
    fn untouched_letter_has_identity_delta() {
        let m = letter_at(5.0).manual_delta().unwrap();
        assert_eq!(m.linear, [1.0, 0.0, 0.0, 1.0]);
        assert!(m.d.hypot() < 1e-9);
    }

    #[test]
    fn move_and_scale_survive_a_relayout() {
        let mut l = letter_at(0.0);
        // user moves by (3, 4) then scales 2x about a far-away pivot
        let g = Affine::translate((100.0, 0.0)) * Affine::scale(2.0) * Affine::translate((-100.0, 0.0)) * Affine::translate((3.0, 4.0));
        l.transform = g * l.transform;
        let m = l.manual_delta().unwrap();

        // relayout puts the letter elsewhere (different base)
        let mut fresh = letter_at(50.0);
        fresh.apply_manual(m);
        let b = fresh.placed_path().bounding_box();
        // scaled 2x about its own centre, then offset by the same delta
        assert!((b.width() - 20.0).abs() < 1e-6 && (b.height() - 40.0).abs() < 1e-6);
        let c0 = (Affine::translate((50.0, 0.0)) * fresh.glyph_path.clone()).bounding_box().center();
        assert!((b.center().x - (c0.x + m.d.x)).abs() < 1e-6);
        assert!((b.center().y - (c0.y + m.d.y)).abs() < 1e-6);
    }

    #[test]
    fn pure_translation_is_preserved_exactly() {
        let mut l = letter_at(0.0);
        l.transform = Affine::translate((7.0, -3.0)) * l.transform;
        let m = l.manual_delta().unwrap();
        let mut fresh = letter_at(40.0);
        fresh.apply_manual(m);
        assert_eq!(fresh.transform.translation(), kurbo::Vec2::new(47.0, -3.0));
    }

    #[test]
    fn distort_is_kept_by_manual_delta_and_moves_the_outline() {
        let mut l = letter_at(0.0);
        let mut corners = l.scene_corners();
        corners[1].x += 10.0; // TR
        l.set_scene_corners(corners);
        assert!((l.placed_path().bounding_box().x1 - 20.0).abs() < 1e-6);
        let m = l.manual_delta().unwrap();
        let mut fresh = letter_at(30.0);
        fresh.apply_manual(m);
        assert!((fresh.placed_path().bounding_box().x1 - 50.0).abs() < 1e-6);
        assert_eq!(fresh.scene_corners()[0], Point::new(30.0, 0.0));
    }

    #[test]
    fn legacy_letters_without_base_have_no_edit() {
        let l = Letter::new('x', Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.1), Affine::translate((9.0, 9.0)));
        assert_eq!(l.manual_delta().unwrap().d.hypot(), 0.0);
    }
}
