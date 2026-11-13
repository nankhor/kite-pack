//! What a dropped file is, in the words the screen uses, read by Forge's `inspect`.

use ps5_dump_forge_core::{Dlc, inspect};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What the screen shows about a game before packing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    /// `folder`, `exfat`, `ffpkg`, `ffpfs`, `ffpfsc`, `pkg`, `zip`, `7z` or `rar`.
    pub kind: String,
    pub title: Option<String>,
    pub title_id: Option<String>,
    pub content_id: Option<String>,
    /// `contentVersion`, e.g. `01.050.000`; above 1.000 an update is merged in.
    pub version: Option<String>,
    pub total_bytes: u64,
    /// DLC already inside the dump.
    pub dlcs: Vec<DlcInfo>,
    /// `sce_sys/icon0.png` as a `data:` URL.
    pub icon_data_url: Option<String>,
    /// Forge's preflight findings, in Forge's words.
    pub findings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcInfo {
    pub content_id: String,
    /// The last 16 characters of the content ID, which tell a game's DLCs apart.
    pub label: String,
    pub name: Option<String>,
    pub bytes: u64,
}

/// Why a dropped file can't be packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceProblem {
    NotAGame,
    AlreadyPacked,
    Ps4Package,
    RetailPackage,
    PatchPackage,
    /// A fake package made with a passcode other than the default debug one.
    OtherPasscode,
    /// A fake package whose compression Forge doesn't decode.
    UnsupportedPackage,
    /// A package that is truncated or damaged.
    DamagedPackage,
    /// A `.pkg` that isn't a PS5 package at all.
    NotAPs5Package,
    Unreadable(String),
}

impl SourceProblem {
    pub fn message(&self) -> String {
        match self {
            SourceProblem::NotAGame => "This is an update or a DLC on its own, not a game".into(),
            SourceProblem::AlreadyPacked => "Already packed".into(),
            SourceProblem::Ps4Package => "PS4 games can't be packed".into(),
            SourceProblem::RetailPackage => "Retail packages can't be read".into(),
            SourceProblem::PatchPackage => "This is an update package, not a game".into(),
            SourceProblem::OtherPasscode => {
                "This package was made with a passcode other than the default, so it can't be read".into()
            }
            SourceProblem::UnsupportedPackage => "This package uses a compression Kite Pack can't read yet".into(),
            SourceProblem::DamagedPackage => "This package is damaged or incomplete".into(),
            SourceProblem::NotAPs5Package => "This isn't a PS5 package".into(),
            SourceProblem::Unreadable(why) => why.clone(),
        }
    }
}

impl From<&Dlc> for DlcInfo {
    fn from(d: &Dlc) -> DlcInfo {
        DlcInfo { content_id: d.content_id.clone(), label: d.label.clone(), name: d.name.clone(), bytes: d.bytes }
    }
}

/// Forge's error in the screen's words. A `.pkg` is read by Forge's FPKG reader, whose errors
/// start with a fixed kind (`ps5-dump-forge-fpkg`: "retail package", "wrong passcode", ...),
/// after the file's path; anything else keeps Forge's own message.
fn problem(text: &str) -> SourceProblem {
    let has = |kind: &str| text.contains(&format!("{kind}:"));
    if text.contains("this is a PS4 package") {
        SourceProblem::Ps4Package
    } else if has("retail package") {
        SourceProblem::RetailPackage
    } else if has("wrong passcode") || has("native-encrypted image this reader cannot decode") {
        SourceProblem::OtherPasscode
    } else if has("unsupported codec") {
        SourceProblem::UnsupportedPackage
    } else if has("corrupt package") {
        SourceProblem::DamagedPackage
    } else if has("not a PS5 package") {
        SourceProblem::NotAPs5Package
    } else {
        SourceProblem::Unreadable(text.to_string())
    }
}

/// Reads `path` and says whether it is a PS5 game Kite Pack can pack.
pub fn inspect_game(path: &Path) -> Result<GameInfo, SourceProblem> {
    let found = inspect(path).map_err(|e| problem(&format!("{e:#}")))?;
    if found.patch {
        return Err(SourceProblem::PatchPackage);
    }
    if found.additional_content {
        return Err(SourceProblem::NotAGame);
    }
    if found.zstd_packed {
        return Err(SourceProblem::AlreadyPacked);
    }
    Ok(GameInfo {
        kind: found.kind,
        title: found.title_name,
        title_id: found.title_id,
        content_id: found.content_id,
        version: found.version,
        total_bytes: found.total_bytes,
        dlcs: found.dlcs.iter().map(DlcInfo::from).collect(),
        icon_data_url: found.cover,
        findings: found.findings,
    })
}

/// Checks that `path` (a DLC `.pkg`, or a DLC folder) is DLC for the game `title_id`.
pub fn check_dlc(path: &Path, title_id: &str) -> Result<DlcInfo, String> {
    let found = inspect(path).map_err(|e| format!("{e:#}"))?;
    let not_ours = || format!("This isn't DLC for {title_id}");
    if !found.additional_content || found.title_id.as_deref() != Some(title_id) {
        return Err(not_ours());
    }
    let content_id = found.content_id.ok_or_else(not_ours)?;
    let label = content_id.chars().rev().take(16).collect::<Vec<_>>().into_iter().rev().collect();
    Ok(DlcInfo { content_id, label, name: found.title_name, bytes: found.total_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_errors_in_plain_words() {
        let at = |kind: &str| problem(&format!("C:\\Games\\x.pkg: {kind}: details"));
        assert_eq!(problem("C:\\x.pkg: not a PS5 package: this is a PS4 package (\\x7FCNT at offset 0)"), SourceProblem::Ps4Package);
        assert_eq!(at("retail package"), SourceProblem::RetailPackage);
        assert_eq!(at("wrong passcode"), SourceProblem::OtherPasscode);
        assert_eq!(at("native-encrypted image this reader cannot decode"), SourceProblem::OtherPasscode);
        assert_eq!(at("unsupported codec"), SourceProblem::UnsupportedPackage);
        assert_eq!(at("corrupt package"), SourceProblem::DamagedPackage);
        assert_eq!(at("not a PS5 package"), SourceProblem::NotAPs5Package);
        assert_eq!(problem("x: no param.json"), SourceProblem::Unreadable("x: no param.json".into()));
    }
}
