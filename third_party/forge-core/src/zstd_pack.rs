//! zstd packs in a conversion (`docs/zstd.md`): the tree the job writes (the zstd
//! runtime installed, a fresh `ampr_emu.index` listing every file, loose and packed), which
//! files go into the packs, and the packing (`ps5-dump-forge-zstd` does the bytes; this is the
//! job's glue). Into a folder, or straight into an image through a measure pass
//! ([`measure`], then `ps5_dump_forge_zstd::tree::PackedTree`); no keep-loose list.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ps5_dump_forge_lz4::runtime::{self, RuntimeKind};
use ps5_dump_forge_lz4::{INDEX, JOURNAL, LOGS, RUNTIME};
use ps5_dump_forge_zstd::writer::PackOutput;
use ps5_dump_forge_zstd::{CHUNK_SIZE, INDEX_HEADER_SIZE, VOLUME_HEADER_SIZE, VOLUME_LIMIT};
use ps5upload_fpkg::source::{SourceFile, SourceTree};

use crate::jobs::Ctx;
use crate::lz4::Prepared;
use crate::preflight::listing;
use crate::{ConvertRequest, Format};

/// The zstd index at the game root.
pub(crate) const PACK_INDEX: &str = ps5_dump_forge_zstd::reader::INDEX;

/// The zstd index's first bytes.
pub(crate) const INDEX_MAGIC: &[u8; 8] = b"AMPRZPK1";

/// Whether `tree` is zstd packed: a root `ampr_zstd.index` that starts with [`INDEX_MAGIC`]
/// (Inspect's and the Library's label). An index that can't be read is not.
pub(crate) fn packed(tree: &mut dyn SourceTree) -> bool {
    let listed = (tree.files().iter()).any(|f| f.path == PACK_INDEX && f.size >= 8);
    listed
        && tree
            .read_range(PACK_INDEX, 0, 8)
            .is_ok_and(|b| b == INDEX_MAGIC)
}

/// Whether `path` is a name the zstd writer puts at the root (`ampr_zstd.index`, `ampr_zstd-*.pak`),
/// as a case-insensitive destination sees it.
pub(crate) fn is_pack_name(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    !p.contains('/') && (p == PACK_INDEX || (p.starts_with("ampr_zstd-") && p.ends_with(".pak")))
}

/// Whether `path` stays loose whatever the rules say: the `always_loose` list (eboot,
/// executables, `sce_sys/`, `sce_module/`, `fakelib*/`, every `.ini`, the ampr_emu artifacts) plus
/// `ampr_zstd.index`, `ampr_zstd-*.pak` and `ampr_emu.index`.
pub(crate) fn loose(path: &str) -> bool {
    ps5_dump_forge_lz4::always_loose(path) || is_pack_name(path) || path == INDEX
}

/// Kite Pack: chunks read and packed to guess a pack's size ([`Plan::sampled_bytes`]): 256 MiB.
const SAMPLES: u64 = 1024;

/// Kite Pack: below this many packed bytes the space check keeps the worst case.
const SAMPLE_FROM: u64 = 4 << 30;

/// Kite Pack: the least the sampled share is raised by (3 points), whatever the sample's spread.
const MARGIN: f64 = 0.03;

/// The files to pack and how.
pub(crate) struct Plan {
    /// The packed files, as the prepared tree lists them.
    pub files: Vec<SourceFile>,
    /// zstd level, 1..=22.
    pub level: u8,
    /// Compression threads: the request's `compression_threads`, else every core (the pak
    /// writer uses at most 8).
    pub threads: usize,
}

impl Plan {
    /// The packed files' paths, for a loose-or-packed test per file.
    pub(crate) fn packed_paths(&self) -> HashSet<&str> {
        self.files.iter().map(|f| f.path.as_str()).collect()
    }

