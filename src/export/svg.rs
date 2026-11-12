use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use super::{PAD, padded_bounds};
use crate::model::scene::Scene;

/// One `<path>` per letter, in scene space, so the file stays editable in Illustrator/Inkscape.
pub fn scene_to_svg(scene: &Scene, background: Option<&str>) -> Result<String> {
    let b = scene.bounds().ok_or_else(|| anyhow!("Nothing to export"))?;
    let b = padded_bounds(b, PAD);
    let mut s = String::new();
    writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{:.3} {:.3} {:.3} {:.3}" width="{:.0}" height="{:.0}">"#,
        b.x0,
        b.y0,
        b.width(),
        b.height(),
        b.width(),
        b.height()
    )?;
    if let Some(bg) = background {
        writeln!(
            s,
            r#"  <rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="{bg}"/>"#,
            b.x0,
            b.y0,
            b.width(),
            b.height()
        )?;
    }
    writeln!(s, r##"  <g fill="#000000" fill-rule="nonzero">"##)?;
    for (i, l) in scene.letters.iter().enumerate() {
        let p = l.placed_path();
        if p.elements().is_empty() {
            continue;
        }
        writeln!(s, r#"    <path id="letter-{i}" d="{}"/>"#, p.to_svg())?;
    }
    writeln!(s, "  </g>\n</svg>")?;
    Ok(s)
}

pub fn export_svg(scene: &Scene, path: &Path, white_background: bool) -> Result<()> {
    let svg = scene_to_svg(scene, white_background.then_some("#ffffff"))?;
    std::fs::write(path, svg).with_context(|| format!("Writing {}", path.display()))
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
    fn svg_has_one_path_per_letter() {
        let svg = scene_to_svg(&scene(), None).unwrap();
        assert!(svg.contains("<svg"));
        assert_eq!(svg.matches("<path ").count(), 4);
        assert!(!svg.contains("<rect"));
    }

    #[test]
    fn svg_background_rect() {
        assert!(scene_to_svg(&scene(), Some("#ffffff")).unwrap().contains("<rect"));
    }

    #[test]
    fn empty_scene_errors() {
        assert!(scene_to_svg(&Scene::default(), None).is_err());
    }
}
