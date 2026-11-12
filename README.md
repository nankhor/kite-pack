# Kite Pack

Pack a PS5 game with zstd so it takes less space. Then, once the Kite helper lands, send it to your PS5 and install its DLC in one go.

This repository holds the core for now:

- **kite-pipeline**: the job and its steps (pack, delete the original, send, install DLC, delete the packed folder). Jobs are saved after every change, so they can carry on after a crash or restart.
- **kite-forge**: reads game folders, images and archives, and packs them with [PS5 Dump Forge](https://github.com/quer3q/ps5-dump-forge).
- **kite-cli**: the `kite-pack` command-line tool.

Sending to the PS5 (the Kite helper) and the desktop app come next.

## Download

Each [release](https://github.com/nankhor/kite-pack/releases) has `kite-pack` for Windows (x64), macOS (Apple silicon and Intel) and Linux (x64). Unpack it and run it from a terminal.

The binaries aren't signed yet. On macOS, run `xattr -d com.apple.quarantine kite-pack` once. On Windows, choose More info, then Run anyway, if SmartScreen stops it.

## Build

You need Rust (stable) and a C compiler.

```sh
cargo build --release
cargo test --workspace
```

The tool ends up at `target/release/kite-pack`.

## Use

```sh
kite-pack inspect "/games/Ashen Tide"
kite-pack pack "/games/Ashen Tide" --out /packed --level medium
kite-pack pack "/games/Ashen Tide" --out /packed --delete-original
kite-pack jobs
kite-pack resume <job id>
```

- `--format`: `folder` (default), `exfat`, `ffpkg` or `ffpfs`.
- `--level`: `fast` (default), `medium` or `max`. Slower levels make smaller packs.
- `--delete-original` moves the original to the Trash, and only once the packed copy is checked and the original hasn't changed since packing.

The tool refuses PS4 games, retail and patch packages, DLC on its own, and games that are already packed.

Jobs are kept in your data folder (`~/.local/share/kite-pack` on Linux, `~/Library/Application Support/kite-pack` on macOS, `%APPDATA%\kite-pack` on Windows). Set `KITE_PACK_DATA` to use another folder.

## Credits

- [PS5 Dump Forge](https://github.com/quer3q/ps5-dump-forge) by quer3q does the reading and packing.
- [ps5upload](https://github.com/phantomptr/ps5upload) showed how to send games and install DLC.
- [AVA1](https://github.com/phantomptr/ava1) by phantomptr is the transfer protocol the Kite helper will use.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
