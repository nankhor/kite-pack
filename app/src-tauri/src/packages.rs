//! Finding .pkg files in a folder the user picked for DLC: in it, or a couple of folders down
//! (a DLC folder sorted into one folder per add-on).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How many folders down to look: the folder itself and two levels inside it.
const DEPTH: usize = 3;

/// The .pkg files in `dir` and up to two folders down, in name order. Hidden files and folders
/// and folder links are left out. Fails only when `dir` itself can't be read.
pub fn packages_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    walk(dir, DEPTH, &mut found, true)?;
    found.sort();
    Ok(found)
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>, top: bool) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // A folder deeper down that can't be read is skipped; the picked one must be readable.
        Err(e) if top => return Err(e),
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_file() && path.extension().is_some_and(|x| x.eq_ignore_ascii_case("pkg")) {
            found.push(path);
        } else if kind.is_dir() && depth > 1 {
            walk(&path, depth - 1, found, false)?;
        }
    }
    Ok(())
}

/// How a package is named in "left out" notes: its path inside the picked folder.
pub fn shown_name(dir: &Path, pkg: &Path) -> String {
    pkg.strip_prefix(dir).unwrap_or(pkg).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn finds_packages_in_the_folder_and_two_levels_down() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        touch(&d.join("b.pkg"));
        touch(&d.join("A.PKG"));
        touch(&d.join("notes.txt"));
        touch(&d.join("Suits/suits.pkg"));
        touch(&d.join("Maps/Arena/arena.pkg"));
        touch(&d.join("Too/Deep/Down/deep.pkg"));
        touch(&d.join(".hidden/x.pkg"));
        touch(&d.join("._b.pkg"));

        let names: Vec<String> = packages_in(d).unwrap().iter().map(|p| shown_name(d, p)).collect();
        assert_eq!(names, ["A.PKG", "Maps/Arena/arena.pkg", "Suits/suits.pkg", "b.pkg"]);
    }

    #[test]
    fn a_dump_folder_has_no_packages() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("sce_sys/param.json"));
        assert!(packages_in(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn a_folder_that_cant_be_read_fails() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(packages_in(&tmp.path().join("missing")).is_err());
    }
}
