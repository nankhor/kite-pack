//! What is installed on the PS5, read through the helper's read-only metadata calls.
//!
//! Three sources, merged the way the console itself sees a title:
//!
//! - `/user/appmeta/<TITLE_ID>` — the console's per-title metadata folder. Every title Sony's
//!   own installer put there has one, and it is where the tile art and `param.json` live.
//! - `/user/app/<TITLE_ID>` — the title's content folder. A folder or image mount made by
//!   ShadowMount+/ampr has this without necessarily having an appmeta folder.
//! - `/mnt/ext0|ext1/user/app/<TITLE_ID>` — the same on either extended drive.
//!
//! Nothing Sony-specific is called: no `app.db`, no AppInstUtil. That means a title installed by
//! a homebrew installer that wrote only `app.db` (and no appmeta folder) is not listed; the
//! screen says so rather than pretending the list is complete.
//!
//! Per title the name comes from `param.json` (appmeta first, then the app's own `sce_sys`),
//! the update from `/user/patch/<id>`, and the add-ons from the folders under
//! `/user/addcont/<id>` — each folder is a DLC label, the tail of its content id, which is the
//! same `label` Forge reports for a local DLC package.

use ava1::session::Session;
use serde::Serialize;

use crate::connect::{open, Console, Keys};
use crate::meta::{self, Entry, MetaError};
use crate::{runtime, Ps5Error};

/// The console's folders this module reads. Kept in one place so a helper started with a test
/// root and a real console use the same code.
const APPMETA: &str = "/user/appmeta";
const APP: &str = "/user/app";
const PATCH: &str = "/user/patch";
const ADDCONT: &str = "/user/addcont";
const EXT_ROOTS: [&str; 2] = ["/mnt/ext0/user/app", "/mnt/ext1/user/app"];

/// More titles than this is not a console; stop and say the list was cut.
const MAX_TITLES: usize = 400;
/// A `param.json` with every language can be large, but not this large.
const PARAM_MAX: usize = 1024 * 1024;
/// Bigger than any `icon0.png` the console holds (the largest seen is about 700 KiB).
const ICON_MAX: usize = 2 * 1024 * 1024;

/// One title installed on the console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledGame {
    pub title_id: String,
    /// The console's name for it, or the title id when no `param.json` names it.
    pub title: String,
    /// `contentVersion` from `param.json`: what the base content is, not the installed update.
    pub version: Option<String>,
    /// `/user/patch/<id>` exists.
    pub update_installed: bool,
    /// One label per installed add-on, sorted.
    pub dlc: Vec<String>,
    /// Its content folder is on an extended drive.
    pub external: bool,
    /// Where it was found: `appmeta`, `app`, or both, joined by `+`.
    pub found: String,
}

impl InstalledGame {
    pub fn dlc_count(&self) -> usize {
        self.dlc.len()
    }
}

/// A library read, and whether it is the whole console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub games: Vec<InstalledGame>,
    /// More title folders exist than the app lists.
    pub truncated: bool,
}

/// One title's icon, as a `data:` URL the screen can put straight in an `<img>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameIcon {
    pub title_id: String,
    pub data_url: String,
}

/// What a title takes on the console, app + update + add-ons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    pub bytes: u64,
    pub app_bytes: u64,
    pub update_bytes: u64,
    pub dlc_bytes: u64,
    /// A walk hit its cap, so these are lower bounds.
    pub partial: bool,
}

/* ---- reading a library ------------------------------------------------------------------------ */

/// The console's installed titles. One session, the checker's key, so a refresh never ends the
/// session a send runs on.
pub fn installed(console: &Console, keys: &Keys) -> Result<Library, Ps5Error> {
    runtime().block_on(async {
        let session = open(console, &keys.checker, keys).await?;
        let out = read_library(&session).await;
        session.close().await;
        out.map_err(meta::into_ps5)
    })
}

/// One title found on the console, and where.
struct Candidate {
    id: String,
    external: bool,
    found: Vec<&'static str>,
}

