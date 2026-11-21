# ps5-dump-forge-lz4, with ampr-emu-ng v0.5.0

Packed games run through ampr-emu-ng, the AMPR emulator that reads zstd packs. Every pack writes it into
the game at `fakelib/libSceAmpr.sprx`. Forge embeds that file in its `ps5-dump-forge-lz4` crate, so the
version a packed game gets is whatever Forge carries. Forge v0.5.6 (the commit Kite Pack pins) carries
ampr-emu-ng v0.4.1. This folder is that crate with v0.5.0 in it instead, put in place by the
`[patch."https://github.com/quer3q/ps5-dump-forge"]` entry in the root `Cargo.toml`.

| | |
|---|---|
| Crate | `crates/ps5-dump-forge-lz4` of https://github.com/quer3q/ps5-dump-forge (GPL-3.0-or-later) |
| Commit | `9e3c9c1a38157b2d560345bee0acff1865bfb6f5`, the same one Kite Pack pins |
| Emulator | ampr-emu-ng v0.5.0, https://github.com/quer3q/ampr-emu-ng/releases/tag/v0.5.0 (published 2026-11-20) |
| Emulator tag | `v0.5.0` (commit `d20210e224871864bee0888193c7c1062890639c`) |

## What differs from Forge's crate

- `ampr-emu-ng/` holds the v0.5.0 release's `libSceAmpr.sprx` byte for byte (715264 bytes, SHA-256
  `7379e49953d8f14f26b45ff06a863ecea56aa90a60ba3d5a375f61c52618a174`, as the release's `SHA256SUMS` lists;
  the debug build's line is left out because the debug build isn't here), and the tag's source
  (`git archive --prefix=ampr-emu-ng-0.5.0/ v0.5.0 | gzip -n -9`), which GPL-3.0 asks to travel with it.
- `ampr_emu/` is Forge's `vendor/ampr_emu/` unchanged (the older upstream runtime the crate also embeds).
- `build.rs` reads both runtimes from these two folders instead of `../../vendor/`.
- `src/runtime.rs`: `NG_VERSION` is `0.5.0`, `ZSTD_SHA256` and the size test name the v0.5.0 file, and
  v0.4.1's hash joins the list of older builds the crate still recognises.
- `Cargo.toml` spells out what Forge's workspace supplied (version 0.5.6, edition, licence, dependency
  versions), with `ps5upload-fpkg` taken from the same Forge commit.

Everything else in `src/` is Forge's, unchanged.

## Why it is safe

v0.5.0 reads the same files Forge writes. The zstd and LZ4 pack formats and the `ampr_emu.index` format are
unchanged between v0.4.1 and v0.5.0 (the magics and versions in `crates/ampr-format` are the same; the
binary carries `AMPRZPK1`, `AMPRPAK4`, `AMPRDAT3` and `AMPRIDX3`). The release notes cover startup, crash
and save fixes, among them:

- the runtime takes 151 MiB of the game's memory, down from 184 MiB in v0.4.1, with a 32 MiB read cache
  that grows into room the index leaves idle once it is built;
- saves: a writing save mount is never handed a read-only one, and backported titles that report an SDK
  below 5.00 can create saves again (Castlevania: Belmont's Curse);
- startup: a file the index lists but the disk lacks is "not found" instead of an error (Demon Slayer
  -Kimetsu no Yaiba- The Hinokami Chronicles), a folder read in one call lists its packed files (Marvel's
  Avengers), and an index whose paths collide in the runtime's hash is no longer refused;
- crashes: a read into game memory that isn't mapped yet waits instead of faulting (Final Fantasy XVI).

The test `packed_games_get_the_emulator_kite_pack_carries` (`crates/kite-forge/tests/pack.rs`) packs a game
and checks that its `fakelib/libSceAmpr.sprx` is this file.

## Updating

When Forge ships v0.5.0 (or later) itself: delete this folder and the `[patch]` entry, then move the Forge
pin in the root `Cargo.toml`. For a newer emulator before Forge has it: replace `ampr-emu-ng/` with that
release, then update `NG_VERSION`, `ZSTD_SHA256` and the size test in `src/runtime.rs` (keep the old hash
in the recognised list). Moving the Forge pin means taking that commit's crate again.
