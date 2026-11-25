//! A package to install as it is: its platform, kind, ids, name and icon.

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// What the screen and the installer need to know about a `.pkg` or `.fpkg`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    /// `ps5` or `ps4`.
    pub platform: String,
    /// `game`, `update` or `dlc`.
    pub kind: String,
    pub title_id: String,
    pub content_id: String,
    pub title: Option<String>,
    pub version: Option<String>,
    /// The package file's size.
    pub bytes: u64,
    pub icon_data_url: Option<String>,
    /// Kite Pack can also pack it (a PS5 game package).
    pub packable: bool,
}

/// Whether `path` is named like a package (`.pkg`, `.fpkg`).
pub fn is_package_name(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pkg") || e.eq_ignore_ascii_case("fpkg"))
}

/// Reads a package: a PS4 one from its own header, a PS5 one through Forge.
pub fn inspect_package(path: &Path) -> Result<PackageInfo, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Couldn't open the package: {e}"))?;
    let bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(|_| "This isn't a package".to_string())?;
    if &magic == b"\x7FCNT" {
        return ps4(&mut file, bytes);
    }
    ps5(path, bytes)
}

fn ps5(path: &Path, bytes: u64) -> Result<PackageInfo, String> {
    let found = ps5_dump_forge_core::inspect(path).map_err(|e| crate::inspect::problem(&format!("{e:#}")).message())?;
    let kind = if found.patch {
        "update"
    } else if found.additional_content {
        "dlc"
    } else {
        "game"
    };
    let content_id = found.content_id.clone().unwrap_or_default();
    let title_id = found.title_id.clone().or_else(|| title_of(&content_id)).ok_or("A package without a game id can't be installed")?;
    Ok(PackageInfo {
        platform: "ps5".into(),
        kind: kind.into(),
        title_id,
        content_id,
        title: found.title_name,
        version: found.version,
        bytes,
        icon_data_url: found.cover,
        packable: kind == "game" && !found.zstd_packed,
    })
}

/// `UP0000-CUSA12345_00-…` → `CUSA12345`.
fn title_of(content_id: &str) -> Option<String> {
    content_id.get(7..16).filter(|t| t.chars().all(|c| c.is_ascii_alphanumeric())).map(str::to_string)
}

const PARAM_SFO: u32 = 0x1000;
const ICON0_PNG: u32 = 0x1200;
const ENCRYPTED: u32 = 0x8000_0000;

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

/// A PS4 package: content id from its header; name, category and version from `param.sfo` and the
/// icon from `icon0.png` when those entries are plain (fake packages keep them so).
fn ps4(file: &mut std::fs::File, bytes: u64) -> Result<PackageInfo, String> {
    let mut head = [0u8; 0x80];
    file.seek(SeekFrom::Start(0)).and_then(|_| file.read_exact(&mut head)).map_err(|_| "This package is damaged or incomplete".to_string())?;
    let content_id = String::from_utf8_lossy(&head[0x40..0x64]).trim_end_matches('\0').to_string();
    let title_id = title_of(&content_id).ok_or("This package has no game id")?;
    let (count, table) = (be32(&head, 0x10) as u64, be32(&head, 0x18) as u64);
    let mut entry = |id: u32| -> Option<Vec<u8>> {
        if count == 0 || count > 10_000 || table.checked_add(count * 32)? > bytes {
            return None;
        }
        let mut entries = vec![0u8; (count * 32) as usize];
        file.seek(SeekFrom::Start(table)).ok()?;
        file.read_exact(&mut entries).ok()?;
        let e = entries.chunks_exact(32).find(|e| be32(e, 0) == id)?;
        let (flags, at, len) = (be32(e, 8), be32(e, 16) as u64, be32(e, 20) as u64);
        if flags & ENCRYPTED != 0 || len == 0 || len > 4 << 20 || at.checked_add(len)? > bytes {
            return None;
        }
        let mut data = vec![0u8; len as usize];
        file.seek(SeekFrom::Start(at)).ok()?;
        file.read_exact(&mut data).ok()?;
        Some(data)
    };
    let param = entry(PARAM_SFO).map(|b| sfo_strings(&b)).unwrap_or_default();
    let icon = entry(ICON0_PNG).filter(|b| b.starts_with(b"\x89PNG")).map(|b| format!("data:image/png;base64,{}", base64(&b)));
    let get = |key: &str| param.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let kind = match get("CATEGORY").as_deref() {
        Some("gp") => "update",
        Some("ac") => "dlc",
        _ => "game",
    };
    Ok(PackageInfo {
        platform: "ps4".into(),
        kind: kind.into(),
        title_id,
        content_id,
        title: get("TITLE"),
        version: get("APP_VER").or_else(|| get("VERSION")),
        bytes,
        icon_data_url: icon,
        packable: false,
    })
}

