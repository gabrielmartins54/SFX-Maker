//! English SFX Dictionary and Phonetic Auto-Pick Engine.
//! Categorized sound-effect catalog and rule-based phonetic analyzer that automatically
//! recommends sound styles and macro intensity sliders (Power, Roughness, Momentum).
//!
//! Pure data and logic; no UI or font handles.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SfxGenre {
    Impact,
    Crash,
    Speed,
    Rumble,
    Water,
    Sparkle,
    Small,
    Shock,
}

impl SfxGenre {
    pub const ALL: [SfxGenre; 8] = [
        Self::Impact,
        Self::Crash,
        Self::Speed,
        Self::Rumble,
        Self::Water,
        Self::Sparkle,
        Self::Small,
        Self::Shock,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Impact => "Impact",
            Self::Crash => "Crash & Explosion",
            Self::Speed => "Speed & Slash",
            Self::Rumble => "Rumble & Earthquake",
            Self::Water => "Water & Splash",
            Self::Sparkle => "Cute & Sparkle",
            Self::Small => "Small & Subtle",
            Self::Shock => "Shock & Surprise",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Impact => "Impact",
            Self::Crash => "Crash",
            Self::Speed => "Speed",
            Self::Rumble => "Rumble",
            Self::Water => "Water",
            Self::Sparkle => "Sparkle",
            Self::Small => "Small",
            Self::Shock => "Shock",
        }
    }
}

pub struct DictCategory {
    pub genre: SfxGenre,
    pub description: &'static str,
    pub words: &'static [&'static str],
}

pub const ENGLISH_DICTIONARY: [DictCategory; 8] = [
    DictCategory {
        genre: SfxGenre::Impact,
        description: "Heavy hits, punches, blunt strikes, gunshots",
        words: &[
            "BOOM!", "BAM!", "POW!", "WHAM!", "THUD!", "BANG!", "CLANG!", "CRUNCH!",
            "SMACK!", "KAPOW!", "BONK!", "THUMP!", "BASH!", "PUNCH!", "WHACK!",
        ],
    },
    DictCategory {
        genre: SfxGenre::Crash,
        description: "Explosions, glass breaking, shattering, destruction",
        words: &[
            "CRASH!", "SMASH!", "KABOOM!", "SHATTER!", "BLAM!", "KRAK!", "KA-BLAM!",
            "SPLINTER!", "BREAK!", "SNAP!", "BURST!", "DETONATE!",
        ],
    },
    DictCategory {
        genre: SfxGenre::Speed,
        description: "Fast motion, sword slashes, dashes, wind, air gusts",
        words: &[
            "WHOOSH!", "SWOOSH!", "ZOOM!", "SLASH!", "SHING!", "DASH!", "SWISH!",
            "ZIP!", "WHIZ!", "FLICK!", "SLICE!", "RUSH!", "VROOOM!",
        ],
    },
    DictCategory {
        genre: SfxGenre::Rumble,
        description: "Earthquakes, heavy machinery, roars, vibrations, monsters",
        words: &[
            "RUMBLE", "RRRRR", "GRRRR", "DOOM", "ROAR", "GROWL", "THROB", "BRRRR",
            "TREMOR", "HUMMM", "GROAN", "CHUG",
        ],
    },
    DictCategory {
        genre: SfxGenre::Water,
        description: "Liquid splashes, diving, drops, rain, bubbles",
        words: &[
            "SPLASH!", "SPLOOSH!", "DRIP", "PLOP", "GLUG", "SLOSH", "SQUIRT",
            "GUSH!", "BUBBLE", "SPURT", "TRICKLE", "SPLAT!",
        ],
    },
    DictCategory {
        genre: SfxGenre::Sparkle,
        description: "Magic, twinkling, cute bouncy sounds, chimes",
        words: &[
            "SPARKLE", "TWINKLE", "BOING!", "POP!", "DING!", "BLING!", "PING!",
            "TINKLE", "SHINE", "GLOW", "CHIME", "JINGLE",
        ],
    },
    DictCategory {
        genre: SfxGenre::Small,
        description: "Quiet mechanical clicks, footsteps, taps, clocks",
        words: &[
            "KNOCK", "CLICK", "TAP", "BEEP", "TICK", "TOCK", "CREAK", "RATTLE",
            "SQUEAK", "CLINK", "FLUTTER", "WHISPER",
        ],
    },
    DictCategory {
        genre: SfxGenre::Shock,
        description: "Gasping, heartbeats, sudden realizations, fear",
        words: &[
            "GASP!", "GULP…", "THUMP THUMP", "EEEK!", "YIKES!", "SHOCK!", "JOLT!",
            "CHOKE!", "FREEZE", "SHIVER", "PULSE",
        ],
    },
];

