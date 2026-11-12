# Kite Pack

Pack a PS5 game with zstd so it takes less space. Then, once the Kite helper lands, send it to your PS5 and install its DLC in one go.

What is in this repository:

- **kite-pipeline**: the job and its steps (pack, delete the original, send, install DLC, delete the packed folder). Jobs are saved after every change, so they can carry on after a crash or restart.
- **kite-forge**: reads game folders, images and archives, and packs them with [PS5 Dump Forge](https://github.com/quer3q/ps5-dump-forge).
- **kite-session**: the app's side of a job: one at a time, reported as events for the screen.
- **kite-cli**: the `kite-pack` command-line tool.
- **app**: the desktop app (Tauri 2 and React), in `app/`.

Sending to the PS5 (the Kite helper) comes next.

## Download

Each [release](https://github.com/nankhor/kite-pack/releases) has the app:

- **Windows:** `Kite-Pack-…-windows-x64.exe`. Just run it; nothing to install. It uses the WebView2 that comes with Windows 10 and 11.
- **macOS:** `Kite-Pack-…-macos.dmg`, for Apple silicon and Intel. Drag Kite Pack to Applications.
- **Linux:** `Kite-Pack-…-linux-x64.AppImage`. Make it executable and run it.

The command-line tool is there too, as `kite-pack-cli-…` archives.

The builds aren't signed yet. On macOS, the first time, right-click Kite Pack and choose Open, or run `xattr -cr "/Applications/Kite Pack.app"`. On Windows, choose More info, then Run anyway, if SmartScreen stops it.

## Build

You need Rust (stable), a C compiler and Node 22. On Linux, also the WebKitGTK 4.1 development libraries (see `.github/workflows/ci.yml`).

```sh
cd app && npm ci && npm test
npx tauri build          # the app, in target/release/bundle
cargo build --release -p kite-cli   # the command-line tool, target/release/kite-pack
cargo test --workspace
```

## Command-line tool

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
