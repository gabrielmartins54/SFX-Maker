use std::path::Path;

use anyhow::{Context, Result, anyhow};
use kurbo::Affine;
use tiny_skia::Pixmap;

use super::{PAD, padded_bounds};
use crate::model::scene::Scene;
use crate::render::raster::{Background, render_scene};

/// Renders the whole scene so its longest side is `max_side` pixels.
pub fn render_export(scene: &Scene, max_side: u32, transparent: bool) -> Result<Pixmap> {
    let b = scene.bounds().ok_or_else(|| anyhow!("Nothing to export"))?;
    let b = padded_bounds(b, PAD);
    let max_side = max_side.clamp(16, 16384);
    let k = f64::from(max_side) / b.width().max(b.height());
    let w = (b.width() * k).round().max(1.0) as u32;
    let h = (b.height() * k).round().max(1.0) as u32;
    let view = Affine::scale(k) * Affine::translate((-b.x0, -b.y0));
    let bg = if transparent { Background::Transparent } else { Background::Solid([255, 255, 255, 255]) };
    render_scene(scene, view, w, h, bg).ok_or_else(|| anyhow!("Invalid export size {w}x{h}"))
}

pub fn export_png(scene: &Scene, path: &Path, max_side: u32, transparent: bool) -> Result<()> {
    let pm = render_export(scene, max_side, transparent)?;
    pm.save_png(path).with_context(|| format!("Writing {}", path.display()))
}

/// Premultiplied pixmap -> straight (non-premultiplied) RGBA bytes, e.g. for the clipboard.
pub fn pixmap_to_rgba(pm: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.data().as_chunks::<4>().0 {
        let a = u32::from(p[3]);
        if a == 0 || a == 255 {
            out.extend_from_slice(&p[..]);
        } else {
            let un = |c: u8| ((u32::from(c) * 255 + a / 2) / a).min(255) as u8;
            out.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2]), p[3]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::text_to_paths::{FontData, text_to_paths};
    use crate::model::scene::LayoutDirection;

    fn scene() -> Scene {
        let bytes = include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec();
        let font = FontData::new("Bangers", bytes, 0).unwrap();
        Scene { letters: text_to_paths(&font, "BOOM", LayoutDirection::Horizontal).unwrap(), ..Scene::default() }
    }

    #[test]
    fn export_size_and_alpha() {
        let pm = render_export(&scene(), 512, true).unwrap();
        assert_eq!(pm.width().max(pm.height()), 512);
        // transparent corner, some ink
        assert_eq!(pm.data()[3], 0);
        assert!(pm.data().chunks(4).any(|p| p[3] == 255));
    }

    #[test]
    fn opaque_background_is_white() {
        let pm = render_export(&scene(), 256, false).unwrap();
        assert_eq!(&pm.data()[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn rgba_roundtrip_of_opaque_and_transparent() {
        let pm = render_export(&scene(), 64, true).unwrap();
        let rgba = pixmap_to_rgba(&pm);
        assert_eq!(rgba.len(), pm.data().len());
        assert_eq!(rgba[3], 0);
        assert!(rgba.chunks(4).any(|p| p[3] == 255 && p[0] == 0));
    }

    #[test]
    fn empty_scene_errors() {
        assert!(render_export(&Scene::default(), 256, true).is_err());
    }
}
