//! The Kite helper's read-only console metadata calls: listing a folder, reading a file,
//! stat, a recursive size, and free space.
//!
//! These are plain filesystem calls with a read-only policy on the helper's side (see
//! `helper/src/meta.h`, which documents the wire layout this module implements). They exist so
//! Kite Pack can show what is installed on the PS5 and check it before sending: no Sony API is
//! involved, and nothing here can write to the console.
//!
//! The bodies are little-endian binary, like AVA1's own codecs: a 16-byte-prefixed path, then
//! an offset and a limit. Every reply is bounded by the helper, and a paged list says whether
//! more entries follow.

use ava1::session::Session;
use serde::Serialize;
use std::time::Duration;

use crate::Ps5Error;

/// One request may take this long before the call gives up.
const CALL_TIMEOUT: Duration = Duration::from_secs(6);
/// The helper's page size and read cap (`meta.h`).
const PAGE: u32 = 256;
const READ_MAX: usize = 256 * 1024;

const METHOD_FS_LIST: u16 = 24;
const METHOD_FS_STAT: u16 = 25;
const METHOD_FS_READ: u16 = 26;
const METHOD_FS_DU: u16 = 27;
const METHOD_FS_FREESPACE: u16 = 28;

/// The helper's "that path is not there", told apart from "there and unreadable": a drive
/// with no `user/app` folder is empty, not broken.
const STATUS_NOT_FOUND: u16 = 40;
/// AVA1's `ERR_PATH`: the request named something the helper may not read.
const STATUS_PATH: u16 = 9;

/// What a metadata call can say went wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetaError {
    /// The path does not exist on the console.
    #[error("not on the console")]
    NotFound,
    /// The path is outside what the helper lets a computer read.
    #[error("the helper will not read that path")]
    Path,
    /// The helper refused with a status and a reason.
    #[error("{message}")]
    Refused { code: u16, message: String },
    /// The helper's answer did not decode: a bug on one side or the other.
    #[error("the helper sent something unreadable: {0}")]
    Protocol(String),
    /// The console is not reachable, or the session could not be opened.
    #[error("{0}")]
    Network(Ps5Error),
}

impl MetaError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, MetaError::NotFound)
    }

}

/// One folder entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    /// `file`, `folder`, `link` or `other`.
    pub kind: String,
    pub size: u64,
    pub mtime: i64,
}

impl Entry {
    pub fn is_folder(&self) -> bool {
        self.kind == "folder"
    }
}

/// What a recursive size counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
    /// A cap stopped the walk, so the numbers are a lower bound.
    pub partial: bool,
}

/// A volume's room, as the console reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeSpace {
    pub total: u64,
    pub free: u64,
    /// What a game can use: free minus the console's own reserve.
    pub usable: u64,
    pub reserve: u64,
}

/* ---- the wire -------------------------------------------------------------------------------- */

/// The one request layout every method takes: `u16 path_len | path | u64 offset | u32 limit`.
fn request(path: &str, offset: u64, limit: u32) -> Result<Vec<u8>, MetaError> {
    let p = path.as_bytes();
    if p.is_empty() || p.len() > u16::MAX as usize {
        return Err(MetaError::Path);
    }
    let mut body = Vec::with_capacity(2 + p.len() + 12);
    body.extend_from_slice(&(p.len() as u16).to_le_bytes());
    body.extend_from_slice(p);
    body.extend_from_slice(&offset.to_le_bytes());
    body.extend_from_slice(&limit.to_le_bytes());
    Ok(body)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], MetaError> {
        let end = self.at.checked_add(n).ok_or_else(|| MetaError::Protocol("length overflow".into()))?;
        if end > self.b.len() {
            return Err(MetaError::Protocol("reply shorter than its own header".into()));
        }
        let out = &self.b[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, MetaError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, MetaError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, MetaError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, MetaError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }

    fn i64(&mut self) -> Result<i64, MetaError> {
        Ok(self.u64()? as i64)
    }
}

fn kind(i: u8) -> String {
    match i {
        1 => "file",
        2 => "folder",
        3 => "link",
        _ => "other",
    }
    .to_string()
}

async fn call(s: &Session, method: u16, path: &str, offset: u64, limit: u32) -> Result<Vec<u8>, MetaError> {
    let body = request(path, offset, limit)?;
    let reply = s
        .rpc_within(method, &body, CALL_TIMEOUT)
        .await
        .map_err(|e| MetaError::Network(crate::connect::map_error(e)))?;
    match reply.status {
        0 => Ok(reply.body),
        STATUS_NOT_FOUND => Err(MetaError::NotFound),
        STATUS_PATH => Err(MetaError::Path),
        code => Err(MetaError::Refused {
            code,
            message: String::from_utf8_lossy(&reply.body).into_owned(),
        }),
    }
}

