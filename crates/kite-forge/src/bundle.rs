//! DLC and a backport folded into the packed copy.
//!
//! Packing used to be one input, one output: the game folder, as it is. This module adds a step
//! in front of it — a staging tree that is the game plus what the user picked — so Forge still
//! packs one folder and every file it writes is the final one (an index of sizes and offsets
//! cannot be made before the contents are known).
//!
//! ## DLC
//!
//! A game reads its add-ons through the DLC emulator (`drakmor/dlc_emu`): replacement
//! `libSceAppContent`/`libSceNpEntitlementAccess`/`libSceGameUpdate` modules that answer from
//! `/app0/dlc_emu.ini`. So DLC goes in as that emulator reads it:
//!
//! ```text
//! Game/
//!   dlc_emu.ini            ← [PSAC] content_id, download_status, mount_point=/app0/addcont0
//!   addcont0/              ← the DLC's own files
//!   fakelib/libSceAppContent.sprx (+ the other two)
//! ```
//!
//! A dump that already carries the emulator keeps its copy; one that doesn't gets the modules
//! from the folder the user picked once in Settings. A DLC whose content id is already in the
//! ini is left out, so merging the same folder twice is harmless.
//!
//! ## Backports
//!
//! A backport is an overlay: its files replace the game's (`eboot.bin`, `sce_sys/param.json`,
//! `fakelib/*.sprx`). It is copied over the staging tree before packing.
//!
//! ## Staging
//!
//! Forge packs one folder, and it refuses a source whose files have more than one name (a hard
//! link is a dump that was linked, which it will not convert), so the staging tree is a real
//! tree: a copy-on-write clone of the game where the filesystem has one (APFS's `clonefile`,
//! btrfs/XFS's reflink: instant, no space until a block changes), a plain copy where it does
//! not. The user's dump is never modified, and removing the staging folder cannot touch it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kite_pipeline::Bundle;
use ps5_dump_forge_core::{ConvertRequest, Event, Inspection, Jobs};

use crate::dlc_emu;
use serde::Serialize;

/// `dlc_emu.ini` is parsed once per module load, at most 32 KiB: a list bigger than this is not
/// a DLC list (and would be refused by the emulator anyway).
const INI_MAX: usize = 32 * 1024;
/// The emulator's own entry cap.
const INI_MAX_ENTRIES: usize = 1024;
/// How deep to look for the modules inside the folder the user picked (an unzipped release).
const EMULATOR_DEPTH: usize = 3;
/// How deep a DLC folder or a backport is walked. Games nest deeper than this, a backport
/// doesn't; a deeper tree is copied by the staging link instead.
const MAX_DEPTH: usize = 32;

/* ---- what the screen shows ------------------------------------------------------------------- */

/// One thing the user picked to go inside the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleItem {
    /// The folder or `.pkg` itself.
    pub path: PathBuf,
    /// `dlc` or `backport`.
    pub kind: String,
    /// The console's name for it, else its content id, else the folder name.
    pub name: String,
    pub title_id: Option<String>,
    pub content_id: Option<String>,
    /// The last 16 characters of the content id: what tells a game's DLC apart.
    pub label: Option<String>,
    /// Bytes it will add to the packed copy.
    pub bytes: u64,
    /// Why it can't go in, in the screen's words; `None` when it can.
    pub problem: Option<String>,
}

/// Whether everything can go in, and why not when it can't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePlan {
    pub items: Vec<BundleItem>,
    /// `inPack` or `unavailable`.
    pub route: String,
    /// The first thing that stops it, in the screen's words.
    pub problem: Option<String>,
    /// Where the DLC emulator comes from: `dump` (the game already has one), `builtin` (Kite
    /// Pack carries it), `picked` (an override folder the caller supplied), or `none` (no DLC).
    pub emulator_from: String,
}

/// What a staged pack got.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub dlc: usize,
    /// DLC whose content id was already in the dump's `dlc_emu.ini`.
    pub dlc_already: usize,
    pub backport_files: u64,
    pub backport_bytes: u64,
    pub emulator_added: bool,
    /// Game files staged (cloned or copied), and bytes.
    pub staged_files: u64,
    pub staged_bytes: u64,
}

/* ---- the plan -------------------------------------------------------------------------------- */

