# ps5-dump-forge-core, with a free-space check that goes by a sample

Before a pack starts, Forge checks the output folder has room. For a zstd pack into a folder it
assumes nothing compresses: it asks for the whole game's size plus 2%. A 580 GB game that packs to
300 GB was refused with 320 GB free.

This folder is the `ps5-dump-forge-core` crate with one option added. The root `Cargo.toml` puts it in
place with the `[patch."https://github.com/quer3q/ps5-dump-forge"]` entry.

| | |
|---|---|
| Crate | `crates/ps5-dump-forge-core` of https://github.com/quer3q/ps5-dump-forge (GPL-3.0-or-later) |
| Commit | `9e3c9c1a38157b2d560345bee0acff1865bfb6f5`, the same one Kite Pack pins |
| Left out | `tests/` (they need Forge's own workspace) |

## The change

`ConvertRequest::sample_space` (default off; Kite Pack turns it on for folder packs). With it, and a
zstd pack from a folder of at least 4 GiB of packed files, the check counts the packed files at what a
sample of them compresses to (`Plan::sampled_bytes` in `src/zstd_pack.rs`, called from
`src/convert.rs`):

1. **The sample.** 1,024 of the game's 256 KiB chunks (256 MiB), one from the middle of each of
   1,024 equal runs of chunks, so every part of the game is in it.
2. **Packed as the packer would.** Each goes through the packer's own `encode_chunk` at the job's
   level (`ps5_dump_forge_zstd::writer::packed_sizes`, added to `third_party/forge-zstd`), raw where
   zstd can't make it smaller.
3. **A margin.** The packed share of the sample is raised by three standard errors, and by at least
   3 points, and never past 100%. The volume headers, the index and Forge's own 2% + 64 MiB come on top.

At Max (level 19) the sample takes a few seconds on all cores. Should a game still shrink less than
that, writing fails with the drive full and Forge removes its `.part` folder, as on any error.

Images are untouched: Forge already measures them exactly before writing. Anything else (an archive
or an image as the source, a small game) keeps the worst case.

The only other edits are this README, `Cargo.toml` (written out, no workspace inheritance) and the
tests in `src/zstd_pack.rs`.

## Second change: letting parts go

When Kite Pack sets a volume hook (`ps5_dump_forge_zstd::hook`, see `third_party/forge-zstd/README.md`)
for a folder pack that sends and deletes the packed folder:

- **The space check** counts the hook's most held parts (4 × 4 GiB) instead of the whole packed size
  (`Plan::held_bytes`).
- **The end check** (`zstd_pack::checks`) accepts the volumes the hook cut down to their header: their
  header must still agree with the index. Files with a chunk in a cut volume are left out of the
  content comparison, since each of their chunks was checked as its volume closed, before it could go.
  A line says how many. Everything still on the PC is compared as before.

Tests: `kite_let_go_tests` in `src/zstd_pack.rs`.
