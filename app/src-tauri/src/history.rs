//! Finished jobs, newest first, for the History tab.
//!
//! The list is one small JSON file. Each game's icon (a `data:` URL of up to a few hundred KB)
//! sits in a file of its own beside it, so the list stays quick to read and write.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

/// The most entries kept; the oldest drop off.
pub const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The job's id: one entry per job.
    pub id: String,
    pub title: String,
    pub title_id: Option<String>,
    /// When it finished, in ms since 1970.
    pub finished_at: u64,
    /// The game before packing.
    pub source_bytes: u64,
    /// The packed copy.
    pub packed_bytes: Option<u64>,
    /// It reached the PS5.
    pub sent: bool,
    /// The packed copy, when it was kept on this computer.
    pub packed: Option<PathBuf>,
    /// DLC installed along with it.
    pub dlc: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_data_url: Option<String>,
}

pub struct History {
    dir: PathBuf,
}

impl History {
    pub fn new(dir: PathBuf) -> History {
        History { dir }
    }

    fn list_path(&self) -> PathBuf {
        self.dir.join("history.json")
    }

    fn icons(&self) -> PathBuf {
        self.dir.join("history-icons")
    }

    /// Where an entry's icon goes; none for an id that isn't a plain file name.
    fn icon_path(&self, id: &str) -> Option<PathBuf> {
        let plain = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        plain.then(|| self.icons().join(format!("{id}.txt")))
    }

    fn read_list(&self) -> Vec<Entry> {
        fs::read(self.list_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write_list(&self, entries: &[Entry]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let bare: Vec<Entry> = entries.iter().map(|e| Entry { icon_data_url: None, ..e.clone() }).collect();
        let tmp = self.dir.join("history.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&bare)?)?;
        fs::rename(tmp, self.list_path())
    }

    /// Every entry, newest first, with its icon. Empty when there is no list or it can't be read.
    pub fn load(&self) -> Vec<Entry> {
        let mut entries = self.read_list();
        for e in &mut entries {
            e.icon_data_url = self.icon_path(&e.id).and_then(|p| fs::read_to_string(p).ok());
        }
        entries
    }

    /// Puts `entry` first, in place of an older one with the same id.
    pub fn add(&self, entry: Entry) -> io::Result<()> {
        let mut entries = self.read_list();
        entries.retain(|e| e.id != entry.id);
        if let (Some(icon), Some(path)) = (&entry.icon_data_url, self.icon_path(&entry.id)) {
            fs::create_dir_all(self.icons())?;
            fs::write(path, icon)?;
        }
        entries.insert(0, entry);
        for gone in entries.drain(MAX_ENTRIES.min(entries.len())..) {
            if let Some(path) = self.icon_path(&gone.id) {
                let _ = fs::remove_file(path);
            }
        }
        self.write_list(&entries)
    }

    pub fn clear(&self) -> io::Result<()> {
        gone_already(fs::remove_file(self.list_path()))?;
        gone_already(fs::remove_dir_all(self.icons()))
    }
}

/// A removal that found nothing to remove counts as done.
fn gone_already(removed: io::Result<()>) -> io::Result<()> {
    match removed {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, icon: Option<&str>) -> Entry {
        Entry {
            id: id.into(),
            title: format!("Game {id}"),
            title_id: Some("PPSA01234".into()),
            finished_at: 1,
            source_bytes: 84_200_000_000,
            packed_bytes: Some(51_600_000_000),
            sent: true,
            packed: None,
            dlc: 2,
            icon_data_url: icon.map(String::from),
        }
    }

    #[test]
    fn empty_without_a_list() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(History::new(tmp.path().into()).load().is_empty());
    }

    #[test]
    fn newest_first_with_icons() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path().into());
        h.add(entry("a", Some("data:image/png;base64,AAAA"))).unwrap();
        h.add(entry("b", None)).unwrap();
        let all = h.load();
        assert_eq!(all.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(all[1].icon_data_url.as_deref(), Some("data:image/png;base64,AAAA"));
        assert_eq!(all[0].icon_data_url, None);
        // The list itself carries no icons.
        let list = fs::read_to_string(tmp.path().join("history.json")).unwrap();
        assert!(!list.contains("base64"));
    }

    #[test]
    fn one_entry_per_job() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path().into());
        h.add(entry("a", None)).unwrap();
        h.add(entry("b", None)).unwrap();
        h.add(Entry { dlc: 5, ..entry("a", None) }).unwrap();
        let all = h.load();
        assert_eq!(all.len(), 2);
        assert_eq!((all[0].id.as_str(), all[0].dlc), ("a", 5));
    }

    #[test]
    fn keeps_the_newest_and_drops_their_icons() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path().into());
        for i in 0..=MAX_ENTRIES {
            h.add(entry(&format!("j{i}"), Some("data:image/png;base64,AAAA"))).unwrap();
        }
        let all = h.load();
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(all[0].id, format!("j{MAX_ENTRIES}"));
        assert!(!tmp.path().join("history-icons/j0.txt").exists());
    }

    #[test]
    fn an_odd_id_keeps_no_icon_file() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path().into());
        h.add(entry("../x", Some("data:image/png;base64,AAAA"))).unwrap();
        assert_eq!(h.load()[0].icon_data_url, None);
        assert!(!tmp.path().join("x.txt").exists());
    }

    #[test]
    fn clear_removes_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path().into());
        h.add(entry("a", Some("data:image/png;base64,AAAA"))).unwrap();
        h.clear().unwrap();
        assert!(h.load().is_empty());
        assert!(!tmp.path().join("history-icons").exists());
        h.clear().unwrap();
    }

    #[test]
    fn a_damaged_list_reads_as_empty() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("history.json"), b"{not json").unwrap();
        assert!(History::new(tmp.path().into()).load().is_empty());
    }
}
