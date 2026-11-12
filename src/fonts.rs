//! Font discovery. Fonts are never read from the system: the user points the app at
//! folders of fonts, and the bundled default font is always available.

use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontSource {
    Bundled,
    File(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontEntry {
    /// Display name and library folder name (file stem for files).
    pub name: String,
    pub source: FontSource,
}

pub fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
}

/// Fonts found in `dir` (searched up to a few levels deep), sorted by name.
/// Unreadable directories are skipped silently.
pub fn scan_dir(dir: &Path) -> Vec<FontEntry> {
    let mut found = Vec::new();
    walk(dir, 0, &mut found);
    found.sort_by_key(|e| e.name.to_lowercase());
    found
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<FontEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_DEPTH {
                walk(&path, depth + 1, out);
            }
        } else if is_font_file(&path) {
            let name = path
                .file_stem()
                .map_or_else(|| "font".to_owned(), |s| s.to_string_lossy().into_owned());
            out.push(FontEntry { name, source: FontSource::File(path) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_font_extensions() {
        assert!(is_font_file(Path::new("a/B.TTF")));
        assert!(is_font_file(Path::new("x.otf")));
        assert!(!is_font_file(Path::new("x.txt")));
        assert!(!is_font_file(Path::new("noext")));
    }

    #[test]
    fn scans_recursively_and_sorts() {
        let root = std::env::temp_dir().join(format!("sfx_fonts_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("b.ttf"), b"x").unwrap();
        std::fs::write(root.join("sub").join("A.otf"), b"x").unwrap();
        std::fs::write(root.join("readme.txt"), b"x").unwrap();
        let names: Vec<_> = scan_dir(&root).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["A", "b"]);
        let _ = std::fs::remove_dir_all(&root);
        assert!(scan_dir(&root).is_empty());
    }
}

