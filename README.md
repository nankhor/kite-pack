# Kite Pack

Pack a PS5 game with zstd so it takes less space, and send it to your PS5 in one go.

What is in this repository:

- **kite-pipeline**: the job and its steps (pack, delete the original, send, install DLC, delete the packed folder). Jobs are saved after every change, so they can carry on after a crash or restart.
- **kite-forge**: reads game folders, images and archives, and packs them with [PS5 Dump Forge](https://github.com/quer3q/ps5-dump-forge).
- **kite-session**: the app's side of a job: one at a time, reported as events for the screen.
- **kite-ps5**: finds the Kite helper on the home network, connects to it and sends packed copies over [AVA1](https://github.com/phantomptr/ava1).
- **helper**: the Kite helper, `kite-helper.elf`, which runs on the PS5 (AVA1's C node plus a little of ours).
- **kite-cli**: the `kite-pack` command-line tool.
- **app**: the desktop app (Tauri 2 and React), in `app/`.

Installing DLC comes next.

## Download

Each [release](https://github.com/nankhor/kite-pack/releases) has the app:

- **Windows:** `Kite-Pack-…-windows-x64.exe`. Just run it; nothing to install. It uses the WebView2 that comes with Windows 10 and 11.
- **macOS:** `Kite-Pack-…-macos.dmg`, for Apple silicon and Intel. Drag Kite Pack to Applications.
- **Linux:** `Kite-Pack-…-linux-x64.AppImage`. Make it executable and run it.

- **The Kite helper:** `kite-helper.elf`, if you'd rather load it on the PS5 yourself. The app carries it too.

The command-line tool is there too, as `kite-pack-cli-…` archives.

The builds aren't signed yet. On macOS, the first time, right-click Kite Pack and choose Open, or run `xattr -cr "/Applications/Kite Pack.app"`. On Windows, choose More info, then Run anyway, if SmartScreen stops it.

## Your PS5

You need a jailbroken PS5 with an ELF loader (port 9021) and ShadowMount+ watching the folder games go to (`/data/homebrew` by default).

1. Start the Kite helper: in Kite Pack, open Settings and press Start the Kite helper (it goes to your ELF loader), or load `kite-helper.elf` from the release with any loader. The PS5 shows "Kite helper is running."
2. That's it: Kite Pack finds the PS5 on your network by itself. If your router blocks that, type the PS5's address in Settings.

The helper accepts any computer on your home network (private addresses) with no code, the same trust as the ELF loader itself, and refuses others. It writes only under `/data` and `/mnt`, and keeps its own files in `/data/kite`.

## Build

You need Rust (stable), a C compiler and Node 22. On Linux, also the WebKitGTK 4.1 development libraries (see `.github/workflows/ci.yml`). The PS5 build of the helper needs the PS5 payload SDK (`scripts/install-ps5-sdk.sh`) and LLVM; `make -C helper host` builds it for this computer, which the tests use.

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
- [AVA1](https://github.com/phantomptr/ava1) by phantomptr carries every send; the Kite helper is its C node.
- The [PS5 payload SDK](https://github.com/ps5-payload-dev/sdk) by John Törnblom builds the Kite helper.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
