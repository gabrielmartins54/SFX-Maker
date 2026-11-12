//! Batch export: everything in one ZIP (transparent PNG, white PNG, SVG, project JSON).

use std::io::{Seek, Write};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use zip::write::SimpleFileOptions;

use super::png::render_export;
use super::project::scene_to_json;
use super::svg::scene_to_svg;
use crate::model::scene::Scene;

/// Writes the bundle to any seekable writer (testable without touching disk).
pub fn write_zip<W: Write + Seek>(scene: &Scene, max_side: u32, out: W) -> Result<()> {
    let mut zip = zip::ZipWriter::new(out);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut add = |name: &str, bytes: &[u8]| -> Result<()> {
        zip.start_file(name, opts).with_context(|| format!("Adding {name}"))?;
        zip.write_all(bytes).with_context(|| format!("Writing {name}"))
    };

    for (name, transparent) in [("sfx.png", true), ("sfx_white.png", false)] {
        let pm = render_export(scene, max_side, transparent)?;
        let png = pm.encode_png().map_err(|e| anyhow!("PNG encoding failed: {e}"))?;
        add(name, &png)?;
    }
    add("sfx.svg", scene_to_svg(scene, None)?.as_bytes())?;
    add("sfx_white.svg", scene_to_svg(scene, Some("#ffffff"))?.as_bytes())?;
    add("project.sfx.json", scene_to_json(scene)?.as_bytes())?;
    zip.finish().context("Finalising ZIP")?;
    Ok(())
}

pub fn export_zip(scene: &Scene, path: &Path, max_side: u32) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("Creating {}", path.display()))?;
    write_zip(scene, max_side, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::text_to_paths::{text_to_paths, FontData};
    use crate::model::scene::LayoutDirection;
    use std::io::{Cursor, Read};

    fn scene() -> Scene {
        let bytes = include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec();
        let font = FontData::new("Bangers", bytes, 0).unwrap();
        Scene { letters: text_to_paths(&font, "BOOM", LayoutDirection::Horizontal).unwrap(), ..Scene::default() }
    }

    #[test]
    fn zip_contains_all_files() {
        let mut buf = Cursor::new(Vec::new());
        write_zip(&scene(), 128, &mut buf).unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(buf.into_inner())).unwrap();
        let mut names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["project.sfx.json", "sfx.png", "sfx.svg", "sfx_white.png", "sfx_white.svg"]);
        let mut png = Vec::new();
        z.by_name("sfx.png").unwrap().read_to_end(&mut png).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn empty_scene_errors() {
        assert!(write_zip(&Scene::default(), 128, Cursor::new(Vec::new())).is_err());
    }
}
