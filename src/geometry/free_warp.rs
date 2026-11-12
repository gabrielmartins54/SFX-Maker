//! Photoshop-style Warp: a 4x4 Bezier control grid over the text bounds. Offsets are stored
//! as fractions of the bounds size, so the warp follows the text when sliders change the
//! layout. Pure geometry; no UI.

use kurbo::{BezPath, PathEl, Point, Rect, Shape, Vec2};
use serde::{Deserialize, Serialize};

use super::warp::flatten_subdivide;
use crate::model::letter::Letter;

pub const GRID: usize = 4;
pub const POINTS: usize = GRID * GRID;
/// Index of control point (column `i`, row `j`).
pub fn idx(i: usize, j: usize) -> usize {
    j * GRID + i
}

/// Curve detail for warped outlines (scene units): fine enough to look smooth when zoomed in.
const TOLERANCE: f64 = 0.04;
const MAX_SEGMENT: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FreeWarp {
    /// Per control point (row-major), offset from its regular lattice position, as a
    /// fraction of the bounds (width, height).
    pub offsets: [[f64; 2]; POINTS],
}

impl Default for FreeWarp {
    fn default() -> Self {
        Self { offsets: [[0.0; 2]; POINTS] }
    }
}

fn bernstein(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t]
}

impl FreeWarp {
    pub fn is_identity(&self) -> bool {
        self.offsets.iter().all(|o| o[0].abs() < 1e-9 && o[1].abs() < 1e-9)
    }

    /// Generates an organic random warp grid from a seed (similar to Giongo's rollWarpGrid).
    pub fn roll(seed: u64, amount: f64) -> FreeWarp {
        use rand::{RngExt, SeedableRng};
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        let mut fw = FreeWarp::default();
        let scale = amount.clamp(0.0, 1.0) * 0.16;
        for j in 0..GRID {
            for i in 0..GRID {
                let is_corner = (i == 0 || i == GRID - 1) && (j == 0 || j == GRID - 1);
                let mul = if is_corner { 0.25 } else { 1.0 };
                let dx: f64 = rng.random_range(-1.0..=1.0) * scale * mul;
                let dy: f64 = rng.random_range(-1.0..=1.0) * scale * mul;
                fw.offsets[idx(i, j)] = [dx, dy];
            }
        }
        fw
    }

    fn lattice(rect: Rect, i: usize, j: usize) -> Point {
        Point::new(
            rect.x0 + rect.width() * i as f64 / (GRID - 1) as f64,
            rect.y0 + rect.height() * j as f64 / (GRID - 1) as f64,
        )
    }

    /// Actual control point positions in scene space.
    pub fn control_points(&self, rect: Rect) -> [Point; POINTS] {
        std::array::from_fn(|k| {
            let (i, j) = (k % GRID, k / GRID);
            let l = Self::lattice(rect, i, j);
            let o = self.offsets[k];
            Point::new(l.x + o[0] * rect.width(), l.y + o[1] * rect.height())
        })
    }

    /// Moves control point `k` so it sits at `pos`.
    pub fn set_point(&mut self, rect: Rect, k: usize, pos: Point) {
        if k >= POINTS || rect.width() < 1e-9 || rect.height() < 1e-9 {
            return;
        }
        let l = Self::lattice(rect, k % GRID, k / GRID);
        self.offsets[k] = [(pos.x - l.x) / rect.width(), (pos.y - l.y) / rect.height()];
    }

    /// Surface point at normalised coordinates (u, v); (0..1) is inside the original bounds.
    pub fn surface(cp: &[Point; POINTS], u: f64, v: f64) -> Point {
        let (bu, bv) = (bernstein(u), bernstein(v));
        let mut p = Vec2::ZERO;
        for j in 0..GRID {
            for i in 0..GRID {
                p += cp[idx(i, j)].to_vec2() * (bu[i] * bv[j]);
            }
        }
        p.to_point()
    }

    /// Maps a scene point through the warp.
    #[cfg(test)]
    pub fn map(&self, rect: Rect, p: Point) -> Point {
        let cp = self.control_points(rect);
        self.map_with(&cp, rect, p)
    }

    fn map_with(&self, cp: &[Point; POINTS], rect: Rect, p: Point) -> Point {
        let u = (p.x - rect.x0) / rect.width();
        let v = (p.y - rect.y0) / rect.height();
        Self::surface(cp, u, v)
    }
}

/// Union of the placed bounding boxes (the rect the warp grid is laid over).
pub fn letters_rect(letters: &[Letter]) -> Option<Rect> {
    letters
        .iter()
        .map(|l| l.placed_path().bounding_box())
        .reduce(|a, b| a.union(b))
        .filter(|r| r.width() > 1e-6 && r.height() > 1e-6)
}

fn warp_path(path: &BezPath, fw: &FreeWarp, cp: &[Point; POINTS], rect: Rect) -> BezPath {
    let flat = flatten_subdivide(path, TOLERANCE, MAX_SEGMENT);
    let mut out = BezPath::new();
    for el in flat.iter() {
        match el {
            PathEl::MoveTo(p) => out.move_to(fw.map_with(cp, rect, p)),
            PathEl::LineTo(p) => out.line_to(fw.map_with(cp, rect, p)),
            PathEl::ClosePath => out.close_path(),
            _ => {}
        }
    }
    out
}