/// Result of analyzing text for sound-effect characteristics.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoPickResult {
    pub genre: SfxGenre,
    pub power: f64,
    pub roughness: f64,
    pub momentum: f64,
}

/// Normalizes a sound effect string for phonetic analysis and dictionary matching.
pub fn normalize_sfx(text: &str) -> String {
    text.trim()
        .to_uppercase()
        .replace(['.', '…', '!', '?', '-', '~', ' '], "")
}

/// Analyzes an English text string phonetically and against the dictionary to pick the best
/// sound category and compute macro intensity sliders (Power, Roughness, Momentum in 0..100).
pub fn auto_pick_sfx(text: &str) -> AutoPickResult {
    let clean = normalize_sfx(text);
    if clean.is_empty() {
        return AutoPickResult {
            genre: SfxGenre::Impact,
            power: 50.0,
            roughness: 50.0,
            momentum: 50.0,
        };
    }

    let mut scores = [0.0f64; 8];
    let genre_idx = |g: SfxGenre| match g {
        SfxGenre::Impact => 0,
        SfxGenre::Crash => 1,
        SfxGenre::Speed => 2,
        SfxGenre::Rumble => 3,
        SfxGenre::Water => 4,
        SfxGenre::Sparkle => 5,
        SfxGenre::Small => 6,
        SfxGenre::Shock => 7,
    };

    // 1. Direct dictionary match bonus (+4.0)
    for cat in &ENGLISH_DICTIONARY {
        for &w in cat.words {
            let nw = normalize_sfx(w);
            if clean == nw || clean.contains(&nw) || nw.contains(&clean) {
                scores[genre_idx(cat.genre)] += 4.0;
                break;
            }
        }
    }

    // 2. Character metrics
    let total_chars = clean.chars().count().max(1) as f64;
    let exclam_count = text.chars().filter(|&c| c == '!').count() as f64;
    let question_count = text.chars().filter(|&c| c == '?').count() as f64;

    // Check letter repetitions (e.g. RRRR, BOOOOM, ZZZZ)
    let chars: Vec<char> = clean.chars().collect();
    let mut repeat_count = 0;
    for i in 1..chars.len() {
        if chars[i] == chars[i - 1] {
            repeat_count += 1;
        }
    }
    let repeat_ratio = repeat_count as f64 / total_chars;

    // 3. Phonetic consonant and vowel classification
    let mut plosives = 0;  // B, P, D, T, K, G -> explosive hits
    let mut sibilants = 0; // S, Z, SH, CH, W -> cutting, speed, air
    let mut vibrants = 0;  // R, M, N -> humming, earthquake, rumble
    let mut high_vowels = 0; // I, E -> small, sharp, sparkle
    let mut low_vowels = 0;  // O, U, A -> deep, large, heavy

    for &c in &chars {
        match c {
            'B' | 'P' | 'D' | 'T' | 'K' | 'G' => plosives += 1,
            'S' | 'Z' | 'W' | 'F' | 'V' => sibilants += 1,
            'R' | 'M' | 'N' => vibrants += 1,
            'I' | 'E' | 'Y' => high_vowels += 1,
            'O' | 'U' | 'A' => low_vowels += 1,
            _ => {}
        }
    }

    let plosive_ratio = plosives as f64 / total_chars;
    let sibilant_ratio = sibilants as f64 / total_chars;
    let vibrant_ratio = vibrants as f64 / total_chars;
    let high_vowel_ratio = high_vowels as f64 / total_chars;
    let low_vowel_ratio = low_vowels as f64 / total_chars;

    // Substring patterns
    let has_sh = clean.contains("SH") || clean.contains("CH");
    let has_wh = clean.contains("WH") || clean.contains("OO");
    let has_ck = clean.contains("CK") || clean.contains("KRAK");
    let has_spl = clean.contains("SPL") || clean.contains("DRIP") || clean.contains("PLOP");
    let has_ing = clean.contains("ING") || clean.contains("INK");

    // Compute genre affinity scores
    // Impact: plosives, low vowels, exclamations, short punchy
    scores[0] += plosive_ratio * 3.0 + low_vowel_ratio * 1.5 + exclam_count.min(3.0) * 0.5;
    if clean.starts_with('B') || clean.starts_with('P') {
        scores[0] += 1.0;
    }

    // Crash: plosives + sibilants (shattering), CK, sharp stops
    scores[1] += plosive_ratio * 2.0 + (if has_ck { 2.0 } else { 0.0 }) + exclam_count.min(2.0) * 0.4;
    if clean.contains("CR") || clean.contains("SM") || clean.contains("BL") {
        scores[1] += 2.0;
    }

    // Speed: sibilants, SH/CH, WH, long vowels
    scores[2] += sibilant_ratio * 3.2 + (if has_sh { 2.0 } else { 0.0 }) + (if has_wh { 1.5 } else { 0.0 });
    if clean.starts_with('Z') || clean.starts_with('W') || clean.starts_with('S') {
        scores[2] += 1.5;
    }

    // Rumble: vibrants (R, M, N), repetitions, low vowels
    scores[3] += vibrant_ratio * 3.5 + repeat_ratio * 3.0 + low_vowel_ratio * 1.5;
    if clean.starts_with('R') || clean.starts_with('G') {
        scores[3] += 1.5;
    }

    // Water: SPL, PL, low sibilants
    scores[4] += (if has_spl { 3.5 } else { 0.0 }) + (if clean.contains("SL") { 1.5 } else { 0.0 });

    // Sparkle: high vowels, ING, B/P cute sounds
    scores[5] += high_vowel_ratio * 2.5 + (if has_ing { 2.0 } else { 0.0 });
    if clean.contains("POP") || clean.contains("BLING") || clean.contains("DING") {
        scores[5] += 2.5;
    }

    // Small: high vowels, short length, question marks, low exclamations
    scores[6] += (if total_chars <= 4.0 { 1.5 } else { 0.0 }) + high_vowel_ratio * 2.0 + question_count * 1.0;
    if clean.contains("CLICK") || clean.contains("TAP") || clean.contains("TICK") {
        scores[6] += 3.0;
    }

    // Shock: question marks, exclamation + question, GASP, GULP
    scores[7] += question_count * 2.0 + (if clean.contains("GASP") || clean.contains("GULP") { 4.0 } else { 0.0 });

    // Pick top genre
    let mut best_genre = SfxGenre::Impact;
    let mut best_score = -1.0;
    for g in SfxGenre::ALL {
        let s = scores[genre_idx(g)];
        if s > best_score {
            best_score = s;
            best_genre = g;
        }
    }

    // 4. Calculate macro slider recommendations (0..100, 50 neutral)
    let power = (50.0
        + plosive_ratio * 30.0
        + low_vowel_ratio * 15.0
        + exclam_count.min(3.0) * 8.0
        - (if best_genre == SfxGenre::Small { 25.0 } else { 0.0 })
        - (if best_genre == SfxGenre::Sparkle { 10.0 } else { 0.0 }))
        .clamp(10.0, 95.0);

    let roughness = (45.0
        + repeat_ratio * 35.0
        + vibrant_ratio * 20.0
        + (if best_genre == SfxGenre::Crash { 20.0 } else { 0.0 })
        + (if best_genre == SfxGenre::Rumble { 25.0 } else { 0.0 })
        - (if best_genre == SfxGenre::Sparkle || best_genre == SfxGenre::Small { 15.0 } else { 0.0 }))
        .clamp(10.0, 95.0);

    let momentum = (42.0
        + sibilant_ratio * 35.0
        + (if has_sh || has_wh { 20.0 } else { 0.0 })
        + (if best_genre == SfxGenre::Speed { 30.0 } else { 0.0 })
        + exclam_count.min(2.0) * 6.0)
        .clamp(10.0, 95.0);

    AutoPickResult {
        genre: best_genre,
        power,
        roughness,
        momentum,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_dictionary_words_match_their_genres() {
        assert_eq!(auto_pick_sfx("BOOM!").genre, SfxGenre::Impact);
        assert_eq!(auto_pick_sfx("CRASH!").genre, SfxGenre::Crash);
        assert_eq!(auto_pick_sfx("WHOOSH!").genre, SfxGenre::Speed);
        assert_eq!(auto_pick_sfx("RUMBLE").genre, SfxGenre::Rumble);
        assert_eq!(auto_pick_sfx("SPLASH!").genre, SfxGenre::Water);
        assert_eq!(auto_pick_sfx("SPARKLE").genre, SfxGenre::Sparkle);
        assert_eq!(auto_pick_sfx("CLICK").genre, SfxGenre::Small);
        assert_eq!(auto_pick_sfx("GASP!").genre, SfxGenre::Shock);
    }

    #[test]
    fn unlisted_words_phonetically_classify() {
        // High plosive and exclamation -> Impact
        let res = auto_pick_sfx("KABANGGG!!!");
        assert!(res.genre == SfxGenre::Impact || res.genre == SfxGenre::Crash);
        assert!(res.power > 60.0);

        // Repetitive vibrants -> Rumble
        let res2 = auto_pick_sfx("GRRRRRR");
        assert_eq!(res2.genre, SfxGenre::Rumble);
        assert!(res2.roughness > 60.0);

        // Fricative whoosh -> Speed
        let res3 = auto_pick_sfx("SWWWWOOOOSH");
        assert_eq!(res3.genre, SfxGenre::Speed);
        assert!(res3.momentum > 60.0);
    }
}

