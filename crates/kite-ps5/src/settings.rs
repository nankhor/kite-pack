//! The app's PS5 settings, saved as JSON in its data folder.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The PS5's address, found on the network or typed.
    pub address: Option<String>,
    pub helper_port: u16,
    pub loader_port: u16,
    /// Where games go on the PS5: a folder ShadowMount+ watches.
    pub destination: String,
    /// Where packed copies go on this computer; None for beside the game.
    pub packed_folder: Option<PathBuf>,
    /// The unzipped dlc_emu release: its modules go into a game's `fakelib/` when DLC is packed
    /// into it. None until the user picks one; a dump that already carries them needs none.
    pub dlc_tools: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            address: None,
            helper_port: crate::DISCOVERY_PORT,
            loader_port: 9021,
            destination: "/data/homebrew".into(),
            packed_folder: None,
            dlc_tools: None,
        }
    }
}

impl Settings {
    /// The saved settings; the defaults when there are none or they can't be read.
    pub fn load(path: &Path) -> Settings {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }
}
