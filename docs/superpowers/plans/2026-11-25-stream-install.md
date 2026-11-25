# Install packages by streaming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop a `.pkg`/`.fpkg` (PS5 or PS4 game, update, DLC) and press **Install on PS5**: the PS5's installer streams it from this computer, nothing is copied first.

**Architecture:** A package is read into a `PackageInfo`. Installing is a job with one step, `InstallPackage`, run by the pipeline like any step; `SonyInstaller::install_package` serves the file (HEAD + ranges, exact coverage counted), asks the helper's existing install call for the link, reports progress and confirms the installed file on the PS5 by size and its first/last 64 KiB. The console operations sit behind a small trait so the whole install is tested against a fake console that fetches over real HTTP.

**Tech Stack:** Rust (`kite-forge`, `kite-ps5`, `kite-pipeline`, `kite-session`, Tauri `app/src-tauri`), React/TS screen.

**Spec:** `docs/superpowers/specs/2026-11-25-stream-install-design.md`

## Global Constraints

- Nothing is copied to the PS5 before installing; no helper change (the helper's install call, `fs.stat`, `fs.read_at` and its read roots already cover `/user/app`, `/user/patch`, `/user/addcont`, `/mnt/ext0|1/user/app`).
- Done = installed file with the package's size and identical first and last 64 KiB: `/user/app/<id>/app.pkg` (or `/mnt/ext0/user/app/…`, `/mnt/ext1/user/app/…`), `/user/patch/<id>/patch.pkg`, any `.pkg` under `/user/addcont/<id>/` (two levels).
- Stall: no new bytes for 10 minutes before the last byte → fail; after the last byte, the file must appear within `max(15 min, size / 50 MB/s)`.
- Resume: a marker per install; when present at start, wait 90 s for the PS5 to fetch before asking again.
- Refusal text adds: "fake packages need kstuff with fake-package support loaded". No mention of `a53_ppr_install_fast.elf`.
- Local Rust 1.97: `--ignore-rust-version`, `--exclude kite-app` for workspace runs; `CARGO_INCREMENTAL=0` (disk is small).

## Review Focus

1. **Sony asks for the end of the file first, and fetches some ranges twice** — progress must not jump to 100% or pass it. Test in Task 2 (`counts_each_byte_once_whatever_the_order`).
2. **Reinstalling a package already on the PS5** — done at once ("Already on the PS5"), no install request. Test in Task 4.
3. **A different build of the same game already installed** (same path, other bytes) — not mistaken for done; installs. Test in Task 4.
4. **Kite Pack restarted mid-install** — same link served again; no second install request while the PS5 keeps fetching. Test in Task 4 (`carries_on_an_install_after_a_restart`).
5. **The PS5 stops fetching** — fails with a message saying so, not a frozen bar. Test in Task 4 (`fails_when_the_ps5_stops_fetching`).

---

### Task 1: Read a package

**Files:**
- Create: `crates/kite-forge/src/package.rs`
- Modify: `crates/kite-forge/src/lib.rs`, `crates/kite-forge/src/inspect.rs` (`GameInfo.package`, `inspect_source`)
- Test: unit tests in `package.rs`; `crates/kite-forge/tests/pkg.rs`

**Interfaces:**
- Produces:
  - `#[serde(rename_all = "camelCase")] pub struct PackageInfo { platform: String /* "ps5" | "ps4" */, kind: String /* "game" | "update" | "dlc" */, title_id: String, content_id: String, title: Option<String>, version: Option<String>, bytes: u64, icon_data_url: Option<String>, packable: bool }`
  - `pub fn inspect_package(path: &Path) -> Result<PackageInfo, String>`
  - `GameInfo { …, #[serde(default)] package: Option<PackageInfo> }`
  - `pub fn inspect_source(path: &Path) -> Result<GameInfo, SourceProblem>`: `inspect_game` plus `package` for a `.pkg`/`.fpkg`; a package `inspect_game` refuses but `inspect_package` reads becomes a `GameInfo { kind: "pkg", total_bytes: size, dlcs: [], findings: [], package: Some(.. packable: false) }`.
- PS4 reader: magic `\x7FCNT`; big-endian header: entry count `u32 @0x10`, entry table offset `u32 @0x18`, content id 36 bytes `@0x40`; entries 32 bytes (`id, name_off, flags1, flags2, data_off, data_size, pad8`); `param.sfo` id `0x1000`, `icon0.png` id `0x1200`, skipped when `flags1 & 0x8000_0000` (encrypted). Kind from `param.sfo` `CATEGORY`: `gd` game, `gp` update, `ac` DLC (no sfo: game). Title id = content id chars 7..16.

- [ ] **Step 1: Failing tests:** `reads_a_ps4_game_package` (a synthetic PS4 package with a plain `param.sfo` TITLE/CATEGORY/APP_VER and an icon: platform ps4, kind game, title id `CUSA12345`, name, version, icon, packable false); `reads_a_ps4_update_by_its_category`; `a_ps4_package_with_encrypted_entries_keeps_its_content_id`; `refuses_what_isnt_a_package`; in `tests/pkg.rs`: `a_fake_ps5_package_can_be_installed_or_packed` (`inspect_source` on `make_fake_pkg`: `package.platform == "ps5"`, kind game, packable true, same title id as the game).
- [ ] **Step 2:** `cargo test --ignore-rust-version -p kite-forge` → fail (no `package` module).
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Same → pass.
- [ ] **Step 5:** Commit "kite-forge: read a package for installing".

### Task 2: Package server: same link after a restart, exact progress

**Files:**
- Modify: `crates/kite-ps5/src/pkgserve.rs`
- Test: unit tests in `pkgserve.rs`

**Interfaces:**
- Produces: `PkgServer::start(pkg, console)` unchanged for DLC; `PkgServer::start_stable(pkg, console) -> Result<PkgServer, String>`: port `40000 + (fnv(path, console) % 20000)` (falls back to a random port when taken), token = 16 hex of the same hash; `PkgServer::received(&self) -> u64` (exact distinct bytes served), `PkgServer::requests(&self) -> u64`, `PkgServer::size(&self) -> u64`.
- Coverage: sorted, disjoint `[start, end)` spans; a served range is merged in after its bytes are written.

- [ ] **Step 1: Failing tests:** `counts_each_byte_once_whatever_the_order` (serve, then GET the last 64 KiB, then 0..half, then 0..half again, then the rest: `received()` goes 64 Ki → half+64 Ki → same → size, never above); `the_same_package_gets_the_same_link` (two `start_stable` one after the other (first dropped) give the same url); `answers_head_and_ranges` stays as is.
- [ ] **Step 2–4:** `cargo test --ignore-rust-version -p kite-ps5 --lib pkgserve` → fail, implement, pass.
- [ ] **Step 5:** Commit "pkgserve: the same link after a restart, bytes received exactly".

### Task 3: Pipeline: an install job

**Files:**
- Modify: `crates/kite-pipeline/src/options.rs` (`install: bool`, serde default; `OptionsError::InstallWithoutPs5` "Installing needs the PS5 set up in Settings"), `src/steps.rs` (`StepKind::InstallPackage`, plan), `src/ports.rs` (`Installer::install_package`), `src/runner.rs` (the step), `crates/kite-session/src/view.rs` (kind `"installPackage"`)
- Test: `crates/kite-pipeline/src/steps.rs` tests, `crates/kite-pipeline/tests/runner_install.rs`

**Interfaces:**
- Produces: `fn install_package(&self, pkg: &Path, target: &SendTarget, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<InstallOutcome, InstallError>` with default `Err(InstallError::Failed("This Kite Pack can't install packages".into()))`; `pub enum InstallOutcome { Installed, AlreadyThere }`. Step notes: "Installed and confirmed" / "Already on the PS5". `Unreachable` → the runner's wait-and-retry, as for DLC.
- `plan_steps` with `install`: `[InstallPackage]` only.

- [ ] **Step 1: Failing tests:** `an_install_job_has_one_step` (steps.rs); in `runner_install.rs`: `installs_and_reports_progress` (fake installer emits `Bytes{1,2}`, `Bytes{2,2}`, returns Installed: step done "Installed and confirmed", progress forwarded with index 0), `waits_for_a_ps5_that_is_away` (Unreachable once, then Installed), `says_when_it_was_already_there`.
- [ ] **Step 2–4:** `cargo test --ignore-rust-version -p kite-pipeline` → fail, implement, pass; `-p kite-session` still passes.
- [ ] **Step 5:** Commit "pipeline: a job that installs a package".

### Task 4: Installing a package on the PS5

**Files:**
- Create: `crates/kite-ps5/src/package_install.rs`
- Modify: `crates/kite-ps5/src/sony_install.rs` (`install_package` delegates), `src/lib.rs`
- Test: unit tests in `package_install.rs` with a fake console

**Interfaces:**
- Consumes: Task 1 `inspect_package`, Task 2 `PkgServer::start_stable`, `received`, `requests`; Task 3 `InstallOutcome`.
- Produces: `trait ConsoleOps { fn request_install(&self, url: &str, name: &str) -> Result<(), Ps5Error>; fn stat_size(&self, path: &str) -> Result<Option<u64>, Ps5Error>; fn read_at(&self, path: &str, at: u64, len: usize) -> Result<Vec<u8>, Ps5Error>; fn list(&self, path: &str) -> Result<Vec<(String, bool)>, Ps5Error>; }` (the real one over the helper); `pub(crate) fn install_with(ops: &dyn ConsoleOps, pkg: &Path, info: &PackageInfo, server: &PkgServer, markers: &Path, progress: &dyn Fn(Progress), cancel: &AtomicBool, clock: &dyn Clock) -> Result<InstallOutcome, InstallError>`; `trait Clock { fn now(&self) -> Instant; fn sleep(&self, d: Duration); }` so tests run in milliseconds. Markers: `<markers>/<hash>.started`, written before asking, removed on success.
- Refusals from `request_install` keep Sony's text and add " — fake packages need kstuff with fake-package support loaded".

- [ ] **Step 1: Failing tests** (fake console: `request_install` spawns a thread that fetches the link with HEAD and ranges — the last 64 KiB first, then the rest in 1 MiB ranges, one twice — then "installs" by recording the bytes as the target file): `installs_a_game_and_confirms_it` (Installed; target `/user/app/CUSA12345/app.pkg`; progress ends at `Bytes{size,size}`; marker gone), `an_update_lands_in_patch`, `a_dlc_lands_under_addcont`, `finds_a_game_on_an_extended_drive`, `already_there_asks_nothing` (AlreadyThere, no request), `another_build_of_the_same_game_is_installed_over` (same path, other bytes → installs), `carries_on_an_install_after_a_restart` (marker present, the console keeps fetching without a request → Installed, zero requests), `asks_again_when_nothing_comes_after_a_restart` (marker present, no fetch in 90 s → one request), `fails_when_the_ps5_stops_fetching` (fetches half then stops: after 10 min (fake clock) → Failed mentioning the PS5 stopped), `a_refusal_names_kstuff`, `cancel_stops_it`.
- [ ] **Step 2–4:** `cargo test --ignore-rust-version -p kite-ps5 --lib package_install` → fail, implement, pass; `--test send` still passes.
- [ ] **Step 5:** Commit "kite-ps5: install a package by streaming it".

### Task 5: App: open packages, start an install, ask before closing

**Files:**
- Modify: `app/src-tauri/src/main.rs` (`inspect` → `inspect_source`; command `install(source)` building `Options { install: true, send: Some(target), .. }`; on `CloseRequested` while a running job is an install: prevent close, emit `UiEvent::CloseAsked`; command `quit()`), `crates/kite-session/src/lib.rs` (`Session::running_install() -> bool`), `crates/kite-session/src/view.rs` (`UiEvent::CloseAsked`)
- Test: `crates/kite-session/tests/session.rs`

- [ ] **Step 1: Failing test:** `knows_when_an_install_is_running` (start an install job with a fake installer that waits; `running_install()` true; after it ends, false; a pack job: false).
- [ ] **Step 2–4:** run `-p kite-session`, implement, pass. `kite-app` builds in CI only (GTK missing locally): keep its change small and type-checked by the session API.
- [ ] **Step 5:** Commit "app: open packages to install, ask before closing during an install".

### Task 6: Screen

**Files:**
- Modify: `app/src/types.ts` (`StepKind` + `"installPackage"`, `GameInfo.package`, `UiEvent` `closeAsked`), `app/src/backend.ts` (`install`, `quit`), `app/src/model.ts` (labels, part, heads, notes), `app/src/parts/Footer.tsx`, `app/src/App.tsx`, `app/src/parts/Options.tsx`
- Test: `app/src/model.test.ts`, `app/src/App.test.tsx`

**Copy (exact):**
- Main button: "Install on PS5"; second (PS5 game package only): "Pack" / "Pack and send".
- Ready note: "Installs on the PS5 straight from this computer: nothing is copied to the PS5 first. Keep Kite Pack open until it's done."
- Not packable note (PS4, update, DLC): "This package installs as it is; it can't be packed."
- Part label "Install on PS5"; running note "12.3 GB of 45.6 GB on the PS5"; head "Installing on PS5" with "27%, 38.2 MB/s, about 12 min left"; all bytes there: head "Installing on PS5", detail "Every byte is on the PS5; it's finishing the install"; done: head "Installed on the PS5", detail "Installed and confirmed" (or "Already on the PS5").
- Close dialog: "An install is running" / "The PS5 is downloading the package from this computer. If Kite Pack closes, the install stops until you open it again." Buttons "Keep it open" (default) and "Close anyway".

- [ ] **Step 1: Failing tests:** model: `an_install_is_one_part`, heads above, `history_records_an_install` (sent, the package's size, no packed folder); App: `offers_install_for_a_package` (Install calls `backend.install(source)`; Pack still there for a PS5 game package), `a_ps4_package_only_installs`, `asks_before_closing_during_an_install`.
- [ ] **Step 2–4:** `npx vitest run`, `npx tsc --noEmit -p .` → fail, implement, pass; `npm run build`.
- [ ] **Step 5:** Commit "Screen: Install on PS5 for packages".

### Task 7: Check and ship

- [ ] Workspace tests, forge copies' tests, screen tests, build; CHANGELOG entry; PR; CI green; merge.