    /// The packed files' logical bytes.
    pub(crate) fn packed_bytes(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |sum, f| sum.saturating_add(f.size))
    }

    /// Volumes the packer may need: each holds at least `VOLUME_LIMIT` less its header and
    /// one chunk that did not fit.
    fn max_volumes(&self) -> u64 {
        let per = VOLUME_LIMIT - VOLUME_HEADER_SIZE as u64 - CHUNK_SIZE;
        self.packed_bytes().div_ceil(per).max(1)
    }

    /// `ampr_zstd.index`'s size: header, 32-byte file records, 8-byte chunk and volume records,
    /// the paths.
    fn index_bytes(&self) -> u64 {
        let chunks = self.files.iter().fold(0u64, |sum, f| {
            sum.saturating_add(f.size.div_ceil(CHUNK_SIZE))
        });
        let paths = self
            .files
            .iter()
            .fold(0u64, |sum, f| sum.saturating_add(f.path.len() as u64));
        (INDEX_HEADER_SIZE as u64)
            .saturating_add(32u64.saturating_mul(self.files.len() as u64))
            .saturating_add(8u64.saturating_mul(chunks))
            .saturating_add(8u64.saturating_mul(self.max_volumes()))
            .saturating_add(paths)
    }

    /// Every byte the packer may write: every chunk stored raw (a zstd chunk is strictly
    /// smaller), the volume headers and the index.
    pub(crate) fn worst_case_bytes(&self) -> u64 {
        self.packed_bytes()
            .saturating_add(self.max_volumes() * VOLUME_HEADER_SIZE as u64)
            .saturating_add(self.index_bytes())
    }

    /// Kite Pack: the bytes the packer should write, going by what a sample of the packed files'
    /// chunks compresses to instead of every chunk stored raw: [`SAMPLES`] chunks spread evenly
    /// through the files, packed as the packer would, their share of the raw bytes raised by three
    /// standard errors (at least [`MARGIN`]), plus the volume headers and the index. None when
    /// there is too little to be worth it (the worst case is close enough).
    pub(crate) fn sampled_bytes(&self, tree: &mut dyn SourceTree, ctx: &Ctx) -> anyhow::Result<Option<u64>> {
        let raw_total = self.packed_bytes();
        let counts: Vec<u64> = self.files.iter().map(|f| f.size.div_ceil(CHUNK_SIZE)).collect();
        let chunks: u64 = counts.iter().sum();
        if raw_total < SAMPLE_FROM || chunks == 0 {
            return Ok(None);
        }
        let n = SAMPLES.min(chunks);
        // The middle of each of n equal runs of chunks, in the files' order.
        let mut sample = Vec::with_capacity(n as usize);
        let (mut file, mut before) = (0usize, 0u64);
        for s in 0..n {
            let k = ((2 * s + 1) as u128 * chunks as u128 / (2 * n) as u128) as u64;
            while before + counts[file] <= k {
                before += counts[file];
                file += 1;
            }
            ctx.check()?;
            let f = &self.files[file];
            let at = (k - before) * CHUNK_SIZE;
            let len = CHUNK_SIZE.min(f.size - at) as usize;
            sample.push(tree.read_range(&f.path, at, len)?);
        }
        let raw: Vec<u64> = sample.iter().map(|c| c.len() as u64).collect();
        let packed = ps5_dump_forge_zstd::writer::packed_sizes(sample, self.level, self.threads)?;
        // A ratio estimate: packed bytes over raw bytes, and its standard error.
        let (sum_raw, sum_packed) = (raw.iter().sum::<u64>() as f64, packed.iter().sum::<u64>() as f64);
        let ratio = sum_packed / sum_raw;
        let spread = raw
            .iter()
            .zip(&packed)
            .map(|(&r, &p)| (p as f64 - ratio * r as f64).powi(2))
            .sum::<f64>()
            / (n.max(2) - 1) as f64;
        let error = spread.sqrt() / (n as f64).sqrt() / (sum_raw / n as f64);
        let share = (ratio + (3.0 * error).max(MARGIN)).min(1.0);
        let guess = ((raw_total as f64) * share).ceil() as u64;
        ctx.log(format!(
            "space: {n} sampled chunks pack to {:.1}% of their size; counting {:.1}% of {raw_total} bytes",
            ratio * 100.0,
            share * 100.0
        ));
        Ok(Some(
            guess
                .min(raw_total)
                .saturating_add(self.max_volumes() * VOLUME_HEADER_SIZE as u64)
                .saturating_add(self.index_bytes()),
        ))
    }

    /// Kite Pack: the bytes the pack folder holds at most while parts are let go: `most_held`
    /// full volumes (or all of them, if fewer), every volume's header and the index.
    pub(crate) fn held_bytes(&self, most_held: u32) -> u64 {
        self.packed_bytes()
            .min(u64::from(most_held).saturating_mul(VOLUME_LIMIT))
            .saturating_add(self.max_volumes() * VOLUME_HEADER_SIZE as u64)
            .saturating_add(self.index_bytes())
    }

    /// The biggest file the packer writes, for the destination's file size limit: a volume
    /// (4 GiB less 64 KiB at most) or the index.
    pub(crate) fn largest_file(&self) -> u64 {
        let volume = self
            .packed_bytes()
            .saturating_add(VOLUME_HEADER_SIZE as u64)
            .min(VOLUME_LIMIT);
        volume.max(self.index_bytes())
    }
}