/// Reads what the user picked against `source` (the game folder) without writing anything.
/// The screen shows this; the packer runs it again before it starts so a stale plan can't
/// write a half-bundle.
pub fn plan(source: &Path, bundle: &Bundle) -> BundlePlan {
    let game = inspect(source).ok();
    let game_title = game.as_ref().and_then(|g| g.title_id.clone());
    let mut items = Vec::new();
    let mut problem: Option<String> = None;

    for path in &bundle.dlc {
        let item = dlc_item(path, game_title.as_deref());
        if problem.is_none() {
            problem = item.problem.clone();
        }
        items.push(item);
    }
    if let Some(path) = &bundle.backport {
        let item = backport_item(path, game_title.as_deref());
        if problem.is_none() {
            problem = item.problem.clone();
        }
        items.push(item);
    }

    // DLC needs the emulator: the dump's own copy, a folder the caller supplied, or the one
    // Kite Pack carries.
    let has_own = has_emulator(source);
    let picked_ok = bundle.emulator.as_deref().is_some_and(emulator_folder_has_modules);
    let emulator_from = if bundle.dlc.is_empty() {
        "none"
    } else if has_own {
        "dump"
    } else if picked_ok {
        "picked"
    } else {
        "builtin"
    };
    if problem.is_none() && !bundle.dlc.is_empty() && bundle.emulator.is_some() && !picked_ok && !has_own {
        problem = Some(
            "That DLC emulator folder has no libSceAppContent modules. Pick the unzipped dlc_emu folder, or leave it unset to use the one Kite Pack carries."
                .into(),
        );
    }

    BundlePlan {
        route: if problem.is_some() { "unavailable".into() } else { "inPack".into() },
        problem,
        items,
        emulator_from: emulator_from.into(),
    }
}

fn dlc_item(path: &Path, game_title: Option<&str>) -> BundleItem {
    let mut item = BundleItem {
        path: path.to_path_buf(),
        kind: "dlc".into(),
        name: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        title_id: None,
        content_id: None,
        label: None,
        bytes: 0,
        problem: None,
    };
    let found = match inspect(path) {
        Ok(f) => f,
        Err(e) => {
            item.problem = Some(format!("Couldn't read it: {e:#}"));
            return item;
        }
    };
    item.title_id = found.title_id.clone();
    item.content_id = found.content_id.clone();
    item.label = found.content_id.as_deref().map(label_of);
    item.bytes = found.total_bytes;
    if let Some(name) = found.title_name.clone() {
        item.name = name;
    } else if let Some(id) = &found.content_id {
        item.name = id.clone();
    }

    if !found.additional_content {
        item.problem = Some("This is a game, not DLC: it has its own eboot.bin".into());
    } else if let (Some(have), Some(want)) = (found.title_id.as_deref(), game_title) {
        if !have.eq_ignore_ascii_case(want) {
            item.problem = Some(format!("DLC for {have}, not for {want}"));
        }
    } else if found.content_id.is_none() {
        item.problem = Some("It carries no content id, so the game can't be told what it is".into());
    }
    item
}

fn backport_item(path: &Path, game_title: Option<&str>) -> BundleItem {
    let mut item = BundleItem {
        path: path.to_path_buf(),
        kind: "backport".into(),
        name: path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        title_id: None,
        content_id: None,
        label: None,
        bytes: 0,
        problem: None,
    };
    if !path.is_dir() {
        item.problem = Some("A backport is a folder: pick the one with eboot.bin and fakelib/".into());
        return item;
    }
    let (files, bytes) = match walk_totals(path) {
        Ok(v) => v,
        Err(e) => {
            item.problem = Some(format!("Couldn't read the backport: {e}"));
            return item;
        }
    };
    item.bytes = bytes;
    if files == 0 {
        item.problem = Some("That folder is empty".into());
        return item;
    }
    let has_eboot = path.join("eboot.bin").is_file();
    let has_fakelib = path.join("fakelib").is_dir() || path.join("fakelib2").is_dir();
    if !has_eboot && !has_fakelib {
        item.problem = Some("That folder has no eboot.bin and no fakelib/: it doesn't look like a backport".into());
        return item;
    }
    if let Ok(found) = inspect(path) {
        item.title_id = found.title_id.clone();
        if let (Some(have), Some(want)) = (found.title_id.as_deref(), game_title) {
            if !have.eq_ignore_ascii_case(want) {
                item.problem = Some(format!("This backport is for {have}, not for {want}"));
            }
        }
    }
    item
}

