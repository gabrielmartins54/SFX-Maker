//! Seeded per-letter effects (rotation / scale / offset jitter, size ramp, slant).
//! Pure: letters in -> letters out. Each letter has its own RNG derived from
//! `seed` and its index, so results never depend on iteration or thread order.

use kurbo::{Affine, Shape};
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::text_to_paths::EM_SIZE;
use crate::model::letter::Letter;
use crate::model::params::Params;
use crate::model::scene::LayoutDirection;

fn letter_rng(seed: u64, index: usize) -> ChaCha8Rng {
    // Golden-ratio multiplier spreads neighbouring indices across the seed space.
    ChaCha8Rng::seed_from_u64(seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Symmetric random value in [-amount, amount]; consumes exactly one draw even for 0
/// so changing one slider never reshuffles the others.
fn jitter(rng: &mut ChaCha8Rng, amount: f64) -> f64 {
    let u: f64 = rng.random_range(-1.0..=1.0);
    u * amount
}

/// Flow-axis shift per letter so gaps follow letter size ("spacing by size").
/// Letters are re-packed: each gap between neighbours is the original gap multiplied by
/// `lerp(1, 1/avg_scale, k)`, so enlarged letters never overlap just because they grew.
fn packing_shifts(letters: &[Letter], scales: &[f64], k: f64, direction: LayoutDirection) -> Vec<f64> {
    let flow = |l: &Letter| {
        let bb = l.glyph_path.bounding_box();
        match direction {
            LayoutDirection::Horizontal => (bb.center().x + l.transform.translation().x, bb.width()),
            LayoutDirection::Vertical => (bb.center().y + l.transform.translation().y, bb.height()),
        }
    };
    let mut shifts = vec![0.0; letters.len()];
    let mut new_c = 0.0;
    for i in 0..letters.len() {
        let (c, w) = flow(&letters[i]);
        if i == 0 {
            new_c = c;
        } else {
            let (pc, pw) = flow(&letters[i - 1]);
            if c < pc - 0.25 * EM_SIZE {
                // New line / column: restart.
                new_c = c;
            } else {
                let gap = (c - pc) - (pw + w) / 2.0;
                let avg = ((scales[i - 1] + scales[i]) / 2.0).max(0.05);
                let g = 1.0 + (1.0 / avg - 1.0) * k;
                new_c += (pw * scales[i - 1] + w * scales[i]) / 2.0 + gap * g;
            }
        }
        shifts[i] = new_c - c;
    }
    shifts
}

/// Applies the effect params to freshly laid-out letters (whose transforms are pure
/// translations). Effects act about each glyph's own bbox centre.
pub fn apply_effects(letters: &mut [Letter], params: &Params, seed: u64, direction: LayoutDirection) {
    let n = letters.len();
    let slant = Affine::skew(-params.slant.to_radians().tan(), 0.0);
    // y-down: a negative y shear per unit x lifts strokes towards the right.
    let rise = Affine::skew(0.0, -params.rise.to_radians().tan());

    // Pass 1: per-letter random draws and scale.
    let mut draws = Vec::with_capacity(n);
    let mut scales = Vec::with_capacity(n);
    for i in 0..n {
        let mut rng = letter_rng(seed, i);
        let rot = jitter(&mut rng, params.rotation_jitter).to_radians();
        let sc = 1.0 + jitter(&mut rng, params.scale_jitter);
        let ox = jitter(&mut rng, params.offset_jitter) * EM_SIZE;
        let oy = jitter(&mut rng, params.offset_jitter) * EM_SIZE;
        let wob = jitter(&mut rng, params.baseline_wobble) * EM_SIZE;
        let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
        let ramp = params.start_size + (params.end_size - params.start_size) * t;
        scales.push((sc * ramp).max(0.01));
        draws.push((rot, ox, oy, wob));
    }
    let shifts = packing_shifts(letters, &scales, params.spacing_by_size, direction);

    // Pass 2: build each letter's local transform.
    for (i, l) in letters.iter_mut().enumerate() {
        let (rot, ox, oy, wob) = draws[i];
        let (wx, wy) = match direction {
            LayoutDirection::Horizontal => (0.0, wob),
            LayoutDirection::Vertical => (wob, 0.0),
        };
        let (px, py) = match direction {
            LayoutDirection::Horizontal => (shifts[i], 0.0),
            LayoutDirection::Vertical => (0.0, shifts[i]),
        };
        let bb = l.glyph_path.bounding_box();
        let c = bb.center().to_vec2();
        let local = Affine::translate((ox + wx + px, oy + wy + py))
            * Affine::translate(c)
            * Affine::rotate(rot)
            * Affine::scale(scales[i])
            * rise
            * slant
            * Affine::translate(-c);
        l.transform *= local;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::text_to_paths::{text_to_paths, FontData};

    fn base() -> Vec<Letter> {
        let bytes = include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec();
        let font = FontData::new("Bangers", bytes, 0).expect("font");
        text_to_paths(&font, "BOOM", LayoutDirection::Horizontal).expect("layout")
    }

    fn wild() -> Params {
        Params {
            rotation_jitter: 15.0,
            scale_jitter: 0.2,
            offset_jitter: 0.1,
            baseline_wobble: 0.1,
            ..Params::default()
        }
    }

    #[test]
    fn default_params_change_nothing() {
        let a = base();
        let mut b = base();
        apply_effects(&mut b, &Params::default(), 7, LayoutDirection::Horizontal);
        for (x, y) in a.iter().zip(&b) {
            for (p, q) in x.transform.as_coeffs().iter().zip(y.transform.as_coeffs()) {
                assert!((p - q).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn same_seed_same_result_different_seed_differs() {
        let (mut a, mut b, mut c) = (base(), base(), base());
        apply_effects(&mut a, &wild(), 1, LayoutDirection::Horizontal);
        apply_effects(&mut b, &wild(), 1, LayoutDirection::Horizontal);
        apply_effects(&mut c, &wild(), 2, LayoutDirection::Horizontal);
        assert_eq!(a[0].transform, b[0].transform);
        assert_ne!(a[0].transform, c[0].transform);
    }

    #[test]
    fn size_ramp_grows_last_letter() {
        let mut l = base();
        let p = Params { end_size: 2.0, ..Params::default() };
        apply_effects(&mut l, &p, 1, LayoutDirection::Horizontal);
        let first = l[0].placed_path().bounding_box().height() / l[0].glyph_path.bounding_box().height();
        let last = l[3].placed_path().bounding_box().height() / l[3].glyph_path.bounding_box().height();
        assert!((first - 1.0).abs() < 1e-6);
        assert!((last - 2.0).abs() < 1e-6);
    }

    #[test]
    fn rise_shears_a_letter_about_its_centre() {
        let mut l = base();
        let before = l[0].placed_path().bounding_box();
        apply_effects(&mut l, &Params { rise: 15.0, ..Params::default() }, 1, LayoutDirection::Horizontal);
        let after = l[0].placed_path().bounding_box();
        assert!(after.height() > before.height(), "shear changes the vertical extent");
        assert!((after.center().x - before.center().x).abs() < 1e-6);
    }

    #[test]
    fn growing_letters_keep_their_gap_when_spacing_by_size_is_zero() {
        let mut l = base();
        apply_effects(&mut l, &Params { end_size: 2.0, ..Params::default() }, 1, LayoutDirection::Horizontal);
        let a = l[2].placed_path().bounding_box();
        let b = l[3].placed_path().bounding_box();
        let orig = base();
        let gap0 = orig[3].placed_path().bounding_box().x0 - orig[2].placed_path().bounding_box().x1;
        assert!((b.x0 - a.x1 - gap0).abs() < 1e-6);
    }

    #[test]
    fn letter_effects_independent_of_other_letters() {
        let (mut a, mut b) = (base(), base());
        apply_effects(&mut a, &wild(), 3, LayoutDirection::Horizontal);
        apply_effects(&mut b[..2], &wild(), 3, LayoutDirection::Horizontal);
        assert_eq!(a[1].transform, b[1].transform);
    }
}