/// What the request's own options make of zstd packing.
fn request_findings(req: &ConvertRequest) -> Vec<String> {
    let mut findings = Vec::new();
    if !matches!(
        req.format,
        Format::Folder | Format::Exfat | Format::Ffpkg | Format::Ffpfs | Format::Ffpfsc
    ) {
        findings.push(format!(
            "zstd packs go into a folder or an exfat, ffpkg, ffpfs or ffpfsc image, not {}",
            crate::lz4::target_name(req.format)
        ));
    }
    if req.lz4.is_some() {
        findings.push("zstd packing takes no LZ4 option (lz4)".into());
    }
    if req.zstd_unpack {
        findings.push("zstd pack and zstd unpack can't go together".into());
    }
    if !(1..=22).contains(&req.zstd_level) {
        findings.push(format!(
            "the zstd level is 1 through 22, not {}",
            req.zstd_level
        ));
    }
    findings
}

/// The tree a zstd pack job writes and its [`Plan`] (`tree` from [`crate::lz4::open`],
/// which decoded any LZ4 or zstd packs; the backport already left out). The zstd runtime
/// replaces whatever is at `fakelib/libSceAmpr.sprx`; the journal, logs and old index are left
/// out and a fresh `ampr_emu.index` (`mtime`: the job's timestamp) lists every file written,
/// loose and packed. `None` with findings.
pub(crate) fn prepare(
    req: &ConvertRequest,
    mut tree: Box<dyn SourceTree>,
    mtime: i64,
    ctx: &Ctx,
) -> anyhow::Result<(Prepared, Option<Plan>)> {
    let mut findings = request_findings(req);
    let eboot = tree.files().iter().any(|f| f.path == "eboot.bin");
    // A title without AMPR is packed too (by user decision): ShadowMount+ loads the injected
    // ampr-emu-ng.
    if eboot && !crate::lz4::imports_ampr(tree.as_mut()) {
        ctx.log("zstd: eboot.bin does not import libSceAmpr; injecting ampr-emu-ng");
    }
    // Every root entry, a folder too (`ampr_zstd.index/x` or an empty one would block the file).
    let mut taken: Vec<String> = (tree.files().iter().map(|f| f.path.as_str()))
        .chain(tree.empty_dirs().iter().map(String::as_str))
        .map(|p| p.split('/').next().unwrap_or(p))
        .filter(|p| is_pack_name(p))
        .map(str::to_owned)
        .collect();
    taken.sort_unstable();
    taken.dedup();
    findings.extend(listing(
        "named like a zstd pack file the target writes",
        &taken,
    ));
    let refused = |tree, findings| Prepared {
        tree,
        findings,
        pack: None,
    };
    if !findings.is_empty() {
        return Ok((refused(tree, findings), None));
    }
    ctx.log(match crate::lz4::runtime_of(tree.as_mut()).kind {
        RuntimeKind::None => format!("zstd: installing the zstd runtime at {RUNTIME}"),
        RuntimeKind::Zstd => format!("zstd: the zstd runtime at {RUNTIME} is written again"),
        _ => format!("zstd: replacing the runtime at {RUNTIME} with the zstd runtime"),
    });
    let mut files: Vec<SourceFile> = Vec::with_capacity(tree.files().len() + 2);
    for f in tree.files() {
        if f.path == JOURNAL || LOGS.contains(&f.path.as_str()) {
            ctx.log(format!("zstd: leaving out {}", f.path));
        } else if f.path != INDEX && f.path != RUNTIME {
            files.push(f.clone());
        }
    }
    files.push(SourceFile {
        path: RUNTIME.into(),
        size: runtime::zstd().len() as u64,
    });
    let rows: Vec<(String, u64)> = files.iter().map(|f| (f.path.clone(), f.size)).collect();
    let index = match ps5upload_fpkg::ampr_index::build(&rows, mtime) {
        Ok(index) => index,
        Err(e) => return Ok((refused(tree, vec![e]), None)),
    };
    ctx.log(format!(
        "zstd: writing a new {INDEX} for {} files",
        rows.len()
    ));
    files.push(SourceFile {
        path: INDEX.into(),
        size: index.len() as u64,
    });
    // Pack everything except system files (`rules::select_everything`): all but `always_loose`,
    // root files, media and configs included; ampr-emu-ng serves plain and libc I/O on packed
    // files. Every source, an LZ4-packed one too (by user decision), takes it. A pack profile,
    // given, decides instead (a `store` file is packed as any other: a chunk zstd can't shrink
    // is kept raw anyway).
    let mut findings = Vec::new();
    let profile = crate::lz4::pack_profile(req, ctx, &mut findings);
    if !findings.is_empty() {
        return Ok((refused(tree, findings), None));
    }
    let packed: Vec<SourceFile> = (files.iter())
        .filter(|f| {
            !loose(&f.path)
                && ps5_dump_forge_lz4::select_everything(&f.path, f.size, profile.as_ref())
                    .is_some()
        })
        .cloned()
        .collect();
    let (n, loose_n) = (packed.len(), files.len() - packed.len());
    match (&req.pack_profile, &profile) {
        (Some(path), Some(p)) => ctx.log(format!(
            "pack rules: {}: {} rules; {n} files packed, {loose_n} loose; zstd level {}, {} KiB chunks",
            path.display(),
            p.rules().len(),
            req.zstd_level,
            CHUNK_SIZE >> 10
        )),
        _ => ctx.log(format!(
            "packing strategy: {}; {n} files to pack, {loose_n} loose; zstd level {}, {} KiB chunks",
            crate::lz4::EVERYTHING,
            req.zstd_level,
            CHUNK_SIZE >> 10
        )),
    }
    let set: HashSet<&str> = packed.iter().map(|f| f.path.as_str()).collect();
    let loose: Vec<(&str, u64, bool)> = (files.iter())
        .filter(|f| !set.contains(f.path.as_str()))
        .map(|f| (f.path.as_str(), f.size, false))
        .collect();
    ctx.log(crate::lz4::everything_loose_line(&loose, profile.as_ref()));
    let mut generated: HashMap<String, Cow<'static, [u8]>> = HashMap::new();
    generated.insert(RUNTIME.into(), Cow::Borrowed(runtime::zstd()));
    generated.insert(INDEX.into(), Cow::Owned(index));
    let plan = Plan {
        files: packed,
        level: req.zstd_level,
        threads: crate::convert::compression_threads(req),
    };
    let tree = crate::lz4::overlay(tree, files, generated);
    Ok((refused(tree, Vec::new()), Some(plan)))
}

