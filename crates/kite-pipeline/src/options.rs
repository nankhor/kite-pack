use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What the packed game is written as. Never `.ffpfsc`, `.pkg` or LZ4 (spec: version 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Folder,
    Exfat,
    Ffpkg,
    Ffpfs,
}

/// Forge's three zstd presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Fast,
    Medium,
    Max,
}

impl Level {
    pub fn zstd_level(self) -> u8 {
        match self {
            Level::Fast => 5,
            Level::Medium => 9,
            Level::Max => 19,
        }
    }
}

/// Where a packed game goes on the PS5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendTarget {
    /// Host name or IP address of the console.
    pub console: String,
    /// A folder ShadowMount+ watches, e.g. `/data/homebrew`.
    pub destination: String,
}

/// Add-ons and a backport to put inside the packed copy, before it is written.
///
/// The DLC are installed into the game as the DLC emulator reads them: each one's files under
/// an `addcontN/` folder, an entry in `dlc_emu.ini`, and the emulator's modules in `fakelib/`.
/// A dump that already carries the emulator needs no `emulator` here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Bundle {
    /// DLC folders or fake `.pkg` files.
    pub dlc: Vec<PathBuf>,
    /// A backport folder: its files replace the game's (eboot.bin, sce_sys/, fakelib/).
    pub backport: Option<PathBuf>,
    /// The unzipped dlc_emu release, when the game's own `fakelib/` has no emulator.
    pub emulator: Option<PathBuf>,
}

impl Bundle {
    pub fn is_empty(&self) -> bool {
        self.dlc.is_empty() && self.backport.is_none()
    }
}

/// Everything the user chose before pressing Pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    pub format: OutputFormat,
    pub level: Level,
    pub output_dir: PathBuf,
    pub delete_original: bool,
    pub send: Option<SendTarget>,
    pub delete_packed: bool,
    /// DLC to install on the PS5 after the send (not to pack in).
    pub dlc: Vec<PathBuf>,
    /// DLC and a backport to put inside the packed copy.
    #[serde(default)]
    pub bundle: Bundle,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OptionsError {
    #[error("Delete the packed folder needs Send to the PS5")]
    DeletePackedWithoutSend,
    #[error("DLC can only be installed when sending to the PS5")]
    DlcWithoutSend,
}

impl Options {
    pub fn validate(&self) -> Result<(), OptionsError> {
        if self.send.is_none() && self.delete_packed {
            return Err(OptionsError::DeletePackedWithoutSend);
        }
        if self.send.is_none() && !self.dlc.is_empty() {
            return Err(OptionsError::DlcWithoutSend);
        }
        Ok(())
    }
}
