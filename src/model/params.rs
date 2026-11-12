use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TransformMode {
    #[default]
    None,
    Trapezoid,
    Grid,
}

impl TransformMode {
    pub const ALL: [TransformMode; 3] = [Self::None, Self::Trapezoid, Self::Grid];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Trapezoid => "Trapezoid",
            Self::Grid => "Grid",
        }
    }
}

/// User-tweakable lettering parameters. Pure data; `Default` is a neutral
/// (no effect) setting so plain text lays out exactly like a normal text box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// Extra advance between letters, in em (can be negative).
    pub tracking: f64,
    /// Line (or column) spacing multiplier.
    pub line_spacing: f64,
    /// Apply the font's `kern` table (horizontal layout only).
    pub kerning: bool,
    /// Max random rotation per letter, degrees (+/-).
    pub rotation_jitter: f64,
    /// Max random scale change per letter, fraction (+/-).
    pub scale_jitter: f64,
    /// Max random offset per letter, in em (+/-, both axes).
    pub offset_jitter: f64,
    /// Max random vertical (horizontal in vertical layout) wobble, in em (+/-).
    pub baseline_wobble: f64,
    /// Letter size at the start / end of the text (multiplier; sizes grow linearly between).
    pub start_size: f64,
    pub end_size: f64,
    /// 0 = letters stay upright on the guide, 1 = letters lean with it (fully bent along it).
    pub lean: f64,
    /// Tilts horizontal strokes up to the right, degrees (vertical strokes stay upright).
    pub rise: f64,
    /// 0 = gaps ignore letter size, 1 = gaps scale inversely with size (small = wider).
    pub spacing_by_size: f64,
    /// Perspective on the whole text: + narrows the top, - narrows the bottom (-1..1).
    pub keystone_v: f64,
    /// + makes the right side taller, - the left side (-1..1).
    pub keystone_h: f64,
    /// Macro sliders (0..100, 50 = neutral). They only move the individual fields above.
    pub power: f64,
    pub roughness: f64,
    pub momentum: f64,
    /// Shear applied to every letter, degrees (italic-like).
    pub slant: f64,
    /// Procedural guide: arc curvature (-1..1, fraction of text length as sagitta). 0 = straight.
    pub bend: f64,
    /// Procedural guide: sine wave amplitude, in em.
    pub wave: f64,
    /// Procedural guide: number of wave periods over the text.
    pub wave_cycles: f64,
    /// Uniform stroke weight change, in em (positive = thicker, negative = thinner).
    pub weight: f64,
    /// TransType / Fontographer Simple algorithm: stem thickness ratio (horizontal / vertical).
    /// Default is 0.60 (standard typographic contrast).
    pub weight_stem_ratio: f64,
    /// TransType / Fontographer constraint: keep vertical dimensions (anchors height & baseline).
    pub weight_keep_v: bool,
    /// TransType / Fontographer constraint: keep horizontal dimensions (preserves advance width).
    pub weight_keep_h: bool,
    /// Extra weight at the start / end of the ramp, in em (added to `weight`).
    pub weight_start: f64,
    pub weight_end: f64,
    /// Ramp runs within each letter instead of across the whole text.
    pub weight_per_letter: bool,
    /// Extra weight for horizontal strokes only, em (FontLab "H weight" vs "V weight"):
    /// vertical stems change by `weight`, horizontal strokes by `weight + weight_contrast`.
    pub weight_contrast: f64,
    /// Round the corners when changing weight (instead of keeping them sharp).
    pub weight_round: bool,
    /// Push letters apart when jitter / size changes would make them overlap.
    pub no_overlap: bool,
    /// Bend the letter shapes themselves along the guide (off = letters stay rigid and are only
    /// moved and rotated along it).
    pub bend_glyphs: bool,

    /// Free transform mode (Giongo style): None | Trapezoid | Grid
    pub transform_mode: TransformMode,
    /// Trapezoid skew: horizontal perspective (-1.0..=1.0)
    pub trap_h: f64,
    /// Trapezoid skew: vertical perspective (-1.0..=1.0)
    pub trap_v: f64,
}

/// Slider ranges, shared by the UI and the random generators so they never disagree.
pub mod ranges {
    use std::ops::RangeInclusive;
    pub const TRACKING: RangeInclusive<f64> = -0.5..=1.2;
    pub const LINE_SPACING: RangeInclusive<f64> = 0.3..=2.5;
    pub const SLANT: RangeInclusive<f64> = -40.0..=40.0;
    pub const SIZE: RangeInclusive<f64> = 0.3..=2.5;
    pub const LEAN: RangeInclusive<f64> = 0.0..=1.0;
    pub const RISE: RangeInclusive<f64> = -10.0..=20.0;
    pub const SPACING_BY_SIZE: RangeInclusive<f64> = 0.0..=1.0;
    pub const KEYSTONE: RangeInclusive<f64> = -1.0..=1.0;
    pub const MACRO: RangeInclusive<f64> = 0.0..=100.0;
    pub const ROTATION_JITTER: RangeInclusive<f64> = 0.0..=40.0;
    pub const SCALE_JITTER: RangeInclusive<f64> = 0.0..=0.8;
    pub const OFFSET_JITTER: RangeInclusive<f64> = 0.0..=0.5;
    pub const BASELINE_WOBBLE: RangeInclusive<f64> = 0.0..=0.5;
    pub const BEND: RangeInclusive<f64> = -1.0..=1.0;
    pub const WAVE: RangeInclusive<f64> = 0.0..=1.0;
    pub const WAVE_CYCLES: RangeInclusive<f64> = 0.5..=4.0;
    pub const WEIGHT: RangeInclusive<f64> = -0.08..=0.12;
    pub const WEIGHT_CONTRAST: RangeInclusive<f64> = -0.08..=0.12;
    pub const WEIGHT_RAMP: RangeInclusive<f64> = -0.08..=0.12;
    pub const STEM_RATIO: RangeInclusive<f64> = 0.0..=1.0;
    pub const TRAP_SKEW: RangeInclusive<f64> = -1.0..=1.0;
}

