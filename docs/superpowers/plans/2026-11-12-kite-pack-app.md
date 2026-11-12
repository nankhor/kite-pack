# Kite Pack desktop app Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Kite Pack desktop app: the chosen screen (Direction A, simpler) over the core pipeline, so a person drops a game, picks how to pack, presses Pack and gets a checked packed copy, on Windows, macOS and Linux.

**Architecture:** A new `kite-session` crate owns one running job at a time (start, cancel, carry on, discard, unfinished-on-launch) and turns pipeline events into serialisable UI events; it has no Tauri dependency so it tests fast. The Tauri 2 crate `app/src-tauri` is thin glue: commands, drag and drop, the folder picker and Show folder. The React screen in `app/src` renders from a pure view model (`model.ts`) so the copy and states are unit-tested.

**Tech Stack:** Rust 2024, Tauri 2.12 (`tauri`, `tauri-plugin-dialog`, `tauri-plugin-opener`), React 19, TypeScript 6, Vite 8, Vitest 5 + Testing Library + jsdom, `@fontsource/figtree`.

**Spec:** `docs/superpowers/specs/2026-11-12-kite-pack-design.md` ("User flow", "The screen", "Errors and recovery"). Look: canvas board `IconFlow.dc.html` (Direction A, simpler), artifact `d54b9c50-c0e3-41c3-8a70-e156d0e7d7e3`.

## Global Constraints

- Everything in Plan 1's Global Constraints still holds (GPL-3.0-or-later, Forge pin, Trash-only deletes, commit trailers).
- Version `0.1.0-alpha.2` for every crate and `app/package.json`.
- Product name "Kite Pack"; window title "Kite Pack"; bundle id `com.vesperstudio.kitepack`.
- Look, from the board: ground `#121417`; text `#ffffff`; muted text `rgba(255,255,255,0.68)` (labels), `0.72` (chip note), `0.78` (footer note); mint `#6ee7a8`, mint text `#b9f5d6`, update chip text `#c9f7df`; amber `#ffc56b`; warning `#ffd28f`; primary button white `#ffffff` with `#0a0d10` text, pill radius 999px, min-height 52px (Pack) / 48px (others); secondary button `rgba(255,255,255,0.06)` with `1px rgba(255,255,255,0.3)` border; Figtree 400–800, bundled (no network fonts); title `clamp(44px,6vw,72px)` weight 800, letter-spacing -0.035em; icon `clamp(128px,14vw,176px)`, radius 22px.
- Pack options offered: format Folder, exFAT, .ffpkg, .ffpfs (default Folder); level Fast, Medium, Max (default Fast). No zstd/LZ4 choice, no 1–9 slider (the board predates these decisions; the spec wins).
- Sending isn't available in this version: the Send toggle shows, off and disabled, with "Needs the Kite helper on your PS5. It comes in the next version." The PS5 chip shows a grey dot, "PS5" and "Not set up", disabled. No Settings button (nothing to set yet). "Delete the packed folder" never shows.
- Copy: sentence case, plain words, no blame, numbers as numerals, sizes in decimal units ("84.2 GB").
- Windows ships the bare app `.exe` (no installer). macOS ships one universal `.dmg`. Linux ships an `.AppImage`.

## Review Focus

1. **A dropped item that isn't a packable game** (a DLC `.pkg`, a PS4 game, a random file, two items at once) → the reason shows where the game would be; a game already on screen stays (test in Task 4).
2. **The app closes mid-pack** → on the next launch the job is on screen and carries on by itself; a job that had failed shows as failed with Retry (tests in Tasks 1 and 4).
3. **Cancel** → Forge removes its partial output, the job file is removed so the next launch doesn't bring it back, and the screen returns to Ready for the same game (test in Task 1).
4. **A game with no icon, no title, or a very long title** → a neutral tile with the brand glow, the title ID or folder name as the title, the title wraps without pushing the buttons off screen (tests in Tasks 3 and 4).
5. **A second Pack while one is running** (double click, or a drop during packing) → refused with "A game is already being packed" and the running job is untouched (test in Task 1).

