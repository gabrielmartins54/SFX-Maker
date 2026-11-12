use kurbo::{Point, Rect, Shape};
use serde::{Deserialize, Serialize};

use super::letter::Letter;
use super::params::Params;
use crate::geometry::free_warp::FreeWarp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutDirection {
    #[default]
    Horizontal,
    Vertical,
}

/// The whole editable document. Pure data: no UI or font handles.
/// Later milestones add guide_path and style.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub text: String,
    pub direction: LayoutDirection,
    pub letters: Vec<Letter>,
    pub seed: u64,
    pub params: Params,
    /// User-drawn guide control points (scene space). Overrides the procedural bend/wave guide.
    #[serde(default)]
    pub guide: Option<Vec<Point>>,
    /// letters[..text_letter_count] come from the text layout; the rest are brush strokes,
    /// which survive a relayout.
    #[serde(default)]
    pub text_letter_count: usize,
    /// Photoshop-style warp grid applied to the text letters after the guide warp.
    #[serde(default)]
    pub free_warp: FreeWarp,
}

impl Scene {
    /// Scene-space bounding box of all letters (None if there is no ink area).
    pub fn bounds(&self) -> Option<Rect> {
        self.letters
            .iter()
            .map(|l| l.placed_path().bounding_box())
            .reduce(|a, b| a.union(b))
            .filter(|r| r.width() > 0.0 && r.height() > 0.0)
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            text: "BOOM".to_owned(),
            direction: LayoutDirection::Horizontal,
            letters: Vec::new(),
            seed: 1,
            params: Params::default(),
            guide: None,
            text_letter_count: 0,
            free_warp: FreeWarp::default(),
        }
    }
}