/// Packs `plan`'s files from `tree` (the job's hashing tree) into `out`: the volumes, then
/// `ampr_zstd.index`. Compression runs chunk-parallel in bounded batches on a pool of
/// `plan.threads` (the pak crate).
pub(crate) fn pack<O: PackOutput>(
    tree: &mut dyn SourceTree,
    plan: &Plan,
    out: &mut O,
    ctx: &Ctx,
) -> anyhow::Result<ps5_dump_forge_zstd::Index> {
    let total = plan.packed_bytes().saturating_mul(2);
    ctx.progress("pack", 0, total);
    let started = std::time::Instant::now();
    let index = ps5_dump_forge_zstd::writer::pack_with(
        tree,
        &plan.files,
        plan.level,
        plan.threads,
        out,
        &mut |n| ctx.progress("pack", n.min(total), total),
        ctx.cancel,
    );
    ctx.check()?;
    let index = index?;
    ctx.progress("pack", total, total);
    let raw = plan.packed_bytes();
    let stored: u64 = (index.volume_sizes.iter())
        .map(|s| s - VOLUME_HEADER_SIZE as u64)
        .sum();
    let zstd = (index.chunks.iter())
        .filter(|c| c.codec == ps5_dump_forge_zstd::Codec::Zstd)
        .count();
    let secs = started.elapsed().as_secs_f64().max(1e-3);
    ctx.log(format!(
        "wrote zstd packs: {} files packed into {} volumes; {} chunks, {zstd} \
         zstd; stored {}% of {raw} bytes; zstd level {}; {:.0} MB/s on {} threads",
        index.files.len(),
        index.volume_sizes.len(),
        index.chunks.len(),
        crate::convert::percent(stored, raw),
        plan.level,
        raw as f64 / 1e6 / secs,
        plan.threads
            .clamp(1, ps5_dump_forge_zstd::writer::MAX_THREADS)
    ));
    Ok(index)
}