---

### Task 1: `kite-session` crate

**Files:**
- Create: `crates/kite-session/Cargo.toml`, `crates/kite-session/src/lib.rs`, `crates/kite-session/src/view.rs`
- Test: `crates/kite-session/tests/session.rs`

**Interfaces:**
- Consumes: `kite_pipeline::{Job, JobStore, Options, Runner, RunEvent, RunOutcome, Packer, Trash, Unavailable, ThreadSleeper, Progress, StepKind, StepState}`; `kite_forge::{ForgePacker}` (dev-dependency for tests with the real packer).
- Produces:
  - `pub struct Ports { pub packer: Arc<dyn Packer + Send + Sync>, pub trash: Arc<dyn Trash + Send + Sync> }`
  - `pub type Emit = Arc<dyn Fn(UiEvent) + Send + Sync>;`
  - `pub struct Session` with `new(jobs_dir: PathBuf, ports: Ports) -> io::Result<Session>`, `start(&self, source: PathBuf, options: Options, emit: Emit) -> Result<JobView, String>`, `carry_on(&self, id: &str, emit: Emit) -> Result<JobView, String>` (retry_from first open step, then run), `cancel(&self)`, `discard(&self, id: &str) -> Result<(), String>`, `unfinished(&self) -> Option<JobView>` (newest), `is_running(&self) -> bool`.
  - `UiEvent` (serde `tag = "type"`, camelCase): `Job { job: JobView }` after every step change and at the end; `Stage { id: String, index: usize, stage: String }`; `Progress { id: String, index: usize, done: u64, total: u64 }`, at most every 100 ms per job.
  - `JobView { id, source: PathBuf, output_dir: PathBuf, delete_original: bool, packed: Option<PathBuf>, packed_bytes: Option<u64>, steps: Vec<StepView>, state: JobState }` camelCase; `JobState` = `running | finished | failed | cancelled`; `StepView { kind: "pack" | "deleteOriginal" | "send" | "installDlc" | "deletePacked", state: "pending" | "running" | "waiting" | "done" | "failed", note: Option<String> }`.
  - `pub fn packed_size(path: &Path) -> u64` (a file's length, or the sum of a folder's file lengths).

- [ ] **Step 1: Write the failing tests** in `tests/session.rs`, using a `make_game` fixture like kite-forge's, the real `ForgePacker`, a fake `Trash` that renames into a temp bin, and an `Emit` that pushes into `Arc<Mutex<Vec<UiEvent>>>` plus a helper that waits up to 30 s for a `Job` event whose state isn't `running`:
  - `start_packs_and_reports_the_packed_size`: final `JobView.state == finished`, `packed` exists, `packed_bytes == Some(packed_size(&packed))`, steps `[pack: done]`, at least one `Job` event with state `running` came first.
  - `delete_original_moves_the_game_to_the_trash`: steps `[pack: done, deleteOriginal: done]`, source gone, fake bin holds it.
  - `second_start_while_running_is_refused`: two starts back to back → second `Err("A game is already being packed")`; the first still finishes.
  - `cancel_removes_the_job`: start, `cancel()` before Forge starts (call immediately), final state `cancelled`, `unfinished()` is `None`, no packed output in the output folder.
  - `unfinished_returns_a_job_left_mid_run`: save a `Job` whose Pack step is `Running` straight into the store (as if the app died), new `Session` on that dir → `unfinished()` is `Some` with pack `pending`; `carry_on(id)` finishes it.
  - `failed_job_is_reported_and_retry_works`: output dir is a file → final `failed` with the pack note set; remove the file; `carry_on` → `finished`.
  - `discard_removes_a_failed_job`.
  - `packed_size_counts_files_in_a_folder_and_a_file`.
