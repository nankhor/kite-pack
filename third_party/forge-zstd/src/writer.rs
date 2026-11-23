//! Bounded chunk-parallel writer for selected files of a `SourceTree`.
//! The caller chooses packed files and writes loose files separately.

use std::cell::RefCell;
use std::io::{Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ps5upload_fpkg::source::{SourceFile, SourceTree};
use ps5upload_fpkg::{Error, Result};
use rayon::prelude::*;

use crate::hook::VolumeHook;
use zstd::zstd_safe::{
    self, CCtx, CParameter, InBuffer, OutBuffer, ResetDirective, zstd_sys::ZSTD_EndDirective,
};

use crate::{
    CHUNK_SHIFT, CHUNK_SIZE, Chunk, Codec, FileRecord, Index, MAX_VOLUMES, VOLUME_HEADER_SIZE,
    VOLUME_LIMIT, VolumeHeader, encode_index, encode_volume_header, path_hash, valid_path,
    volume_name,
};

/// The caller owns creation, durability, and cleanup of output files. `reopen`
/// changes only a volume's first 64 bytes after its final count is known.
pub trait PackOutput {
    type W: Write + Seek;
    fn create(&mut self, name: &str) -> Result<Self::W>;
    fn reopen(&mut self, name: &str) -> Result<Self::W>;
    fn finish(&mut self, name: &str, writer: Self::W) -> Result<()>;
}

fn bad(message: impl Into<String>) -> Error {
    Error::Format(message.into())
}

fn format_error(error: crate::Error) -> Error {
    bad(error.to_string())
}

pub(crate) fn read_chunk<S: SourceTree + ?Sized>(
    source: &mut S,
    path: &str,
    offset: u64,
    size: usize,
) -> Result<Vec<u8>> {
    let data = source.read_range(path, offset, size)?;
    if data.len() != size {
        return Err(bad(format!(
            "{path}: file changed while packing at {offset}"
        )));
    }
    Ok(data)
}

thread_local! {
    /// Each pool thread's zstd context and its level, kept from chunk to chunk (a context
    /// costs far more to set up than a small chunk takes to compress). The pool is the
    /// pack's own, so the contexts end with it.
    static CONTEXT: RefCell<Option<(u8, CCtx<'static>)>> = const { RefCell::new(None) };
    /// Each pool thread's context at [`PROBE_LEVEL`], kept apart so the main one keeps its level.
    static PROBE: RefCell<Option<CCtx<'static>>> = const { RefCell::new(None) };
}

/// Kite Pack: a chunk whose bytes look random is tried at this level before the asked one.
/// Level 1 runs at ~1 GB/s per core; level 19 at ~13 MB/s on data that won't shrink (compressed
/// audio, video, archives), and nearly every such chunk ends up stored raw anyway.
const PROBE_LEVEL: u8 = 1;
/// Kite Pack: the lowest level that is probed. Up to level 5 zstd itself gives up on such data
/// about as fast as the probe would (measured: level 5 lost 7% with it, level 9 gained 15%,
/// level 19 ran 13 times as fast on 68 MB of gzip archives).
const PROBE_FROM: u8 = 6;
/// Kite Pack: the order-0 entropy (bits per byte) from which a chunk looks random. Compressed
/// media measures 7.95 to 8; code, text and most game data well below.
const DENSE_BITS: f64 = 7.9;
/// Kite Pack: a probe that saves less than 1 part in this many stores the chunk raw.
const PROBE_SAVES: usize = 100;

/// Kite Pack: whether `raw`'s byte histogram is as flat as compressed data's. Chunks under
/// 4 KiB are never probed (a file's last few bytes compress in no time at any level).
fn dense(raw: &[u8]) -> bool {
    if raw.len() < 4096 {
        return false;
    }
    // Four histograms, so consecutive bytes don't wait on each other's count.
    let mut counts = [[0u32; 256]; 4];
    let mut quads = raw.chunks_exact(4);
    for q in &mut quads {
        counts[0][q[0] as usize] += 1;
        counts[1][q[1] as usize] += 1;
        counts[2][q[2] as usize] += 1;
        counts[3][q[3] as usize] += 1;
    }
    for &b in quads.remainder() {
        counts[0][b as usize] += 1;
    }
    let n = raw.len() as f64;
    let bits: f64 = (0..256)
        .map(|b| counts.iter().map(|c| c[b]).sum::<u32>())
        .filter(|&c| c > 0)
        .map(|c| {
            let p = f64::from(c) / n;
            -p * p.log2()
        })
        .sum();
    bits >= DENSE_BITS
}

/// Kite Pack: whether the fastest level saves at least 1% of `raw`. It stops as soon as its
/// frame grows past 99% of the chunk, so a chunk that won't shrink costs a fraction of a pass.
fn probe_saves(raw: &[u8]) -> Result<bool> {
    let mut frame = Vec::with_capacity(raw.len() - raw.len() / PROBE_SAVES);
    PROBE.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Some(context(PROBE_LEVEL)?);
        }
        let context = slot.as_mut().expect("set above");
        compress(context, raw, &mut frame)
    })
}

fn zstd_error(code: usize) -> Error {
    bad(format!("zstd: {}", zstd_safe::get_error_name(code)))
}

fn context(level: u8) -> Result<CCtx<'static>> {
    let mut context = CCtx::try_create().ok_or_else(|| bad("zstd: no memory for a context"))?;
    for parameter in [
        CParameter::CompressionLevel(i32::from(level)),
        CParameter::WindowLog(u32::from(CHUNK_SHIFT)),
        CParameter::ChecksumFlag(true),
        CParameter::ContentSizeFlag(true),
    ] {
        context.set_parameter(parameter).map_err(zstd_error)?;
    }
    Ok(context)
}

