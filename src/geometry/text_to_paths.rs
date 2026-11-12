//! Font bytes + string -> one `BezPath` per letter. Pure; no UI dependency.

use std::sync::Arc;

use kurbo::{Affine, BezPath};
use ttf_parser::{Face, OutlineBuilder};

use crate::model::letter::Letter;
use crate::model::params::Params;
use crate::model::scene::LayoutDirection;

/// Nominal em size in scene units. Everything is laid out at this size and
/// scaled later via transforms.
pub const EM_SIZE: f64 = 200.0;

#[derive(Debug)]
pub enum FontError {
    Parse(String),
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::Parse(e) => write!(f, "Could not read font: {e}"),
        }
    }
}

impl std::error::Error for FontError {}

/// A validated font: raw bytes plus face index. Cheap to clone.
#[derive(Clone)]
pub struct FontData {
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
    pub index: u32,
}

impl FontData {
    /// Parses once to make sure the bytes are a usable font.
    pub fn new(name: impl Into<String>, bytes: Vec<u8>, index: u32) -> Result<Self, FontError> {
        let face = Face::parse(&bytes, index).map_err(|e| FontError::Parse(e.to_string()))?;
        if face.units_per_em() == 0 {
            return Err(FontError::Parse("font has zero units-per-em".into()));
        }
        Ok(Self {
            name: name.into(),
            bytes: Arc::new(bytes),
            index,
        })
    }
}

struct PathSink {
    path: BezPath,
    scale: f64,
}

impl PathSink {
    /// Font space is y-up; scene space is y-down.
    fn p(&self, x: f32, y: f32) -> (f64, f64) {
        (f64::from(x) * self.scale, -f64::from(y) * self.scale)
    }
}

impl OutlineBuilder for PathSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.p(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.p(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(self.p(x1, y1), self.p(x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path
            .curve_to(self.p(x1, y1), self.p(x2, y2), self.p(x, y));
    }
    fn close(&mut self) {
        self.path.close_path();
    }
}

/// Lays out `text`. Whitespace advances the pen but produces no `Letter`.
/// `\n` starts a new line (horizontal) or column (vertical).
/// Kerning uses the legacy `kern` table only (no GPOS).
#[cfg(test)]
pub fn text_to_paths(
    font: &FontData,
    text: &str,
    direction: LayoutDirection,
) -> Result<Vec<Letter>, FontError> {
    text_to_paths_with(font, text, direction, &Params::default())
}

/// Horizontal kerning (scene units) between two glyphs from the legacy `kern` table.
fn kern_pair(face: &Face, left: ttf_parser::GlyphId, right: ttf_parser::GlyphId, scale: f64) -> f64 {
    let Some(kern) = face.tables().kern else {
        return 0.0;
    };
    kern.subtables
        .into_iter()
        .filter(|s| s.horizontal && !s.variable && !s.has_cross_stream)
        .find_map(|s| s.glyphs_kerning(left, right))
        .map_or(0.0, |k| f64::from(k) * scale)
}

/// Like [`text_to_paths`] but honours tracking, line spacing and kerning from `params`.
pub fn text_to_paths_with(
    font: &FontData,
    text: &str,
    direction: LayoutDirection,
    params: &Params,
) -> Result<Vec<Letter>, FontError> {
    let face = Face::parse(&font.bytes, font.index).map_err(|e| FontError::Parse(e.to_string()))?;
    let upem = f64::from(face.units_per_em());
    if upem <= 0.0 {
        return Err(FontError::Parse("font has zero units-per-em".into()));
    }
    let scale = EM_SIZE / upem;
    let ascender = f64::from(face.ascender()) * scale;
    let descender = f64::from(face.descender()) * scale;
    let glyph_height = (ascender - descender).max(EM_SIZE * 0.5);
    let line_height = glyph_height * params.line_spacing;

    let tracking = params.tracking * EM_SIZE;
    let mut letters = Vec::new();
    let (mut pen_x, mut pen_y) = (0.0_f64, 0.0_f64);
    let mut line = 0.0_f64;
    let mut prev: Option<ttf_parser::GlyphId> = None;

    for ch in text.chars() {
        if ch == '\n' {
            line += 1.0;
            prev = None;
            match direction {
                LayoutDirection::Horizontal => {
                    pen_x = 0.0;
                    pen_y = line * line_height;
                }
                LayoutDirection::Vertical => {
                    pen_x = -line * EM_SIZE * 1.1 * params.line_spacing;
                    pen_y = 0.0;
                }
            }
            continue;
        }

        let glyph = face.glyph_index(ch);
        if params.kerning
            && direction == LayoutDirection::Horizontal
            && let (Some(p), Some(g)) = (prev, glyph)
        {
            pen_x += kern_pair(&face, p, g, scale);
        }
        prev = glyph;
        let advance = glyph
            .and_then(|g| face.glyph_hor_advance(g))
            .map_or(EM_SIZE * 0.5, |a| f64::from(a) * scale);

        if let Some(gid) = glyph {
            let mut sink = PathSink {
                path: BezPath::new(),
                scale,
            };
            if face.outline_glyph(gid, &mut sink).is_some() && !sink.path.elements().is_empty() {
                let offset = match direction {
                    LayoutDirection::Horizontal => (pen_x, pen_y),
                    // Centre the glyph in its column; baseline sits below the pen.
                    LayoutDirection::Vertical => (pen_x - advance / 2.0, pen_y + ascender),
                };
                letters.push(Letter::new(
                    ch,
                    sink.path,
                    Affine::translate(offset),
                ));
            }
        }

        match direction {
            LayoutDirection::Horizontal => pen_x += advance + tracking,
            LayoutDirection::Vertical => pen_y += glyph_height + tracking,
        }
    }
    Ok(letters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn font() -> FontData {
        let bytes = include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec();
        FontData::new("Bangers", bytes, 0).expect("bundled font parses")
    }

    #[test]
    fn one_letter_per_visible_char() {
        let l = text_to_paths(&font(), "A B", LayoutDirection::Horizontal).unwrap();
        assert_eq!(l.len(), 2);
        assert!(l.iter().all(|l| l.glyph_path.bounding_box().area() > 0.0));
    }

    #[test]
    fn empty_text_is_ok() {
        assert!(text_to_paths(&font(), "", LayoutDirection::Horizontal)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn horizontal_advances_right_vertical_advances_down() {
        let h = text_to_paths(&font(), "AB", LayoutDirection::Horizontal).unwrap();
        assert!(h[1].transform.translation().x > h[0].transform.translation().x);
        let v = text_to_paths(&font(), "AB", LayoutDirection::Vertical).unwrap();
        assert!(v[1].transform.translation().y > v[0].transform.translation().y);
    }

    #[test]
    fn bad_font_is_error_not_panic() {
        assert!(FontData::new("bad", vec![0, 1, 2, 3], 0).is_err());
    }

    #[test]
    fn layout_is_deterministic() {
        let a = text_to_paths(&font(), "BOOM", LayoutDirection::Horizontal).unwrap();
        let b = text_to_paths(&font(), "BOOM", LayoutDirection::Horizontal).unwrap();
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.glyph_path, y.glyph_path);
            assert_eq!(x.transform, y.transform);
        }
    }
}