- [ ] **Step 2: Run** `cargo test -p kite-session --ignore-rust-version`. Expected: fails to compile (crate empty).
- [ ] **Step 3: Implement.** One `Mutex<Option<Running { id, cancel: Arc<AtomicBool>, done: Arc<AtomicBool> }>>`; `start` validates with `Job::new`, saves, spawns a thread that builds a `Runner` from the Arcs (`sender`/`installer` = `Unavailable`, `sleeper` = `ThreadSleeper`, `retry_every` 5 s) and maps each `RunEvent`: step changes → `Job` event with a fresh `JobView`; `Progress::Stage` → `Stage`; `Progress::Bytes` → `Progress` throttled to 100 ms. On `RunOutcome::Cancelled` remove the job file and emit state `cancelled`. On `Finished` compute `packed_bytes`. A thread that ends clears `running`. `is_running` reads it.
- [ ] **Step 4: Run** the same command. Expected: 8 passed. Then `cargo test --workspace --ignore-rust-version`: all green.
- [ ] **Step 5: Commit** `feat(session): one job at a time for the app, with UI events`.

### Task 2: Frontend scaffold and the view model

**Files:**
- Create: `app/package.json`, `app/index.html`, `app/vite.config.ts`, `app/tsconfig.json`, `app/src/main.tsx`, `app/src/styles.css`, `app/src/types.ts`, `app/src/model.ts`
- Test: `app/src/model.test.ts`

**Interfaces:**
- Consumes: Task 1's `JobView`, `UiEvent` shapes (mirrored in `types.ts`); kite-forge's `GameInfo`/`DlcInfo` (camelCase on the wire: `titleId`, `totalBytes`, `iconDataUrl`, ...).
- Produces (`model.ts`): `formatSize(bytes: number): string`; `updateLabel(version: string | null): string`; `stepRows(job: JobView, progress: ProgressState | null, now: number): StepRow[]` with `StepRow { label, status, state: "done" | "active" | "paused" | "pending" | "failed", bar: number | null }`; `footerNote(screen: Screen): string`; `minutesLeft(progress: ProgressState, now: number): number | null`; `Screen` = `{ kind: "empty" } | { kind: "refused", message } | { kind: "ready", game, options } | { kind: "running" | "failed" | "done", game, job }`.

- [ ] **Step 1: Scaffold** with npm: `react`, `react-dom`, `@tauri-apps/api`, `@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-opener`, `@fontsource/figtree`; dev: `@tauri-apps/cli`, `vite`, `@vitejs/plugin-react`, `typescript@~6.0`, `vitest`, `jsdom`, `@testing-library/react`, `@testing-library/user-event`, `@types/react`, `@types/react-dom`. Scripts: `dev`, `build` (`tsc --noEmit && vite build`), `test` (`vitest run`), `tauri`. `styles.css` holds the Global Constraints tokens as CSS variables and imports Figtree 400–800.
- [ ] **Step 2: Write the failing tests** in `model.test.ts`:
  - `formatSize(84_200_000_000) === "84.2 GB"`, `formatSize(716_978) === "717 KB"`, `formatSize(0) === "0 bytes"`.
  - `updateLabel("01.050.000") === "Update 1.05"`, `updateLabel("02.000.000") === "Update 2.00"`, `updateLabel("01.000.000") === "Base game, no update"`, `updateLabel(null) === "Base game, no update"`.
  - `stepRows`: pack running with progress 62/100 started 60 s ago → `{ label: "Pack", state: "active", status: "62%, about 1 min left", bar: 62 }`; pack running before any bytes with stage "scan" → status "Reading the game"; pack done with `packedBytes` 51_600_000_000 and output dir `/Games/Packed` → status "51.6 GB in Packed", bar 100; deleteOriginal pending → "After the packed copy is checked"; done → "In the Trash"; a failed step → state "failed", status = its note.
  - `footerNote`: ready → "Packs into /Games/Packed."; running → "You can leave this running. If you close Kite Pack, it carries on next time."; failed → "Pack failed: <note>"; done with delete original → "The original is in the Trash until you empty it. Play the game once first."; done without → "Nothing was deleted."
