use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// The sizes and change times of every file in the original, taken before packing. Deleting
/// the original is refused unless a fresh snapshot is identical: nothing changed since packing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Sorted by path.
    pub entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// Relative to the root, `/`-separated; empty when the root is itself a file.
    pub path: String,
    pub len: u64,
    /// Nanoseconds since the Unix epoch (negative before it).
    pub modified_ns: i128,
}

impl Snapshot {
    pub fn take(root: &Path) -> io::Result<Snapshot> {
        let meta = std::fs::symlink_metadata(root)?;
        if !meta.is_dir() {
            return Ok(Snapshot { entries: vec![entry(String::new(), &meta)?] });
        }
        let mut entries = Vec::new();
        for item in walkdir::WalkDir::new(root).follow_links(false).min_depth(1) {
            let item = item.map_err(io::Error::other)?;
            if item.file_type().is_dir() {
                continue;
            }
            let rel = item.path().strip_prefix(root).map_err(io::Error::other)?;
            let path = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let meta = item.path().symlink_metadata()?;
            let mut e = entry(path, &meta)?;
            if item.file_type().is_symlink() {
                e.len = 0;
            }
            entries.push(e);
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Snapshot { entries })
    }

    /// Whether `root` still looks exactly as it did; a missing root never does.
    pub fn matches(&self, root: &Path) -> io::Result<bool> {
        match Snapshot::take(root) {
            Ok(now) => Ok(now == *self),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

fn entry(path: String, meta: &std::fs::Metadata) -> io::Result<SnapshotEntry> {
    let modified = meta.modified()?;
    let modified_ns = match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    };
    Ok(SnapshotEntry { path, len: meta.len(), modified_ns })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn game(dir: &Path) -> PathBuf {
        let root = dir.join("game");
        fs::create_dir_all(root.join("sce_sys")).unwrap();
        fs::write(root.join("eboot.bin"), b"boot").unwrap();
        fs::write(root.join("sce_sys/param.json"), b"{}").unwrap();
        root
    }

    #[test]
    fn unchanged_folder_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let root = game(tmp.path());
        let snap = Snapshot::take(&root).unwrap();
        assert_eq!(snap.entries.len(), 2);
        assert!(snap.matches(&root).unwrap());
    }

    #[test]
    fn changed_size_does_not_match() {
        let tmp = tempfile::tempdir().unwrap();
        let root = game(tmp.path());
        let snap = Snapshot::take(&root).unwrap();
        fs::write(root.join("eboot.bin"), b"boot!").unwrap();
        assert!(!snap.matches(&root).unwrap());
    }

    #[test]
    fn added_file_does_not_match() {
        let tmp = tempfile::tempdir().unwrap();
        let root = game(tmp.path());
        let snap = Snapshot::take(&root).unwrap();
        fs::write(root.join("new.bin"), b"x").unwrap();
        assert!(!snap.matches(&root).unwrap());
    }

    #[test]
    fn removed_file_does_not_match() {
        let tmp = tempfile::tempdir().unwrap();
        let root = game(tmp.path());
        let snap = Snapshot::take(&root).unwrap();
        fs::remove_file(root.join("eboot.bin")).unwrap();
        assert!(!snap.matches(&root).unwrap());
    }

    #[test]
    fn touched_file_does_not_match() {
        let tmp = tempfile::tempdir().unwrap();
        let root = game(tmp.path());
        let snap = Snapshot::take(&root).unwrap();
        let f = fs::File::options().write(true).open(root.join("eboot.bin")).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(10)).unwrap();
        assert!(!snap.matches(&root).unwrap());
    }

    #[test]
    fn single_file_source() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("game.ffpkg");
        fs::write(&file, b"image bytes").unwrap();
        let snap = Snapshot::take(&file).unwrap();
        assert_eq!(snap.entries.len(), 1);
        assert_eq!(snap.entries[0].path, "");
        assert_eq!(snap.entries[0].len, 11);
        assert!(snap.matches(&file).unwrap());
    }

    #[test]
    fn missing_root_does_not_match() {
        let tmp = tempfile::tempdir().unwrap();
        let root = game(tmp.path());
        let snap = Snapshot::take(&root).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert!(!snap.matches(&root).unwrap());
    }
}
