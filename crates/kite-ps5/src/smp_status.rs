//! What ShadowMount+ says about this console, for the DLC panel's badges.

use serde::Serialize;

/// ShadowMount+ answered, and what it knows of the game (when one was asked about).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmpStatus {
    pub host: String,
    pub port: u16,
    pub version: String,
    pub api_version: u32,
    /// The base is installed as a native package: ShadowMount+ will install DLC for it.
    pub installed: bool,
    /// The base is registered from a folder or image (mounted, not installed).
    pub base_installed: bool,
    /// `folder`, `image`, `pkg` ... empty when unknown.
    pub source_type: String,
}