/// `UP9000-PPSA01234_00-TIDEBREAKER000` → `TIDEBREAKER000`, as Forge and the console name it.
pub fn label_of(content_id: &str) -> String {
    content_id.rsplit_once('-').map(|(_, l)| l).unwrap_or(content_id).to_string()
}

/// Does this game folder carry the DLC emulator already?
pub fn has_emulator(game: &Path) -> bool {
    for lib in ["fakelib", "fakelib2"] {
        if emulator_modules_in(&game.join(lib)).iter().next().is_some() {
            return true;
        }
    }
    false
}

fn emulator_modules_in(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return found };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if dlc_emu::is_module(name) {
                    found.push(path);
                }
            }
        }
    }
    found
}

/// Whether the folder picked in Settings is actually an unzipped dlc_emu release.
fn emulator_folder_has_modules(dir: &Path) -> bool {
    !find_modules(dir, EMULATOR_DEPTH).is_empty()
}

/// Every `libSceAppContent.*`, `libSceNpEntitlementAccess.*`, `libSceGameUpdate.*` below `dir`,
/// a little way down: releases ship them in a folder of their own.
fn find_modules(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if depth == 0 {
        return found;
    }
    let Ok(entries) = fs::read_dir(dir) else { return found };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if dlc_emu::is_module(name) {
                    found.push(path);
                }
            }
        } else if path.is_dir() {
            found.extend(find_modules(&path, depth - 1));
        }
    }
    found
}

/* ---- staging --------------------------------------------------------------------------------- */

/// A staging tree that removes itself, hard links and all: removing the folder never touches
/// the game it was built from.
pub struct Staging {
    path: PathBuf,
}

impl Staging {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Builds the tree Forge will pack: the game, then the DLC and backport folded in.
///
/// Staging goes beside the source so the game's files can be hard-linked rather than copied;
/// when that isn't possible the files are copied. The source is only ever read.
pub fn stage(source: &Path, bundle: &Bundle) -> Result<(Staging, Staged), String> {
    if !source.is_dir() {
        return Err(
            "DLC and backports can be packed into a game folder. An image or a .pkg has to be unpacked first."
                .into(),
        );
    }
    let checked = plan(source, bundle);
    if let Some(problem) = checked.problem {
        return Err(problem);
    }

    let staging = new_staging(source)?;
    let mut out = Staged::default();
    let (files, bytes) = walk_totals(source)?;
    stage_tree(source, staging.path())?;
    out.staged_files = files;
    out.staged_bytes = bytes;

    let ini_path = staging.path().join("dlc_emu.ini");
    let mut ini = match fs::read_to_string(&ini_path) {
        Ok(text) if text.len() <= INI_MAX => text,
        Ok(_) => return Err("The dump's dlc_emu.ini is too big for the emulator to read".into()),
        Err(_) => String::new(),
    };
    if ini.len() > INI_MAX {
        return Err("The dump's dlc_emu.ini is too big for the emulator to read".into());
    }
    let mut used: BTreeSet<String> = ini_content_ids(&ini);

    for path in &bundle.dlc {
        let found = inspect(path).map_err(|e| format!("Couldn't read {}: {e:#}", path.display()))?;
        let content_id = found.content_id.clone().ok_or("A DLC without a content id can't be added")?;
        if used.contains(&content_id) {
            out.dlc_already += 1;
            continue;
        }
        if ini_entry_count(&ini) >= INI_MAX_ENTRIES {
            return Err("The dump already has the emulator's 1024-entry limit of DLC".into());
        }
        let index = next_addcont_index(staging.path(), &ini)?;
        let mount = format!("/app0/addcont{index}");
        let dest = staging.path().join(format!("addcont{index}"));
        fs::create_dir_all(&dest).map_err(|e| format!("Couldn't make {}: {e}", dest.display()))?;
        copy_item(path, &dest)?;
        if !ini.is_empty() && !ini.ends_with('\n') {
            ini.push('\n');
        }
        ini.push_str(&format!(
            "[PSAC]\ncontent_id={content_id}\ndownload_status=INSTALLED\nmount_point={mount}\n"
        ));
        used.insert(content_id);
        out.dlc += 1;
    }

    if out.dlc > 0 && !has_emulator(staging.path()) {
        let fakelib = staging.path().join("fakelib");
        fs::create_dir_all(&fakelib).map_err(|e| format!("Couldn't make {}: {e}", fakelib.display()))?;
        match bundle.emulator.as_deref() {
            // A folder the caller supplied wins, for a newer emulator than the one carried.
            Some(from) => {
                let modules = find_modules(from, EMULATOR_DEPTH);
                if modules.is_empty() {
                    return Err("That DLC emulator folder has no libSceAppContent modules".into());
                }
                for m in modules {
                    let name = m.file_name().ok_or("an emulator module has no name")?;
                    let bytes =
                        fs::read(&m).map_err(|e| format!("Couldn't read {}: {e}", m.display()))?;
                    write_fresh(&fakelib.join(name), &bytes)?;
                }
            }
            // Otherwise the one Kite Pack carries, so DLC packing needs no setup.
            None => {
                for module in dlc_emu::modules() {
                    write_fresh(&fakelib.join(module.name), module.bytes)?;
                }
            }
        }
        out.emulator_added = true;
    }

    if !ini.is_empty() {
        write_fresh(&ini_path, ini.as_bytes())?;
    }

    if let Some(backport) = &bundle.backport {
        let (files, bytes) = copy_tree(backport, staging.path(), true)?;
        out.backport_files = files;
        out.backport_bytes = bytes;
    }

    Ok((staging, out))
}

/// A staging folder beside the game, so hard links work; the system temp folder if the game's
/// own folder can't hold one (a read-only drive), where files are copied instead.
fn new_staging(source: &Path) -> Result<Staging, String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let name = format!(".kite-bundle-{}-{nanos}", std::process::id());
    let beside = source.parent().map(|p| p.join(&name));
    let mut candidates: Vec<PathBuf> = beside.into_iter().collect();
    candidates.push(std::env::temp_dir().join(&name));
    for path in candidates {
        if fs::create_dir_all(&path).is_ok() {
            return Ok(Staging { path });
        }
    }
    Err("Couldn't make room to prepare the packed copy".into())
}

