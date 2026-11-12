//! "Roll" (random look) and "Mutate" (small random tweak) for `Params`.
//! Deterministic given the RNG, so both are unit-testable.

use std::ops::RangeInclusive;

use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::model::params::{ranges, Params};

pub fn rng_from_seed(seed: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed)
}

fn pick(rng: &mut ChaCha8Rng, r: &RangeInclusive<f64>, lo: f64, hi: f64) -> f64 {
    let (a, b) = (*r.start(), *r.end());
    a + (b - a) * rng.random_range(lo..=hi)
}

/// Fresh random look. Each effect is switched on with some probability and kept
/// within the "looks reasonable" part of its range.
pub fn roll(rng: &mut ChaCha8Rng, current: &Params) -> Params {
    let on = |rng: &mut ChaCha8Rng, p: f64| rng.random_range(0.0..1.0) < p;
    // Wave settings are the user's own: Roll never touches them.
    let mut p = Params { wave: current.wave, wave_cycles: current.wave_cycles, ..Params::default() };
    if on(rng, 0.7) {
        p.tracking = rng.random_range(-0.12..=0.12);
    }
    if on(rng, 0.6) {
        p.slant = rng.random_range(-25.0..=30.0);
    }
    if on(rng, 0.5) {
        p.start_size = rng.random_range(0.6..=1.2);
        p.end_size = rng.random_range(0.8..=2.2);
    }
    if on(rng, 0.8) {
        p.rotation_jitter = pick(rng, &ranges::ROTATION_JITTER, 0.05, 0.5);
    }
    if on(rng, 0.7) {
        p.scale_jitter = pick(rng, &ranges::SCALE_JITTER, 0.05, 0.5);
    }
    if on(rng, 0.5) {
        p.offset_jitter = pick(rng, &ranges::OFFSET_JITTER, 0.02, 0.3);
    }
    if on(rng, 0.6) {
        p.baseline_wobble = pick(rng, &ranges::BASELINE_WOBBLE, 0.03, 0.4);
    }
    if on(rng, 0.4) {
        p.bend = rng.random_range(-0.7..=0.7);
    }
    p.clamped()
}

/// Nudges each field with ~60% probability by up to `amount` (0..1) of its slider range.
pub fn mutate(base: &Params, rng: &mut ChaCha8Rng, amount: f64) -> Params {
    let mut p = *base;
    let amount = amount.clamp(0.0, 1.0);
    let nudge = |rng: &mut ChaCha8Rng, v: &mut f64, r: &RangeInclusive<f64>| {
        if rng.random_range(0.0..1.0) < 0.6 {
            let span = r.end() - r.start();
            *v += rng.random_range(-1.0..=1.0) * amount * span;
        }
    };
    nudge(rng, &mut p.tracking, &ranges::TRACKING);
    nudge(rng, &mut p.line_spacing, &ranges::LINE_SPACING);
    nudge(rng, &mut p.slant, &ranges::SLANT);
    nudge(rng, &mut p.start_size, &ranges::SIZE);
    nudge(rng, &mut p.end_size, &ranges::SIZE);
    nudge(rng, &mut p.rise, &ranges::RISE);
    nudge(rng, &mut p.rotation_jitter, &ranges::ROTATION_JITTER);
    nudge(rng, &mut p.scale_jitter, &ranges::SCALE_JITTER);
    nudge(rng, &mut p.offset_jitter, &ranges::OFFSET_JITTER);
    nudge(rng, &mut p.baseline_wobble, &ranges::BASELINE_WOBBLE);
    nudge(rng, &mut p.bend, &ranges::BEND);
    p.clamped()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_is_deterministic_and_in_range() {
        for s in 0..50 {
            let a = roll(&mut rng_from_seed(s), &Params::default());
            assert_eq!(a, roll(&mut rng_from_seed(s), &Params::default()));
            assert_eq!(a, a.clamped());
        }
        assert_ne!(roll(&mut rng_from_seed(1), &Params::default()), roll(&mut rng_from_seed(2), &Params::default()));
    }

    #[test]
    fn roll_and_mutate_keep_wave_settings() {
        let cur = Params { wave: 0.37, wave_cycles: 3.3, ..Params::default() };
        for s in 0..40 {
            let r = roll(&mut rng_from_seed(s), &cur);
            assert_eq!((r.wave, r.wave_cycles), (0.37, 3.3));
            let m = mutate(&r, &mut rng_from_seed(s + 100), 0.6);
            assert_eq!((m.wave, m.wave_cycles), (0.37, 3.3));
        }
    }

    #[test]
    fn roll_keeps_line_spacing_neutral() {
        assert_eq!(roll(&mut rng_from_seed(9), &Params::default()).line_spacing, 1.0);
    }

    #[test]
    fn mutate_zero_is_identity_and_small_stays_close() {
        let base = roll(&mut rng_from_seed(3), &Params::default());
        assert_eq!(mutate(&base, &mut rng_from_seed(5), 0.0), base);
        let m = mutate(&base, &mut rng_from_seed(5), 0.05);
        assert!((m.rotation_jitter - base.rotation_jitter).abs() <= 0.05 * 45.0 + 1e-9);
        assert_eq!(m, m.clamped());
    }
}
