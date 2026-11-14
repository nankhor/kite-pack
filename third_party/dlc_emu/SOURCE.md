# dlc_emu, vendored (GPL-3.0)

From https://github.com/drakmor/dlc_emu, tag **0.3** (commit `43ae9aa406214d04cbb54c409e712ceb64f1f867`).

- `src/`, `msbuild/`, `tools/`, `DlcEmu.sln`, `find_codes.py`, `dlc_emu.example.ini`, `README.md`,
  `VERSION`, `LICENSE` — the tag's tree, unchanged.
- `bin/` — the three modules from the official `dlc_emu_0.3.zip` release, `nolog/` build (logging
  off, which is what a packed game wants). Kite Pack copies these into a packed game's `fakelib/`
  when the dump carries no DLC emulator of its own.

Why vendored rather than downloaded: Kite Pack embeds the modules, so DLC packing works offline
and without a setup step. GPL-3.0 asks for the corresponding source, so it travels here beside
them. Kite Pack does not modify them; a newer upstream release is a deliberate update of this
folder (and `crates/kite-forge/src/dlc_emu.rs`'s version).