/// Copies the game into the staging folder: a copy-on-write clone when the filesystem has one
/// (`cp -cR` on APFS, `cp -a --reflink=auto` on btrfs/XFS), a plain copy otherwise.
fn stage_tree(from: &Path, to: &Path) -> Result<(), String> {
    if clone_with_cp(from, to) {
        return Ok(());
    }
    copy_tree(from, to, true).map(|_| ())
}

/// The platform's clone copy, into an existing folder. `false` when it isn't available or the
/// filesystem refused (`cp -c` on a drive that is not APFS falls back to a real copy on its
/// own, which is what a plain `cp` would do anyway).
fn clone_with_cp(from: &Path, to: &Path) -> bool {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut cmd = std::process::Command::new("cp");
        #[cfg(target_os = "macos")]
        cmd.arg("-cR");
        #[cfg(target_os = "linux")]
        cmd.args(["-a", "--reflink=auto"]);
        cmd.arg(format!("{}/.", from.display()));
        cmd.arg(format!("{}/", to.display()));
        return cmd.status().map(|s| s.success()).unwrap_or(false);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (from, to);
        false
    }
}

/* ---- writing the pieces in ------------------------------------------------------------------- */

/// The addcont index to use: the first `addcontN` that neither exists nor is mounted yet.
fn next_addcont_index(staging: &Path, ini: &str) -> Result<usize, String> {
    for n in 0..64usize {
        if staging.join(format!("addcont{n}")).exists() || ini.contains(&format!("/app0/addcont{n}")) {
            continue;
        }
        return Ok(n);
    }
    Err("This game already has 64 addon folders".into())
}

/// Content ids already listed in a `dlc_emu.ini`, whatever section they are in.
fn ini_content_ids(ini: &str) -> BTreeSet<String> {
    ini.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim().eq_ignore_ascii_case("content_id"))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn ini_entry_count(ini: &str) -> usize {
    ini.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with('[') && t.contains(']')
        })
        .count()
}