/// Warps every letter through `fw` over `rect`. Warped letters get an identity transform.
pub fn warp_letters_free(letters: &mut [Letter], fw: &FreeWarp, rect: Rect) {
    if fw.is_identity() || rect.width() < 1e-9 || rect.height() < 1e-9 {
        return;
    }
    let cp = fw.control_points(rect);
    for l in letters.iter_mut() {
        l.glyph_path = warp_path(&l.placed_path(), fw, &cp, rect);
        l.transform = kurbo::Affine::IDENTITY;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpStyle {
    Arc,
    Arch,
    Bulge,
    Flag,
    Rise,
    Squeeze,
}

impl WarpStyle {
    pub const ALL: [WarpStyle; 6] = [Self::Arc, Self::Arch, Self::Bulge, Self::Flag, Self::Rise, Self::Squeeze];

    pub fn label(self) -> &'static str {
        match self {
            Self::Arc => "Arc",
            Self::Arch => "Arch",
            Self::Bulge => "Bulge",
            Self::Flag => "Flag",
            Self::Rise => "Rise",
            Self::Squeeze => "Squeeze",
        }
    }

    /// Photoshop-like warp style with `bend` in -1..=1 (0 = no change).
    pub fn build(self, bend: f64) -> FreeWarp {
        let mut fw = FreeWarp::default();
        // Bernstein control values of a parabola with unit sagitta, per column.
        let arc = [0.0, -4.0 / 3.0, -4.0 / 3.0, 0.0];
        let third = |k: usize| k as f64 / (GRID - 1) as f64;
        for j in 0..GRID {
            for i in 0..GRID {
                let (u, v) = (third(i), third(j));
                let (mut dx, mut dy) = (0.0, 0.0);
                match self {
                    Self::Arc => dy = arc[i] * 0.5,
                    Self::Arch => dy = arc[i] * 0.5 * (1.0 - v),
                    Self::Bulge => {
                        // Interior lines move away from the centre; the outer edge stays put.
                        let inner = |t: f64| if t > 0.0 && t < 1.0 { (t - 0.5) * 2.0 } else { 0.0 };
                        dx = 0.15 * inner(u);
                        dy = 0.30 * inner(v);
                    }
                    Self::Flag => dy = [0.0, 1.0, -1.0, 0.0][i] * 0.25,
                    Self::Rise => dy = -0.5 * u,
                    Self::Squeeze => dx = -(u - 0.5) * 0.7 * (1.0 - v),
                }
                fw.offsets[idx(i, j)] = [dx * bend, dy * bend];
            }
        }
        fw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Affine;

    fn rect() -> Rect {
        Rect::new(0.0, 0.0, 300.0, 100.0)
    }

    fn square_letter(x: f64, y: f64) -> Letter {
        let mut l = Letter::new('x', Rect::new(0.0, 0.0, 20.0, 20.0).to_path(0.1), Affine::translate((x, y)));
        l.commit_base();
        l
    }

    #[test]
    fn zero_offsets_are_the_identity_map() {
        let fw = FreeWarp::default();
        assert!(fw.is_identity());
        for p in [Point::new(0.0, 0.0), Point::new(120.0, 33.0), Point::new(300.0, 100.0), Point::new(-20.0, 140.0)] {
            let q = fw.map(rect(), p);
            assert!((q - p).hypot() < 1e-9, "{p:?} -> {q:?}");
        }
    }

    #[test]
    fn dragging_a_corner_moves_that_corner() {
        let mut fw = FreeWarp::default();
        fw.set_point(rect(), idx(3, 0), Point::new(340.0, -30.0));
        let q = fw.map(rect(), Point::new(300.0, 0.0));
        assert!((q - Point::new(340.0, -30.0)).hypot() < 1e-9);
        // opposite corner untouched
        assert!((fw.map(rect(), Point::new(0.0, 100.0)) - Point::new(0.0, 100.0)).hypot() < 1e-9);
        assert_eq!(fw.control_points(rect())[idx(3, 0)], Point::new(340.0, -30.0));
    }

    #[test]
    fn arc_style_lifts_the_middle_not_the_ends() {
        let fw = WarpStyle::Arc.build(1.0);
        let mid = fw.map(rect(), Point::new(150.0, 50.0));
        let end = fw.map(rect(), Point::new(0.0, 50.0));
        assert!(mid.y < 50.0 - 10.0);
        assert!((end.y - 50.0).abs() < 1e-9);
        assert!(WarpStyle::ALL.iter().all(|s| s.build(0.0).is_identity()));
    }

    #[test]
    fn warp_letters_moves_geometry_and_resets_transform() {
        let mut letters = vec![square_letter(0.0, 40.0), square_letter(280.0, 40.0)];
        let fw = WarpStyle::Rise.build(1.0);
        let r = Rect::new(0.0, 40.0, 300.0, 60.0);
        warp_letters_free(&mut letters, &fw, r);
        assert_eq!(letters[0].transform, Affine::IDENTITY);
        let (a, b) = (letters[0].placed_path().bounding_box(), letters[1].placed_path().bounding_box());
        assert!(b.center().y < a.center().y - 5.0);
    }

    #[test]
    fn identity_warp_keeps_curves_untouched() {
        let mut letters = vec![square_letter(5.0, 5.0)];
        let before = letters[0].clone();
        warp_letters_free(&mut letters, &FreeWarp::default(), rect());
        assert_eq!(letters[0].glyph_path, before.glyph_path);
        assert_eq!(letters[0].transform, before.transform);
    }

    #[test]
    fn old_files_without_field_deserialize() {
        let fw: FreeWarp = serde_json::from_str("{}").unwrap();
        assert!(fw.is_identity());
    }
}
