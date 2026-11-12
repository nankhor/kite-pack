# Kite Pack — design spec

2026-11-12 · Parwez. Live copy with diagrams: https://claude.ai/artifact/Up2BbLSVTjGtaE2XbdFQtn · Mockups: https://claude.ai/artifact/TLe7khQJtjMn9PWtAJG1Rk ("Direction A, simpler: just the game icon" row)

## Goal and scope

A desktop app for Windows, macOS and Linux that packs a PS5 game with zstd and, when asked, sends it to the PS5, installs its DLC and cleans up. One screen, one game at a time.

It is built for a jailbroken PS5 on firmware 13.60 running kstuff and ShadowMount+, plus the Kite helper this app brings. Success means: drop a game, press Pack, walk away; the game ends up on the PS5 smaller, its DLC installed, and no file is ever deleted before its copy has been checked.

**Version 1 includes**

- Sources: a game folder, `.exfat`, `.ffpkg`, `.ffpfs`, `.ffpfsc`, a fake (debug) `.pkg`, or a `.zip`, `.7z` or `.rar` holding one game.
- Packing with zstd at three levels: Fast, Medium, Max.
- Output as a folder (default), `.exfat`, `.ffpkg` or `.ffpfs`.
- Three toggles: delete the original, send to the PS5, and (with send on) delete the packed folder.
- DLC `.pkg` files installed on the PS5 after the game is sent.
- Waiting for an unreachable PS5 and resuming a send where it stopped.

**Version 1 leaves out**

- LZ4 packs, PS4 games, installing updates, and `.ffpfsc` output (it is already a compressed image).
- Edit, Convert, a game library, a queue of several games, NAS sources.
- Everything else ps5upload does (Remote Play, cheats, saves, file browser).

## User flow

1. **Drop a game.** The app reads it and shows its icon, name, title ID, size, the update it already contains and the DLC already inside it.
2. **Pick how to pack.** zstd level (Fast, Medium, Max), Save as (Folder, `.exfat`, `.ffpkg`, `.ffpfs`) and where on this computer.
3. **Pick what happens after.** Delete the original. Send to the PS5. With send on, two more appear: delete the packed folder once it is on the PS5, and a DLC list.
4. **Add DLC (optional).** Drop DLC `.pkg` files on the list. Each is checked to be DLC for this game.
5. **Press Pack.** The options fold away and a list of the chosen steps takes their place, each showing done, running, waiting or failed.
6. **Done.** The screen says where everything ended up. Buttons: Show folder (when a packed copy is kept here) and Pack another game.

The app can be closed at any point after step 5. On the next launch the job carries on from its last finished step.

## The screen

Dark grey ground, Figtree, white pill buttons, the game icon as the only colour.