impl Params {
    /// Clamps every field into its slider range.
    pub fn clamped(mut self) -> Self {
        use ranges::*;
        let c = |v: f64, r: std::ops::RangeInclusive<f64>| v.clamp(*r.start(), *r.end());
        self.tracking = c(self.tracking, TRACKING);
        self.line_spacing = c(self.line_spacing, LINE_SPACING);
        self.slant = c(self.slant, SLANT);
        self.start_size = c(self.start_size, SIZE);
        self.end_size = c(self.end_size, SIZE);
        self.lean = c(self.lean, LEAN);
        self.rise = c(self.rise, RISE);
        self.spacing_by_size = c(self.spacing_by_size, SPACING_BY_SIZE);
        self.keystone_v = c(self.keystone_v, KEYSTONE);
        self.keystone_h = c(self.keystone_h, KEYSTONE);
        self.power = c(self.power, MACRO);
        self.roughness = c(self.roughness, MACRO);
        self.momentum = c(self.momentum, MACRO);
        self.rotation_jitter = c(self.rotation_jitter, ROTATION_JITTER);
        self.scale_jitter = c(self.scale_jitter, SCALE_JITTER);
        self.offset_jitter = c(self.offset_jitter, OFFSET_JITTER);
        self.baseline_wobble = c(self.baseline_wobble, BASELINE_WOBBLE);
        self.bend = c(self.bend, BEND);
        self.wave = c(self.wave, WAVE);
        self.wave_cycles = c(self.wave_cycles, WAVE_CYCLES);
        self.weight = c(self.weight, WEIGHT);
        self.weight_stem_ratio = c(self.weight_stem_ratio, STEM_RATIO);
        self.weight_contrast = c(self.weight_contrast, WEIGHT_CONTRAST);
        self.weight_start = c(self.weight_start, WEIGHT_RAMP);
        self.weight_end = c(self.weight_end, WEIGHT_RAMP);
        self.trap_h = c(self.trap_h, TRAP_SKEW);
        self.trap_v = c(self.trap_v, TRAP_SKEW);
        self
    }
}

impl Params {
    /// Power macro (as on the original web tool): moves stroke weight and the start / end
    /// size progression together. `v` is 0..100; only the change from the stored value is applied,
    /// so individual fine-tuning is kept.
    pub fn set_power(&mut self, v: f64) {
        let d = v - self.power;
        self.power = v;
        self.weight += d * 0.0008;
        self.start_size -= d * 0.0028;
        self.end_size += d * 0.0046;
        *self = self.clamped();
    }

    /// Roughness macro: angle / size variation and positional wobble.
    pub fn set_roughness(&mut self, v: f64) {
        let d = v - self.roughness;
        self.roughness = v;
        self.rotation_jitter += d * 0.16;
        self.scale_jitter += d * 0.004;
        self.offset_jitter += d * 0.0004;
        *self = self.clamped();
    }

    /// Momentum macro: lean, slant and rise.
    pub fn set_momentum(&mut self, v: f64) {
        let d = v - self.momentum;
        self.momentum = v;
        self.lean += d * 0.007;
        self.slant += d * 0.24;
        self.rise += d * 0.06;
        *self = self.clamped();
    }
}

impl Default for Params {
    fn default() -> Self {
        Self {
            tracking: 0.0,
            line_spacing: 1.0,
            kerning: true,
            rotation_jitter: 0.0,
            scale_jitter: 0.0,
            offset_jitter: 0.0,
            baseline_wobble: 0.0,
            start_size: 1.0,
            end_size: 1.0,
            lean: 1.0,
            rise: 0.0,
            spacing_by_size: 0.0,
            keystone_v: 0.0,
            keystone_h: 0.0,
            power: 50.0,
            roughness: 50.0,
            momentum: 50.0,
            slant: 0.0,
            bend: 0.0,
            wave: 0.0,
            wave_cycles: 2.0,
            weight: 0.0,
            weight_stem_ratio: 0.60,
            weight_keep_v: false,
            weight_keep_h: false,
            weight_start: 0.0,
            weight_end: 0.0,
            weight_per_letter: false,
            weight_contrast: 0.0,
            weight_round: false,
            no_overlap: true,
            bend_glyphs: false,
            transform_mode: TransformMode::None,
            trap_h: 0.0,
            trap_v: 0.0,
        }
    }
}

