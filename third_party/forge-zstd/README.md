# ps5-dump-forge-zstd, with a quick skip for data that won't shrink

Forge's zstd packer compresses a game 256 KiB at a time and keeps a piece raw when zstd can't make it
smaller. Compressed audio, video and archives never get smaller, but each piece still went through the full
compression first. At level 19 (Kite Pack's **Max**) that runs at about 13 MB/s per core.

This folder is the `ps5-dump-forge-zstd` crate with one change in `src/writer.rs`'s `encode_chunk`. The root
`Cargo.toml` puts it in place with the `[patch."https://github.com/quer3q/ps5-dump-forge"]` entry.

| | |
|---|---|
| Crate | `crates/ps5-dump-forge-zstd` of https://github.com/quer3q/ps5-dump-forge (GPL-3.0-or-later) |
| Commit | `9e3c9c1a38157b2d560345bee0acff1865bfb6f5`, the same one Kite Pack pins |

## The change

From level 6 up, a piece whose bytes look random goes through the fastest level first:

1. **Looks random?** Its byte histogram's entropy is at least 7.9 bits per byte. Compressed data measures
   7.95 to 8; code, text and most game data measure well below. This check is a single count over the piece.
2. **Fails the quick test?** If level 1 can't save 1% of it, the piece is stored raw at once. Level 1 runs at
   about 1 GB/s and stops as soon as its output passes 99% of the piece.

Anything else is compressed at the level asked, byte for byte as before. The decision depends only on the
bytes and the level, so Forge's second pass for images (`tree.rs`) makes the same one. A raw piece is a normal
part of the format, so ampr-emu-ng reads it as it always did, with no decoding.

Levels 1 to 5 are not probed: there zstd gives up on such data about as fast as the probe would.

## Measured

On one core, with 256 KiB pieces as Forge packs them:

| Data | Level | Before | After | Saved before → after |
|---|---|---|---|---|
| 68 MB of `.crate` (gzip) archives | 19 | 13 MB/s | 168 MB/s | 1.05% → 0.54% |
| same | 9 | 595 MB/s | 686 MB/s | 0.47% → 0.42% |
| 33 MB of Linux libraries | 5, 9, 19 | unchanged | unchanged | unchanged |

On already-compressed data the pack ends about 0.5% bigger, in exchange for Max running more than ten times as
fast on it. Data that does compress is not touched.

Tests: `random_looking_chunks_are_probed_first` in `src/writer.rs`. Forge's own tests are unchanged and pass.

## Updating

Moving the Forge pin means taking that commit's crate again and reapplying the change. It is marked
"Kite Pack" in `src/writer.rs`: `PROBE`, `PROBE_LEVEL`, `PROBE_FROM`, `DENSE_BITS`, `PROBE_SAVES`, `dense`,
`probe_saves`, the first lines of `encode_chunk`, and the test. If Forge takes the change itself, delete this
folder and the `[patch]` entry.