- **Header:** app name; a PS5 chip with a status dot and "Helper running" or "Not reachable"; Settings.
- **Game:** `icon0.png` at 128 to 176 px with a soft glow in its own colours (Forge's `glow.ts`), the name in large type, then Title ID, Size and Packed (an estimate until packing ends).
- **Includes row:** an update chip ("Update 1.05", or "Base game, no update") and one chip per DLC already in the dump. DLC queued for install shows in its own list under Send.
- **Options (before Pack):** Pack column: format, level, Save as and folder. After packing column: the three toggles and, with send on, the DLC list with an **Add DLC (.pkg)** button. All three on → note "Only the copy on PS5-Salon will be left."
- **Steps (after Pack):** one row per chosen step with an icon (done, running, waiting, failed), its status, and a bar for pack and send.

| State | Buttons | What the line says |
| --- | --- | --- |
| Ready | Pack | Where the packed copy goes and, with send on, that sending starts by itself |
| Packing or sending | Cancel | That it can be left running |
| Waiting for PS5 | Cancel send, Retry now | Since when, and to turn the PS5 on and start the helper |
| Failed | Retry from this step, Cancel | Which step failed and why, in plain words |
| Done | Show folder (if kept), Pack another game | What was kept and what is in the Trash |

## Architecture

Tauri 2 shell, React + TypeScript screen, Rust workspace. Forge's crates read and pack games; the AVA1 library moves files. New: the pipeline on the computer, and a small Kite helper on the PS5.

| Part | Comes from | What it does here |
| --- | --- | --- |
| `ps5-dump-forge-core` and its format crates | Forge (quer3q/ps5-dump-forge), pinned commit | Read every source format, inspect the game (icon, `param.json`, version, embedded DLC), pack with zstd, write the output, verify with BLAKE3 |
| ampr-emu-ng (`libSceAmpr.sprx`) | Forge's bundled copy | Runtime packed games load; installed into `fakelib/` by the pack job |
| `ava1` crate | AVA1 (phantomptr/ava1), pinned release | Pair once with a code, send with resume and BLAKE3 checks, call the helper's commands |
| Package host and DLC wait | Ported from ps5upload's engine (`pkg_install.rs`) | Serve a `.pkg` over HTTP with Range support to Sony's installer; wait for the DLC to appear |
| Sony install error hints | Ported from ps5upload (`hints.c`) | Turn an install error code into a plain sentence |
| `pipeline` crate | New | The job: steps, saved state, ordering rules, resume after a restart, a snapshot proving the original is unchanged before it moves to the Trash |
| Screen | New | The one screen |
| Kite helper (`kite-helper.elf`) | New, C, runs on the PS5 | AVA1's C node plus four commands: version, free space, install a `.pkg`, is this DLC in `/user/addcont`; installer call ported from Forge's `installer.c` |

## The pipeline

Each game is one job: a fixed list of steps, filtered by the toggles, saved to disk after every step change.

1. Pack, then check the pack (Forge's BLAKE3 verify).
2. *Delete the original*: a check that it is unchanged since packing (a snapshot of its files' sizes and change times; Forge keeps its own check private), then a move to the Trash.
3. *Send to the PS5* over AVA1, which checks every file with BLAKE3. While the PS5 is unreachable the step shows Waiting, and resumes from AVA1's journal when it comes back.
4. *Install each DLC*, one at a time: the Kite helper hands the `.pkg` to Sony's installer, then the step waits for it to land in `/user/addcont/<title ID>/<label>/` (or its metadata in `/user/appmeta/addcont/`).
5. *Delete the packed folder.*

**Rules**

- The original is deleted only after the pack check passes.
- The packed folder is deleted only after the PS5 check passes and every DLC is confirmed.
- Deletes move files to the Trash on the computer, never past it.
- A failed step stops the job there. Nothing after it runs and nothing is deleted. The job offers Retry from this step.
- On launch, an unfinished job reopens and continues from the last finished step; a step that was running restarts (pack) or resumes (send).

## Checks before a job starts

| Check | Comes from | If it fails, the screen says |
| --- | --- | --- |
| The source is a PS5 game (has `eboot.bin`, a valid `param.json`) | Forge inspect | "This is an update or a DLC on its own, not a game" |
| Not a PS4 package, not a retail package | Forge fpkg reader | "PS4 games can't be packed" or "Retail packages can't be read" |
| Not already packed with zstd | Forge inspect | "Already packed" |
| Forge's preflight: names, special files, depth, name limits | Forge preflight | Its own finding, worded as Forge words it |
| Free space on this computer for the packed copy | Forge | How much is needed and how much is free |
| Each DLC `.pkg` is additional content for this game's title ID | Forge fpkg reader | "This isn't DLC for <game>" |
| The PS5 answers, runs the matching Kite helper and is paired | Kite helper | "Start the helper" or "Pair again" |
| Free space on the PS5 for the packed game and the DLC | Kite helper | How much is needed on the PS5 and how much is free |

## Errors and recovery

No error ever deletes anything, and each one leaves the job at a step it can retry.

| What happens | What the app does |
| --- | --- |
| Packing fails (read error, disk full) | Removes its `.part` output, keeps the original, marks Pack failed with Forge's reason |
| The pack check finds a bad file | Marks Pack failed; nothing after it runs |
| The PS5 stops answering during a send | Shows "Waiting for PS5-Salon since 14:02", keeps trying, resumes from AVA1's journal; no timeout, Cancel send stops it |
| A file fails its check on the PS5 | AVA1 fetches the damaged part again; a file that keeps failing marks Send failed |
| The helper is missing, a different version or no longer paired | Pauses the step and offers Start helper or Pair again |
| Sony refuses a DLC install | Shows the code with its plain meaning, marks that DLC failed, installs the others, keeps the packed folder |
| A DLC is handed over but never appears | After about a minute, marks it "Not confirmed" and keeps the packed folder |
| The app or the computer stops mid-job | On the next launch, reopens the job at its last finished step |
| Cancel | Stops the running step, removes its partial output here; a partial upload on the PS5 stays so a retry can resume it |

## PS5 side and first launch

The PS5 needs kstuff, ShadowMount+ 1.7 or newer, and an ELF loader on port 9021. The app brings the Kite helper.

- **Where games go:** a folder ShadowMount+ watches, chosen once. Default `/data/homebrew`.
- **Where DLC goes:** Sony's installer puts it in `/user/addcont/`, outside the game folder, so packing never touches it.
- **The Kite helper:** `kite-helper.elf`, bundled. Own port (9130, configurable), own folder `/data/kite/`, runs beside ps5upload's helper. Start helper sends it to the ELF loader.
- **Commands:** AVA1 transfers with resume, plus version, free space, install a `.pkg`, has a DLC landed in `/user/addcont/`.

First launch: 1) find the PS5 (scan or IP); 2) start the Kite helper and pair with the code it shows; 3) pick the destination on the PS5 and the default folder for packed copies.

## Testing

- **Pipeline tests (Rust, no network):** every step change for every toggle combination (6 combinations, each with and without DLC); a crash at each step then a relaunch; no delete before its check; fakes for Forge's pack and the AVA1 client.
- **Upstream tests:** Forge's suite and AVA1's shared test vectors run in CI.
- **Console checklist (FW 13.60):** pack+send folder and `.ffpkg`; network pulled mid-send resumes; rest mode mid-send resumes; one DLC installed and confirmed; all toggles on leaves only the PS5 copy; failed DLC keeps the packed folder; quit mid-pack and mid-send then relaunch.

## Risks, licence and open questions

| Risk | What we do |
| --- | --- |
| ShadowMount+'s fakelib update replaces ampr-emu-ng | Say so once in the app; rerun the console checklist after ShadowMount+ updates |
| Forge changes its zstd format or packing rules | Pin a commit; move Forge and ampr-emu-ng together |
| AVA1 changes its protocol | Pin one release; app and helper ship together |
| We maintain the Kite helper | Four commands only; port installer fixes from Forge and ps5upload |

**Licence:** GPL-3.0-or-later (Forge is GPL-3.0-or-later, ported ps5upload code is GPL-3, AVA1 is MIT/Apache-2.0).

**Open questions:** does a DLC install need ShadowMount+ to have registered the game first; the final app name (working name Kite Pack); exact Forge commit and AVA1 release; is port 9130 free on the console.

## After version 1

1. A queue of several games. 2. Add DLC to a game already on the PS5. 3. Warn when the title is already on the PS5. 4. Unpack. 5. LZ4 if a game needs it.