/// The measure pass of zstd packs into an image: [`pack`]'s compression over `plan`'s files,
/// keeping only the index, so the image can be laid out from exact sizes. The write pass
/// reads the packed files again.
pub(crate) fn measure(
    tree: &mut dyn SourceTree,
    plan: &Plan,
    req: &ConvertRequest,
    ctx: &Ctx,
) -> anyhow::Result<ps5_dump_forge_zstd::Index> {
    ctx.log(
        "zstd into an image: packing straight into it, no temporary folder; the packed files \
         are read again to write them",
    );
    // Each file counts twice (one pass, counted as two: the writer's progress).
    let total = plan.packed_bytes().saturating_mul(2);
    ctx.expect_rest(total.saturating_add(crate::convert::after_measure(tree, req)));
    ctx.progress("measure", 0, total);
    let started = std::time::Instant::now();
    let index = ps5_dump_forge_zstd::writer::measure(
        tree,
        &plan.files,
        plan.level,
        plan.threads,
        &mut |n| ctx.progress("measure", n.min(total), total),
        ctx.cancel,
    );
    ctx.check()?;
    let index = index?;
    ctx.progress("measure", total, total);
    let raw = plan.packed_bytes();
    let stored: u64 = (index.volume_sizes.iter())
        .map(|s| s - VOLUME_HEADER_SIZE as u64)
        .sum();
    ctx.log(format!(
        "measured zstd packs: {} files packed into {} volumes; {} chunks; stored {}% of {raw} \
         bytes; zstd level {}; {:.0} MB/s",
        index.files.len(),
        index.volume_sizes.len(),
        index.chunks.len(),
        crate::convert::percent(stored, raw),
        plan.level,
        raw as f64 / 1e6 / started.elapsed().as_secs_f64().max(1e-3),
    ));
    Ok(index)
}