/* ---- the calls ------------------------------------------------------------------------------- */

/// Every entry of `path`, paged. An empty folder and a missing one both give an empty list only
/// when the caller asked with [`list_lenient`]; `list` reports the missing one.
pub async fn list(s: &Session, path: &str) -> Result<Vec<Entry>, MetaError> {
    let mut out = Vec::new();
    let mut offset = 0u64;
    // A folder with more entries than this is not a game folder; stop rather than loop forever.
    const MAX_ENTRIES: usize = 100_000;
    loop {
        let body = call(s, METHOD_FS_LIST, path, offset, PAGE).await?;
        let mut r = Reader::new(&body);
        let count = r.u32()? as usize;
        for _ in 0..count {
            let name = r.u16()? as usize;
            let name = String::from_utf8(r.take(name)?.to_vec())
                .map_err(|_| MetaError::Protocol("a name was not UTF-8".into()))?;
            let kind = kind(r.u8()?);
            let size = r.u64()?;
            let mtime = r.i64()?;
            out.push(Entry { name, kind, size, mtime });
        }
        let more = r.u8()? != 0;
        let next = r.u64()?;
        if !more || out.len() >= MAX_ENTRIES || (count == 0 && next == offset) {
            break;
        }
        offset = next;
    }
    Ok(out)
}

/// [`list`] for a folder that may not exist (an empty `/mnt/ext1/user/app`), where "not there"
/// means "nothing in it".
pub async fn list_lenient(s: &Session, path: &str) -> Result<Vec<Entry>, MetaError> {
    match list(s, path).await {
        Err(MetaError::NotFound) => Ok(Vec::new()),
        other => other,
    }
}

/// A path's kind, size and modification time.
pub async fn stat(s: &Session, path: &str) -> Result<Entry, MetaError> {
    let body = call(s, METHOD_FS_STAT, path, 0, 0).await?;
    let mut r = Reader::new(&body);
    let kind = kind(r.u8()?);
    let size = r.u64()?;
    let mtime = r.i64()?;
    let _mode = r.u32()?;
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    Ok(Entry { name, kind, size, mtime })
}

/// A whole file, looped over the helper's per-call read cap, up to `max` bytes.
pub async fn read_all(s: &Session, path: &str, max: usize) -> Result<Vec<u8>, MetaError> {
    let max = max.min(READ_MAX * 64);
    let mut out = Vec::new();
    let mut offset = 0u64;
    loop {
        let want = (max - out.len()).min(READ_MAX) as u32;
        if want == 0 {
            break;
        }
        let body = call(s, METHOD_FS_READ, path, offset, want).await?;
        let mut r = Reader::new(&body);
        let len = r.u32()? as usize;
        let eof = r.u8()? != 0;
        let bytes = r.take(len)?;
        out.extend_from_slice(bytes);
        offset += len as u64;
        if eof || len == 0 {
            break;
        }
    }
    Ok(out)
}

/// The recursive size of `path`, as the console's drives count it.
pub async fn du(s: &Session, path: &str) -> Result<Usage, MetaError> {
    let body = call(s, METHOD_FS_DU, path, 0, 0).await?;
    let mut r = Reader::new(&body);
    Ok(Usage { bytes: r.u64()?, files: r.u64()?, dirs: r.u64()?, partial: r.u8()? != 0 })
}

/// The room left on the volume `path` is on.
pub async fn freespace(s: &Session, path: &str) -> Result<FreeSpace, MetaError> {
    let body = call(s, METHOD_FS_FREESPACE, path, 0, 0).await?;
    let mut r = Reader::new(&body);
    Ok(FreeSpace { total: r.u64()?, free: r.u64()?, usable: r.u64()?, reserve: r.u64()? })
}

/// The words the screen uses for a metadata failure.
pub fn into_ps5(e: MetaError) -> Ps5Error {
    match e {
        MetaError::NotFound => Ps5Error::Failed("That isn't on the PS5".into()),
        MetaError::Path => Ps5Error::Failed("The helper won't read that folder".into()),
        MetaError::Refused { message, .. } if !message.is_empty() => Ps5Error::Failed(message),
        MetaError::Refused { .. } => Ps5Error::Failed("The PS5 refused".into()),
        MetaError::Protocol(why) => Ps5Error::Failed(why),
        MetaError::Network(e) => e,
    }
}