/// The ids on this console, from the three sources. A source that cannot be read is skipped:
/// a busy console must not read as an empty one, and the caller sees the titles it did get.
async fn title_ids(s: &Session) -> (Vec<Candidate>, bool) {
    let mut found: Vec<Candidate> = Vec::new();
    let mut truncated = false;

    let mut add = |entries: Vec<Entry>, source: &'static str, external: bool| {
        for e in entries {
            if !looks_like_title_id(&e.name) || is_console_app(&e.name) {
                continue;
            }
            match found.iter_mut().find(|c| c.id == e.name) {
                Some(c) => {
                    if !c.found.contains(&source) {
                        c.found.push(source);
                    }
                    c.external &= external;
                }
                None => {
                    if found.len() >= MAX_TITLES {
                        truncated = true;
                        return;
                    }
                    found.push(Candidate { id: e.name, external, found: vec![source] });
                }
            }
        }
    };

    if let Ok(entries) = meta::list_lenient(s, APPMETA).await {
        add(entries, "appmeta", false);
    }
    if let Ok(entries) = meta::list_lenient(s, APP).await {
        add(entries, "app", false);
    }
    for root in EXT_ROOTS {
        if let Ok(entries) = meta::list_lenient(s, root).await {
            add(entries, "app", true);
        }
    }
    (found, truncated)
}

async fn read_library(s: &Session) -> Result<Library, MetaError> {
    let (found, mut truncated) = title_ids(s).await;

    let mut games = Vec::with_capacity(found.len());
    for c in found {
        let addcont = format!("{ADDCONT}/{}", c.id);
        let patch_dir = format!("{PATCH}/{}", c.id);
        let (param, dlc, patch) =
            tokio::join!(read_param(s, &c.id), meta::list_lenient(s, &addcont), meta::stat(s, &patch_dir));

        let mut labels: Vec<String> = dlc
            .unwrap_or_default()
            .iter()
            .filter(|e| e.is_folder())
            .map(|e| e.name.clone())
            .collect();
        labels.sort();

        let (title, version) = match param {
            Some(p) => (p.title.unwrap_or_else(|| c.id.clone()), p.version),
            None => (c.id.clone(), None),
        };

        games.push(InstalledGame {
            title_id: c.id,
            title,
            version,
            update_installed: patch.as_ref().map(|e| e.is_folder()).unwrap_or(false),
            dlc: labels,
            external: c.external,
            found: c.found.join("+"),
        });
    }

    games.sort_by(|a, b| {
        a.title.to_lowercase().cmp(&b.title.to_lowercase()).then(a.title_id.cmp(&b.title_id))
    });
    if games.len() >= MAX_TITLES {
        truncated = true;
    }
    Ok(Library { games, truncated })
}

/// A `param.json` as the console keeps it.
pub(crate) struct Param {
    title: Option<String>,
    version: Option<String>,
}

/// The title's `param.json`: the appmeta copy first (what the console shows), then the app's own
/// `sce_sys`, which is all a folder mount made by another tool has.
async fn read_param(s: &Session, title_id: &str) -> Option<Param> {
    for path in [
        format!("{APPMETA}/{title_id}/param.json"),
        format!("{APP}/{title_id}/sce_sys/param.json"),
        format!("{}/{title_id}/sce_sys/param.json", EXT_ROOTS[0]),
        format!("{}/{title_id}/sce_sys/param.json", EXT_ROOTS[1]),
    ] {
        let bytes = match meta::read_all(s, &path, PARAM_MAX).await {
            Ok(b) if !b.is_empty() => b,
            _ => continue,
        };
        if let Some(p) = parse_param(&bytes) {
            return Some(p);
        }
    }
    None
}