/// Compresses `raw` as one frame into `frame` (its capacity, `raw.len()`, the most a kept
/// frame may take): false once the frame can't be smaller than `raw`. The chunk goes in
/// with "continue" and the frame is closed with "end", as zstd's streaming encoder does,
/// so a whole 256 KiB chunk ends in an empty last block, byte for byte the frames this
/// writer always made (`ZSTD_compress2` would flag its second block last, 3 bytes less).
fn compress(context: &mut CCtx, raw: &[u8], frame: &mut Vec<u8>) -> Result<bool> {
    context
        .reset(ResetDirective::SessionOnly)
        .map_err(zstd_error)?;
    context
        .set_pledged_src_size(Some(raw.len() as u64))
        .map_err(zstd_error)?;
    let mut output = OutBuffer::around(frame);
    let mut input = InBuffer::around(raw);
    let mut end = ZSTD_EndDirective::ZSTD_e_continue;
    loop {
        let left = context
            .compress_stream2(&mut output, &mut input, end)
            .map_err(zstd_error)?;
        if end == ZSTD_EndDirective::ZSTD_e_end && left == 0 {
            return Ok(output.pos() < raw.len());
        }
        if output.pos() == output.capacity() {
            return Ok(false);
        }
        if input.pos() == raw.len() {
            end = ZSTD_EndDirective::ZSTD_e_end;
        }
    }
}

/// One zstd frame per chunk: content size and checksum on, no dictionary, and a
/// window of at most one chunk so the runtime's 256 KiB window limit accepts it.
/// A frame that is not smaller than the chunk is dropped and the chunk stored raw.
///
/// Kite Pack: at [`PROBE_FROM`] and up, a chunk that looks random ([`dense`]) and that the fastest
/// level can't shrink by 1% ([`probe_saves`]) is stored raw at once, without compressing it at
/// `level`. The decision
/// depends on the bytes and the level alone, so a second pass (`tree.rs`) makes the same one.
pub(crate) fn encode_chunk(raw: Vec<u8>, level: u8) -> Result<(Codec, Vec<u8>)> {
    if level >= PROBE_FROM && dense(&raw) && !probe_saves(&raw)? {
        return Ok((Codec::Raw, raw));
    }
    let mut frame = Vec::with_capacity(raw.len());
    let smaller = CONTEXT.with_borrow_mut(|slot| {
        if slot.as_ref().is_none_or(|(at, _)| *at != level) {
            *slot = Some((level, context(level)?));
        }
        let (_, context) = slot.as_mut().expect("set above");
        compress(context, &raw, &mut frame)
    })?;
    if smaller {
        // A kept frame holds only its own bytes, so buffered chunks cost what they count.
        frame.shrink_to_fit();
        Ok((Codec::Zstd, frame))
    } else {
        Ok((Codec::Raw, raw))
    }
}

/// Kite Pack: what each of `chunks` comes to in a pack at `level`, as [`encode_chunk`] stores it
/// (zstd, or raw when that is no smaller), compressed on `threads` workers (0: every core). For
/// guessing a pack's size from a sample of its chunks before packing it.
pub fn packed_sizes(chunks: Vec<Vec<u8>>, level: u8, threads: usize) -> Result<Vec<u64>> {
    pool(threads)?.install(|| {
        chunks
            .into_par_iter()
            .map(|raw| encode_chunk(raw, level).map(|(_, stored)| stored.len() as u64))
            .collect()
    })
}

/// Most compression threads a pack uses, one per chunk of a batch. At most two batches are
/// held raw (one compressing, the next read meanwhile: 16 MiB) and one encoded (8 MiB), beside
/// the zstd contexts, at most 32 x 5.75 MiB (184 MiB
/// at levels 16..=22 with the 256 KiB window; 72 MiB at level 5).
// ponytail: 32, not every core of a bigger machine, so the contexts stay well inside a job's
// 512 MiB.
pub const MAX_THREADS: usize = 32;

/// The pool a pack compresses on: `threads` workers (0: every core), at most [`MAX_THREADS`].
pub(crate) fn pool(threads: usize) -> Result<rayon::ThreadPool> {
    let threads = match threads {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    };
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads.clamp(1, MAX_THREADS))
        .thread_name(|i| format!("zstd-pack-{i}"))
        .build()
        .map_err(|error| bad(format!("compression threads: {error}")))
}

/// Where a pack run reports: raw bytes done so far (each packed file counts twice, once per
/// pass, a pass it skips included), and the flag that stops it between batches with
/// [`Error::Cancelled`].
struct Progress<'a> {
    done: u64,
    report: &'a mut dyn FnMut(u64),
    cancel: &'a AtomicBool,
}

impl Progress<'_> {
    fn add(&mut self, bytes: u64) {
        self.done += bytes;
        (self.report)(self.done);
    }
}

fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    Ok(())
}

/// The next `width` raw chunks of `file` from `offset` (fewer at its end, none past it).
fn read_batch<S: SourceTree + ?Sized>(
    source: &mut S,
    file: &SourceFile,
    mut offset: u64,
    width: usize,
) -> Result<Vec<Vec<u8>>> {
    let mut raw = Vec::with_capacity(width);
    while raw.len() < width && offset < file.size {
        let size = (file.size - offset).min(CHUNK_SIZE) as usize;
        raw.push(read_chunk(source, &file.path, offset, size)?);
        offset += size as u64;
    }
    Ok(raw)
}

fn raw_bytes(raw: &[Vec<u8>]) -> u64 {
    raw.iter().map(|data| data.len() as u64).sum()
}