/// Copies one DLC into `dest`: a folder as it is, a fake `.pkg` through Forge's Extract, which
/// reads the package and writes its files (nothing is unpacked beside it).
fn copy_item(path: &Path, dest: &Path) -> Result<(), String> {
    if path.is_dir() {
        return copy_tree(path, dest, true).map(|_| ());
    }
    let found = inspect(path)?;
    let names = top_level(&found);
    if names.is_empty() {
        return Err(format!("{} holds no files", path.display()));
    }
    fs::create_dir_all(dest).map_err(|e| format!("Couldn't make {}: {e}", dest.display()))?;
    let request: ConvertRequest = serde_json::from_value(serde_json::json!({
        "source": path,
        "format": "folder",
        "output": dest,
        "extract": names,
        "replace": true,
    }))
    .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    run_forge(request)
}

/// The top-level entries of a source's file list: what an Extract selects (`sce_sys`, `data`,
/// `eboot.bin`, ...). Selecting a folder takes everything under it.
fn top_level(found: &Inspection) -> Vec<String> {
    let mut names = BTreeSet::new();
    for file in &found.files {
        if let Some(first) = file.path.split('/').next() {
            if !first.is_empty() {
                names.insert(first.to_string());
            }
        }
    }
    for dir in &found.empty_dirs {
        if let Some(first) = dir.split('/').next() {
            if !first.is_empty() {
                names.insert(first.to_string());
            }
        }
    }
    names.into_iter().collect()
}

/// Runs one Forge job to its end and reports its failure in words.
fn run_forge(request: ConvertRequest) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let jobs = Jobs::new(move |event| {
        let _ = tx.send(event);
    });
    let id = jobs.start(request);
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Event::Done { job, result }) if job == id => {
                return result.map(|_| ()).map_err(|e| format!("{e:#}"));
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Forge stopped without finishing".into());
            }
        }
    }
}

/// Writes `bytes` to `path` as a new file, never through an existing hard link: a file this
/// process linked from the user's game must not be written to.
fn write_fresh(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("Couldn't replace {}: {e}", path.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}

/// Copies a folder's contents into `dest`, replacing same-named files when `replace`.
fn copy_tree(from: &Path, dest: &Path, replace: bool) -> Result<(u64, u64), String> {
    copy_tree_at(from, dest, replace, 0)
}

fn copy_tree_at(from: &Path, dest: &Path, replace: bool, depth: usize) -> Result<(u64, u64), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{} nests deeper than a game should", from.display()));
    }
    let mut files = 0;
    let mut bytes = 0;
    for entry in fs::read_dir(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("Couldn't read {}: {e}", from.display()))?;
        let path = entry.path();
        let target = dest.join(entry.file_name());
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            fs::create_dir_all(&target)
                .map_err(|e| format!("Couldn't make {}: {e}", target.display()))?;
            let (f, b) = copy_tree_at(&path, &target, replace, depth + 1)?;
            files += f;
            bytes += b;
            continue;
        }
        if target.exists() {
            if !replace {
                continue;
            }
            // Staging files are hard links: copying over one would write through to the game's
            // own file. Unlink first, so the replacement is a new file and the dump is intact.
            fs::remove_file(&target)
                .map_err(|e| format!("Couldn't replace {}: {e}", target.display()))?;
        }
        fs::copy(&path, &target).map_err(|e| format!("Couldn't copy {}: {e}", path.display()))?;
        files += 1;
        bytes += meta.len();
    }
    Ok((files, bytes))
}

