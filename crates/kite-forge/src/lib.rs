//! Kite Pack's bridge to PS5 Dump Forge (quer3q, GPL-3.0-or-later): reading games and packing
//! them with zstd.

pub mod bundle;
pub mod dlc_emu;
mod inspect;
mod let_go;
mod packer;
mod words;

pub use bundle::{BundleItem, BundlePlan, Staged};
pub use inspect::{DlcInfo, DlcPackage, GameInfo, SourceProblem, check_dlc, inspect_dlc, inspect_game};
pub use packer::{ForgePacker, to_forge_format};
