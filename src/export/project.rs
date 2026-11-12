//! Project files: the whole `Scene` as JSON (letters included, so no font is needed to reopen).

use std::path::Path;

use anyhow::{Context, Result};

use crate::model::scene::Scene;

pub fn scene_to_json(scene: &Scene) -> Result<String> {
    serde_json::to_string(scene).context("Serialising project")
}

pub fn scene_from_json(json: &str) -> Result<Scene> {
    let mut s: Scene = serde_json::from_str(json).context("Not a valid SFX project file")?;
    s.params = s.params.clamped();
    // Older files had no brush/text split: treat everything as text.
    s.text_letter_count = s.text_letter_count.min(s.letters.len());
    Ok(s)
}

pub fn save_project(scene: &Scene, path: &Path) -> Result<()> {
    std::fs::write(path, scene_to_json(scene)?).with_context(|| format!("Writing {}", path.display()))
}

pub fn load_project(path: &Path) -> Result<Scene> {
    let json = std::fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    scene_from_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::text_to_paths::{text_to_paths, FontData};
    use crate::model::scene::LayoutDirection;

    #[test]
    fn roundtrip_preserves_letters_and_params() {
        let bytes = include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec();
        let font = FontData::new("Bangers", bytes, 0).unwrap();
        let mut s = Scene { letters: text_to_paths(&font, "AB", LayoutDirection::Horizontal).unwrap(), ..Scene::default() };
        s.params.bend = 0.4;
        s.guide = Some(vec![kurbo::Point::new(0.0, 0.0), kurbo::Point::new(5.0, 1.0)]);
        let back = scene_from_json(&scene_to_json(&s).unwrap()).unwrap();
        assert_eq!(back.letters.len(), 2);
        assert_eq!(back.letters[0].glyph_path, s.letters[0].glyph_path);
        assert_eq!(back.params, s.params);
        assert_eq!(back.guide, s.guide);
        assert_eq!(back.text, s.text);
    }

    #[test]
    fn garbage_is_error() {
        assert!(scene_from_json("{nope").is_err());
    }
}

