//! Kite Pack's bridge to PS5 Dump Forge (quer3q, GPL-3.0-or-later): reading games and packing
//! them with zstd.

mod inspect;

pub use inspect::{DlcInfo, GameInfo, SourceProblem, check_dlc, inspect_game};
