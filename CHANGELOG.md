# Changelog

## 0.1.0-alpha.3

Send to PS5 works.

- **The Kite helper** (`kite-helper.elf`) runs on the PS5 and receives packed games. Start it from Settings (the app sends it to your ELF loader on port 9021) or load it yourself with any loader.
- **No code to type.** With the helper running, Kite Pack finds the PS5 on your home network by itself. The helper accepts computers on the home network (192.168.x.x, 10.x.x.x, 172.16–31.x.x) and refuses others. The link is encrypted.
- **Send to PS5** sends the packed copy once packing ends, into the folder set in Settings (`/data/homebrew` by default). Every file is checked on the PS5 as it lands. If the PS5 goes to sleep or drops off the network, the step shows Paused and picks up where it stopped when the PS5 is back.
- **Delete the packed folder** appears under Send: the packed copy goes to the Trash once the PS5 has every file.
- **Settings**: the PS5 found on the network, its address, Start the Kite helper, where games go on the PS5, and where packed copies go on this computer.
- The packed size now shows correctly when the packed folder is deleted after sending.

Not in this build yet: installing DLC.

## 0.1.0-alpha.2

The first build of the Kite Pack app, with the screen we chose: the game's icon with a glow in its own colour, Figtree, white pill buttons on dark grey.

- Drop a game (a dump folder, an exFAT, `.ffpkg` or `.ffpfs` image, or a `.zip`, `.7z` or `.rar`) or choose it. The app shows its icon, title, title ID, size, the update it already contains and the DLC inside it.
- Pick Save as (Folder, exFAT, .ffpkg, .ffpfs), Level (Fast, Medium, Max) and where it goes, then Pack. The step shows its progress and the time left.
- Delete the original moves it to the Trash once the packed copy is checked, and only if it hasn't changed.
- Done shows the packed size and what you saved, with Show folder.
- Close the app mid-pack and it carries on next time. Cancel stops and removes the partial copy.
- Downloads: `Kite-Pack-…-windows-x64.exe` (just run it), `Kite-Pack-…-macos.dmg` (Apple silicon and Intel), `Kite-Pack-…-linux-x64.AppImage`. The command-line tool is still there as `kite-pack-cli-…`.

Not in this build yet: Send to PS5 and DLC install. The toggle is on the screen and waits for the Kite helper.

The builds aren't signed. On macOS, the first time, right-click Kite Pack and choose Open, or run `xattr -cr "/Applications/Kite Pack.app"`. On Windows, choose More info, then Run anyway, if SmartScreen stops it. On Linux, make the AppImage executable (`chmod +x`).

## 0.1.0-alpha.1

The first build of Kite Pack: the `kite-pack` command-line tool for Windows, macOS and Linux.

- `kite-pack inspect <game>` shows the title, title ID, version, size and the DLC inside a game folder, image or archive.
- `kite-pack pack <game> --out <folder>` packs it with zstd (Fast, Medium or Max) as a folder, exFAT image, `.ffpkg` or `.ffpfs`, and checks the result.
- `--delete-original` moves the original to the Trash once the packed copy is checked, and only if the original hasn't changed since packing.
- Every pack is saved as a job. If the tool or the computer stops, `kite-pack jobs` lists it and `kite-pack resume <id>` carries on.
- PS4 games, retail and update packages, DLC on its own and games that are already packed are refused with a reason.

Not in this build yet: sending to the PS5, installing DLC and the desktop app. They come with the Kite helper.

The binaries aren't signed. On macOS, run `xattr -d com.apple.quarantine kite-pack` once after unpacking. On Windows, choose More info, then Run anyway, if SmartScreen stops it.