/// Every file and byte in a folder, for the screen's "what will this add".
fn walk_totals(dir: &Path) -> Result<(u64, u64), String> {
    let mut files = 0;
    let mut bytes = 0;
    for entry in fs::read_dir(dir).map_err(|e| format!("Couldn't read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("Couldn't read {}: {e}", dir.display()))?;
        let meta = entry
            .metadata()
            .map_err(|e| format!("Couldn't read {}: {e}", entry.path().display()))?;
        if meta.is_dir() {
            let (f, b) = walk_totals(&entry.path())?;
            files += f;
            bytes += b;
        } else {
            files += 1;
            bytes += meta.len();
        }
    }
    Ok((files, bytes))
}

/// Forge's inspection of a folder or a `.pkg`, with its error as text the screen can show.
fn inspect(path: &Path) -> Result<Inspection, String> {
    ps5_dump_forge_core::inspect(path).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write(path: impl AsRef<Path>, bytes: &[u8]) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn param(id: &str, title: &str, category: Option<&str>) -> String {
        let cat = category.map(|c| format!(r#","applicationCategoryType":"{c}""#)).unwrap_or_default();
        format!(
            r#"{{"localizedParameters":{{"defaultLanguage":"en-US","en-US":{{"titleName":"{title}"}}}},
                "contentId":"{id}","titleId":"{id}", "contentVersion":"01.000.000"{cat}}}"#
        )
    }

    /// A game folder with a DLC emulator already in it.
    fn game(dir: &Path) -> PathBuf {
        let root = dir.join("[Ashen Tide]-[PPSA01234]");
        write(root.join("sce_sys/param.json"), param("EP0000-PPSA01234_00-ASHENTIDE0000000", "Ashen Tide", None).as_bytes());
        write(root.join("eboot.bin"), b"elf");
        write(root.join("data/big.bin"), &[0u8; 4096]);
        write(root.join("fakelib/libSceAppContent.sprx"), b"emu");
        root
    }

    fn dlc_folder(dir: &Path, label: &str) -> PathBuf {
        let root = dir.join(format!("Tidebreaker-{label}"));
        write(
            root.join("sce_sys/param.json"),
            param(&format!("EP0000-PPSA01234_00-{label}"), "Tidebreaker", Some("ac")).as_bytes(),
        );
        write(root.join("data/dlc.bin"), &[7u8; 1024]);
        root
    }

    #[test]
    fn a_dlc_folder_is_named_validated_and_measured() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");
        let p = plan(&g, &Bundle { dlc: vec![d], ..Default::default() });

        assert_eq!(p.route, "inPack", "{:?}", p.problem);
        assert_eq!(p.items.len(), 1);
        let item = &p.items[0];
        assert_eq!(item.kind, "dlc");
        assert_eq!(item.label.as_deref(), Some("TIDEBREAKER00000"));
        assert_eq!(item.content_id.as_deref(), Some("EP0000-PPSA01234_00-TIDEBREAKER00000"));
        assert!(item.bytes > 0);
        assert_eq!(item.name, "Tidebreaker");
    }

    #[test]
    fn a_dlc_for_another_game_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        let mut d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");
        write(
            d.join("sce_sys/param.json"),
            param("EP0000-PPSA99999_00-TIDEBREAKER00000", "Elsewhere", Some("ac")).as_bytes(),
        );
        // Inspect reads the title id from contentId's middle when param.json has no titleId.
        let p = plan(&g, &Bundle { dlc: vec![d], ..Default::default() });
        assert_eq!(p.route, "unavailable");
        let problem = p.problem.unwrap();
        assert!(problem.contains("not for PPSA01234") || problem.contains("another game"), "{problem}");
    }

    #[test]
    fn without_an_emulator_the_pack_carries_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        fs::remove_dir_all(g.join("fakelib")).unwrap();
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");

        let p = plan(&g, &Bundle { dlc: vec![d], ..Default::default() });
        assert_eq!(p.route, "inPack", "{:?}", p.problem);
        assert_eq!(p.emulator_from, "builtin");
    }

    #[test]
    fn a_dump_that_has_its_own_emulator_keeps_it() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");
        let p = plan(&g, &Bundle { dlc: vec![d], ..Default::default() });
        assert_eq!(p.emulator_from, "dump");
    }

    #[test]
    fn a_picked_emulator_folder_wins_and_a_bad_one_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        fs::remove_dir_all(g.join("fakelib")).unwrap();
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");
        let tools = tmp.path().join("tools");
        write(tools.join("dlc_emu/fakelib/libSceAppContent.sprx"), b"emu");

        let p = plan(&g, &Bundle { dlc: vec![d.clone()], emulator: Some(tools), ..Default::default() });
        assert_eq!(p.route, "inPack", "{:?}", p.problem);
        assert_eq!(p.emulator_from, "picked");

        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let p = plan(&g, &Bundle { dlc: vec![d], emulator: Some(empty), ..Default::default() });
        assert_eq!(p.route, "unavailable");
        assert!(p.problem.unwrap().contains("no libSceAppContent"));
    }

    #[test]
    fn staging_adds_the_dlc_an_ini_entry_and_the_emulator_without_touching_the_game() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        fs::remove_dir_all(g.join("fakelib")).unwrap();
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");

        let (staging, out) = stage(&g, &Bundle { dlc: vec![d], ..Default::default() }).unwrap();
        let root = staging.path();

        assert_eq!(out.dlc, 1);
        assert!(out.emulator_added);
        // The modules Kite Pack carries, not a folder the user had to find.
        let mut wanted = crate::dlc_emu::modules();
        for module in wanted.iter_mut() {
            assert!(
                root.join("fakelib").join(module.name).is_file(),
                "{} missing",
                module.name
            );
        }
        assert_eq!(fs::read(root.join("addcont0/data/dlc.bin")).unwrap(), vec![7u8; 1024]);
        let ini = fs::read_to_string(root.join("dlc_emu.ini")).unwrap();
        assert!(ini.contains("[PSAC]"), "{ini}");
        assert!(ini.contains("content_id=EP0000-PPSA01234_00-TIDEBREAKER00000"));
        assert!(ini.contains("mount_point=/app0/addcont0"));
        // The staged file is a copy: the same bytes, and the game's own is untouched.
        assert_eq!(fs::read(root.join("data/big.bin")).unwrap(), vec![0u8; 4096]);
        // And the game itself is unchanged: no ini, no emulator, no addcont.
        assert!(!g.join("dlc_emu.ini").exists());
        assert!(!g.join("fakelib").exists());
        assert!(!g.join("addcont0").exists());
    }

    #[test]
    fn the_same_dlc_twice_is_added_once() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");
        let (staging, out) =
            stage(&g, &Bundle { dlc: vec![d.clone(), d], ..Default::default() }).unwrap();
        assert_eq!(out.dlc, 1);
        assert_eq!(out.dlc_already, 1);
        let ini = fs::read_to_string(staging.path().join("dlc_emu.ini")).unwrap();
        assert_eq!(ini.matches("[PSAC]").count(), 1);
    }

    #[test]
    fn a_second_dlc_takes_the_next_addon_number() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        write(g.join("addcont0/data/old.bin"), b"old");
        write(g.join("dlc_emu.ini"), b"[PSAC]\ncontent_id=EP0000-PPSA01234_00-OLDDLC000000001\nmount_point=/app0/addcont0\n");
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");

        let (staging, out) = stage(&g, &Bundle { dlc: vec![d], ..Default::default() }).unwrap();
        assert_eq!(out.dlc, 1);
        let ini = fs::read_to_string(staging.path().join("dlc_emu.ini")).unwrap();
        assert!(ini.contains("mount_point=/app0/addcont1"), "{ini}");
        assert!(ini.contains("OLDDLC000000001"), "the old entry is kept: {ini}");
        assert!(staging.path().join("addcont1/data/dlc.bin").exists());
    }

    #[test]
    fn a_backport_is_overlaid_before_packing() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        let bp = tmp.path().join("backport");
        write(bp.join("eboot.bin"), b"patched");
        write(bp.join("sce_sys/param.json"), param("EP0000-PPSA01234_00-ASHENTIDE0000000", "Ashen Tide", None).as_bytes());
        write(bp.join("fakelib/libSceVideoOut.sprx"), b"fake");

        let (staging, out) = stage(&g, &Bundle { backport: Some(bp), ..Default::default() }).unwrap();
        let root = staging.path();
        assert_eq!(out.backport_files, 3);
        assert_eq!(fs::read(root.join("eboot.bin")).unwrap(), b"patched");
        assert!(root.join("fakelib/libSceVideoOut.sprx").exists());
        // The original dump still has its own eboot.
        assert_eq!(fs::read(g.join("eboot.bin")).unwrap(), b"elf");
    }

    #[test]
    fn a_folder_that_is_not_a_backport_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let g = game(tmp.path());
        let empty = tmp.path().join("notabackport");
        write(empty.join("readme.txt"), b"hello");
        let p = plan(&g, &Bundle { backport: Some(empty), ..Default::default() });
        assert_eq!(p.route, "unavailable");
        assert!(p.problem.unwrap().contains("doesn't look like a backport"));
    }

    #[test]
    fn an_image_source_is_refused_in_words() {
        let tmp = tempfile::tempdir().unwrap();
        let img = tmp.path().join("game.exfat");
        std::fs::write(&img, b"x").unwrap();
        let d = dlc_folder(&tmp.path().join("in"), "TIDEBREAKER00000");
        let err = stage(&img, &Bundle { dlc: vec![d], ..Default::default() }).err().unwrap();
        assert!(err.contains("game folder"), "{err}");
    }
}
