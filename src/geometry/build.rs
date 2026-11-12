//! The scene-build step: text + params -> final letters (layout, effects, guide warp).
//! UI code calls this instead of chaining the stages itself.

use super::effects::apply_effects;
use super::weight::apply_weight;
use super::keystone::apply_keystone;
use super::free_warp::{letters_rect, warp_letters_free};
use super::text_to_paths::{text_to_paths_with, FontData, FontError};
use super::spacing::separate_letters;
use super::warp::{catmull_rom, place_letters, procedural_guide, warp_letters, Guide};
use crate::model::letter::Letter;
use crate::model::scene::Scene;

use kurbo::Shape;

/// Result of the build step.
pub struct Built {
    pub letters: Vec<Letter>,
    /// Bounds the Photoshop-style warp grid is laid over (text after layout, effects and guide).
    pub warp_rect: Option<kurbo::Rect>,
}

/// Builds the text letters for `scene` (ignores `scene.letters`).
pub fn build_text_letters(font: &FontData, scene: &Scene) -> Result<Vec<Letter>, FontError> {
    build_text(font, scene).map(|b| b.letters)
}

/// Layout -> effects -> guide warp -> free warp. Every letter's `base` is set to its result.
pub fn build_text(font: &FontData, scene: &Scene) -> Result<Built, FontError> {
    let dir = scene.direction;
    let mut letters = text_to_paths_with(font, &scene.text, dir, &scene.params)?;
    let baseline = letters.clone();
    apply_effects(&mut letters, &scene.params, scene.seed, dir);
    apply_weight(&mut letters, &scene.params, dir);
    if scene.params.no_overlap {
        separate_letters(&baseline, &mut letters, dir);
    }

    let guide_pts = match &scene.guide {
        Some(ctrl) if ctrl.len() >= 2 => Some(catmull_rom(ctrl, 16)),
        _ => {
            let bounds = letters
                .iter()
                .map(|l| l.placed_path().bounding_box())
                .reduce(|a, b| a.union(b));
            bounds.and_then(|b| {
                let (u0, u1) = match dir {
                    crate::model::scene::LayoutDirection::Horizontal => (b.x0, b.x1),
                    crate::model::scene::LayoutDirection::Vertical => (b.y0, b.y1),
                };
                procedural_guide(u0, u1, &scene.params, dir)
            })
        }
    };
    if let Some(g) = guide_pts.as_deref().and_then(Guide::new) {
        if scene.params.bend_glyphs {
            warp_letters(&mut letters, &g, dir, scene.params.lean);
        } else {
            place_letters(&mut letters, &g, dir, scene.params.lean);
        }
    }
    apply_keystone(&mut letters, &scene.params);
    let warp_rect = letters_rect(&letters);
    if let Some(r) = warp_rect {
        warp_letters_free(&mut letters, &scene.free_warp, r);
    }
    for l in &mut letters {
        l.commit_base();
    }
    Ok(Built { letters, warp_rect })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::params::Params;

    fn font() -> FontData {
        FontData::new("Bangers", include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec(), 0).expect("font")
    }

    #[test]
    fn bend_changes_vertical_extent() {
        let flat = Scene::default();
        let bent = Scene { params: Params { bend: 0.6, ..Params::default() }, ..Scene::default() };
        let h = |s: &Scene| {
            let l = build_text_letters(&font(), s).unwrap();
            Scene { letters: l, ..Scene::default() }.bounds().unwrap().height()
        };
        assert!(h(&bent) > h(&flat) * 1.3);
    }

    #[test]
    fn custom_guide_overrides_procedural() {
        let s = Scene {
            guide: Some(vec![
                kurbo::Point::new(0.0, 0.0),
                kurbo::Point::new(300.0, -100.0),
                kurbo::Point::new(600.0, 0.0),
            ]),
            ..Scene::default()
        };
        let l = build_text_letters(&font(), &s).unwrap();
        let b = Scene { letters: l, ..Scene::default() }.bounds().unwrap();
        assert!(b.y0 < -100.0);
    }
}

