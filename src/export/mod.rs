//! Export. PNG goes through the same `render_scene` as the canvas; SVG is built from the
//! same placed paths, so all outputs match.

pub mod png;
pub mod project;
pub mod svg;
pub mod bundle;

use kurbo::Rect;

/// Scene bounds grown by `pad` (fraction of the longest side) on every side.
pub fn padded_bounds(b: Rect, pad: f64) -> Rect {
    let m = b.width().max(b.height()) * pad;
    Rect::new(b.x0 - m, b.y0 - m, b.x1 + m, b.y1 + m)
}

pub const PAD: f64 = 0.04;
