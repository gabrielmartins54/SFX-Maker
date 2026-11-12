//! Presets: named `Params` bundles. Built-ins are code; user presets live in a JSON file
//! in the platform config directory.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::params::Params;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub params: Params,
}

impl Preset {
    pub fn new(name: &str, params: Params) -> Self {
        Self { name: name.to_owned(), params }
    }
}

pub fn builtin() -> Vec<Preset> {
    let d = Params::default;
    vec![
        Preset::new("Plain", d()),
        // The web tool's default "Impact" look.
        Preset::new(
            "Impact",
            Params {
                start_size: 0.78,
                end_size: 1.30,
                tracking: 0.10,
                lean: 0.35,
                rise: 6.0,
                rotation_jitter: 7.0,
                spacing_by_size: 0.5,
                ..d()
            },
        ),
        Preset::new(
            "Crash",
            Params {
                start_size: 1.05,
                end_size: 1.25,
                tracking: -0.06,
                rotation_jitter: 9.0,
                scale_jitter: 0.10,
                offset_jitter: 0.03,
                lean: 0.30,
                ..d()
            },
        ),
        Preset::new(
            "Slash",
            Params {
                start_size: 1.25,
                end_size: 0.85,
                slant: 18.0,
                tracking: -0.05,
                lean: 0.70,
                rise: 6.0,
                ..d()
            },
        ),
        Preset::new(
            "Rumble",
            Params {
                start_size: 1.35,
                end_size: 0.65,
                tracking: 0.04,
                rotation_jitter: 5.0,
                baseline_wobble: 0.10,
                wave: 0.08,
                wave_cycles: 3.0,
                lean: 0.60,
                ..d()
            },
        ),
        Preset::new(
            "Splash",
            Params {
                start_size: 0.90,
                end_size: 1.20,
                tracking: -0.03,
                bend: 0.08,
                lean: 0.50,
                ..d()
            },
        ),
        Preset::new(
            "Sparkle",
            Params {
                start_size: 1.0,
                end_size: 1.10,
                tracking: -0.02,
                rotation_jitter: 10.0,
                offset_jitter: 0.06,
                lean: 0.40,
                ..d()
            },
        ),
        Preset::new(
            "Small",
            Params {
                start_size: 1.0,
                end_size: 1.0,
                tracking: 0.08,
                lean: 0.0,
                rotation_jitter: 0.0,
                ..d()
            },
        ),
        Preset::new(
            "Shock",
            Params {
                start_size: 0.90,
                end_size: 1.30,
                slant: -8.0,
                tracking: -0.04,
                lean: 0.50,
                bend: -0.10,
                ..d()
            },
        ),
        Preset::new(
            "BOOM (rising, tight)",
            Params { tracking: -0.08, end_size: 1.8, slant: 8.0, rotation_jitter: 4.0, scale_jitter: 0.08, ..d() },
        ),
        Preset::new("Arc", Params { bend: 0.5, tracking: 0.03, ..d() }),
        Preset::new("Wave", Params { wave: 0.25, wave_cycles: 1.5, rotation_jitter: 3.0, ..d() }),
        Preset::new("Zoom (italic, growing)", Params { slant: 22.0, end_size: 2.4, tracking: -0.05, ..d() }),
        Preset::new(
            "Chaos",
            Params {
                rotation_jitter: 22.0,
                scale_jitter: 0.35,
                offset_jitter: 0.12,
                baseline_wobble: 0.15,
                tracking: -0.05,
                ..d()
            },
        ),
    ]
}

pub fn user_presets_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "sfx_maker").map(|d| d.config_dir().join("presets.json"))
}

pub fn parse_presets(json: &str) -> Vec<Preset> {
    serde_json::from_str::<Vec<Preset>>(json)
        .map(|v| v.into_iter().map(|p| Preset { params: p.params.clamped(), ..p }).collect())
        .unwrap_or_default()
}

/// Missing or corrupt files simply yield no user presets.
pub fn load_user_presets() -> Vec<Preset> {
    user_presets_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| parse_presets(&s))
        .unwrap_or_default()
}

pub fn save_user_presets(presets: &[Preset]) -> anyhow::Result<()> {
    use anyhow::Context;
    let path = user_presets_path().context("No config directory available")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("Creating {}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(presets)?;
    std::fs::write(&path, json).with_context(|| format!("Writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_in_range_and_unique() {
        let b = builtin();
        for p in &b {
            assert_eq!(p.params, p.params.clamped(), "{} out of range", p.name);
        }
        let mut names: Vec<_> = b.iter().map(|p| &p.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), b.len());
    }

    #[test]
    fn json_roundtrip_and_garbage() {
        let v = builtin();
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(parse_presets(&json), v);
        assert!(parse_presets("not json").is_empty());
    }

    #[test]
    fn old_files_missing_fields_still_load() {
        let json = r#"[{"name":"old","params":{"tracking":0.2}}]"#;
        let p = parse_presets(json);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].params.tracking, 0.2);
        assert_eq!(p[0].params.wave_cycles, 2.0);
    }
}