/// The name the console shows: `localizedParameters.<defaultLanguage>.titleName`, else the first
/// language that has one, else a top-level `titleName`. Same rule the console's own tools use.
pub(crate) fn parse_param(bytes: &[u8]) -> Option<Param> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let localized = v.get("localizedParameters").and_then(|l| l.as_object());
    let mut title = None;
    if let Some(lp) = localized {
        let preferred = lp.get("defaultLanguage").and_then(|d| d.as_str());
        if let Some(lang) = preferred.and_then(|l| lp.get(l)) {
            title = lang.get("titleName").and_then(|t| t.as_str()).map(str::to_string);
        }
        if title.is_none() {
            for (_lang, locale) in lp {
                if let Some(t) = locale.get("titleName").and_then(|t| t.as_str()) {
                    title = Some(t.to_string());
                    break;
                }
            }
        }
    }
    if title.is_none() {
        title = v.get("titleName").and_then(|t| t.as_str()).map(str::to_string);
    }
    title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());

    let version = ["contentVersion", "version"]
        .iter()
        .find_map(|k| v.get(k).and_then(|x| x.as_str()))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    Some(Param { title, version })
}

/// Exactly four uppercase letters then five digits, and not one of the console's own apps.
pub(crate) fn looks_like_title_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 9
        && b[..4].iter().all(u8::is_ascii_uppercase)
        && b[4..].iter().all(u8::is_ascii_digit)
}

/// The shell's own entries (`NPXS...`) and the helper's tile are not games.
fn is_console_app(id: &str) -> bool {
    id.starts_with("NPXS") || id == "PSUP00001"
}

/* ---- icons ------------------------------------------------------------------------------------ */

/// The icons of `ids`, one title at a time, as `data:` URLs. A title with no icon is left out.
pub fn icons(console: &Console, keys: &Keys, ids: &[String]) -> Result<Vec<GameIcon>, Ps5Error> {
    runtime().block_on(async {
        let session = open(console, &keys.checker, keys).await?;
        let mut out = Vec::new();
        for id in ids.iter().take(200) {
            if !looks_like_title_id(id) {
                continue;
            }
            match read_icon(&session, id).await {
                Ok(Some(bytes)) => out.push(GameIcon {
                    title_id: id.clone(),
                    data_url: data_url(&bytes),
                }),
                // No icon, or a console that stopped answering mid-way: the rest still show.
                _ => continue,
            }
        }
        session.close().await;
        Ok(out)
    })
}