- [ ] **Step 3: Run** `npm test --prefix app`. Expected: FAIL, functions missing.
- [ ] **Step 4: Implement** `model.ts` (stage words: scan "Reading the game", preflight "Checking", measure "Measuring", write "Writing", pack "Packing", verify "Checking the packed copy", finalize "Finishing"; minutes left from the bytes rate since the first `Progress`, rounded up, at least 1).
- [ ] **Step 5: Run** `npm test --prefix app`. Expected: PASS.
- [ ] **Step 6: Commit** `feat(app): frontend scaffold and view model`.

### Task 3: The icon glow

**Files:**
- Create: `app/src/glow.ts`
- Test: `app/src/glow.test.ts`

**Interfaces:**
- Produces: `dominantHue(px: Uint8ClampedArray): [number, number, number] | null` (ported from Forge's `glow.ts` `dominant` + `tame`, first colour only); `glowShadow(rgb: [number,number,number] | null, done: boolean): string` → ready `0 24px 80px -16px rgba(r,g,b,0.38)`, done `0 24px 90px -8px rgba(r,g,b,0.6)`, brand fallback `rgb(194,112,70)`; `useIconGlow(dataUrl: string | null): [number,number,number] | null` (24×24 canvas, cached per URL).

- [ ] **Step 1: Write the failing tests**: a 24×24 all-`#c27046` pixel array → a hue within 10° of its own; all-grey `#808080` → `null`; transparent → `null`; `glowShadow(null, false)` contains `rgba(194, 112, 70, 0.38)`; `glowShadow([10,20,30], true)` contains `0.6`.
- [ ] **Step 2: Run** `npm test --prefix app`. Expected: FAIL.
- [ ] **Step 3: Implement**, with a header comment crediting Forge's `app/src/glow.ts` (GPL-3.0, quer3q).
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): icon glow from the game's own colours`.

### Task 4: The screen

**Files:**
- Create: `app/src/backend.ts`, `app/src/App.tsx`, `app/src/parts/Header.tsx`, `app/src/parts/Game.tsx`, `app/src/parts/Options.tsx`, `app/src/parts/Steps.tsx`, `app/src/parts/Footer.tsx`, `app/src/parts/Empty.tsx`
- Test: `app/src/App.test.tsx`

**Interfaces:**
- Consumes: Task 2 `model.ts`, Task 3 `glow.ts`; Task 5's commands by name: `inspect(path) -> GameInfo` (error: the problem's message), `start(source, options) -> JobView`, `cancel()`, `carry_on(id) -> JobView`, `discard(id)`, `unfinished() -> JobView | null`, `initial_path() -> string | null`; event `kite://event` carrying `UiEvent`; Tauri webview drag-drop events; `@tauri-apps/plugin-dialog` `open({ directory })`; `@tauri-apps/plugin-opener` `revealItemInDir`.
- Produces: `backend.ts` exporting `Backend` (an interface with those calls plus `onEvent(cb)`, `onDrop(cb)`, `pickFolder()`, `pickFile()`, `reveal(path)`) and `tauriBackend`; `App({ backend }: { backend: Backend })`.

- [ ] **Step 1: Write the failing tests** with a fake `Backend` (in-memory; `emit()` to push events):
  - Empty screen shows "Drop a game here" and Choose folder / Choose file.
  - Dropping a game → title, "PPSA01234", size, "Update 1.05" chip, one chip per DLC ("Tidebreaker Expansion, 6.1 GB"), format pills (Folder pressed), level pills (Fast pressed), "Save to" with the game's parent folder, Pack button.
  - Send toggle is disabled with "Needs the Kite helper on your PS5. It comes in the next version."; no "Delete the packed folder" anywhere.
  - Dropping a DLC → its message shows; dropping it while a game is on screen keeps the game and shows the message under the drop.
  - Pack calls `start` with `{ format: "folder", level: "fast", outputDir, deleteOriginal: false }`; a `Job` event `running` → step row "Pack" and Cancel; `Progress` events → "62%"; a `finished` event → "Packed" headline, "Saved" fact, Show folder and Pack another game; Show folder calls `reveal(packed)`.
  - A `failed` event → Retry and Cancel; Retry calls `carry_on`.
  - On mount with `unfinished()` returning a pending job → its game is inspected and `carry_on` is called; with a failed job → failed screen, no `carry_on`.
  - A game with no icon and no title → the title shows the title ID; a neutral tile renders.
