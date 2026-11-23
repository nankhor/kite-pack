# Delete as you go Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A folder pack that is sent to the PS5 with **Delete the packed folder** on holds at most about 4 parts (≈16 GB) on the PC, by cutting each part the PS5 has confirmed down to its 64-byte header.

**Architecture:** Kite Pack sets a process-wide volume hook in its copy of Forge's packer. The packer checks each volume as it closes and calls the hook before starting the next one; Kite Pack's hook cuts confirmed parts and holds packing while 2 parts wait. Forge's core, seeing the hook, sizes the space check by parts and accepts cut volumes in its end check. The pipeline tells the hook which parts the PS5 confirmed.

**Tech Stack:** Rust (vendored `third_party/forge-zstd`, `third_party/forge-core`; crates `kite-pipeline`, `kite-forge`, `kite-ps5`, `kite-session`), React/TS screen.

**Spec:** `docs/superpowers/specs/2026-11-23-delete-as-you-go-design.md`

## Global Constraints

- Only with Save as Folder, Send to PS5 and Delete the packed folder on; every other job behaves exactly as today.
- A part is cut only after the PS5 confirmed it (`send_ahead` returned `Ok(true)`), and only from inside the packer's thread before a new volume opens (never during Forge's check or publish).
- Cut = `set_len(VOLUME_HEADER_SIZE)` (64) on the part in Forge's `.part` folder.
- At most 2 finished parts wait for the PS5 (`HOLD = 2`); the space check counts `MOST_HELD = 4` full parts (`VOLUME_LIMIT` each).
- Local Rust is 1.97: run cargo with `--ignore-rust-version`; leave out `kite-app` (`--exclude kite-app`).

## Review Focus

1. **PS5 switched off mid-pack** — packing must hold (not fill the disk) and carry on when the PS5 is back; Cancel must still stop it. Test in Task 3 (worker retries on Unreachable) and Task 4 (hook returns on cancel).
2. **Older helper / send-ahead gives up** — the hook must stop holding at once, or the pack hangs forever. Test in Task 4 (`ends_holding_when_letting_go_ends`).
3. **A part that doesn't read back** (disk error) — the pack must fail before that part can be cut. Test in Task 1 (`a_volume_that_reads_back_wrong_fails_the_pack`).
4. **Retry after the job failed while sending the rest** — cut parts must still count as on the PS5 (header patched, not resent). Test in Task 5 (`a_part_cut_to_its_header_is_patched_not_sent`).
5. **Packed size in History** — must show the real packed size, not the size of the stubs. Test in Task 5.

---

### Task 1: Packer: volume hook, check on close, reader for cut volumes

**Files:**
- Create: `third_party/forge-zstd/src/hook.rs`
- Modify: `third_party/forge-zstd/src/lib.rs` (pub mod hook), `src/writer.rs` (`Volumes`, `pack_file`, `compressed`), `src/reader.rs` (`unpack_cut`)
- Test: unit tests in `src/writer.rs` and `src/reader.rs`; `third_party/forge-zstd/README.md` (describe the second change)

**Interfaces:**
- Produces:
  - `pub trait VolumeHook: Send + Sync { fn open(&self, name: &str) -> std::io::Result<std::fs::File>; fn before_next(&self, next: u32, cancel: &AtomicBool) -> std::io::Result<()>; fn cut(&self) -> Vec<u32>; fn most_held(&self) -> u32; }`
  - `pub fn set_volume_hook(hook: Option<Arc<dyn VolumeHook>>)`, `pub fn volume_hook() -> Option<Arc<dyn VolumeHook>>` (one global `Mutex`)
  - `reader::unpack_cut(tree, cut: &[u32]) -> Result<PackedTree<…>>`: as `unpack`, but a volume in `cut` must be exactly 64 bytes with a header that matches the index; reading a chunk from it fails with "… was let go".
- With a hook: each raw chunk's `crc32fast::hash` is kept beside its `Chunk`; `Volumes::close` reads the volume back via `hook.open(name)`, decodes every chunk it holds (`reader::decode_chunk`) and compares CRCs, failing with `bad("{name} doesn't read back as written")`; opening volume `n ≥ 1` first calls `hook.before_next(n, cancel)` (an `Err` fails the pack; `Interrupted` maps to `Error::Cancelled`). Without a hook nothing changes.

