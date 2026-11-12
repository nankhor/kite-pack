# Changelog

## 0.1.0-alpha.1

The first build of Kite Pack: the `kite-pack` command-line tool for Windows, macOS and Linux.

- `kite-pack inspect <game>` shows the title, title ID, version, size and the DLC inside a game folder, image or archive.
- `kite-pack pack <game> --out <folder>` packs it with zstd (Fast, Medium or Max) as a folder, exFAT image, `.ffpkg` or `.ffpfs`, and checks the result.
- `--delete-original` moves the original to the Trash once the packed copy is checked, and only if the original hasn't changed since packing.
- Every pack is saved as a job. If the tool or the computer stops, `kite-pack jobs` lists it and `kite-pack resume <id>` carries on.
- PS4 games, retail and update packages, DLC on its own and games that are already packed are refused with a reason.

Not in this build yet: sending to the PS5, installing DLC and the desktop app. They come with the Kite helper.

The binaries aren't signed. On macOS, run `xattr -d com.apple.quarantine kite-pack` once after unpacking. On Windows, choose More info, then Run anyway, if SmartScreen stops it.
