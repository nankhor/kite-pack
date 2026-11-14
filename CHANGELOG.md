# Changelog

## 0.1.3

The Mac app opens.

- **macOS:** the whole app is now signed (ad hoc), as Apple silicon Macs expect. Before, only the program inside it was, and macOS could refuse to open it or leave the icon bouncing in the Dock. Each release build now checks the signature on the app inside the `.dmg` before publishing it.

Nothing else changed: Windows and Linux are the same as 0.1.2, and the Kite helper is the same as in 0.1.0, so the one already on your PS5 keeps working.

Downloads: `Kite-Pack-…-windows-x64.exe` (just run it), `Kite-Pack-…-macos.dmg` (Apple silicon and Intel), `Kite-Pack-…-linux-x64.AppImage`, `kite-helper.elf` for the PS5, and the command-line tool as `kite-pack-cli-…`.

The builds aren't signed with an Apple or Microsoft certificate yet. On macOS, the first time, run `xattr -cr "/Applications/Kite Pack.app"` in Terminal, or open it once, then click Open Anyway in System Settings → Privacy & Security. If the icon keeps bouncing, restart the Mac and try again. On Windows, choose More info, then Run anyway, if SmartScreen stops it. On Linux, make the AppImage executable (`chmod +x`).

## 0.1.2

Fake PKGs.

- **Choose file** now offers fake PKGs (`.pkg`) and `.ffpfsc` images too. Dropping them already worked; the file dialog didn't list them. Retail packages from Sony are still refused: they're encrypted.
- A `.pkg` that can't be read now says why in plain words: made with a passcode other than the default, a compression Kite Pack can't read yet, damaged or incomplete, or not a PS5 package.

The Kite helper is the same as in 0.1.0, so the one already on your PS5 keeps working.

## 0.1.1

- **Send to PS5 no longer keeps pausing.** While a game was sending, the step kept switching between sending and "Paused at …", and each pause cost a few seconds of the send. The app checks the PS5 every 10 seconds for the PS5 chip at the top, and each check cut the send. The checks now connect on their own, so the send runs straight through.

The Kite helper is the same as in 0.1.0, so the one already on your PS5 keeps working. A send left by a closed 0.1.0 carries on in 0.1.1 where it stopped.

Downloads: `Kite-Pack-…-windows-x64.exe` (just run it), `Kite-Pack-…-macos.dmg` (Apple silicon and Intel), `Kite-Pack-…-linux-x64.AppImage`, `kite-helper.elf` for the PS5, and the command-line tool as `kite-pack-cli-…`.

The builds aren't signed. On macOS, the first time, right-click Kite Pack and choose Open, or run `xattr -cr "/Applications/Kite Pack.app"`. On Windows, choose More info, then Run anyway, if SmartScreen stops it. On Linux, make the AppImage executable (`chmod +x`).

## 0.1.0

The first full release of Kite Pack, for Windows, macOS and Linux. It's the same as 0.1.0-alpha.4, released as 0.1.0, and the Kite helper now reports 0.1.0 too.

- **Pack a game.** Drop a dump folder, an exFAT, `.ffpkg` or `.ffpfs` image, or a `.zip`, `.7z` or `.rar` archive, or choose it. Kite Pack shows its icon, title, title ID, size, the update it already contains and the DLC inside it. Pick Save as (Folder, exFAT, .ffpkg, .ffpfs), Level (Fast, Medium, Max) and where it goes, then Pack. The packed copy is checked.
- **Delete the original** moves it to the Trash once the packed copy is checked, and only if it hasn't changed.
- **Send to PS5** sends the packed copy once packing ends. With the Kite helper running on the PS5, Kite Pack finds it on your home network by itself, with no code to type. Every file is checked on the PS5 as it lands. The step shows the speed and the time left. If the PS5 drops off the network, it shows Paused and picks up where it stopped.
- **Delete the packed folder** moves the packed copy to the Trash once the PS5 has every file.
- **Carries on.** Close the app mid-job and it carries on next time. Cancel stops and removes the partial copy.
- **Settings**: the PS5 found on the network, its address, Start the Kite helper, where games go on the PS5, and where packed copies go on this computer.
- **The command-line tool** `kite-pack` inspects and packs games, and lists and resumes jobs.

Downloads: `Kite-Pack-…-windows-x64.exe` (just run it), `Kite-Pack-…-macos.dmg` (Apple silicon and Intel), `Kite-Pack-…-linux-x64.AppImage`, `kite-helper.elf` for the PS5, and the command-line tool as `kite-pack-cli-…`.

Not in this release: installing DLC.

The builds aren't signed. On macOS, the first time, right-click Kite Pack and choose Open, or run `xattr -cr "/Applications/Kite Pack.app"`. On Windows, choose More info, then Run anyway, if SmartScreen stops it. On Linux, make the AppImage executable (`chmod +x`).

## 0.1.0-alpha.4

- **Send to PS5 no longer says Paused while it's sending.** When the connection to the PS5 dropped and came back, the send carried on, but the step stayed orange, read "Paused at …" and said the PS5 wasn't reachable until it ended. It now shows as sending again as soon as files move.
- **The send shows its speed** beside the percentage and the time left, for example "16%, 38.2 MB/s, about 9 min left". It's the speed over the last ten seconds. After a pause, the time left counts from when the send moved again instead of counting the pause.

The Kite helper is the same as in alpha.3, so the one already on your PS5 keeps working.

## 0.1.0-alpha.3

Send to PS5 works.

- **The Kite helper** (`kite-helper.elf`) runs on the PS5 and receives packed games. Start it from Settings (the app sends it to your ELF loader on port 9021) or load it yourself with any loader.
- **No code to type.** With the helper running, Kite Pack finds the PS5 on your home network by itself. The helper accepts computers on the home network (192.168.x.x, 10.x.x.x, 172.16–31.x.x) and refuses others. The link is encrypted.
- **Send to PS5** sends the packed copy once packing ends, into the folder set in Settings (`/data/homebrew` by default). Every file is checked on the PS5 as it lands. If the PS5 goes to sleep or drops off the network, the step shows Paused and picks up where it stopped when the PS5 is back.
- **Delete the packed folder** appears under Send: the packed copy goes to the Trash once the PS5 has every file.
- On macOS, moving a folder to the Trash no longer asks for permission to control Finder. To get it back, drag it out of the Trash.
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
