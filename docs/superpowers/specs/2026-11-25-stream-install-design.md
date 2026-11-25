# Install packages by streaming

Agreed with Nankhor on 2026-11-25 (option A).

## Why

Fake packages now install and start on FW 13.60 (kstuff's fake-package build). ps5upload installs them
by streaming: the PS5's own installer downloads the package from the computer while it installs, so
nothing is copied to the PS5 first and the PS5 only needs room for the installed game. Kite Pack does
this already for DLC after a send; this makes it the way any package is installed.

## Goal

Drop a `.pkg` or `.fpkg` (a PS5 or PS4 game, an update or a DLC) and press **Install on PS5**. A PS5
game package can still be packed instead (the second button). Nothing is copied to the PS5 first.

Non-goals: building a package from a dump; several packages queued at once; installing without the
Kite helper; loading kstuff for the user.

## Approaches

1. **A job with one step, Install package (picked).** The install is a job like a pack: saved, resumed
   after a restart, shown by the same bar and footer, recorded in History.
2. A separate install screen outside jobs: duplicates the bar, resume and History for nothing.
3. Copying the package to the PS5 first: what this feature exists to avoid.

## Design

**Reading a package (`kite-forge`):** `inspect_package(path) -> PackageInfo` — platform (PS5/PS4),
kind (game, update, DLC), title id, content id, name, version, size, icon. PS5 through Forge's reader;
PS4 through a small reader of the package header and its `param.sfo` / `icon0.png` entries (fake PS4
packages keep those unencrypted). The app's `inspect` returns the game as before plus `package` for a
package; a package Forge can't pack (PS4, update, DLC) still opens, for installing only.

**The job:** `Options.install` (default off) plans one step, `InstallPackage`; it needs the PS5 set up.
The runner hands the step to the installer with a progress callback; a PS5 that can't be reached when
the step starts is waited for, as for DLC.

**Streaming (`kite-ps5`):** the package server answers HEAD and byte ranges as now, and counts the exact
byte spans served (Sony asks for the end first and fetches some ranges twice), which is the progress.
Its port and link are derived from the package and the PS5, so after a restart Kite Pack serves the
same link again. The installer:
1. serves the package;
2. if the PS5 already has it (same size, same first and last 64 KiB), the step is done;
3. if a marker says an install of it was started and not finished, gives the PS5 90 s to carry on
   fetching; otherwise (or if nothing comes) asks Sony's installer to install it from the link;
4. reports the bytes the PS5 has received;
5. is done when the installed file is on the PS5 with the package's size and first and last 64 KiB:
   `/user/app/<id>/app.pkg` (or on an extended drive), `/user/patch/<id>/patch.pkg`, or a `.pkg` under
   `/user/addcont/<id>/` for DLC;
6. fails when no new bytes arrive for 10 minutes before the end, or the install doesn't appear
   within 15 minutes (more for a big package) after the last byte.

**Screen:** for a package, the main button is **Install on PS5**; a PS5 game package keeps **Pack** as
the second. The note says nothing is copied to the PS5 first and Kite Pack must stay open. The bar is one
part, Install on PS5: the bytes on the PS5, the speed and time left; "Installing on the PS5" once every
byte is there; "Installed on the PS5" at the end.

**Closing during an install** (new: today closing just pauses a job until the next launch): the window
asks first, since the PS5 needs this computer until the install ends. Closing anyway pauses the job as
now; the next launch serves the same link again.

A refusal adds that fake packages need kstuff with fake-package support. `a53_ppr_install_fast.elf` is
not asked for: ps5upload lists it, but installs work without it on Nankhor's 13.60 setup.

## Risks

- A restart while the PS5's installer has already given up on the link: Kite Pack waits 90 s, then asks
  again, and Sony starts that install over.
- PS4 packages with encrypted `param.sfo`: shown by content id, without a name or icon.
