use serde::{Deserialize, Serialize};

use super::letter::Letter;

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
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            text: "BOOM".to_owned(),
            direction: LayoutDirection::Horizontal,
            letters: Vec::new(),
            seed: 1,
        }
    }
}
