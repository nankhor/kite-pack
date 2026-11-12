# Kite Pack: helper, Settings and Send Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With the Kite helper running on the PS5, the app finds it on the home network by itself and connects with no code; Settings shows it and where games go; Send to PS5 works: the packed copy goes to the console over AVA1, checked and resumable, and Delete the packed folder can follow.

**Architecture:** `helper/` is the Kite helper: AVA1's C node (vendored, v1.0.9) plus about 250 lines of ours (port 9130, state in `/data/kite`, writes only under `/data` and `/mnt`, trusts computers on the home network with no code through AVA1's `admit_new`, answers a UDP discovery broadcast). It builds for the PS5 with the PS5 payload SDK in CI, and for the host with the system C compiler so tests can run against it. `crates/kite-ps5` is the PC side: discovery, connecting (the PS5's key is remembered on first use), status, starting the helper through the ELF loader, and a `Sender` for the pipeline, on the `ava1` crate 1.0.9. The app gets a Settings page and the Send toggle.

**Tech Stack:** C11 + AVA1 C node 1.0.9; PS5 payload SDK v0.45 (sha256 `5fac64093d0e3e69f2584ce7935f24f703f622d9681f1495c85b9f00d1291a2c`) with LLVM 19; Rust `ava1 = "=1.0.9"` + tokio; Tauri 2; React.

**Spec:** `docs/superpowers/specs/2026-11-12-kite-pack-design.md` ("PS5 side and first launch", "The screen", "The pipeline", "Errors and recovery"). Look: board `IconFlow.dc.html` (Send toggle, third toggle indented under it, header chip).

## Global Constraints

- Everything in the earlier plans' Global Constraints still holds.
- Version `0.1.0-alpha.3`.
- Helper: file `kite-helper.elf`; TCP port 9130; state folder `/data/kite` (identity, peers, jobs, events.log); clients may write and read only under `/data/` and `/mnt/`.
- No pairing code (the user's decision): the helper admits a new computer at once when its address is private (10/8, 172.16/12, 192.168/16, 169.254/16, 127/8) and refuses any other; the app adds the helper's key to its peers on first contact and reconnects.
- Discovery: the app sends the UDP datagram `KITE-DISCOVER 1` to port 9130 (the limited broadcast and each interface's broadcast address); the helper answers `KITE-HELPER 1 <tcp port> <version>`. On start the helper shows a PS5 notification `Kite helper is running`.
- ELF loader port default 9021. Destination default `/data/homebrew`.
- AVA1 versions pinned: C sources from tag v1.0.9 (commit `2d0c21a`), Rust `ava1 = "=1.0.9"`. They ship together.
- The PS5 SDK runs only in CI (prebuilt toolchain binaries are not run locally). Locally the helper builds for the host only.
- Copy, from the board: "Send to PS5", "Starts by itself when packing ends, into <destination>."; "Delete the packed folder", "It goes to the Trash once the PS5 confirms every file."; "Only the copy on the PS5 will be left."; step "Send to PS5": "Starts when packing ends", "62%, about 8 min left", "Paused at 38%", "Every file checked"; waiting note "The PS5 isn't reachable. Turn it on and start the Kite helper; sending picks up where it stopped."
- The third toggle sits under Send, indented so its switch lines up with Send's label (46 px switch + 14 px gap = 60 px).

## Review Focus

1. **The PS5 goes away mid-send** (rest mode, Wi-Fi drop) → the step shows Paused, the runner keeps retrying, and the send resumes from the helper's journal instead of starting over (test in Task 2: kill and restart the host helper mid-send).
2. **The helper's key changed (reinstalled, new identity) or the computer isn't on a private address** → connecting fails with a plain reason ("The PS5's helper has a new key. Forget it in Settings and connect again." / "The Kite helper only accepts computers on your home network.") rather than hanging (tests in Tasks 1 and 2).
3. **A single-file pack (.ffpkg, .ffpfs, exFAT)** → arrives as that one file in the destination, not as a folder (test in Task 2).
4. **Start helper while one is already running** → the app doesn't send the ELF again; the status says it's running (test in Task 3).
5. **Settings with no address, a bad address, or a closed port** → the status reads "Not set up" / "Not reachable" within a few seconds; nothing hangs the screen (tests in Tasks 3 and 5).

---

### Task 1: The Kite helper, discovery and connecting

**Files:**
- Create: `helper/Makefile`, `helper/src/kite_helper.c`, `helper/src/notify.h`, `helper/src/notify_ps5.c`, `helper/src/notify_host.c`, `helper/third_party/ava1/` (C sources, `gen`, `third_party` from tag v1.0.9, with `LICENSE-MIT`, `LICENSE-APACHE`)
- Create: `crates/kite-ps5/Cargo.toml`, `crates/kite-ps5/src/lib.rs`, `crates/kite-ps5/src/connect.rs`, `crates/kite-ps5/src/discover.rs`, `crates/kite-ps5/tests/support/mod.rs` (builds `make -C helper host` once and spawns it), `crates/kite-ps5/tests/connect.rs`

**Interfaces:**
- Produces (helper): `kite-helper [--state DIR] [--allow DIR]... [--port N] [--trust CIDR]...` (default trust: the private ranges above); prints `kite helper listening on <port>`; node.info name `Kite helper`, platform `ps5` (host build `host`), version `0.1.0-alpha.3`; UDP discovery answer on the same port number.
- Produces (Rust): `pub struct Console { pub address: String, pub port: u16 }`; `pub struct Keys` with `Keys::open(dir) -> io::Result<Keys>` (identity + peers); `pub fn hello(console: &Console, keys: &Keys) -> Result<HelperInfo, Ps5Error>` (connects, remembers the helper's key on first contact, reconnects, returns `HelperInfo { name, version }`); `pub fn discover(wait: Duration) -> Vec<Found>` and `discover_at(targets: &[SocketAddr], wait) -> Vec<Found>` with `Found { address, port, version }`; `Ps5Error = Unreachable(String) | NotHomeNetwork | NewKey | Failed(String)` with `message()`.

- [ ] **Step 1: Write the failing tests** (`#[cfg(unix)]`): `connects_with_no_code` (hello → name "Kite helper"); `remembers_the_helper` (second hello works and the peers file holds one entry); `refuses_outside_the_home_network` (helper started with `--trust 10.99.0.0/16` → `NotHomeNetwork`); `new_helper_key_is_reported` (restart the helper with a fresh state dir → `NewKey`); `closed_port_is_unreachable`; `discovery_finds_the_helper` (`discover_at([127.0.0.1:<port>])` → one `Found` with the port and version).
- [ ] **Step 2: Run** `cargo test -p kite-ps5`. Expected: compile errors (crate empty).
- [ ] **Step 3: Implement** the helper (from `examples/c/node.c`: `admit_new` returns 1 for a trusted range and -1 otherwise; a UDP responder thread; notify on start) with Makefile targets `host` (system `cc`, `platform_posix.c`, `notify_host.c`) and `ps5` (SDK `prospero.mk`, `platform_ps5.c`, `notify_ps5.c` resolving `sceKernelSendNotificationRequest` with `dlsym`, a no-op if missing); then `connect.rs` and `discover.rs`.
- [ ] **Step 4: Run** the tests. Expected: 6 passed.
- [ ] **Step 5: Commit** `feat(helper): the Kite helper, discovery and connecting with no code`.

### Task 2: Sending

**Files:**
- Create: `crates/kite-ps5/src/send.rs`
- Test: `crates/kite-ps5/tests/send.rs`

**Interfaces:**
- Consumes: Task 1's `Console`, `Keys`; `kite_pipeline::{Sender, SendTarget, SendError, Progress}`.
- Produces: `pub struct Ps5Sender { keys: Keys, port: u16, outboards: PathBuf }` implementing `Sender`. `SendTarget.console` is the address; the port comes from the sender. A folder lands at `<destination>/<folder name>`; a file at `<destination>/<file name>`. The AVA1 job id is derived from the local path and destination, so a retry resumes. Bytes progress every 200 ms. Errors: connect refused / timed out / link lost → `Unreachable`; `NotHomeNetwork` / `NewKey` → `Failed(<message>)`; cancel → `Cancelled`; the helper refusing → `Failed(<its message>)`.

- [ ] **Step 1: Write the failing tests** (`#[cfg(unix)]`, host helper with `--allow <tmp>/ps5`): `sends_a_packed_folder_whole` (files and bytes match); `sends_a_single_image_file` (lands as one file); `reports_progress`; `outside_the_home_network_gets_a_plain_reason`; `closed_port_is_unreachable`; `cancel_stops_the_send`; `resumes_after_the_helper_restarts` (kill the helper once some bytes are durable, restart it on the same state dir, send again: completes and fewer bytes are re-sent than the total).
- [ ] **Step 2: Run** `cargo test -p kite-ps5 --test send`. Expected: compile errors.
- [ ] **Step 3: Implement** `send.rs` on `ava1::send::{send_job, SendOptions}` (`upload(root)`, shared `progress` and `cancel`, `persist` under `outboards`).
- [ ] **Step 4: Run.** Expected: 7 passed.
- [ ] **Step 5: Commit** `feat(ps5): send packed copies to the helper`.

### Task 3: Starting the helper, status and settings

**Files:**
- Create: `crates/kite-ps5/src/launch.rs`, `crates/kite-ps5/src/status.rs`, `crates/kite-ps5/src/settings.rs`
- Test: `crates/kite-ps5/tests/launch.rs`, `crates/kite-ps5/tests/settings.rs`

**Interfaces:**
- Produces: `pub fn start_helper(address: &str, loader_port: u16, helper_port: u16, elf: &[u8]) -> Result<StartOutcome, Ps5Error>` where `StartOutcome = AlreadyRunning | Sent`; it checks the helper port first (1.5 s) and sends the ELF to the loader only if it is closed. `pub fn status(console: Option<&Console>, keys: &Keys) -> Status` where `Status = NotSetUp | NotReachable | Running { version } | Refused { reason }`, through `hello` with a 3 s limit. `pub struct Settings { address: Option<String>, helper_port: u16 (9130), loader_port: u16 (9021), destination: String ("/data/homebrew"), packed_folder: Option<PathBuf> }` (camelCase JSON) with `Settings::load(path) -> Settings` (defaults when missing or unreadable) and `save(&self, path) -> io::Result<()>` (temp file and rename).

- [ ] **Step 1: Write the failing tests**: `start_sends_the_elf_to_the_loader` (a fake loader listener receives exactly the bytes); `start_does_nothing_when_the_helper_is_up` (helper port open → AlreadyRunning, loader never contacted); `status_*` for NotSetUp, NotReachable and Running; `settings_round_trip`, `missing_settings_are_the_defaults`, `corrupt_settings_are_the_defaults`.
- [ ] **Step 2: Run.** Expected: compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: all pass.
- [ ] **Step 5: Commit** `feat(ps5): start the helper, status and settings`.

### Task 4: Wiring the session and the app

**Files:**
- Modify: `crates/kite-session/src/lib.rs` (`Ports` gains `sender: Arc<dyn Sender>`), its tests, `app/src-tauri/Cargo.toml`, `app/src-tauri/build.rs`, `app/src-tauri/src/main.rs`

**Interfaces:**
- Produces commands: `settings() -> Settings`, `save_settings(settings)`, `ps5_status() -> Status`, `discover() -> Found[]` (2 s), `start_helper() -> StartOutcome`, `forget_ps5()` (drops its key so a reinstalled helper can be accepted), and `start`'s options gain `send: bool` and `deletePacked: bool` (the target comes from the settings). On launch, with no address saved, the app discovers and saves the address when exactly one helper answers. The helper ELF is embedded from `KITE_HELPER_ELF` (path) at build time, empty when unset; `start_helper` with no ELF fails "This build doesn't include the Kite helper".

- [ ] **Step 1: Write the failing test** in kite-session: `send_step_runs_and_reports_progress` with a fake `Sender` (two `Progress::Bytes`, then Ok) → steps `[pack done, send done]`, `Progress` events with the send index.
- [ ] **Step 2: Run.** Expected: FAIL (Ports has no sender).
- [ ] **Step 3: Implement** the session change and the app commands.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: green.
- [ ] **Step 5: Commit** `feat(app): settings, discovery and send in the app`.

### Task 5: Settings page, PS5 chip and the Send toggles

**Files:**
- Create: `app/src/parts/Settings.tsx`
- Modify: `app/src/backend.ts`, `app/src/types.ts`, `app/src/state.ts`, `app/src/App.tsx`, `app/src/parts/Header.tsx`, `app/src/parts/Options.tsx`, `app/src/model.ts`, `app/src/screen.css`
- Test: `app/src/App.test.tsx`, `app/src/model.test.ts`, `app/src/Settings.test.tsx`

- [ ] **Step 1: Write the failing tests**:
  - Header chip: "Not set up" → click opens Settings; "Helper running" with a mint dot; "Not reachable" amber; with no address and one helper found on launch, the address is saved and the chip reads "Helper running".
  - Settings: "Looking for your PS5…" then each found helper as a choice ("192.168.1.42, Kite helper 0.1.0-alpha.3") with Use this PS5; none found → "No Kite helper found on your network. Start it on the PS5, or type its address."; typing an address and leaving the field saves it; Start the Kite helper calls `startHelper` and shows "Sent to the PS5. It starts in a few seconds." or "It's already running."; a refused status shows its reason with Forget this PS5; destination and packed-copies folder save.
  - Options: Send is disabled with "Set up your PS5 in Settings first." when not set up; when not reachable it stays switchable with "The PS5 isn't reachable right now. Sending waits for it."; when running, hint "Starts by itself when packing ends, into /data/homebrew."; switching it on shows "Delete the packed folder" with class `toggle sub`; all three on shows "Only the copy on the PS5 will be left."; Pack sends `send: true, deletePacked: true`.
  - Steps: send running "62%, about 8 min left"; waiting "Paused at 38%" and the waiting note; done "Every file checked"; done headline "On the PS5".
- [ ] **Step 2: Run** `npm test --prefix app`. Expected: FAIL.
- [ ] **Step 3: Implement.** `.toggle.sub { padding-left: 60px }`. The chip polls `ps5Status` every 10 s and after any settings change.
- [ ] **Step 4: Run** tests and `npm run build`. Expected: green.
- [ ] **Step 5: Commit** `feat(app): Settings page, PS5 chip and Send`.

### Task 6: Building the helper in CI, release and docs

**Files:**
- Create: `scripts/install-ps5-sdk.sh` (downloads SDK v0.45, checks the sha256, unpacks to `$PS5_PAYLOAD_SDK`)
- Modify: `.github/workflows/ci.yml` (job `helper`: Ubuntu 24.04, `clang-19 lld-19 llvm-19-dev`, the script, `make -C helper ps5`, `file` shows an x86-64 ELF), `.github/workflows/release.yml` (job `helper` uploads `kite-helper.elf`; app jobs download it and build with `KITE_HELPER_ELF`; release attaches `kite-helper.elf`), version files, `README.md`, `CHANGELOG.md`

- [ ] **Step 1: Write** the script and workflow changes.
- [ ] **Step 2: Run** locally what can run: `make -C helper host`, `cargo test --workspace`, `npm test --prefix app`, YAML parse. Expected: green.
- [ ] **Step 3: Push and check** the CI `helper` job builds `kite-helper.elf`. Expected: success, an ELF 64-bit x86-64.
- [ ] **Step 4: Commit** `build: Kite helper in CI and releases, version 0.1.0-alpha.3`.
