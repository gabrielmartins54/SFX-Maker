//! SFX library: saved SFX on disk, one folder per font, one file per SFX.
//!
//! Layout: `<root>/<font name>/<sfx name>.sfx.json`. The SFX name is derived from its text
//! and made unique inside the font's folder ("BOOM", "BOOM 2", ...).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::scene::Scene;

const EXT: &str = ".sfx.json";
const MAX_NAME: usize = 40;

#[derive(Serialize, Deserialize)]
struct FileFormat {
    font: String,
    scene: Scene,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub entries: Vec<Entry>,
}

pub struct Library {
    root: PathBuf,
}

/// Makes `s` safe as a file/folder name on Windows, macOS and Linux.
pub fn sanitize(s: &str, fallback: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(MAX_NAME).collect();
    out = out.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_owned();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit()));
    if out.is_empty() || reserved {
        fallback.to_owned()
    } else {
        out
    }
}

pub fn default_root() -> PathBuf {
    directories::ProjectDirs::from("", "", "sfx_maker")
        .map_or_else(|| PathBuf::from("library"), |d| d.data_dir().join("library"))
}

impl Library {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Saves `scene` into the folder for `font`, named after its text. Returns the new path.
    pub fn save(&self, font: &str, scene: &Scene) -> Result<PathBuf> {
        let dir = self.root.join(sanitize(font, "Unknown font"));
        std::fs::create_dir_all(&dir).with_context(|| format!("Creating {}", dir.display()))?;
        let base = sanitize(&scene.text, "SFX");
        let mut name = base.clone();
        let mut n = 2;
        while dir.join(format!("{name}{EXT}")).exists() {
            name = format!("{base} {n}");
            n += 1;
        }
        let path = dir.join(format!("{name}{EXT}"));
        let json = serde_json::to_string(&FileFormat { font: font.to_owned(), scene: scene.clone() })?;
        std::fs::write(&path, json).with_context(|| format!("Writing {}", path.display()))?;
        Ok(path)
    }

    /// All folders (sorted by name) with their entries (sorted by name). Empty folders are hidden.
    pub fn scan(&self) -> Vec<Folder> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut folders: Vec<Folder> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| Folder { name: e.file_name().to_string_lossy().into_owned(), entries: scan_folder(&e.path()) })
            .filter(|f| !f.entries.is_empty())
            .collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        folders
    }

    /// Returns the font name it was saved with and the scene.
    pub fn load(path: &Path) -> Result<(String, Scene)> {
        let json = std::fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
        let mut f: FileFormat = serde_json::from_str(&json).context("Not a valid library file")?;
        f.scene.params = f.scene.params.clamped();
        f.scene.text_letter_count = f.scene.text_letter_count.min(f.scene.letters.len());
        Ok((f.font, f.scene))
    }

    /// Deletes an entry (and its folder if that was the last one).
    pub fn delete(path: &Path) -> Result<()> {
        std::fs::remove_file(path).with_context(|| format!("Deleting {}", path.display()))?;
        if let Some(dir) = path.parent() {
            // Only succeeds when empty, which is exactly what we want.
            let _ = std::fs::remove_dir(dir);
        }
        Ok(())
    }
}

fn scan_folder(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(EXT)?.to_owned();
            Some(Entry { name, path: e.path() })
        })
        .collect();
    v.sort_by_key(|e| e.name.to_lowercase());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("sfx_lib_test_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn scene(text: &str) -> Scene {
        Scene { text: text.to_owned(), ..Scene::default() }
    }

    #[test]
    fn sanitize_cases() {
        assert_eq!(sanitize("BOOM!", "x"), "BOOM!");
        assert_eq!(sanitize("a/b:c\nd", "x"), "a b c d");
        assert_eq!(sanitize("  ..  ", "x"), "x");
        assert_eq!(sanitize("CON", "x"), "x");
        assert_eq!(sanitize("", "SFX"), "SFX");
        assert_eq!(sanitize(&"a".repeat(100), "x").len(), MAX_NAME);
    }

    #[test]
    fn save_names_after_text_and_dedups() {
        let root = temp_root("save");
        let lib = Library::new(root.clone());
        let a = lib.save("Bangers", &scene("BOOM")).unwrap();
        let b = lib.save("Bangers", &scene("BOOM")).unwrap();
        let c = lib.save("Other Font", &scene("")).unwrap();
        assert!(a.ends_with("Bangers/BOOM.sfx.json") || a.ends_with("Bangers\\BOOM.sfx.json"));
        assert_eq!(b.file_name().unwrap().to_string_lossy(), "BOOM 2.sfx.json");
        assert_eq!(c.file_name().unwrap().to_string_lossy(), "SFX.sfx.json");

        let folders = lib.scan();
        assert_eq!(folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Bangers", "Other Font"]);
        assert_eq!(folders[0].entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["BOOM", "BOOM 2"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_roundtrip_and_delete_removes_empty_folder() {
        let root = temp_root("load");
        let lib = Library::new(root.clone());
        let p = lib.save("F", &scene("ZAP")).unwrap();
        let (font, s) = Library::load(&p).unwrap();
        assert_eq!((font.as_str(), s.text.as_str()), ("F", "ZAP"));
        Library::delete(&p).unwrap();
        assert!(lib.scan().is_empty());
        assert!(!root.join("F").exists());
        assert!(Library::load(&p).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}