/// Every check the runtime makes on the packs `tree` holds (the index and each volume header),
/// then the logical files read back through the packs against `expected`.
///
/// Kite Pack: with a `hook` that let volumes go, those are cut down to their header; their header
/// must still agree with the index, and the files with a chunk in one are left out of the
/// comparison (each chunk was checked as its volume closed, before it could go).
pub(crate) fn checks(
    expected: &crate::verify::Expected,
    tree: Box<dyn SourceTree>,
    ctx: &Ctx,
    mode: crate::verify::Mode,
    hook: Option<&dyn ps5_dump_forge_zstd::hook::VolumeHook>,
) -> anyhow::Result<Vec<String>> {
    let cut = hook.map(|h| h.cut()).unwrap_or_default();
    let mut tree = ps5_dump_forge_zstd::reader::unpack_cut(tree, &cut)
        .map_err(|e| anyhow::Error::new(e).context("reading the zstd packs back"))?;
    let mut checks = vec![format!(
        "zstd packs: {PACK_INDEX} ({} packed files) and {} volume headers pass the reader's checks",
        tree.index().files.len(),
        tree.index().volume_sizes.len()
    )];
    if cut.is_empty() {
        checks.extend(crate::verify::compare(expected, &mut tree, ctx, mode)?);
        return Ok(checks);
    }
    let index = tree.index();
    let gone: HashSet<String> = (index.files.iter())
        .filter(|f| {
            let first = f.first_chunk as usize;
            let n = f.logical_size.div_ceil(CHUNK_SIZE) as usize;
            (index.chunks[first..first + n].iter()).any(|c| cut.contains(&u32::from(c.volume_id)))
        })
        .map(|f| f.path.clone())
        .collect();
    let (files, digests) = (expected.files.iter().zip(&expected.digests))
        .filter(|(f, _)| !gone.contains(&f.path))
        .map(|(f, d)| (f.clone(), d.clone()))
        .unzip();
    let kept = crate::verify::Expected { files, digests, empty_dirs: expected.empty_dirs.clone() };
    let inner: &mut dyn SourceTree = &mut tree;
    let mut left = crate::convert::Only::new(inner, |p| !gone.contains(p));
    checks.extend(crate::verify::compare(&kept, &mut left, ctx, mode)?);
    checks.push(format!(
        "{} packed files were checked as their part was written, before it left this computer",
        gone.len()
    ));
    Ok(checks)
}

/// Kite Pack: the size guess from a sample ([`Plan::sampled_bytes`]).
#[cfg(test)]
mod kite_sample_tests {
    use super::*;
    use crate::Event;
    use std::sync::atomic::AtomicBool;

    const GIB: u64 = 1 << 30;

    /// A game made up as it is read: text that compresses, and noise that doesn't.
    struct Made {
        files: Vec<SourceFile>,
        reads: usize,
    }

    fn text(at: u64, len: usize) -> Vec<u8> {
        (0..len as u64)
            .map(|i| b"the quick brown fox jumps over the lazy dog, level 7 of 12\n"[((at + i) % 59) as usize])
            .collect()
    }

    fn noise(at: u64, len: usize) -> Vec<u8> {
        (0..len as u64)
            .map(|i| {
                let mut x = (at + i).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
                x ^= x >> 29;
                x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                (x >> 32) as u8
            })
            .collect()
    }

    impl SourceTree for Made {
        fn files(&self) -> &[SourceFile] {
            &self.files
        }
        fn read(&mut self, _: &str) -> ps5upload_fpkg::Result<Vec<u8>> {
            panic!("a sample reads ranges, never a whole file")
        }
        fn read_range(&mut self, path: &str, at: u64, len: usize) -> ps5upload_fpkg::Result<Vec<u8>> {
            self.reads += 1;
            Ok(if path.ends_with(".txt") { text(at, len) } else { noise(at, len) })
        }
        fn describe(&self) -> String {
            "made".into()
        }
    }

    fn plan(files: &[(&str, u64)], level: u8) -> (Plan, Made) {
        let files: Vec<SourceFile> = files.iter().map(|(p, s)| SourceFile { path: p.to_string(), size: *s }).collect();
        (Plan { files: files.clone(), level, threads: 0 }, Made { files, reads: 0 })
    }