- [ ] **Step 1: Failing tests** in `writer.rs` tests, with a `DirOutput` (`PackOutput` over a temp dir) and a test hook recording calls: `checks_each_volume_and_asks_before_the_next` (roll a volume the way `volume_rolls_over` fakes it near `VOLUME_LIMIT`; assert the hook saw `open("ampr_zstd-000.pak")` then `before_next(1)`), `a_volume_that_reads_back_wrong_fails_the_pack` (hook's `open` returns a copy with one payload byte flipped → error mentions `ampr_zstd-000.pak`), `no_hook_no_calls`. In `reader.rs`: `reads_a_pack_with_a_volume_cut_to_its_header` (two volumes, volume 0 cut after headers: listing intact, a file wholly in volume 1 reads, one in volume 0 errors; `unpack` without the cut list refuses it).
- [ ] **Step 2:** `cargo test --ignore-rust-version -p ps5-dump-forge-zstd` → new tests fail (no `hook` module).
- [ ] **Step 3:** Implement as above.
- [ ] **Step 4:** Same command → all pass (17 old + new).
- [ ] **Step 5:** Commit "forge-zstd: volume hook, check each volume as it closes".

### Task 2: Core: space need by parts, end check with cut volumes

**Files:**
- Modify: `third_party/forge-core/src/zstd_pack.rs` (`Plan::held_bytes`, `checks`), `src/convert.rs` (need), `third_party/forge-core/README.md`
- Test: `zstd_pack.rs` `#[cfg(test)] mod let_go_tests`

**Interfaces:**
- Consumes: Task 1's `volume_hook()`, `VolumeHook::{cut, most_held}`, `reader::unpack_cut`.
- Produces: `Plan::held_bytes(&self, most_held: u32) -> u64` = `min(packed worst case, most_held × VOLUME_LIMIT)` + headers + index. In `convert.rs`, with a hook the packed part of `need` is `held_bytes(hook.most_held())` (instead of the sample or the worst case). `checks` with a hook: `unpack_cut(tree, &hook.cut())`; files with any chunk in a cut volume are left out of `compare` (filter both `expected` and the tree, the tree through `Only`), and one check line reads `"{n} packed files were checked as their part was written, before it left this computer"`.

- [ ] **Step 1: Failing tests:** `holds_at_most_four_parts` (a 6 GiB plan: `held_bytes(4)` = 4 × `VOLUME_LIMIT` + headers + index; a 1 GiB plan: its worst case); `the_end_check_skips_files_in_cut_volumes` (pack a small game with the zstd writer into a temp dir, cut volume 0 to 64 bytes, set a test hook whose `cut()` is `[0]`: `checks` passes and has the line; with no hook it fails).
- [ ] **Step 2:** `cargo test --ignore-rust-version -p ps5-dump-forge-core --lib let_go_tests` → fail.
- [ ] **Step 3:** Implement. Clear the global hook at the end of each test.
- [ ] **Step 4:** Same → pass; `sample_tests` still pass. Add `let_go_tests` to the CI core test step (`cargo test --locked -p ps5-dump-forge-core --lib sample_tests let_go_tests` won't take two filters: use the `kite_` prefix: rename both modules `kite_sample_tests`, `kite_let_go_tests` and filter `kite_`).
- [ ] **Step 5:** Commit "forge-core: let parts go: space by parts, end check skips cut volumes".

### Task 3: Pipeline: tell the packer what the PS5 confirmed

**Files:**
- Modify: `crates/kite-pipeline/src/ports.rs` (`LetGo`, `PackRequest.let_go`), `src/runner.rs` (request, worker, `copy_out`), `src/lib.rs` (export)
- Test: `crates/kite-pipeline/tests/runner_ahead.rs`

**Interfaces:**
- Produces: `pub struct LetGo` (Debug, Default) with `pub fn confirm(&self, part: &Part)`, `pub fn end(&self)`, `pub fn confirmed(&self) -> Vec<Part>`, `pub fn ended(&self) -> bool`, `pub fn wait_change(&self, timeout: Duration)` (Mutex + Condvar). `PackRequest { …, let_go: Option<Arc<LetGo>> }` — `PartialEq` compares only `is_some()`.
- Runner: `let_go` is `Some` when the target is set (folder + send) and `job.options.delete_packed`. Worker: `Ok(true)` → `confirm`; `Err(Unreachable)` with `let_go` → sleep `retry_every` in 50 ms steps watching `stop`, retry the same part; `Ok(false)`, `Failed` or giving up → `end()`. The runner calls `end()` when `packer.pack` returns. `copy_out`'s room need with `let_go`: `part.size × 2 + COPY_SPARE` (the copy and one more part) instead of `left + part.size + COPY_SPARE`.

- [ ] **Step 1: Failing tests:** `tells_the_packer_each_part_the_ps5_has` (fake packer reads `req.let_go` and, after its last `PartReady`, waits until `confirmed()` has all parts; assert order and that it was `Some` only with `delete_packed`); `waits_for_a_ps5_that_is_away_while_letting_go` (fake sender returns `Unreachable` twice then `Ok(true)`: part confirmed, nothing ended); `ends_letting_go_when_parts_stop_going` (sender `Ok(false)`: `ended()` becomes true while the packer is still running).
- [ ] **Step 2:** `cargo test --ignore-rust-version -p kite-pipeline --test runner_ahead` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Same → pass; whole `kite-pipeline` passes.
- [ ] **Step 5:** Commit "pipeline: tell the packer which parts the PS5 has".

### Task 4: Kite Pack's hook: cut confirmed parts, hold at 2 waiting

**Files:**
- Create: `crates/kite-forge/src/let_go.rs`
- Modify: `crates/kite-forge/src/packer.rs` (register the hook around the Forge job when `req.let_go` is set and the format is Folder; clear it on every exit), `src/lib.rs`
- Test: unit tests in `let_go.rs`; `crates/kite-forge/tests/pack.rs`

**Interfaces:**
- Consumes: Task 1's `VolumeHook`, `set_volume_hook`; Task 3's `LetGo`.
- Produces: `pub(crate) struct Shrink { output: PathBuf, let_go: Arc<LetGo>, cut: Mutex<Vec<u32>> }` implementing `VolumeHook`: finds the `.part` folder as `Parts::find_part_dir` does (share that function); `open` opens `<part>/<name>`; `before_next(next, cancel)` cuts every confirmed part not yet cut (`set_len(64)`, recording its volume number), then waits while the parts closed and not yet confirmed (`next − confirmed`) are ≥ `HOLD` (2), `!ended` and `!cancel` (`wait_change(200 ms)`), cutting newly confirmed parts each time round; a cancel returns `Err(Interrupted)`. `most_held()` = 4.

- [ ] **Step 1: Failing tests** in `let_go.rs` (temp dir standing in for the `.part` folder, files of 300 bytes): `cuts_confirmed_parts_before_the_next`, `holds_while_two_parts_wait` (a thread confirms part 0 after 300 ms; `before_next(2)` returns only then, part 0 is 64 bytes), `ends_holding_when_letting_go_ends`, `stops_on_cancel`. In `tests/pack.rs`: `packs_the_same_with_letting_go` (one-volume game; the output equals a normal pack's apart from the random header ID; the hook is cleared afterwards).
- [ ] **Step 2:** `cargo test --ignore-rust-version -p kite-forge` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Same → pass.
- [ ] **Step 5:** Commit "kite-forge: cut parts the PS5 has, hold at two waiting".

### Task 5: Sending the rest and the packed size

**Files:**
- Modify: `crates/kite-ps5/src/send.rs` (`fixed_up`), `crates/kite-session/src/view.rs` (`packed_size_with`), `crates/kite-session/src/lib.rs` (use it)
- Test: `crates/kite-ps5/tests/send.rs`, `crates/kite-session` tests

**Interfaces:**
- `fixed_up`: the local file may be `part.size` long or exactly `part.settled_from` long (cut); either way the head is read from it and patched.
- `pub fn packed_size_with(path: &Path, parts: &[Part]) -> u64`: `packed_size(path)` plus, for each part whose file under `path` is shorter than `part.size`, the difference.

- [ ] **Step 1: Failing tests:** `a_part_cut_to_its_header_is_patched_not_sent` (send ahead a part, cut it locally to 64 bytes with a new header, `send_rest`: the PS5 file keeps its size, has the new header, and no part bytes were sent again); `counts_cut_parts_at_full_size`.
- [ ] **Step 2:** `cargo test --ignore-rust-version -p kite-ps5 --test send` and `-p kite-session` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Same → pass.
- [ ] **Step 5:** Commit "Count parts cut on the PC as on the PS5 and at full size".

### Task 6: Screen, end-to-end check, PR

**Files:**
- Modify: `app/src/parts/Options.tsx` (hint under Delete the packed folder), `app/src/App.test.tsx`, `CHANGELOG.md`, `crates/kite-ps5/tests/pack_and_send.rs` (a letting-go variant)

- [ ] **Step 1:** Hint, shown when Send and Delete the packed folder are on and `sendsAlongside(format, status)`: `"Each part leaves this computer once the PS5 has it, so packing needs about 16 GB free."`; otherwise today's text. Test it in `App.test.tsx` (helper 0.4.0: shown; 0.3.0: not).
- [ ] **Step 2:** End-to-end, by hand: temporarily set `VOLUME_LIMIT` to 256 MiB (revert after), run `pack_and_send` with `KITE_E2E_MB=1600` and Delete the packed folder: the PS5 copy equals a normal pack, and the `.part` folder never held more than 4 full parts (sample its file sizes every 100 ms in the test).
- [ ] **Step 3:** `npx vitest run`, `npx tsc --noEmit -p .`, `npm run build`, `cargo test --workspace --exclude kite-app --ignore-rust-version`, core and zstd tests.
- [ ] **Step 4:** Commit, PR, CI green, merge.
