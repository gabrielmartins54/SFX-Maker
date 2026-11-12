//! tiny-skia scene renderer. Used for both the on-screen canvas and PNG export
//! so the two always match: callers only differ in the `view` transform.

use kurbo::{Affine, BezPath, PathEl};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::model::scene::Scene;

#[derive(Clone, Copy, Debug)]
pub enum Background {
    Transparent,
    Solid([u8; 4]),
}

pub fn to_skia_transform(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32)
}

/// Converts a kurbo path to a tiny-skia path. `None` for empty/degenerate paths.
pub fn to_skia_path(path: &BezPath) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let f = |p: kurbo::Point| (p.x as f32, p.y as f32);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let (x, y) = f(p);
                pb.move_to(x, y);
            }
            PathEl::LineTo(p) => {
                let (x, y) = f(p);
                pb.line_to(x, y);
            }
            PathEl::QuadTo(a, b) => {
                let (x1, y1) = f(a);
                let (x, y) = f(b);
                pb.quad_to(x1, y1, x, y);
            }
            PathEl::CurveTo(a, b, c) => {
                let (x1, y1) = f(a);
                let (x2, y2) = f(b);
                let (x, y) = f(c);
                pb.cubic_to(x1, y1, x2, y2, x, y);
            }
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

/// Renders `scene` into a fresh `width` x `height` pixmap.
/// `view` maps scene space to pixel space. Returns `None` if the size is invalid.
pub fn render_scene(
    scene: &Scene,
    view: Affine,
    width: u32,
    height: u32,
    background: Background,
) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    match background {
        Background::Transparent => {}
        Background::Solid([r, g, b, a]) => {
            pixmap.fill(tiny_skia::Color::from_rgba8(r, g, b, a));
        }
    }

    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    paint.anti_alias = true;

    for letter in &scene.letters {
        if let Some(path) = to_skia_path(&letter.local_path()) {
            let t = to_skia_transform(view * letter.transform);
            pixmap.fill_path(&path, &paint, FillRule::Winding, t, None);
        }
    }
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::text_to_paths::{text_to_paths, FontData};
    use crate::model::scene::LayoutDirection;

    #[test]
    fn renders_some_ink() {
        let bytes = include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec();
        let font = FontData::new("Bangers", bytes, 0).unwrap();
        let scene = Scene {
            letters: text_to_paths(&font, "A", LayoutDirection::Horizontal).unwrap(),
            ..Scene::default()
        };
        let view = Affine::translate((20.0, 200.0));
        let pm = render_scene(&scene, view, 300, 300, Background::Solid([255, 255, 255, 255])).unwrap();
        assert!(pm.data().chunks(4).any(|p| p[0] < 128));
    }

    #[test]
    fn zero_size_is_none() {
        assert!(render_scene(&Scene::default(), Affine::IDENTITY, 0, 10, Background::Transparent).is_none());
    }
}