    fn sampled(plan: &Plan, tree: &mut Made) -> Option<u64> {
        let emit: Box<dyn Fn(Event) + Send + Sync> = Box::new(|_| {});
        let cancel = AtomicBool::new(false);
        plan.sampled_bytes(tree, &Ctx::new(1, &emit, &cancel)).unwrap()
    }

    #[test]
    fn guesses_a_bit_over_what_the_pack_takes() {
        // 3 GiB of text and 2 GiB of noise, in files of uneven sizes.
        let (plan, mut tree) = plan(
            &[("a/movie.bin", 2 * GIB), ("b/script.txt", GIB + 12_345), ("c/more.txt", 2 * GIB - 12_345)],
            5,
        );
        let guess = sampled(&plan, &mut tree).expect("5 GiB is worth a sample");
        assert_eq!(tree.reads, SAMPLES as usize);
        // What it really takes: every text chunk packs like any other, the noise stays raw.
        let one = ps5_dump_forge_zstd::writer::packed_sizes(vec![text(0, CHUNK_SIZE as usize)], 5, 1).unwrap()[0];
        let real = 2 * GIB + (3 * GIB).div_ceil(CHUNK_SIZE) * one;
        let extra = plan.max_volumes() * VOLUME_HEADER_SIZE as u64 + plan.index_bytes();
        assert!(guess >= real + extra, "{guess} under {real}");
        // At most the margin over it, and well under storing everything raw.
        assert!(guess <= real + extra + (5 * GIB) / 20, "{guess} far over {real}");
        assert!(guess < plan.worst_case_bytes() * 3 / 4);
    }

    #[test]
    fn never_guesses_over_the_worst_case() {
        let (plan, mut tree) = plan(&[("x/noise.bin", 6 * GIB)], 19);
        let guess = sampled(&plan, &mut tree).unwrap();
        assert_eq!(guess, plan.worst_case_bytes());
    }

    #[test]
    fn leaves_a_small_game_to_the_worst_case() {
        let (plan, mut tree) = plan(&[("x/script.txt", GIB)], 5);
        assert_eq!(sampled(&plan, &mut tree), None);
        assert_eq!(tree.reads, 0);
    }
}

/// Kite Pack: letting parts go ([`Plan::held_bytes`], [`checks`] with a hook).
#[cfg(test)]
mod kite_let_go_tests {
    use super::*;
    use crate::Event;
    use crate::verify::Mode;
    use ps5_dump_forge_zstd::hook::VolumeHook;
    use std::sync::atomic::AtomicBool;

    const GIB: u64 = 1 << 30;

    fn plan(size: u64) -> Plan {
        Plan { files: vec![SourceFile { path: "data/world.bin".into(), size }], level: 5, threads: 1 }
    }

    #[test]
    fn holds_at_most_four_parts() {
        let big = plan(20 * GIB);
        let rest = big.max_volumes() * VOLUME_HEADER_SIZE as u64 + big.index_bytes();
        assert_eq!(big.held_bytes(4), 4 * VOLUME_LIMIT + rest);
        let small = plan(GIB);
        assert_eq!(small.held_bytes(4), small.worst_case_bytes());
    }

    /// Files in memory: a game, and the folder a pack of it makes.
    #[derive(Default, Clone)]
    struct Files(Vec<SourceFile>, HashMap<String, Vec<u8>>);

    impl Files {
        fn add(&mut self, path: &str, bytes: Vec<u8>) {
            self.0.retain(|f| f.path != path);
            self.0.push(SourceFile { path: path.into(), size: bytes.len() as u64 });
            self.1.insert(path.into(), bytes);
        }
    }

