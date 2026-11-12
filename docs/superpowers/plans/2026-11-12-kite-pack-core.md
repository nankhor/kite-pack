# Kite Pack Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Rust core of Kite Pack: a saved, resumable pipeline (pack → delete original → send → install DLC → delete packed) with Forge doing the zstd packing, the Trash doing every delete, and a `kite-pack` command-line tool that packs a game end to end.

**Architecture:** `kite-pipeline` holds the job model, the ordering rules and the runner, and talks to the outside world only through traits (`Packer`, `Trash`, `Sender`, `Installer`, `Sleeper`), so every rule is tested with fakes. `kite-forge` implements `Packer` and the source checks on top of `ps5-dump-forge-core`. `kite-cli` wires the real implementations together. Sending and DLC install get real implementations in plan 2 (Kite helper + AVA1); the screen comes in plan 3 (Tauri).

**Tech Stack:** Rust 2024 edition, `ps5-dump-forge-core` (git, pinned), `serde`/`serde_json`, `thiserror`, `walkdir`, `uuid`, `trash`, `clap`, `tempfile` and `assert_cmd` for tests.

**Spec:** `docs/superpowers/specs/2026-11-12-kite-pack-design.md`

## Global Constraints

- Licence GPL-3.0-or-later for every crate (`license = "GPL-3.0-or-later"`).
- Forge pinned to `https://github.com/quer3q/ps5-dump-forge` rev `8a3c7d3e4bfeac3b4ddf8e9ff020e819ae8ff1cd`, with Forge's own patch repeated in our root manifest: `[patch.crates-io] lzma-rust2 = { git = <same url>, rev = <same rev> }`.
- Forge declares `rust-version = "1.99"`. Local builds on an older toolchain pass `--ignore-rust-version` to any cargo command that builds `kite-forge` or `kite-cli`; CI uses current stable without it.
- zstd levels: Fast = 5, Medium = 9, Max = 19 (Forge's own presets).
- Output formats offered: `folder` (default), `exfat`, `ffpkg`, `ffpfs`. Never `ffpfsc`, `pkg` or `lz4`.
- Every delete goes through the `Trash` trait. Nothing in this repo calls `std::fs::remove_*` on user data, and Forge's `delete_source` is never set.
- The original is trashed only after Pack is Done and its snapshot still matches; the packed output is trashed only after Send is Done and every InstallDlc is Done.
- User-facing strings are sentence case, plain words, no blame (spec's copy).
- Commit messages end with:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_016C2d7CifA9ZVXQdxaTewAR`

## Review Focus

1. **A packed copy with the same name already exists** in the output folder → the new pack gets a `-2` name and the old copy is untouched (test in Task 8).
2. **A source path with spaces, brackets and accents** (`Ashen Tide (EU) – é`) → inspect and pack work (fixture name in Tasks 7 and 8).
3. **The Trash refuses** (a network share, a drive with no Trash) → DeleteOriginal fails with the reason, the original stays, there is no permanent-delete fallback (test in Task 4).
4. **A job file that is corrupt or from a newer version** → `JobStore::unfinished` skips it, the app still starts (test in Task 3).
5. **The source changes while it is being packed** → DeleteOriginal refuses with "The original changed since packing; it was kept" (test in Task 4).

---

## File Structure

```
Cargo.toml                         workspace, shared deps, Forge patch
LICENSE                            GPL-3.0 text (copy of Forge's LICENSE)
README.md                          what it is, how to build and test
.gitignore
.github/workflows/ci.yml           cargo test on ubuntu, macos, windows
crates/kite-pipeline/
  Cargo.toml
  src/lib.rs                       re-exports
  src/options.rs                   OutputFormat, Level, SendTarget, Options, OptionsError
  src/steps.rs                     StepKind, StepState, Step, plan_steps
  src/snapshot.rs                  Snapshot
  src/job.rs                       Job, JobStore
  src/ports.rs                     Packer, Trash, Sender, Installer, Sleeper + their types, Unavailable
  src/runner.rs                    Runner, RunEvent, RunOutcome
  src/system_trash.rs              SystemTrash (trash crate)
  tests/fakes/mod.rs               FakePacker, FakeTrash, FakeSender, FakeInstaller, NoSleep, CallLog
  tests/runner_pack.rs             Task 4 tests
  tests/runner_send.rs             Task 5 tests
crates/kite-forge/
  Cargo.toml
  src/lib.rs
  src/inspect.rs                   GameInfo, DlcInfo, SourceProblem, inspect_game, check_dlc_pkg
  src/packer.rs                    ForgePacker
  tests/common/mod.rs              make_game, make_dlc, make_ps4_pkg fixtures
  tests/inspect.rs
  tests/pack.rs
crates/kite-cli/
  Cargo.toml                       [[bin]] name = "kite-pack"
  src/main.rs
  tests/cli.rs
```

---

### Task 1: Workspace, CI and the pipeline's options and steps

**Files:**
- Create: `Cargo.toml`, `LICENSE`, `README.md`, `.gitignore`, `.github/workflows/ci.yml`
- Create: `crates/kite-pipeline/Cargo.toml`, `src/lib.rs`, `src/options.rs`, `src/steps.rs`

**Interfaces:**
- Produces:
  - `enum OutputFormat { Folder, Exfat, Ffpkg, Ffpfs }` (serde lowercase, `Default` = Folder)
  - `enum Level { Fast, Medium, Max }` with `fn zstd_level(self) -> u8` (serde lowercase, `Default` = Fast)
  - `struct SendTarget { console: String, destination: String }`
  - `struct Options { format, level, output_dir: PathBuf, delete_original: bool, send: Option<SendTarget>, delete_packed: bool, dlc: Vec<PathBuf> }` with `fn validate(&self) -> Result<(), OptionsError>`
  - `enum OptionsError { DeletePackedWithoutSend, DlcWithoutSend }` (thiserror; messages: "Delete the packed folder needs Send to the PS5", "DLC can only be installed when sending to the PS5")
  - `enum StepKind { Pack, DeleteOriginal, Send, InstallDlc { pkg: PathBuf }, DeletePacked }` (serde tag `kind`, snake_case)
  - `enum StepState { Pending, Running, Waiting { since_unix: u64 }, Done { note: String }, Failed { reason: String } }` (serde tag `state`, snake_case)
  - `struct Step { kind: StepKind, state: StepState }`
  - `fn plan_steps(options: &Options) -> Vec<StepKind>`
  - All of the above derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`.

- [ ] **Step 1: Scaffold the workspace**

Root `Cargo.toml`: `[workspace] members = ["crates/*"]`, `resolver = "3"`, `[workspace.package] edition = "2024", license = "GPL-3.0-or-later", version = "0.1.0"`, `[workspace.dependencies]` for serde (derive), serde_json, thiserror 2, walkdir 2, uuid 1 (v4), trash 5, clap 4 (derive), dirs 6, ctrlc 3, tempfile 3, assert_cmd 2, and `ps5-dump-forge-core = { git, rev }` per Global Constraints; the `[patch.crates-io]` block; `[profile.dev.package."*"] opt-level = 3` (Forge's hashing is slow unoptimised). `LICENSE` = Forge's `LICENSE` file. `.gitignore`: `/target`. CI: one job, matrix `ubuntu-latest, macos-latest, windows-latest`, `dtolnay/rust-toolchain@stable`, `cargo test --workspace --locked`.

- [ ] **Step 2: Write the failing tests in `src/steps.rs` (`#[cfg(test)] mod tests`)**

```rust
fn opts(delete_original: bool, send: bool, delete_packed: bool, dlc: usize) -> Options
// builds Options with format Folder, level Fast, output_dir "/out",
// send Some(SendTarget { console: "ps5".into(), destination: "/data/homebrew".into() }) when send,
// dlc = (0..dlc).map(|i| PathBuf::from(format!("/dlc/{i}.pkg"))).collect()

#[test] fn pack_only() { assert_eq!(plan_steps(&opts(false,false,false,0)), vec![StepKind::Pack]); }
#[test] fn pack_and_delete_original() { assert_eq!(plan_steps(&opts(true,false,false,0)), vec![Pack, DeleteOriginal]); }
#[test] fn send_without_delete_packed() { assert_eq!(plan_steps(&opts(false,true,false,0)), vec![Pack, Send]); }
#[test] fn send_and_delete_packed() { assert_eq!(plan_steps(&opts(false,true,true,0)), vec![Pack, Send, DeletePacked]); }
#[test] fn every_toggle_with_two_dlc() {
    assert_eq!(plan_steps(&opts(true,true,true,2)), vec![Pack, DeleteOriginal, Send,
        InstallDlc { pkg: "/dlc/0.pkg".into() }, InstallDlc { pkg: "/dlc/1.pkg".into() }, DeletePacked]);
}
#[test] fn delete_original_and_send() { assert_eq!(plan_steps(&opts(true,true,false,1)), vec![Pack, DeleteOriginal, Send, InstallDlc { pkg: "/dlc/0.pkg".into() }]); }
#[test] fn delete_packed_needs_send() { assert_eq!(opts(false,false,true,0).validate(), Err(OptionsError::DeletePackedWithoutSend)); }
#[test] fn dlc_needs_send() { assert_eq!(opts(false,false,false,1).validate(), Err(OptionsError::DlcWithoutSend)); }
#[test] fn levels_match_forge_presets() { assert_eq!((Level::Fast.zstd_level(), Level::Medium.zstd_level(), Level::Max.zstd_level()), (5, 9, 19)); }
#[test] fn step_state_json_shape() {
    let json = serde_json::to_string(&StepState::Failed { reason: "x".into() }).unwrap();
    assert_eq!(json, r#"{"state":"failed","reason":"x"}"#);
}
```

- [ ] **Step 2b: Run them to see them fail** — `cargo test -p kite-pipeline` → compile errors (types not defined).

- [ ] **Step 3: Implement `options.rs` and `steps.rs`** to the Interfaces above. `plan_steps` order: Pack; DeleteOriginal if `delete_original`; if `send` is Some: Send, one InstallDlc per `dlc` entry in order, DeletePacked if `delete_packed`. `plan_steps` does not validate; callers validate first.

- [ ] **Step 4: Run** `cargo test -p kite-pipeline` → all pass.

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(pipeline): workspace, options and step planning"` (with the attribution lines).

---

### Task 2: Snapshot of the original

**Files:**
- Create: `crates/kite-pipeline/src/snapshot.rs` (tests inside)

**Interfaces:**
- Produces: `struct Snapshot { entries: Vec<SnapshotEntry> }`, `struct SnapshotEntry { path: String, len: u64, modified_ns: i128 }` (both serde, Debug, Clone, PartialEq, Eq); `impl Snapshot { fn take(root: &Path) -> io::Result<Snapshot>; fn matches(&self, root: &Path) -> io::Result<bool> }`.

`take`: a file → one entry with `path = ""`; a folder → one entry per regular file found by `walkdir` (links not followed, link entries recorded with `len` 0), `path` relative with `/` separators, entries sorted by path. `modified_ns` = mtime as nanoseconds since the Unix epoch (negative allowed). `matches` = `Snapshot::take(root)? == *self`; a missing root returns `Ok(false)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn unchanged_folder_matches()        // take, then matches → true
#[test] fn changed_size_does_not_match()     // append a byte to one file → false
#[test] fn added_file_does_not_match()       // create a new file → false
#[test] fn removed_file_does_not_match()     // delete a file → false
#[test] fn touched_file_does_not_match()     // set mtime +10 s with File::set_modified → false
#[test] fn single_file_source()              // root is a file: entries == [SnapshotEntry { path: "".into(), len, .. }]
#[test] fn missing_root_does_not_match()     // take, remove the folder → Ok(false)
```

- [ ] **Step 2: Run** `cargo test -p kite-pipeline snapshot` → fail (not defined).
- [ ] **Step 3: Implement** to the interface.
- [ ] **Step 4: Run** → pass.
- [ ] **Step 5: Commit** `feat(pipeline): snapshot of the original`.

---

### Task 3: Job and JobStore

**Files:**
- Create: `crates/kite-pipeline/src/job.rs` (tests inside)

**Interfaces:**
- Consumes: `Options`, `OptionsError`, `Step`, `StepKind`, `StepState`, `plan_steps`, `Snapshot`.
- Produces:
  - `struct Job { id: String, source: PathBuf, options: Options, steps: Vec<Step>, packed: Option<PathBuf>, snapshot: Option<Snapshot>, created_unix: u64 }` (serde)
  - `impl Job { fn new(source: PathBuf, options: Options) -> Result<Job, OptionsError>; fn next_open(&self) -> Option<usize>; fn is_finished(&self) -> bool; fn retry_from(&mut self, index: usize) }`
  - `struct JobStore` with `fn open(dir: PathBuf) -> io::Result<JobStore>; fn save(&self, job: &Job) -> io::Result<()>; fn load(&self, id: &str) -> io::Result<Job>; fn unfinished(&self) -> Vec<Job>; fn remove(&self, id: &str) -> io::Result<()>`

Rules: `new` validates, ids are `uuid::Uuid::new_v4().simple()`, every step starts Pending. `next_open` = index of the first step not Done. `is_finished` = every step Done. `retry_from(i)` sets step `i` and every later Failed step back to Pending. `save` writes `<dir>/<id>.json.tmp` then renames it to `<dir>/<id>.json`. `unfinished` reads every `*.json`, skips files that fail to parse (a corrupt or newer-format job never stops the app), drops finished jobs, and turns Running and Waiting steps back into Pending; newest `created_unix` first.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn new_job_has_pending_steps()        // every state == Pending; steps.len() == plan_steps(&o).len()
#[test] fn invalid_options_are_refused()     // Job::new(.., delete_packed without send) == Err(DeletePackedWithoutSend)
#[test] fn save_and_load_round_trip()        // load(id) == job
#[test] fn save_leaves_no_tmp_file()         // after save, no "*.tmp" in dir
#[test] fn unfinished_resets_running_and_waiting() // steps [Done, Running, Waiting] saved → loaded as [Done, Pending, Pending]
#[test] fn unfinished_skips_finished_jobs()
#[test] fn unfinished_skips_corrupt_files()  // write "{not json" to bad.json; unfinished() still returns the good job
#[test] fn retry_from_resets_failed_steps()  // [Done, Failed, Pending, Failed] retry_from(1) → [Done, Pending, Pending, Pending]
```

- [ ] **Step 2: Run** `cargo test -p kite-pipeline job` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → pass.
- [ ] **Step 5: Commit** `feat(pipeline): saved jobs`.

---

### Task 4: Ports and the runner, part 1 (pack, delete original, cancel, saving)

**Files:**
- Create: `crates/kite-pipeline/src/ports.rs`, `src/runner.rs`
- Create: `crates/kite-pipeline/tests/fakes/mod.rs`, `tests/runner_pack.rs`

**Interfaces:**
- Produces in `ports.rs`:
  - `struct PackRequest { source: PathBuf, output_dir: PathBuf, format: OutputFormat, level: Level }`
  - `struct PackOutcome { output: PathBuf, bytes: u64, files: u64, checks: Vec<String> }`
  - `enum Progress { Stage(String), Bytes { done: u64, total: u64 } }`
  - `enum PackError { Failed(String), Cancelled }`
  - `trait Packer: Send + Sync { fn pack(&self, req: &PackRequest, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<PackOutcome, PackError>; }`
  - `trait Trash: Send + Sync { fn trash(&self, path: &Path) -> Result<(), String>; }`
  - `enum SendError { Unreachable(String), Failed(String), Cancelled }`
  - `trait Sender: Send + Sync { fn send(&self, local: &Path, target: &SendTarget, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<(), SendError>; }`
  - `enum InstallError { Unreachable(String), Refused { code: u32, hint: String }, NotConfirmed, Failed(String), Cancelled }`
  - `trait Installer: Send + Sync { fn is_installed(&self, pkg: &Path, target: &SendTarget) -> Result<bool, InstallError>; fn install(&self, pkg: &Path, target: &SendTarget, cancel: &AtomicBool) -> Result<(), InstallError>; }`
  - `trait Sleeper: Send + Sync { fn sleep(&self, d: Duration); }` and `struct ThreadSleeper` (std::thread::sleep)
  - `struct Unavailable;` implementing `Sender` and `Installer`, every call `Failed("Sending to the PS5 isn't available yet")`
- Produces in `runner.rs`:
  - `enum RunEvent { StepStarted { index: usize }, Progress { index: usize, progress: Progress }, StepWaiting { index: usize, since_unix: u64 }, StepDone { index: usize, note: String }, StepFailed { index: usize, reason: String }, Cancelled { index: usize }, Finished }`
  - `enum RunOutcome { Finished, Failed { index: usize }, Cancelled { index: usize } }`
  - `struct Runner<'a> { packer: &'a dyn Packer, trash: &'a dyn Trash, sender: &'a dyn Sender, installer: &'a dyn Installer, sleeper: &'a dyn Sleeper, store: &'a JobStore, retry_every: Duration }`
  - `impl Runner<'_> { fn run(&self, job: &mut Job, cancel: &AtomicBool, events: &dyn Fn(RunEvent)) -> RunOutcome }`

Runner rules for this task:
- Starts at `job.next_open()`; a Failed step at or after it returns `Failed { index }` without running anything (the caller calls `retry_from` first).
- Before each step: if `cancel` is set → step stays Pending, `Cancelled { index }`.
- Every state change is saved with `store.save(job)` before the matching event is emitted. A save error marks the current step `Failed { reason: "Couldn't save progress: <err>" }` and stops.
- **Pack:** if `options.delete_original`, `job.snapshot = Some(Snapshot::take(&job.source)?)` before packing (a take error fails Pack). On `Ok(out)`: `job.packed = Some(out.output)`, `Done { note: "<files> files, <bytes> bytes, checked" }`. `PackError::Failed(r)` → `Failed { reason: r }`. `Cancelled` → step back to Pending, outcome Cancelled.
- **DeleteOriginal:** requires the Pack step Done and `job.snapshot` matching `job.source`; a mismatch (or a missing snapshot) → `Failed { reason: "The original changed since packing; it was kept" }` and `trash` is never called. Then `trash.trash(&job.source)`: Ok → `Done { note: "Moved to the Trash" }`, Err(r) → `Failed { reason: format!("Couldn't move the original to the Trash: {r}") }`.
- After the last step: `Finished`.

- [ ] **Step 1: Write the fakes in `tests/fakes/mod.rs`**

`CallLog = Arc<Mutex<Vec<String>>>`, each fake pushes one line per call: `"pack"`, `"trash <path>"`, `"send"`, `"installed? <pkg file name>"`, `"install <pkg file name>"`. `FakePacker { log, result: PackResultKind (Ok | Fail(String) | Cancel), output: PathBuf, before: Option<Box<dyn Fn() + Send + Sync>> }` (runs `before` first: lets a test change the source mid-pack). `FakeTrash { log, fail: Option<String> }` (on success it really renames the path into `<tempdir>/trash/`, so tests can assert the original is gone). `FakeSender { log, script: Mutex<VecDeque<Result<(), SendError>>> }` (pops the next result; empty → Ok). `FakeInstaller { log, installed: Mutex<HashSet<String>>, script: Mutex<VecDeque<Result<(), InstallError>>> }`. `NoSleep` (records the durations it was asked to sleep).

- [ ] **Step 2: Write the failing tests in `tests/runner_pack.rs`**

```rust
#[test] fn pack_only_finishes()                       // outcome Finished; job.packed == Some(fake output); log == ["pack"]
#[test] fn pack_then_delete_original()                // log == ["pack", "trash <source>"]; source no longer exists
#[test] fn pack_failure_deletes_nothing()             // FakePacker Fail("disk full") → Failed{index:0}; step 0 reason "disk full"; log has no "trash"; step 1 Pending
#[test] fn changed_source_is_kept()                   // FakePacker.before appends a file to the source → DeleteOriginal Failed "The original changed since packing; it was kept"; no trash call; source still there
#[test] fn trash_refusal_keeps_the_original()         // FakeTrash fail "no Trash on this drive" → reason "Couldn't move the original to the Trash: no Trash on this drive"; source still there
#[test] fn cancel_before_start()                      // cancel set → Cancelled{index:0}; step 0 Pending; log empty
#[test] fn pack_cancelled_returns_step_to_pending()   // PackResultKind::Cancel → Cancelled{index:0}, step Pending
#[test] fn every_state_change_is_saved()              // after run, JobStore::load(id) == job
#[test] fn failed_step_blocks_until_retry()           // run after a failure → Failed{index} again with no calls; retry_from(index) then run → Finished
#[test] fn events_follow_the_steps()                  // collected events for pack+delete: [StepStarted{0}, StepDone{0,..}, StepStarted{1}, StepDone{1,..}, Finished] (Progress events filtered out)
```

- [ ] **Step 3: Run** `cargo test -p kite-pipeline --test runner_pack` → fail.
- [ ] **Step 4: Implement** `ports.rs` and `runner.rs` (Send, InstallDlc and DeletePacked return `Failed { reason: "not implemented" }` until Task 5).
- [ ] **Step 5: Run** → pass, and `cargo test -p kite-pipeline` still passes.
- [ ] **Step 6: Commit** `feat(pipeline): runner with pack and delete original`.

---

### Task 5: Runner, part 2 (send, waiting, DLC, delete packed, resume)

**Files:**
- Modify: `crates/kite-pipeline/src/runner.rs`
- Create: `crates/kite-pipeline/tests/runner_send.rs`

**Interfaces:**
- Consumes: Task 4's ports, fakes and `Runner`.
- Produces: no new public names; the three remaining step kinds work.

Runner rules for this task:
- **Send:** requires `job.packed`. Loop: `sender.send(packed, target, ..)`. `Ok` → `Done { note: "Sent and checked" }`. `Unreachable(_)` → if not already Waiting, set `Waiting { since_unix: now }`, save, emit `StepWaiting`; `sleeper.sleep(retry_every)`; check `cancel` (set → Pending, Cancelled); try again. There is no retry limit. `Failed(r)` → Failed. `Cancelled` → Pending, Cancelled.
- **InstallDlc { pkg }:** `installer.is_installed(pkg)` first: `true` → `Done { note: "Already on the PS5" }`. Otherwise `install`: Ok → `Done { note: "Installed and confirmed" }`; `Refused { code, hint }` → `Failed { reason: format!("{hint} (0x{code:08X})") }`; `NotConfirmed` → `Failed { reason: "Handed to the PS5 but never showed up" }`; `Failed(r)` → Failed; `Unreachable` → the same waiting loop as Send. **A failed DLC does not stop the next DLC**: the runner records the first failed index and carries on with the following InstallDlc steps.
- **DeletePacked:** runs only when the Send step and every InstallDlc step are Done; otherwise it stays Pending and the outcome is `Failed { index: <first failed step> }`. Then `trash.trash(packed)`: Ok → `Done { note: "Moved to the Trash" }`, Err(r) → `Failed { reason: format!("Couldn't move the packed folder to the Trash: {r}") }`.
- `retry_every` default used by callers: 5 seconds.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn all_six_toggle_combinations()   // for each (delete_original, send, delete_packed) valid combo, with 0 and 1 DLC:
                                            // outcome Finished, every step Done, log order == expected (e.g. full: pack, trash source, send, installed? 0.pkg, install 0.pkg, trash packed)
#[test] fn unreachable_ps5_waits_then_sends()          // FakeSender script [Unreachable, Unreachable, Ok] → Finished; NoSleep recorded two 5 s sleeps; exactly one StepWaiting event
#[test] fn waiting_since_is_kept_across_retries()      // since_unix in the saved job equals the first StepWaiting's since_unix
#[test] fn cancel_while_waiting()                      // sender always Unreachable; NoSleep sets cancel on its 3rd sleep → Cancelled{index of Send}; Send Pending
#[test] fn send_failure_keeps_packed_copy()            // FakeSender Failed("checksum") → Failed; log has no "trash <packed>"
#[test] fn refused_dlc_does_not_stop_the_next_one()    // two DLC, first Refused{code:0x80B22404, hint:"Sony refused it"} → second still installed; DeletePacked Pending;
                                                        // outcome Failed{index of first DLC}; reason "Sony refused it (0x80B22404)"
#[test] fn unconfirmed_dlc_keeps_packed_copy()         // NotConfirmed → reason "Handed to the PS5 but never showed up"; no "trash <packed>"
#[test] fn already_installed_dlc_is_skipped()          // FakeInstaller.installed contains "0.pkg" → note "Already on the PS5"; no "install 0.pkg"
#[test] fn resume_after_crash_mid_send()               // job saved with [Done, Running(Send), Pending]; JobStore::unfinished() → run → log == ["send", ...]; no "pack"
#[test] fn resume_after_crash_mid_pack()               // [Running(Pack), Pending] → run packs again, then continues
```

- [ ] **Step 2: Run** `cargo test -p kite-pipeline --test runner_send` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p kite-pipeline` → all pass.
- [ ] **Step 5: Commit** `feat(pipeline): send, DLC install, delete packed, resume`.

---

### Task 6: The real Trash

**Files:**
- Create: `crates/kite-pipeline/src/system_trash.rs` (tests inside)

**Interfaces:**
- Produces: `struct SystemTrash;` implementing `Trash` with `trash::delete(path)`; the error's `to_string()` becomes the `Err`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn moves_a_folder_to_the_trash()   // tempdir/game with a file → Ok; path no longer exists
#[test] fn missing_path_is_an_error()      // SystemTrash.trash(nonexistent) → Err(_)
```
Mark both `#[cfg_attr(target_os = "linux", ignore = "needs a desktop Trash")]` only if CI shows Linux has none; run them first without it.

- [ ] **Step 2: Run** `cargo test -p kite-pipeline system_trash` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → pass.
- [ ] **Step 5: Commit** `feat(pipeline): system Trash`.

---

### Task 7: Forge adapter, part 1 (inspect and checks)

**Files:**
- Create: `crates/kite-forge/Cargo.toml`, `src/lib.rs`, `src/inspect.rs`
- Create: `crates/kite-forge/tests/common/mod.rs`, `tests/inspect.rs`

**Interfaces:**
- Consumes: `ps5_dump_forge_core::{inspect, Inspection, Dlc}`.
- Produces:
  - `struct GameInfo { kind: String, title: Option<String>, title_id: Option<String>, content_id: Option<String>, version: Option<String>, total_bytes: u64, dlcs: Vec<DlcInfo>, icon_data_url: Option<String>, findings: Vec<String> }` (serde)
  - `struct DlcInfo { content_id: String, label: String, name: Option<String>, bytes: u64 }` (serde)
  - `enum SourceProblem { NotAGame, AlreadyPacked, Ps4Package, RetailPackage, PatchPackage, Unreadable(String) }` with `fn message(&self) -> String`: "This is an update or a DLC on its own, not a game", "Already packed", "PS4 games can't be packed", "Retail packages can't be read", "This is an update package, not a game", and the reader's own words for Unreadable.
  - `fn inspect_game(path: &Path) -> Result<GameInfo, SourceProblem>`
  - `fn check_dlc(path: &Path, title_id: &str) -> Result<DlcInfo, String>`: a DLC `.pkg` (or a DLC folder; Forge inspects both). Ok only when Forge reports `additional_content` and the same `title_id`; then `DlcInfo { content_id, label: last 16 chars of content_id, name: title_name, bytes: total_bytes }`. Otherwise `Err(format!("This isn't DLC for {title_id}"))`, or Forge's own error text when it can't read the file.

Mapping: inspect error text containing "this is a PS4 package" → Ps4Package; "retail package" → RetailPackage; any other error → Unreadable(text). Then `patch` → PatchPackage, `additional_content` → NotAGame, `zstd_packed` → AlreadyPacked. `title` = `title_name`.

Fixtures (`tests/common/mod.rs`): `make_game(dir: &Path, name: &str) -> PathBuf` writes `eboot.bin` (4096 random-ish bytes: a fixed LCG, not `/dev/urandom`, so tests are deterministic), `sce_sys/param.json` = `{"titleId":"PPSA01234","contentId":"UP0000-PPSA01234_00-ASHENTIDE0000000","contentVersion":"01.050.000","requiredSystemSoftwareVersion":"0x0700000000000000","localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Ashen Tide"}}}`, `data/level1.bin` (600 000 bytes of "AB" repeated) and `data/audio.bnk` (200 000 LCG bytes). `make_dlc_folder(dir, name)`: only `sce_sys/param.json` with contentId `UP0000-PPSA01234_00-TIDEBREAKER00000`. `make_ps4_pkg(dir) -> PathBuf`: `ps4.pkg` = `b"\x7FCNT"` + 4096 zero bytes.

- [ ] **Step 1: Write the failing tests in `tests/inspect.rs`**

```rust
#[test] fn reads_the_test_game()          // title Some("Ashen Tide"), title_id Some("PPSA01234"), version Some("01.050.000"), dlcs empty, findings empty
#[test] fn odd_folder_name_works()        // make_game(tmp, "Ashen Tide (EU) – é") → Ok
#[test] fn dlc_folder_is_not_a_game()     // make_dlc_folder → Err(NotAGame)
#[test] fn ps4_package_is_refused()       // make_ps4_pkg → Err(Ps4Package)
#[test] fn missing_path_is_unreadable()   // → Err(Unreadable(_))
#[test] fn messages_are_plain()           // SourceProblem::Ps4Package.message() == "PS4 games can't be packed"
#[test] fn dlc_for_this_game_is_accepted() // check_dlc(dlc_folder, "PPSA01234") → Ok(DlcInfo { label: "TIDEBREAKER00000", .. })
#[test] fn dlc_for_another_game_is_refused() // check_dlc(dlc_folder, "PPSA99999") → Err("This isn't DLC for PPSA99999")
#[test] fn a_game_is_not_dlc()            // check_dlc(game_folder, "PPSA01234") → Err("This isn't DLC for PPSA01234")
```

- [ ] **Step 2: Run** `cargo test -p kite-forge --ignore-rust-version --test inspect` → fail. The first build fetches Forge; if `[patch.crates-io]` cannot find `lzma-rust2` in the git repo, the error says so — then vendor Forge's `vendor/lzma-rust2` folder into `vendor/lzma-rust2` in this repo and point the patch at that path instead.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → pass.
- [ ] **Step 5: Commit** `feat(forge): inspect and source checks`.

---

### Task 8: Forge adapter, part 2 (the packer)

**Files:**
- Create: `crates/kite-forge/src/packer.rs`, `tests/pack.rs`

**Interfaces:**
- Consumes: `kite_pipeline::{Packer, PackRequest, PackOutcome, PackError, Progress, OutputFormat}`; `ps5_dump_forge_core::{Jobs, ConvertRequest, Event, Format, PackKind, generated_output}`.
- Produces: `struct ForgePacker { full_verify: bool }` implementing `Packer`; `fn to_forge_format(f: OutputFormat) -> Format`.

`pack`: output = `generated_output(&req.source, format, &req.output_dir, &[], Some(PackKind::Zstd), None)` (Forge never returns a path that exists). Build `ConvertRequest { source, format, output, zstd_pack: true, zstd_level: req.level.zstd_level(), full_verify: self.full_verify, delete_source: false, ..Default-like }` (Forge's struct has serde defaults; build it with `serde_json::from_value(json!({...}))` so new Forge fields keep their defaults). Create `Jobs::new` with a closure sending every `Event` down an `mpsc` channel, `start` the request, forward `Progress { stage, done, total }` as `Progress::Stage` on stage change and `Progress::Bytes` otherwise, poll `cancel` every 200 ms (`recv_timeout`) and call `jobs.cancel(id)` once when it is set. `Done { Ok(report) }` → `PackOutcome { output: report.output, bytes: report.bytes, files: report.files, checks: report.checks }`. `Done { Err(e) }` → `Cancelled` when `e` starts with "cancelled", else `Failed(e)`.

- [ ] **Step 1: Write the failing tests in `tests/pack.rs`**

```rust
#[test] fn packs_the_test_game_to_a_folder()  // Ok(out); out.output.join("ampr_zstd.index").is_file(); out.output.join("eboot.bin").is_file();
                                               // inspect_game(&out.output) == Err(AlreadyPacked); the source folder is unchanged (Snapshot before == after)
#[test] fn second_pack_gets_a_new_name()       // pack twice into the same dir → second output != first, file name ends with "-2"; first still has ampr_zstd.index
#[test] fn packs_to_ffpkg()                    // format Ffpkg → output extension "ffpkg", is_file()
#[test] fn reports_progress()                  // collected Progress contains at least one Bytes { total > 0 } and a Stage("verify")
#[test] fn cancel_stops_the_pack()             // cancel set before pack → Err(Cancelled); no new entry in output_dir
```

- [ ] **Step 2: Run** `cargo test -p kite-forge --ignore-rust-version --test pack` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → pass.
- [ ] **Step 5: Commit** `feat(forge): zstd packer`.

---

### Task 9: The `kite-pack` command-line tool

**Files:**
- Create: `crates/kite-cli/Cargo.toml` (`[[bin]] name = "kite-pack"`), `src/main.rs`, `tests/cli.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: commands
  - `kite-pack inspect <path> [--json]` — prints title, title ID, version, size, DLC; or the `SourceProblem` message and exit code 2.
  - `kite-pack pack <source> --out <dir> [--format folder|exfat|ffpkg|ffpfs] [--level fast|medium|max] [--delete-original]` — validates with `inspect_game` first (problem → message, exit 2), creates a Job, saves it, runs it with `ForgePacker`, `SystemTrash`, `Unavailable`, `ThreadSleeper`, `retry_every` 5 s; prints one line per RunEvent (`Pack: done, 6 files, 917199 bytes, checked`); exit 0 on Finished, 1 on Failed or Cancelled; Ctrl-C sets the cancel flag.
  - `kite-pack jobs` — lists unfinished jobs: id, source, the next open step and its state.
  - `kite-pack resume <id>` — loads the job, `retry_from(next_open)`, runs it.
  - Jobs live in `$KITE_PACK_DATA/jobs` when that variable is set, else `<OS data dir>/kite-pack/jobs` (`dirs::data_dir()`).

- [ ] **Step 1: Write the failing tests in `tests/cli.rs`** (`assert_cmd`, `KITE_PACK_DATA` set to a tempdir)

```rust
#[test] fn inspect_prints_the_title()            // stdout contains "Ashen Tide" and "PPSA01234"; exit 0
#[test] fn inspect_refuses_a_ps4_package()      // exit code 2; stdout or stderr contains "PS4 games can't be packed"
#[test] fn pack_writes_a_packed_folder()        // exit 0; out dir has one entry containing ampr_zstd.index; `jobs` prints nothing unfinished
#[test] fn pack_with_delete_original()          // HOME/XDG_DATA_HOME pointed at a tempdir so the Trash is a test one; exit 0; the source folder is gone
#[test] fn bad_level_is_rejected()              // --level huge → non-zero exit; clap error
```

- [ ] **Step 2: Run** `cargo test -p kite-cli --ignore-rust-version` → fail.
- [ ] **Step 3: Implement** with `clap` derive.
- [ ] **Step 4: Run** `cargo test --workspace --ignore-rust-version` → everything passes.
- [ ] **Step 5: Write `README.md`**: one paragraph on what Kite Pack is, build (`cargo build --release`), the `kite-pack` commands, the licence line, and credit to Forge (quer3q), ps5upload and AVA1 (phantomptr).
- [ ] **Step 6: Commit** `feat(cli): kite-pack inspect, pack, jobs, resume`.

---

## After this plan

- **Plan 2: Kite helper and sending.** The C helper (AVA1 C node + version, free space, install `.pkg`, DLC landed), built with the PS5 payload SDK in Docker; `kite-ps5` crate implementing `Sender` and `Installer` with the `ava1` crate, the package host and the ported install hints; `kite-pack pack --send`.
- **Plan 3: The screen.** Tauri 2 + React app over these crates, following the "Direction A, simpler" mockup.
