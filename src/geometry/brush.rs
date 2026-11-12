//! Brush lettering: turns a freehand polyline into a filled, variable-width stroke outline.

use kurbo::{BezPath, Circle, Point, Shape, Vec2};

use super::warp::catmull_rom;

/// Points closer than this (scene units) to the previous kept point are dropped.
const MIN_STEP: f64 = 1.5;
/// Fraction of full width at the very tips of the stroke.
const TIP_WIDTH: f64 = 0.12;

/// `width` is the full stroke width in scene units. `taper` (0..=0.5) is the fraction of the
/// stroke length at each end over which the width ramps from the tip to full width.
/// Returns `None` for no points.
pub fn stroke_to_path(points: &[Point], width: f64, taper: f64) -> Option<BezPath> {
    let width = width.max(0.5);
    let mut pts: Vec<Point> = Vec::with_capacity(points.len());
    for &p in points {
        if pts.last().is_none_or(|q| q.distance(p) >= MIN_STEP) {
            pts.push(p);
        }
    }
    match pts.len() {
        0 => return None,
        1 => return Some(Circle::new(pts[0], width * 0.5).to_path(0.1)),
        _ => {}
    }
    let smooth = catmull_rom(&pts, 6);

    // Cumulative length for the taper profile.
    let mut cum = vec![0.0];
    for w in smooth.windows(2) {
        cum.push(cum[cum.len() - 1] + w[0].distance(w[1]));
    }
    let total = cum[cum.len() - 1].max(1e-9);
    let taper = taper.clamp(0.0, 0.5);

    let half = |t: f64| {
        let ramp = if taper <= 1e-6 {
            1.0
        } else {
            (t / taper).min((1.0 - t) / taper).clamp(0.0, 1.0)
        };
        // Ease so the tip is pointed but the body fills out quickly.
        let e = ramp * ramp * (3.0 - 2.0 * ramp);
        width * 0.5 * (TIP_WIDTH + (1.0 - TIP_WIDTH) * e)
    };

    let n = smooth.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let a = smooth[i.saturating_sub(1)];
        let b = smooth[(i + 1).min(n - 1)];
        let d = b - a;
        let len = d.hypot();
        let dir = if len > 1e-12 { d / len } else { Vec2::new(1.0, 0.0) };
        let normal = Vec2::new(-dir.y, dir.x);
        let h = half(cum[i] / total);
        left.push(smooth[i] + normal * h);
        right.push(smooth[i] - normal * h);
    }

    let mut path = BezPath::new();
    path.move_to(left[0]);
    for &p in &left[1..] {
        path.line_to(p);
    }
    for &p in right.iter().rev() {
        path.line_to(p);
    }
    path.close_path();
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> Vec<Point> {
        (0..=20).map(|i| Point::new(f64::from(i) * 5.0, 0.0)).collect()
    }

    #[test]
    fn empty_is_none_and_dot_is_circle() {
        assert!(stroke_to_path(&[], 10.0, 0.3).is_none());
        let d = stroke_to_path(&[Point::new(5.0, 5.0)], 10.0, 0.3).unwrap();
        assert!((d.bounding_box().width() - 10.0).abs() < 0.5);
    }

    #[test]
    fn middle_is_wider_than_tips() {
        let p = stroke_to_path(&line(), 20.0, 0.3).unwrap();
        let b = p.bounding_box();
        assert!((b.height() - 20.0).abs() < 0.5, "full width reached: {}", b.height());
        // Near the start the stroke is much thinner than the full width.
        assert!(p.contains(Point::new(50.0, 8.0)));
        assert!(!p.contains(Point::new(1.0, 8.0)));
    }

    #[test]
    fn no_taper_is_uniform() {
        let p = stroke_to_path(&line(), 10.0, 0.0).unwrap();
        assert!(p.contains(Point::new(2.0, 4.0)));
    }
}