- [ ] **Step 2: Run** `npm test --prefix app`. Expected: FAIL.
- [ ] **Step 3: Implement** the parts with the board's markup and the Global Constraints tokens. Remember the last chosen output folder in `localStorage` (`kite.outputDir`, wrapped in try/catch); default is the game's parent folder. Ready footer adds a secondary "Change game" button.
- [ ] **Step 4: Run.** Expected: PASS; `npm run build --prefix app` succeeds.
- [ ] **Step 5: Commit** `feat(app): the screen`.

### Task 5: The Tauri app

**Files:**
- Create: `app/src-tauri/Cargo.toml` (crate `kite-app`), `app/src-tauri/build.rs`, `app/src-tauri/tauri.conf.json`, `app/src-tauri/capabilities/default.json`, `app/src-tauri/src/main.rs`, `app/src-tauri/icons/*` (generated by `tauri icon` from a 1024 px kite mark)
- Modify: root `Cargo.toml` members → `["crates/*", "app/src-tauri"]`

**Interfaces:**
- Consumes: Task 1 `Session`, `Ports`, `UiEvent`, `JobView`; `kite_forge::{inspect_game, ForgePacker}`; `kite_pipeline::{SystemTrash, Options, OutputFormat, Level}`.
- Produces: commands `inspect`, `start`, `cancel`, `carry_on`, `discard`, `unfinished`, `initial_path` (first CLI argument that exists, so "Open with" and dropping onto the app icon work); events on `kite://event`.

- [ ] **Step 1: Write the app** (`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`), jobs dir `<app data dir>/jobs`, window 1180×780, min 860×620, background `#121417`, title "Kite Pack", drag and drop on. `start` takes `{ format, level, outputDir, deleteOriginal }` and builds `Options` with `send: None`.
- [ ] **Step 2: Build** `npm run tauri build --prefix app -- --no-bundle` (with `--ignore-rust-version` passed through `CARGO_*` if needed). Expected: `target/release/kite-app` exists.
- [ ] **Step 3: Run it under Xvfb** with the test game as the argument; screenshot Ready, press Pack (xdotool), screenshot Done. Expected: matches the board's layout; read both screenshots.
- [ ] **Step 4: Commit** `feat(app): Tauri app`.

### Task 6: CI, release and docs

**Files:**
- Modify: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `Cargo.toml` (version), `app/package.json` (version), `README.md`, `CHANGELOG.md`

- [ ] **Step 1: CI**: Node 22 + `npm ci`, `npm test`, `npm run build` in `app/` on all three systems before `cargo test`; on Linux install `libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev` first.
- [ ] **Step 2: Release**: keep the CLI archives; add app builds: Windows `tauri build --no-bundle` → `Kite-Pack-<v>-windows-x64.exe`; macOS `--target universal-apple-darwin --bundles dmg` → `Kite-Pack-<v>-macos.dmg`; Linux (ubuntu-22.04) `--bundles appimage` → `Kite-Pack-<v>-linux-x64.AppImage`. Every per-platform artifact is a single file uploaded with `actions/upload-artifact@v7` `archive: false` (no zip around it); downloads use `actions/download-artifact@v8`.
- [ ] **Step 3: Version** `0.1.0-alpha.2`; CHANGELOG section; README "Download" names the app files first and how to open unsigned builds.
- [ ] **Step 4: Run** `cargo test --workspace --ignore-rust-version`, `npm test --prefix app`, `npm run build --prefix app`. Expected: all green.
- [ ] **Step 5: Commit** `build: app in CI and releases, version 0.1.0-alpha.2`.