/// Compresses `file` from offset 0, one chunk per thread of `pool` at a time, while this
/// thread (the only one that reads `source`) reads the next batch. `consume` sees each batch
/// in index order with its raw byte count.
fn compressed<S: SourceTree + ?Sized>(
    source: &mut S,
    file: &SourceFile,
    level: u8,
    pool: &rayon::ThreadPool,
    cancel: &AtomicBool,
    mut consume: impl FnMut(Vec<(Codec, Vec<u8>, u32, u32)>, u64) -> Result<()>,
) -> Result<()> {
    let width = pool.current_num_threads();
    cancelled(cancel)?;
    let mut raw = read_batch(source, file, 0, width)?;
    let mut offset = raw_bytes(&raw);
    while !raw.is_empty() {
        cancelled(cancel)?;
        let bytes = raw_bytes(&raw);
        let mut encoded = None;
        let slot = &mut encoded;
        let next = pool.in_place_scope(|scope| {
            scope.spawn(move |_| {
                *slot = Some(
                    raw.into_par_iter()
                        .map(|data| {
                            // Kite Pack: the raw bytes' CRC-32 and length, for reading the volume
                            // back once it closes (with a hook).
                            let (crc, len) = (crc32fast::hash(&data), data.len() as u32);
                            encode_chunk(data, level).map(|(codec, stored)| (codec, stored, crc, len))
                        })
                        .collect::<Result<Vec<_>>>(),
                );
            });
            read_batch(source, file, offset, width)
        });
        let encoded = encoded.expect("the scope ran the task")?;
        raw = next?;
        offset += raw_bytes(&raw);
        consume(encoded, bytes)?;
    }
    Ok(())
}

fn validate_files<S: SourceTree + ?Sized>(source: &S, files: &[SourceFile]) -> Result<()> {
    let mut paths = std::collections::HashSet::with_capacity(files.len());
    let source_files: std::collections::HashMap<_, _> = source
        .files()
        .iter()
        .map(|file| (file.path.as_str(), file.size))
        .collect();
    let mut chunks = 0u64;
    for file in files {
        if !valid_path(&file.path)
            || file.path.len() > u16::MAX as usize
            || !paths.insert(file.path.as_str())
            || source_files.get(file.path.as_str()) != Some(&file.size)
        {
            return Err(bad(format!(
                "invalid or changed packed file: {}",
                file.path
            )));
        }
        chunks = chunks
            .checked_add(file.size.div_ceil(CHUNK_SIZE))
            .ok_or_else(|| bad("too many chunks"))?;
    }
    if chunks > u32::MAX as u64 {
        return Err(bad("too many chunks"));
    }
    Ok(())
}

struct Volumes<'a, O: PackOutput> {
    output: &'a mut O,
    open: Option<(String, O::W, u64)>,
    sizes: Vec<u64>,
    chunks: Vec<Chunk>,
    /// Bytes a volume may reach: [`VOLUME_LIMIT`] (less in tests).
    limit: u64,
    /// Kite Pack: with a hook, each volume is read back as it closes and the hook is asked
    /// before the next one opens ([`crate::hook`]).
    hook: Option<Arc<dyn VolumeHook>>,
    cancel: &'a AtomicBool,
    /// The open volume's chunks: the CRC-32 and length of each one's raw bytes (with a hook).
    raw: Vec<(u32, u32)>,
}

impl<'a, O: PackOutput> Volumes<'a, O> {
    fn new(output: &'a mut O, limit: u64, hook: Option<Arc<dyn VolumeHook>>, cancel: &'a AtomicBool) -> Self {
        Volumes { output, open: None, sizes: Vec::new(), chunks: Vec::new(), limit, hook, cancel, raw: Vec::new() }
    }

    /// One encoded chunk, `data` as stored; `crc` and `len` are its raw bytes' CRC-32 and length.
    fn put(&mut self, codec: Codec, data: &[u8], crc: u32, len: u32) -> Result<()> {
        let size = data.len() as u64;
        if size == 0 || size > CHUNK_SIZE {
            return Err(bad("invalid encoded chunk size"));
        }
        if self
            .open
            .as_ref()
            .is_some_and(|(_, _, at)| at + size > self.limit)
        {
            self.close()?;
        }
        if self.open.is_none() {
            if self.sizes.len() >= MAX_VOLUMES {
                return Err(bad("too many zstd pack volumes"));
            }
            if let (Some(hook), next @ 1..) = (&self.hook, self.sizes.len() as u32) {
                hook.before_next(next, self.cancel).map_err(|e| match e.kind() {
                    std::io::ErrorKind::Interrupted => Error::Cancelled,
                    _ => Error::Io(e),
                })?;
            }
            let name = volume_name(self.sizes.len() as u32);
            let mut writer = self.output.create(&name)?;
            writer.write_all(&[0; VOLUME_HEADER_SIZE])?;
            self.open = Some((name, writer, VOLUME_HEADER_SIZE as u64));
        }
        let (_, writer, at) = self.open.as_mut().expect("opened above");
        let chunk = Chunk {
            offset: *at as u32,
            volume_id: self.sizes.len() as u16,
            stored_size: size as u32,
            codec,
        };
        chunk.pack().map_err(format_error)?;
        writer.write_all(data)?;
        *at += size;
        self.chunks.push(chunk);
        if self.hook.is_some() {
            self.raw.push((crc, len));
        }
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if let Some((name, writer, size)) = self.open.take() {
            self.output.finish(&name, writer)?;
            self.sizes.push(size);
            if let Some(hook) = self.hook.clone() {
                let raw = std::mem::take(&mut self.raw);
                let chunks = &self.chunks[self.chunks.len() - raw.len()..];
                check_volume(&*hook, &name, chunks, &raw)?;
            }
        }
        Ok(())
    }

    fn write_headers(&mut self, archive_id: [u8; 16]) -> Result<()> {
        let count = self.sizes.len() as u32;
        for (id, &size) in self.sizes.iter().enumerate() {
            let name = volume_name(id as u32);
            let header = encode_volume_header(VolumeHeader {
                id: id as u32,
                count,
                archive_id,
                payload_bytes: size - VOLUME_HEADER_SIZE as u64,
            })
            .map_err(format_error)?;
            let mut writer = self.output.reopen(&name)?;
            writer.seek(SeekFrom::Start(0))?;
            writer.write_all(&header)?;
            self.output.finish(&name, writer)?;
        }
        Ok(())
    }
}