/// `icon0.png` from appmeta, the app's `sce_sys`, or either extended drive.
async fn read_icon(s: &Session, title_id: &str) -> Result<Option<Vec<u8>>, MetaError> {
    for path in [
        format!("{APPMETA}/{title_id}/icon0.png"),
        format!("{APP}/{title_id}/sce_sys/icon0.png"),
        format!("{}/{title_id}/sce_sys/icon0.png", EXT_ROOTS[0]),
        format!("{}/{title_id}/sce_sys/icon0.png", EXT_ROOTS[1]),
    ] {
        match meta::read_all(s, &path, ICON_MAX).await {
            Ok(b) if !b.is_empty() => return Ok(Some(b)),
            Ok(_) => continue,
            Err(MetaError::NotFound) | Err(MetaError::Path) => continue,
            Err(MetaError::Network(e)) => return Err(MetaError::Network(e)),
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// A PNG or JPEG as a `data:` URL. Icons are one of the two; anything else is served as PNG and
/// the browser decides whether it can draw it.
fn data_url(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) { "image/jpeg" } else { "image/png" };
    format!("data:{mime};base64,{}", base64(bytes))
}

/// Standard base64, padded, for a `data:` URL. Small enough not to want a dependency.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/* ---- sizes ------------------------------------------------------------------------------------ */

/// What one title takes: its app folder, its update, and its add-ons, each walked on the console.
pub fn measure(console: &Console, keys: &Keys, title_id: &str) -> Result<Measured, Ps5Error> {
    if !looks_like_title_id(title_id) {
        return Err(Ps5Error::Failed("That isn't a title id".into()));
    }
    let id = title_id.to_string();
    runtime().block_on(async {
        let session = open(console, &keys.checker, keys).await?;
        let out = measure_on(&session, &id).await;
        session.close().await;
        out.map_err(meta::into_ps5)
    })
}

async fn measure_on(s: &Session, title_id: &str) -> Result<Measured, MetaError> {
    let app_dir = format!("{APP}/{title_id}");
    let patch_dir = format!("{PATCH}/{title_id}");
    let dlc_dir = format!("{ADDCONT}/{title_id}");
    let (app, update, dlc) =
        tokio::join!(meta::du(s, &app_dir), meta::du(s, &patch_dir), meta::du(s, &dlc_dir));
    let (app, update, dlc) = (or_zero(app)?, or_zero(update)?, or_zero(dlc)?);
    Ok(Measured {
        bytes: app.bytes + update.bytes + dlc.bytes,
        app_bytes: app.bytes,
        update_bytes: update.bytes,
        dlc_bytes: dlc.bytes,
        partial: app.partial || update.partial || dlc.partial,
    })
}

/// A folder that is not there takes no space: an update or add-on folder a title does not have.
fn or_zero(u: Result<meta::Usage, MetaError>) -> Result<meta::Usage, MetaError> {
    match u {
        Ok(u) => Ok(u),
        Err(MetaError::NotFound) => Ok(meta::Usage { bytes: 0, files: 0, dirs: 0, partial: false }),
        Err(e) => Err(e),
    }
}

/// A whole file from the console, up to `max` bytes: the post-send check for a tile and its art,
/// and anything else that has to look at one file without knowing its size.
pub fn read_file(
    console: &Console,
    keys: &Keys,
    path: &str,
    max: usize,
) -> Result<Vec<u8>, Ps5Error> {
    runtime().block_on(async {
        let session = open(console, &keys.checker, keys).await?;
        let out = meta::read_all(&session, path, max).await;
        session.close().await;
        out.map_err(meta::into_ps5)
    })
}

/// The room left where games go, for the Send step's "will this fit?".
pub fn free_space(console: &Console, keys: &Keys, path: &str) -> Result<meta::FreeSpace, Ps5Error> {
    runtime().block_on(async {
        let session = open(console, &keys.checker, keys).await?;
        let out = meta::freespace(&session, path).await;
        session.close().await;
        out.map_err(meta::into_ps5)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_param_s_json_names_the_title_in_its_default_language() {
        let p = r#"{"localizedParameters":{"defaultLanguage":"en-US",
            "en-US":{"titleName":"Ashen Tide"},"fr-FR":{"titleName":"Marée de cendre"}},
            "contentVersion":"01.050.000"}"#;
        let p = parse_param(p.as_bytes()).unwrap();
        assert_eq!(p.title.as_deref(), Some("Ashen Tide"));
        assert_eq!(p.version.as_deref(), Some("01.050.000"));
    }

    #[test]
    fn a_missing_default_language_uses_the_first_one_that_names_the_title() {
        let p = r#"{"localizedParameters":{"fr-FR":{"titleName":"Marée"}}}"#;
        assert_eq!(parse_param(p.as_bytes()).unwrap().title.as_deref(), Some("Marée"));
    }

    #[test]
    fn a_flat_title_name_still_works_and_blank_names_do_not() {
        assert_eq!(
            parse_param(br#"{"titleName":"  Ashen Tide "}"#).unwrap().title.as_deref(),
            Some("Ashen Tide")
        );
        assert_eq!(parse_param(br#"{"titleName":"   "}"#).unwrap().title, None);
        assert!(parse_param(b"not json").is_none());
    }

    #[test]
    fn title_ids_are_four_letters_and_five_digits() {
        assert!(looks_like_title_id("PPSA01234"));
        assert!(looks_like_title_id("CUSA00900"));
        assert!(!looks_like_title_id("PPSA0123"));
        assert!(!looks_like_title_id("ppsa01234"));
        assert!(!looks_like_title_id("PPSA0123X"));
        assert!(is_console_app("NPXS39041"));
        assert!(is_console_app("PSUP00001"));
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(&[0x89, b'P', b'N', b'G']), "iVBORw==");
    }
}
