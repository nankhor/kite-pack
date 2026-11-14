//! The DLC emulator Kite Pack carries.
//!
//! A packed game sees its add-ons through `drakmor/dlc_emu`: three replacement system modules
//! that answer "what DLC do I own / is it installed" from `/app0/dlc_emu.ini`. They are generic
//! (one build for every game); the per-game part is the ini Kite Pack writes.
//!
//! The modules and their source are vendored under `third_party/dlc_emu/` (GPL-3.0). They are
//! compiled in here rather than read from a folder, so DLC packing works with no setup step and
//! offline. The `nolog` release build is used: a packed game has no use for the emulator's log.
//!
//! Updating means replacing `third_party/dlc_emu/` with a newer tag (source and `bin/`), moving
//! [`VERSION`], and re-running the tests — the names are the contract with a game's `fakelib/`.

/// The upstream release these modules came from.
pub const VERSION: &str = "0.3";

/// One replacement module, by the name a game's `fakelib/` expects.
pub struct Module {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

/// The three modules, as they go into `fakelib/`.
pub fn modules() -> [Module; 3] {
    [
        Module {
            name: "libSceAppContent.sprx",
            bytes: include_bytes!("../../../third_party/dlc_emu/bin/libSceAppContent.sprx"),
        },
        Module {
            name: "libSceNpEntitlementAccess.sprx",
            bytes: include_bytes!("../../../third_party/dlc_emu/bin/libSceNpEntitlementAccess.sprx"),
        },
        Module {
            name: "libSceGameUpdate.sprx",
            bytes: include_bytes!("../../../third_party/dlc_emu/bin/libSceGameUpdate.sprx"),
        },
    ]
}

/// Whether a file name is one of the modules (any extension: releases ship `.prx` and `.sprx`).
pub fn is_module(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    modules().iter().any(|m| {
        let stem = m.name.trim_end_matches(".sprx").to_ascii_lowercase();
        lower.starts_with(&stem)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modules_are_embedded_and_named_as_a_fakelib_expects() {
        let modules = modules();
        assert_eq!(modules.len(), 3);
        for m in &modules {
            assert!(m.name.starts_with("libSce") && m.name.ends_with(".sprx"), "{}", m.name);
            assert!(m.bytes.len() > 1000, "{} is empty", m.name);
            // A PS5 fake-self module: the SELF magic, then its version words.
            assert_eq!(&m.bytes[..4], &[0x4f, 0x15, 0x3d, 0x1d], "{} is not a module", m.name);
        }
        assert_eq!(VERSION, "0.3");
    }

    #[test]
    fn names_are_recognised_with_either_extension() {
        assert!(is_module("libSceAppContent.sprx"));
        assert!(is_module("libSceAppContent.prx"));
        assert!(is_module("libSceNpEntitlementAccess.sprx"));
        assert!(!is_module("libSceVideoOut.sprx"));
    }
}