/// Kite Pack: reads volume `name` back through `hook` and checks each of its `chunks` decodes to
/// raw bytes with the CRC-32 and length in `raw`, a batch at a time, decoded in parallel.
fn check_volume(hook: &dyn VolumeHook, name: &str, chunks: &[Chunk], raw: &[(u32, u32)]) -> Result<()> {
    use std::io::Read;
    let wrong = |why: String| bad(format!("{name} doesn't read back as written: {why}"));
    let mut file = std::io::BufReader::with_capacity(1 << 20, hook.open(name).map_err(|e| wrong(e.to_string()))?);
    let mut at = 0u64;
    for (batch, sums) in chunks.chunks(32).zip(raw.chunks(32)) {
        let mut stored = Vec::with_capacity(batch.len());
        for chunk in batch {
            let offset = u64::from(chunk.offset);
            if offset < at {
                return Err(wrong(format!("chunk at {offset} overlaps")));
            }
            std::io::copy(&mut (&mut file).take(offset - at), &mut std::io::sink())
                .map_err(|e| wrong(e.to_string()))?;
            let mut bytes = vec![0; chunk.stored_size as usize];
            file.read_exact(&mut bytes).map_err(|e| wrong(e.to_string()))?;
            at = offset + u64::from(chunk.stored_size);
            stored.push(bytes);
        }
        let bad_one = batch
            .par_iter()
            .zip(stored.par_iter())
            .zip(sums.par_iter())
            .find_any(|((chunk, bytes), (crc, len))| {
                crate::reader::decode_chunk(bytes, chunk.codec, *len as usize)
                    .map_or(true, |plain| crc32fast::hash(&plain) != *crc)
            });
        if let Some(((chunk, _), _)) = bad_one {
            return Err(wrong(format!("the chunk at {} doesn't match", chunk.offset)));
        }
    }
    Ok(())
}

/// Packs one file in one pass: every chunk compressed, a chunk zstd doesn't shrink kept raw.
/// No file is store-only (the old rule stored a file saving under 1% whole and raw; dropped
/// on this branch: the chunk rule already keeps incompressible data raw).
fn pack_file<S: SourceTree + ?Sized, O: PackOutput>(
    source: &mut S,
    file: &SourceFile,
    level: u8,
    pool: &rayon::ThreadPool,
    volumes: &mut Volumes<O>,
    progress: &mut Progress,
) -> Result<FileRecord> {
    let first_chunk = volumes.chunks.len() as u32;
    compressed(
        source,
        file,
        level,
        pool,
        progress.cancel,
        |encoded, bytes| {
            for (codec, data, crc, len) in encoded {
                volumes.put(codec, &data, crc, len)?;
            }
            // Read and compressed once; progress still counts each file twice (core's estimate).
            progress.add(2 * bytes);
            Ok(())
        },
    )?;
    Ok(FileRecord {
        path: file.path.clone(),
        logical_size: file.size,
        first_chunk,
        store_only: false,
    })
}

