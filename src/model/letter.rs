use kurbo::{Affine, BezPath};
use serde::{Deserialize, Serialize};

/// One glyph of the lettering. `glyph_path` lives in the letter's local space
/// (pen origin at 0,0, y-down); `transform` places it in scene space.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Letter {
    pub ch: char,
    pub glyph_path: BezPath,
    pub transform: Affine,
}

impl Letter {
    pub fn new(ch: char, glyph_path: BezPath, transform: Affine) -> Self {
        Self {
            ch,
            glyph_path,
            transform,
        }
    }

    /// Path in scene space.
    pub fn placed_path(&self) -> BezPath {
        self.transform * self.glyph_path.clone()
    }
}
