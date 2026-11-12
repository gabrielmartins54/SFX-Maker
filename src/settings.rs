//! Persistent app settings (currently: the font folders the user added).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub font_dirs: Vec<PathBuf>,
}

fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "sfx_maker").map(|d| d.config_dir().join("settings.json"))
}

impl Settings {
    /// Missing or corrupt settings fall back to defaults.
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        use anyhow::Context;
        let p = path().context("No config directory available")?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("Creating {}", dir.display()))?;
        }
        std::fs::write(&p, serde_json::to_string_pretty(self)?).with_context(|| format!("Writing {}", p.display()))
    }

    /// Adds a font folder; returns false if it was already present.
    pub fn add_font_dir(&mut self, dir: PathBuf) -> bool {
        if self.font_dirs.contains(&dir) {
            return false;
        }
        self.font_dirs.push(dir);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dir_dedups_and_roundtrips() {
        let mut s = Settings::default();
        assert!(s.add_font_dir("C:/fonts".into()));
        assert!(!s.add_font_dir("C:/fonts".into()));
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), Settings::default());
    }
}