/// Writes only `files`, which must be entries from `source.files()`. The caller
/// handles loose files and output cleanup on error. Compresses on every core (at most [`MAX_THREADS`]).
pub fn pack<S: SourceTree + ?Sized, O: PackOutput>(
    source: &mut S,
    files: &[SourceFile],
    level: u8,
    output: &mut O,
) -> Result<Index> {
    pack_with(
        source,
        files,
        level,
        0,
        output,
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

/// [`pack`], reporting the raw bytes done so far to `progress` after each batch (each file
/// counts twice, so they end at twice the packed bytes) and stopping with [`Error::Cancelled`]
/// once `cancel` is set. `threads` compress (0: every core), at most [`MAX_THREADS`], one per
/// chunk of a batch. The output does not depend on `threads`.
pub fn pack_with<S: SourceTree + ?Sized, O: PackOutput>(
    source: &mut S,
    files: &[SourceFile],
    level: u8,
    threads: usize,
    output: &mut O,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<Index> {
    // Kite Pack: a pack into a folder goes through the hook, when one is set.
    pack_run(source, files, level, threads, output, progress, cancel, crate::hook::volume_hook(), VOLUME_LIMIT)
}

/// An output that keeps nothing, for [`measure`].
struct Nowhere;

impl Write for Nowhere {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Nowhere {
    fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

impl PackOutput for Nowhere {
    type W = Nowhere;
    fn create(&mut self, _: &str) -> Result<Nowhere> {
        Ok(Nowhere)
    }
    fn reopen(&mut self, _: &str) -> Result<Nowhere> {
        Ok(Nowhere)
    }
    fn finish(&mut self, _: &str, _: Nowhere) -> Result<()> {
        Ok(())
    }
}

/// [`pack_with`] without writing: the same compression, every chunk's codec and stored size
/// recorded, no byte kept. The [`Index`] it returns (a fresh archive ID) is what
/// [`crate::tree::PackedTree`] serves the packs from; memory is the index (16 B per chunk)
/// and the compression's own.
pub fn measure<S: SourceTree + ?Sized>(
    source: &mut S,
    files: &[SourceFile],
    level: u8,
    threads: usize,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<Index> {
    pack_run(
        source,
        files,
        level,
        threads,
        &mut Nowhere,
        progress,
        cancel,
        None,
        VOLUME_LIMIT,
    )
}

#[allow(clippy::too_many_arguments)]
fn pack_run<S: SourceTree + ?Sized, O: PackOutput>(
    source: &mut S,
    files: &[SourceFile],
    level: u8,
    threads: usize,
    output: &mut O,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
    hook: Option<Arc<dyn VolumeHook>>,
    limit: u64,
) -> Result<Index> {
    let mut progress = Progress {
        done: 0,
        report: progress,
        cancel,
    };
    if !(1..=22).contains(&level) {
        return Err(bad("zstd level must be 1..=22"));
    }
    validate_files(source, files)?;
    let pool = pool(threads)?;
    let mut ordered: Vec<_> = files.iter().collect();
    ordered.sort_unstable_by(|a, b| {
        (path_hash(&a.path), a.path.as_bytes()).cmp(&(path_hash(&b.path), b.path.as_bytes()))
    });
    let mut archive_id = [0; 16];
    getrandom::fill(&mut archive_id).map_err(|error| bad(format!("random archive ID: {error}")))?;
    let mut volumes = Volumes::new(output, limit, hook, cancel);
    let mut records = Vec::with_capacity(ordered.len());
    for file in ordered {
        records.push(pack_file(
            source,
            file,
            level,
            &pool,
            &mut volumes,
            &mut progress,
        )?);
    }
    // A cancel during the last batch still stops the pack before its index is written.
    cancelled(cancel)?;
    volumes.close()?;
    let index = Index {
        archive_id,
        zstd_level: level,
        files: records,
        chunks: std::mem::take(&mut volumes.chunks),
        volume_sizes: volumes.sizes.clone(),
    };
    let bytes = encode_index(&index).map_err(format_error)?;
    volumes.write_headers(archive_id)?;
    let mut writer = volumes.output.create(crate::INDEX_NAME)?;
    writer.write_all(&bytes)?;
    volumes.output.finish(crate::INDEX_NAME, writer)?;
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The streaming encoder the writer used before it kept one context per thread.
    fn streamed(raw: &[u8], level: u8) -> Vec<u8> {
        let mut encoder = zstd::stream::Encoder::new(Vec::new(), i32::from(level)).unwrap();
        encoder.window_log(u32::from(CHUNK_SHIFT)).unwrap();
        encoder.include_checksum(true).unwrap();
        encoder
            .set_pledged_src_size(Some(raw.len() as u64))
            .unwrap();
        encoder.include_contentsize(true).unwrap();
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn reused_context_frames_match_the_streaming_encoder() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut random = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        };
        let text: Vec<u8> = (0..CHUNK_SIZE as usize)
            .map(|i| {
                if i % 5 == 0 {
                    random()
                } else {
                    b"ampr zstd "[i % 10]
                }
            })
            .collect();
        let noise: Vec<u8> = (0..CHUNK_SIZE).map(|_| random()).collect();
        let chunks = [
            &text[..],
            &text[..1000],
            &text[7..CHUNK_SIZE as usize / 2 + 3],
        ];
        // One thread runs every level in turn, so its context is reused and re-levelled,
        // also after a chunk whose frame outgrew it (left unfinished in the context).
        for level in [1u8, 3, 5, 9, 19, 22, 5] {
            let (codec, stored) = encode_chunk(noise.clone(), level).unwrap();
            assert_eq!((codec, &stored), (Codec::Raw, &noise));
            assert!(streamed(&noise, level).len() >= noise.len());
            for chunk in chunks {
                let (codec, frame) = encode_chunk(chunk.to_vec(), level).unwrap();
                assert_eq!(codec, Codec::Zstd);
                assert_eq!(
                    frame,
                    streamed(chunk, level),
                    "level {level}, {} bytes",
                    chunk.len()
                );
            }
        }
    }

    /// Kite Pack's probe: from level 6, what looks random and won't shrink 1% at level 1 is
    /// stored raw; what looks random but repeats itself is still compressed at its level.
    #[test]
    fn random_looking_chunks_are_probed_first() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut random = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        };
        // Random bytes whose last 1 KiB repeats their first: zstd saves ~0.4% on it at
        // level 19, so it used to be kept compressed; the probe stores it raw.
        let mut almost: Vec<u8> = (0..CHUNK_SIZE).map(|_| random()).collect();
        let head = almost[..1024].to_vec();
        let n = almost.len();
        almost[n - 1024..].copy_from_slice(&head);
        assert!(dense(&almost));
        assert!(!probe_saves(&almost).unwrap());
        assert!(streamed(&almost, 19).len() < almost.len(), "level 19 alone shrinks it");
        for level in [6u8, 9, 19] {
            let made = encode_chunk(almost.clone(), level).unwrap();
            assert!(made == (Codec::Raw, almost.clone()), "level {level}: {:?}", made.0);
        }
        // Below level 6 nothing is probed: chunks encode as they always did.
        for level in [1u8, 5] {
            let old = streamed(&almost, level);
            let want = if old.len() < almost.len() { (Codec::Zstd, old) } else { (Codec::Raw, almost.clone()) };
            assert!(encode_chunk(almost.clone(), level).unwrap() == want, "level {level}");
        }

        // Random-looking but half of it repeated: the probe sees the repeat, the chunk is
        // compressed at its own level, byte for byte as before.
        let half: Vec<u8> = (0..CHUNK_SIZE / 2).map(|_| random()).collect();
        let twice = [half.clone(), half].concat();
        assert!(dense(&twice));
        assert!(probe_saves(&twice).unwrap());
        for level in [9u8, 19] {
            let made = encode_chunk(twice.clone(), level).unwrap();
            assert!(made == (Codec::Zstd, streamed(&twice, level)), "level {level}: {:?}", made.0);
        }

        // Text is not random-looking and never probed; tiny chunks never are.
        assert!(!dense(&b"ampr zstd ".repeat(26_000)));
        assert!(!dense(&almost[..4095]));
    }

    use std::collections::HashMap;
    use std::io::Cursor;

    const CHUNK: usize = CHUNK_SIZE as usize;

    /// An in-memory source that counts the bytes read from each file.
    struct Mem {
        files: Vec<SourceFile>,
        bytes: HashMap<String, Vec<u8>>,
        reads: HashMap<String, u64>,
    }

    impl Mem {
        fn new(files: &[(String, Vec<u8>)]) -> Self {
            Self {
                files: files
                    .iter()
                    .map(|(path, bytes)| SourceFile {
                        path: path.clone(),
                        size: bytes.len() as u64,
                    })
                    .collect(),
                bytes: files.iter().cloned().collect(),
                reads: HashMap::new(),
            }
        }
    }

    impl SourceTree for Mem {
        fn files(&self) -> &[SourceFile] {
            &self.files
        }
        fn read(&mut self, path: &str) -> Result<Vec<u8>> {
            self.read_range(path, 0, usize::MAX)
        }
        fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
            let bytes = &self.bytes[path];
            let start = (offset as usize).min(bytes.len());
            let data = bytes[start..start.saturating_add(len).min(bytes.len())].to_vec();
            *self.reads.entry(path.into()).or_default() += data.len() as u64;
            Ok(data)
        }
        fn describe(&self) -> String {
            "memory".into()
        }
    }

    #[derive(Default)]
    struct Output(HashMap<String, Vec<u8>>);

    impl PackOutput for Output {
        type W = Cursor<Vec<u8>>;
        fn create(&mut self, _: &str) -> Result<Self::W> {
            Ok(Cursor::new(Vec::new()))
        }
        fn reopen(&mut self, name: &str) -> Result<Self::W> {
            Ok(Cursor::new(self.0.remove(name).unwrap()))
        }
        fn finish(&mut self, name: &str, writer: Self::W) -> Result<()> {
            self.0.insert(name.into(), writer.into_inner());
            Ok(())
        }
    }

    /// Deterministic incompressible bytes (xorshift64).
    fn noise(len: usize, mut state: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    fn text(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| b"ampr zstd chunk "[i % 16] ^ (i / 997) as u8)
            .collect()
    }

    fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    /// Compressible, incompressible and mixed files.
    fn sample() -> Vec<(String, Vec<u8>)> {
        let edge = |zeros: usize| {
            let mut last = noise(CHUNK, 9);
            last[..zeros].fill(0);
            concat(&[noise(3 * CHUNK, 8), last])
        };
        vec![
            ("text.bin".into(), text(6 * CHUNK + 77)),
            ("noise.bin".into(), noise(3 * CHUNK + 5, 1)),
            (
                "front.bin".into(),
                concat(&[vec![0; CHUNK], noise(5 * CHUNK, 2)]),
            ),
            (
                "tail.bin".into(),
                concat(&[noise(4 * CHUNK, 3), vec![0; CHUNK]]),
            ),
            (
                "mixed.bin".into(),
                concat(&[vec![0; CHUNK], noise(CHUNK, 4), vec![1; 1000]]),
            ),
            ("exact.bin".into(), text(CHUNK)),
            ("small.bin".into(), text(1000)),
            ("empty.bin".into(), Vec::new()),
            ("edge-store.bin".into(), edge(4 << 10)),
            ("edge-packed.bin".into(), edge(32 << 10)),
        ]
    }

    /// A volume file that keeps only its first `HEAD` bytes and tracks its length, so a
    /// volume can sit near `VOLUME_LIMIT` without 4 GiB of memory.
    #[derive(Default)]
    struct Sparse {
        head: Vec<u8>,
        pos: u64,
        len: u64,
    }

    const HEAD: usize = 128;

    impl Write for Sparse {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let at = self.pos as usize;
            if at < HEAD {
                let end = (at + buf.len()).min(HEAD);
                if self.head.len() < end {
                    self.head.resize(end, 0);
                }
                self.head[at..end].copy_from_slice(&buf[..end - at]);
            }
            self.pos += buf.len() as u64;
            self.len = self.len.max(self.pos);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Seek for Sparse {
        fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
            self.pos = match to {
                SeekFrom::Start(at) => at,
                SeekFrom::End(by) => self.len.checked_add_signed(by).unwrap(),
                SeekFrom::Current(by) => self.pos.checked_add_signed(by).unwrap(),
            };
            Ok(self.pos)
        }
    }

    /// Finished volumes by name, and every name created, in order.
    #[derive(Default)]
    struct SparseOutput {
        done: HashMap<String, Sparse>,
        created: Vec<String>,
    }

    impl PackOutput for SparseOutput {
        type W = Sparse;
        fn create(&mut self, name: &str) -> Result<Sparse> {
            self.created.push(name.into());
            Ok(Sparse::default())
        }
        fn reopen(&mut self, name: &str) -> Result<Sparse> {
            Ok(self.done.remove(name).unwrap())
        }
        fn finish(&mut self, name: &str, writer: Sparse) -> Result<()> {
            self.done.insert(name.into(), writer);
            Ok(())
        }
    }

    #[test]
    fn a_chunk_ending_at_the_limit_stays_and_the_next_starts_volume_two() {
        let mut output = SparseOutput::default();
        // Volume 0 already holds chunks up to one chunk + 100 bytes short of the limit.
        let name = volume_name(0);
        let mut writer = output.create(&name).unwrap();
        writer.write_all(&[0; VOLUME_HEADER_SIZE]).unwrap();
        let at = VOLUME_LIMIT - CHUNK_SIZE - 100;
        writer.seek(SeekFrom::Start(at)).unwrap();
        let cancel = AtomicBool::new(false);
        let mut volumes = Volumes::new(&mut output, VOLUME_LIMIT, None, &cancel);
        volumes.open = Some((name, writer, at));
        volumes.put(Codec::Raw, &[1; 100], 0, 100).unwrap();
        // Ends exactly at VOLUME_LIMIT: still volume 0.
        volumes.put(Codec::Raw, &[2; CHUNK], 0, CHUNK as u32).unwrap();
        // One byte more doesn't fit: volume 1, right after its header.
        volumes.put(Codec::Raw, &[3], 0, 1).unwrap();
        let place = |c: &Chunk| (c.volume_id, u64::from(c.offset), c.stored_size);
        let places: Vec<_> = volumes.chunks.iter().map(place).collect();
        assert_eq!(
            places,
            [
                (0, at, 100),
                (0, VOLUME_LIMIT - CHUNK_SIZE, CHUNK_SIZE as u32),
                (1, VOLUME_HEADER_SIZE as u64, 1),
            ]
        );
        volumes.close().unwrap();
        let sizes = vec![VOLUME_LIMIT, VOLUME_HEADER_SIZE as u64 + 1];
        assert_eq!(volumes.sizes, sizes);
        let archive_id = [9; 16];
        volumes.write_headers(archive_id).unwrap();
        assert_eq!(output.created, [volume_name(0), volume_name(1)]);
        assert_eq!(volume_name(1), "ampr_zstd-001.pak");
        for (id, &size) in sizes.iter().enumerate() {
            let volume = &output.done[&volume_name(id as u32)];
            assert_eq!(volume.len, size);
            let header = crate::decode_volume_header(&volume.head).unwrap();
            assert_eq!(
                header,
                VolumeHeader {
                    id: id as u32,
                    count: 2,
                    archive_id,
                    payload_bytes: size - VOLUME_HEADER_SIZE as u64,
                }
            );
        }
        assert_eq!(output.done[&volume_name(1)].head[VOLUME_HEADER_SIZE..], [3]);
    }

    #[test]
    fn a_cancel_during_the_last_batch_writes_no_index() {
        // No empty file, whose own pass would check the flag before the end.
        let files: Vec<_> = sample()
            .into_iter()
            .filter(|(_, bytes)| !bytes.is_empty())
            .collect();
        let total: u64 = files.iter().map(|(_, bytes)| bytes.len() as u64).sum();
        {
            let mut source = Mem::new(&files);
            let listed = source.files().to_vec();
            let mut output = Output::default();
            let cancel = AtomicBool::new(false);
            let mut last = 0;
            let result = pack_with(
                &mut source,
                &listed,
                5,
                2,
                &mut output,
                &mut |done| {
                    last = done;
                    if done == 2 * total {
                        cancel.store(true, Ordering::Relaxed);
                    }
                },
                &cancel,
            );
            assert!(matches!(result, Err(Error::Cancelled)));
            // The flag went up only at the very end.
            assert_eq!(last, 2 * total);
            assert!(!output.0.contains_key("ampr_zstd.index"));
        }
    }
}

/// Kite Pack: a hook ([`crate::hook::VolumeHook`]) sees each volume read back as it closes and is
/// asked before the next one opens.
#[cfg(test)]
mod hook_tests {
    use super::*;
    use crate::hook::VolumeHook;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    /// Volumes as files in a folder.
    struct DirOutput(PathBuf);

    impl PackOutput for DirOutput {
        type W = std::fs::File;
        fn create(&mut self, name: &str) -> Result<std::fs::File> {
            Ok(std::fs::File::create(self.0.join(name))?)
        }
        fn reopen(&mut self, name: &str) -> Result<std::fs::File> {
            Ok(std::fs::OpenOptions::new().write(true).open(self.0.join(name))?)
        }
        fn finish(&mut self, _: &str, _: std::fs::File) -> Result<()> {
            Ok(())
        }
    }

    /// Records what it is asked; `flip` hands back a copy with one stored byte changed.
    struct Seen {
        dir: PathBuf,
        flip: bool,
        calls: Mutex<Vec<String>>,
    }

    impl VolumeHook for Seen {
        fn open(&self, name: &str) -> std::io::Result<std::fs::File> {
            self.calls.lock().unwrap().push(format!("open {name}"));
            let path = self.dir.join(name);
            if !self.flip {
                return std::fs::File::open(path);
            }
            let mut bytes = std::fs::read(&path)?;
            bytes[VOLUME_HEADER_SIZE + 10] ^= 0xFF;
            let copy = self.dir.join(format!("{name}.flipped"));
            std::fs::write(&copy, bytes)?;
            std::fs::File::open(copy)
        }
        fn before_next(&self, next: u32, _: &AtomicBool) -> std::io::Result<()> {
            self.calls.lock().unwrap().push(format!("next {next}"));
            Ok(())
        }
        fn cut(&self) -> Vec<u32> {
            Vec::new()
        }
        fn most_held(&self) -> u32 {
            4
        }
    }

    fn seen(dir: &Path, flip: bool) -> Arc<Seen> {
        Arc::new(Seen { dir: dir.to_path_buf(), flip, calls: Mutex::new(Vec::new()) })
    }

    /// Text that zstd shrinks, `len` bytes from `at`.
    fn text(at: usize, len: usize) -> Vec<u8> {
        (at..at + len).map(|i| b"kite pack volume hook "[i % 22] ^ (i / 1009) as u8).collect()
    }

    /// Puts `n` chunks of text, as the packer would, into `volumes`.
    fn put_text<O: PackOutput>(volumes: &mut Volumes<O>, n: usize) {
        for i in 0..n {
            let raw = text(i * 1000, CHUNK_SIZE as usize);
            let (crc, len) = (crc32fast::hash(&raw), raw.len() as u32);
            let (codec, data) = encode_chunk(raw, 3).unwrap();
            volumes.put(codec, &data, crc, len).unwrap_or_else(|e| panic!("chunk {i}: {e}"));
        }
    }

    fn volumes<'a>(out: &'a mut DirOutput, hook: Option<Arc<dyn VolumeHook>>, cancel: &'a AtomicBool) -> Volumes<'a, DirOutput> {
        // Room for about two text chunks (each packs to a few KiB) per volume.
        let one = encode_chunk(text(0, CHUNK_SIZE as usize), 3).unwrap().1.len() as u64;
        Volumes::new(out, VOLUME_HEADER_SIZE as u64 + 2 * one + one / 2, hook, cancel)
    }

    #[test]
    fn checks_each_volume_and_asks_before_the_next() {
        let tmp = tempfile::tempdir().unwrap();
        let hook = seen(tmp.path(), false);
        let cancel = AtomicBool::new(false);
        let mut out = DirOutput(tmp.path().to_path_buf());
        let mut v = volumes(&mut out, Some(hook.clone()), &cancel);
        put_text(&mut v, 3);
        assert_eq!(*hook.calls.lock().unwrap(), ["open ampr_zstd-000.pak", "next 1"]);
        v.close().unwrap();
        assert_eq!(*hook.calls.lock().unwrap(), ["open ampr_zstd-000.pak", "next 1", "open ampr_zstd-001.pak"]);
        assert_eq!(v.sizes.len(), 2);
    }

    #[test]
    fn a_volume_that_reads_back_wrong_fails_the_pack() {
        let tmp = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut out = DirOutput(tmp.path().to_path_buf());
        let mut v = volumes(&mut out, Some(seen(tmp.path(), true)), &cancel);
        put_text(&mut v, 2);
        let raw = text(0, CHUNK_SIZE as usize);
        let crc = crc32fast::hash(&raw);
        let (codec, data) = encode_chunk(raw, 3).unwrap();
        let error = v.put(codec, &data, crc, CHUNK_SIZE as u32).unwrap_err().to_string();
        assert!(error.contains("ampr_zstd-000.pak"), "{error}");
    }

    #[test]
    fn no_hook_no_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut out = DirOutput(tmp.path().to_path_buf());
        let mut v = volumes(&mut out, None, &cancel);
        put_text(&mut v, 5);
        v.close().unwrap();
        assert_eq!(v.sizes.len(), 3);
    }

    #[test]
    fn a_volume_that_cannot_be_read_back_fails_the_pack() {
        let tmp = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut out = DirOutput(tmp.path().to_path_buf());
        // Asked for a folder that isn't there: nothing can be opened.
        let mut v = volumes(&mut out, Some(seen(&tmp.path().join("nowhere"), false)), &cancel);
        let raw = text(0, CHUNK_SIZE as usize);
        let (codec, data) = encode_chunk(raw.clone(), 3).unwrap();
        let crc = crc32fast::hash(&raw);
        v.put(codec, &data, crc, CHUNK_SIZE as u32).unwrap();
        v.put(codec, &data, crc, CHUNK_SIZE as u32).unwrap();
        assert!(v.put(codec, &data, crc, CHUNK_SIZE as u32).is_err());
    }

    #[test]
    fn a_hook_interrupted_before_the_next_volume_cancels_the_pack() {
        struct Stop(Seen);
        impl VolumeHook for Stop {
            fn open(&self, name: &str) -> std::io::Result<std::fs::File> {
                self.0.open(name)
            }
            fn before_next(&self, _: u32, _: &AtomicBool) -> std::io::Result<()> {
                Err(std::io::ErrorKind::Interrupted.into())
            }
            fn cut(&self) -> Vec<u32> {
                Vec::new()
            }
            fn most_held(&self) -> u32 {
                4
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut out = DirOutput(tmp.path().to_path_buf());
        let hook = Stop(Seen { dir: tmp.path().to_path_buf(), flip: false, calls: Mutex::new(Vec::new()) });
        let mut v = volumes(&mut out, Some(Arc::new(hook)), &cancel);
        let raw = text(0, CHUNK_SIZE as usize);
        let (codec, data) = encode_chunk(raw.clone(), 3).unwrap();
        let crc = crc32fast::hash(&raw);
        v.put(codec, &data, crc, CHUNK_SIZE as u32).unwrap();
        v.put(codec, &data, crc, CHUNK_SIZE as u32).unwrap();
        assert!(matches!(v.put(codec, &data, crc, CHUNK_SIZE as u32), Err(Error::Cancelled)));
    }

    /// Files in memory: a game to pack, then the pack to read.
    #[derive(Default)]
    struct Files(Vec<SourceFile>, std::collections::HashMap<String, Vec<u8>>);

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
        fn read(&mut self, path: &str) -> Result<Vec<u8>> {
            Ok(self.1[path].clone())
        }
        fn read_range(&mut self, path: &str, at: u64, len: usize) -> Result<Vec<u8>> {
            let b = &self.1[path];
            let at = (at as usize).min(b.len());
            Ok(b[at..(at + len).min(b.len())].to_vec())
        }
        fn describe(&self) -> String {
            "files".into()
        }
    }

    impl PackOutput for Files {
        type W = std::io::Cursor<Vec<u8>>;
        fn create(&mut self, _: &str) -> Result<Self::W> {
            Ok(std::io::Cursor::new(Vec::new()))
        }
        fn reopen(&mut self, name: &str) -> Result<Self::W> {
            Ok(std::io::Cursor::new(self.1.remove(name).unwrap()))
        }
        fn finish(&mut self, name: &str, writer: Self::W) -> Result<()> {
            let bytes = writer.into_inner();
            self.add(name, bytes);
            Ok(())
        }
    }

    fn noise(len: usize, mut x: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    #[test]
    fn reads_a_pack_with_a_volume_cut_to_its_header() {
        // Two files of noise (stored raw), one volume each.
        let mut game = Files::default();
        game.add("a/one.bin", noise(CHUNK_SIZE as usize, 1));
        game.add("b/two.bin", noise(CHUNK_SIZE as usize, 2));
        let files = game.0.clone();
        let mut pack = Files::default();
        let cancel = AtomicBool::new(false);
        let index = pack_run(&mut game, &files, 3, 1, &mut pack, &mut |_| {}, &cancel, None, VOLUME_HEADER_SIZE as u64 + CHUNK_SIZE)
            .unwrap();
        assert_eq!(index.volume_sizes.len(), 2);
        let in_zero = |path: &str| {
            let record = index.files.iter().find(|f| f.path == path).unwrap();
            index.chunks[record.first_chunk as usize].volume_id == 0
        };
        let (gone, kept) = if in_zero("a/one.bin") { ("a/one.bin", "b/two.bin") } else { ("b/two.bin", "a/one.bin") };
        // Volume 0 cut to its header, as Kite Pack leaves a part the PS5 has.
        let zero = volume_name(0);
        let head = pack.1[&zero][..VOLUME_HEADER_SIZE].to_vec();
        pack.add(&zero, head.clone());

        let whole = |pack: &Files| {
            let mut copy = Files::default();
            for f in &pack.0 {
                copy.add(&f.path, pack.1[&f.path].clone());
            }
            copy
        };
        assert!(crate::reader::unpack(Box::new(whole(&pack))).is_err(), "a cut volume needs the cut list");
        let mut tree = crate::reader::unpack_cut(Box::new(whole(&pack)), &[0]).unwrap();
        let mut listed: Vec<_> = tree.files().iter().map(|f| f.path.clone()).collect();
        listed.sort();
        assert_eq!(listed, ["a/one.bin", "b/two.bin"]);
        assert_eq!(tree.read_range(kept, 0, 64).unwrap(), game.1[kept][..64]);
        let error = tree.read_range(gone, 0, 64).unwrap_err().to_string();
        assert!(error.contains("let go"), "{error}");
        // A volume in the list must really be cut to its header.
        let mut uncut = whole(&pack);
        uncut.add(&zero, [head, vec![0]].concat());
        assert!(crate::reader::unpack_cut(Box::new(uncut), &[0]).is_err());
    }
}
