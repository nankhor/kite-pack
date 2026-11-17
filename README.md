# Kite Pack

Pack a PS5 game with ZSTD so it takes less space, and send it to your PS5 in one go.

> **Kite Pack doesn't include, download or share any games.** It works on games you already own and have dumped yourself, to save space on your PS5. Use your own copies and support the developers who make them.

What is in this repository:

- **kite-pipeline**: the job and its steps (pack, delete the original, send, install DLC, delete the packed folder). Jobs are saved after every change, so they can carry on after a crash or restart.
- **kite-forge**: reads game folders, images and archives, and packs them with [PS5 Dump Forge](https://github.com/quer3q/ps5-dump-forge).
- **kite-session**: the app's side of a job: one at a time, reported as events for the screen.
- **kite-ps5**: finds the Kite helper on the home network, reads what the PS5 has installed, connects to it and sends packed copies over [AVA1](https://github.com/phantomptr/ava1).
- **helper**: the Kite helper, `kite-helper.elf`, which runs on the PS5 (AVA1's C node plus a little of ours).
- **kite-cli**: the `kite-pack` command-line tool.
- **app**: the desktop app (Tauri 2 and React), in `app/`.

Installing DLC comes next.

## The screen

The app has three tabs: **Pack** (the game and its options), **PS5** (connecting to the console,
where games go, and what is already installed) and **History** (finished jobs, newest first, with
their sizes, how much was saved, and where the packed copy is). Settings, behind the gear, keeps
where packed copies go and which versions are running.

## What is on the PS5

The **PS5** tab has **Show what's installed**, and the start screen's **See what's on the PS5**
opens the tab with it showing: every title the console has, with its tile, name, title id,
version, whether its update is installed, how many add-ons it has, and which drive it is on.
Its size is read as soon as the list opens (no button): app, update, add-ons and the dump it is
mounted from, counted separately, and the screen says how much room is left where games go.
Click any title for its page: version, update, storage, the size breakdown, its add-ons, and its
story and screenshots from IGDB.

Opening a game also says whether the console already has it, and marks the DLC that is already
installed, so nothing is packed or sent blind.

All of it is read-only and comes from the console's own folders: `/user/appmeta`, `/user/app`,
`/user/patch` and `/user/addcont`. Nothing else is touched, and no Sony installer API is called.
A title that only another tool's `app.db` entry knows about (no folder of its own) is not
listed; the screen says so rather than pretending the list is complete.

## DLC and backports inside the pack

On the Pack tab, **Add DLC** (one button: **Packages…** for `.pkg` files, **Folder…** for a DLC
dump folder or a folder of packages) and **Add backport** put files inside the packed copy before
it is written, so one thing goes to the PS5 and the DLC is there without being installed
separately. Each entry says where it goes: **In the pack**, **On the PS5** (already installed, so
it is left out), **Via ShadowMount+** (installed natively, when the base is an installed PKG), or
**After install** for an image, which still needs unpacking or a package installer.

- DLC is added the way the console's DLC emulator reads it: the files under `addcontN/`, an entry
  in `dlc_emu.ini`, and the emulator's modules in `fakelib/` when the dump has none of its own.
  **Kite Pack carries the emulator** ([dlc_emu](https://github.com/drakmor/dlc_emu) 0.3, GPL-3.0,
  source and modules under `third_party/dlc_emu/`), so there is nothing to install or pick; a dump
  that already carries an emulator keeps its own.
- A backport is a folder whose files replace the game's (`eboot.bin`, `sce_sys/`, `fakelib/`).
- The original dump is never changed: it is cloned beside itself (copy-on-write on APFS and
  btrfs/XFS, a copy elsewhere), the additions are folded into the clone, and the clone is removed
  once packing ends.
- This works on a game folder and on a `.pkg`. With **Save as Folder**, the package is packed
  once and the DLC are then unpacked straight into the finished folder, as loose files the game
  reads from disk — fast, and the zstd pack is untouched. For an image output the package is
  unpacked, the DLC folded in, and the image written again (the DLC are compressed too, but the
  whole game is packed a second time). An image *source* still packs as it is; its DLC are
  marked **After install** until it is unpacked or the base is installed as a PKG.

This needs the Kite helper 0.3.0. Packing and sending still work with an older helper; the new
screen asks you to start the new one.

### Installing DLC through Sony's installer

With the helper 0.3.0 and **Send** on, picked `.pkg` DLC are installed through **Sony's
installer** (the route ps5upload uses) after the game has been sent, instead of being packed:
the app serves each package from this computer, the console fetches it, Sony writes it to
`/user/addcont`, and the job only marks it done once it appears there. The list says
**Installs on the PS5** for those, and no `dlc_emu`/`fakelib` is involved. This works for a
mounted folder game too: Sony only needs the title registered, which ShadowMount+ has done.

With Send off, or an older helper, the DLC go inside the packed copy instead (**In the pack**),
as before. Keep the game closed while the DLC install.

## Download

Each [release](https://github.com/nankhor/kite-pack/releases) has the app:

- **Windows:** `Kite-Pack-…-windows-x64.exe`. Just run it; nothing to install. It uses the WebView2 that comes with Windows 10 and 11.
- **macOS:** `Kite-Pack-…-macos.dmg`, for Apple silicon and Intel. Drag Kite Pack to Applications.
- **Linux:** `Kite-Pack-…-linux-x64.AppImage`. Make it executable and run it.

- **The Kite helper:** `kite-helper.elf`, if you'd rather load it on the PS5 yourself. The app carries it too.

The command-line tool isn't in the downloads: build it from source (below).

The builds aren't signed with an Apple or Microsoft certificate yet, so the first launch needs one extra step.

- **macOS:** after dragging Kite Pack to Applications, run `xattr -cr "/Applications/Kite Pack.app"` in Terminal, then open it. Or open it once, let macOS refuse, then go to System Settings → Privacy & Security, click **Open Anyway** next to Kite Pack and confirm with Touch ID or your password. (Right-click → Open no longer works on macOS 15 and later.) If the icon keeps bouncing and no window comes, restart the Mac and open it again: macOS sometimes gets stuck checking a new app, and a restart clears it.
- **Windows:** if SmartScreen stops it, choose More info, then Run anyway.

After that, Kite Pack updates itself. When a newer release is out it says so under the tabs; **Update** downloads the file for your computer, checks it against the release's `SHA256SUMS.txt`, and puts it in place of the one you run, and **Restart now** opens the new version (never in the middle of a pack or a send). Updates installed this way skip the first-launch step above. Settings can stop it looking, or let it install updates by itself and only ask before the restart. On Linux it updates the AppImage it runs from.

## Your PS5

You need a jailbroken PS5 with an ELF loader (port 9021) and ShadowMount+ watching the folder games go to (`/data/homebrew` by default).

1. Start the Kite helper: in Kite Pack, open Settings and press Start the Kite helper (it goes to your ELF loader), or load `kite-helper.elf` from the release with any loader. The PS5 shows "Kite helper is running."
2. That's it: Kite Pack finds the PS5 on your network by itself. If your router blocks that, type the PS5's address in Settings.

The helper accepts any computer on your home network (private addresses) with no code, the same trust as the ELF loader itself, and refuses others. It writes only under `/data` and `/mnt`, and keeps its own files in `/data/kite`.

### Installing through ShadowMount+

ShadowMount+ has an HTTP API (`/api/v1`, port 10101 by default). When it answers, Kite Pack uses it for two things instead of packing them into the game:

- **DLC packages**: sent to the PS5 under `/data/kite/incoming`, then installed by Sony's installer through ShadowMount+. This is a real install (`/user/addcont`), with no `dlc_emu` involved. It needs the base game installed as a package: for a game mounted from a folder or image, ShadowMount+ refuses DLC, and Kite Pack keeps the DLC inside the packed copy instead.
- **Backports**: **Install backport on the PS5** sends the folder, then ShadowMount+ copies it to `/data/homebrew/backports/` and applies it at the next launch. This is ShadowMount+'s own supported route, so its fakelib handling decides how it is applied.

The port is `smpPort` in `settings.json` (10101 unless ShadowMount+'s config says otherwise). The API has no authentication, so Kite Pack only talks to the PS5 you chose in Settings, on your own network.

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