    impl SourceTree for Files {
        fn files(&self) -> &[SourceFile] {
            &self.0
        }
        fn read(&mut self, path: &str) -> ps5upload_fpkg::Result<Vec<u8>> {
            Ok(self.1[path].clone())
        }
        fn read_range(&mut self, path: &str, at: u64, len: usize) -> ps5upload_fpkg::Result<Vec<u8>> {
            let b = &self.1[path];
            let at = (at as usize).min(b.len());
            Ok(b[at..(at + len).min(b.len())].to_vec())
        }
        fn describe(&self) -> String {
            "files".into()
        }
    }

    impl ps5_dump_forge_zstd::writer::PackOutput for Files {
        type W = std::io::Cursor<Vec<u8>>;
        fn create(&mut self, _: &str) -> ps5upload_fpkg::Result<Self::W> {
            Ok(std::io::Cursor::new(Vec::new()))
        }
        fn reopen(&mut self, name: &str) -> ps5upload_fpkg::Result<Self::W> {
            Ok(std::io::Cursor::new(self.1.remove(name).unwrap()))
        }
        fn finish(&mut self, name: &str, w: Self::W) -> ps5upload_fpkg::Result<()> {
            self.add(name, w.into_inner());
            Ok(())
        }
    }

    struct Cut(Vec<u32>);

    impl VolumeHook for Cut {
        fn open(&self, _: &str) -> std::io::Result<std::fs::File> {
            unreachable!("the end check reads nothing back through the hook")
        }
        fn before_next(&self, _: u32, _: &AtomicBool) -> std::io::Result<()> {
            Ok(())
        }
        fn cut(&self) -> Vec<u32> {
            self.0.clone()
        }
        fn most_held(&self) -> u32 {
            4
        }
    }

    /// A game with one loose file and two packed ones, its expected listing, and its pack with
    /// volume 0 cut down to its header.
    fn packed_and_cut(ctx: &Ctx) -> (crate::verify::Expected, Files) {
        let mut game = Files::default();
        game.add("eboot.bin", b"\x7fELF loose".repeat(10));
        game.add("data/a.bin", (0..300_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect());
        game.add("data/b.bin", b"text that packs well ".repeat(10_000));
        let packed: Vec<SourceFile> = game.0.iter().filter(|f| f.path.starts_with("data/")).cloned().collect();
        let mut out = Files::default();
        ps5_dump_forge_zstd::writer::pack(&mut game, &packed, 3, &mut out).unwrap();
        out.add("eboot.bin", game.1["eboot.bin"].clone());
        let expected = crate::verify::expected_from(&mut game, &HashMap::new(), ctx, Mode::Full).unwrap();
        let zero = ps5_dump_forge_zstd::volume_name(0);
        let head = out.1[&zero][..VOLUME_HEADER_SIZE].to_vec();
        out.add(&zero, head);
        (expected, out)
    }

    fn with_ctx<T>(f: impl FnOnce(&Ctx) -> T) -> T {
        let emit: Box<dyn Fn(Event) + Send + Sync> = Box::new(|_| {});
        let cancel = AtomicBool::new(false);
        f(&Ctx::new(1, &emit, &cancel))
    }

    #[test]
    fn the_end_check_skips_files_in_cut_volumes() {
        with_ctx(|ctx| {
            let (expected, out) = packed_and_cut(ctx);
            let found = checks(&expected, Box::new(out.clone()), ctx, Mode::Full, Some(&Cut(vec![0]))).unwrap();
            assert!(
                found.iter().any(|c| c == "2 packed files were checked as their part was written, before it left this computer"),
                "{found:?}"
            );
            assert!(checks(&expected, Box::new(out), ctx, Mode::Full, None).is_err());
        });
    }

    #[test]
    fn the_end_check_still_compares_what_stays() {
        with_ctx(|ctx| {
            let (expected, mut out) = packed_and_cut(ctx);
            let mut eboot = out.1["eboot.bin"].clone();
            eboot[3] ^= 1;
            out.add("eboot.bin", eboot);
            assert!(checks(&expected, Box::new(out), ctx, Mode::Full, Some(&Cut(vec![0]))).is_err());
        });
    }
}