/// The UTF-8 values of a `param.sfo` (format 0x0204), by key.
fn sfo_strings(b: &[u8]) -> Vec<(String, String)> {
    let le32 = |at: usize| b.get(at..at + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize);
    let le16 = |at: usize| b.get(at..at + 2).map(|x| u16::from_le_bytes(x.try_into().unwrap()) as usize);
    let cstr = |at: usize| b.get(at..).map(|r| String::from_utf8_lossy(r.split(|&c| c == 0).next().unwrap_or(&[])).into_owned());
    let (Some(keys), Some(data), Some(n)) = (le32(8), le32(12), le32(16)) else { return Vec::new() };
    if !b.starts_with(b"\0PSF") || n > 1000 {
        return Vec::new();
    }
    (0..n)
        .filter_map(|i| {
            let e = 20 + i * 16;
            let (key_at, fmt, len, at) = (le16(e)?, le16(e + 2)?, le32(e + 4)?, le32(e + 12)?);
            if fmt != 0x0204 {
                return None;
            }
            let key = cstr(keys + key_at)?;
            let value = b.get(data + at..data + at + len).map(|v| String::from_utf8_lossy(v.split(|&c| c == 0).next().unwrap_or(&[])).into_owned())?;
            Some((key, value))
        })
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `param.sfo` with UTF-8 values.
    fn sfo(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut keys = Vec::new();
        let mut data = Vec::new();
        let mut index = Vec::new();
        for (k, v) in pairs {
            let value = [v.as_bytes(), &[0]].concat();
            index.extend_from_slice(&(keys.len() as u16).to_le_bytes());
            index.extend_from_slice(&0x0204u16.to_le_bytes());
            index.extend_from_slice(&(value.len() as u32).to_le_bytes());
            index.extend_from_slice(&(value.len() as u32).to_le_bytes());
            index.extend_from_slice(&(data.len() as u32).to_le_bytes());
            keys.extend_from_slice(k.as_bytes());
            keys.push(0);
            data.extend_from_slice(&value);
        }
        let key_start = 20 + index.len();
        let data_start = key_start + keys.len();
        let mut out = b"\0PSF".to_vec();
        out.extend_from_slice(&0x101u32.to_le_bytes());
        out.extend_from_slice(&(key_start as u32).to_le_bytes());
        out.extend_from_slice(&(data_start as u32).to_le_bytes());
        out.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
        out.extend(index);
        out.extend(keys);
        out.extend(data);
        out
    }

    /// A fake PS4 package: its header, an entry table and the entries' data.
    fn ps4_package(dir: &std::path::Path, entries: &[(u32, u32, Vec<u8>)]) -> std::path::PathBuf {
        let mut pkg = vec![0u8; 0x1000];
        pkg[..4].copy_from_slice(b"\x7FCNT");
        pkg[0x10..0x14].copy_from_slice(&(entries.len() as u32).to_be_bytes());
        pkg[0x18..0x1C].copy_from_slice(&0x400u32.to_be_bytes());
        pkg[0x40..0x64].copy_from_slice(b"UP0000-CUSA12345_00-ASHENTIDE0000000");
        let mut at = 0x1000usize;
        for (i, (id, flags, data)) in entries.iter().enumerate() {
            let e = 0x400 + i * 32;
            pkg[e..e + 4].copy_from_slice(&id.to_be_bytes());
            pkg[e + 8..e + 12].copy_from_slice(&flags.to_be_bytes());
            pkg[e + 16..e + 20].copy_from_slice(&(at as u32).to_be_bytes());
            pkg[e + 20..e + 24].copy_from_slice(&(data.len() as u32).to_be_bytes());
            pkg.extend_from_slice(data);
            at += data.len();
        }
        pkg.extend(vec![7u8; 4096]);
        let path = dir.join("Ashen Tide [CUSA12345].pkg");
        std::fs::write(&path, &pkg).unwrap();
        path
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake icon";

    #[test]
    fn reads_a_ps4_game_package() {
        let tmp = tempfile::tempdir().unwrap();
        let param = sfo(&[("APP_VER", "01.00"), ("CATEGORY", "gd"), ("TITLE", "Ashen Tide"), ("TITLE_ID", "CUSA12345")]);
        let pkg = ps4_package(tmp.path(), &[(0x1000, 0, param), (0x1200, 0, PNG.to_vec())]);
        let info = inspect_package(&pkg).unwrap();
        assert_eq!(info.platform, "ps4");
        assert_eq!(info.kind, "game");
        assert_eq!(info.title_id, "CUSA12345");
        assert_eq!(info.content_id, "UP0000-CUSA12345_00-ASHENTIDE0000000");
        assert_eq!(info.title.as_deref(), Some("Ashen Tide"));
        assert_eq!(info.version.as_deref(), Some("01.00"));
        assert_eq!(info.bytes, std::fs::metadata(&pkg).unwrap().len());
        assert!(info.icon_data_url.as_deref().is_some_and(|u| u.starts_with("data:image/png;base64,")));
        assert!(!info.packable, "a PS4 package can't be packed");
    }

    #[test]
    fn reads_a_ps4_update_and_dlc_by_their_category() {
        let tmp = tempfile::tempdir().unwrap();
        let update = ps4_package(tmp.path(), &[(0x1000, 0, sfo(&[("CATEGORY", "gp"), ("APP_VER", "01.05")]))]);
        assert_eq!(inspect_package(&update).unwrap().kind, "update");
        let dlc = ps4_package(tmp.path(), &[(0x1000, 0, sfo(&[("CATEGORY", "ac")]))]);
        assert_eq!(inspect_package(&dlc).unwrap().kind, "dlc");
    }

    #[test]
    fn a_ps4_package_with_encrypted_entries_keeps_its_content_id() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = ps4_package(tmp.path(), &[(0x1000, 0x8000_0000, vec![0xAB; 64]), (0x1200, 0x8000_0000, vec![0xCD; 64])]);
        let info = inspect_package(&pkg).unwrap();
        assert_eq!((info.title_id.as_str(), info.kind.as_str()), ("CUSA12345", "game"));
        assert!(info.title.is_none() && info.icon_data_url.is_none());
    }

    #[test]
    fn refuses_what_isnt_a_package() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("notes.pkg");
        std::fs::write(&path, b"hello").unwrap();
        assert!(inspect_package(&path).is_err());
        assert!(inspect_package(&tmp.path().join("missing.pkg")).is_err());
    }
}
