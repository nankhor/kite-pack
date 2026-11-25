use crate::options::{Options, OutputFormat};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One thing a job does. Which ones a job has depends on the toggles ([`plan_steps`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Pack with zstd, then Forge's check of the output.
    Pack,
    DeleteOriginal,
    Send,
    InstallDlc { pkg: PathBuf },
    DeletePacked,
    /// A package installed on the PS5 by its installer: the packed copy when the job packs one
    /// (Convert), otherwise the source as it is.
    InstallPackage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StepState {
    Pending,
    Running,
    Waiting { since_unix: u64 },
    Done { note: String },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub kind: StepKind,
    pub state: StepState,
}

/// The steps `options` asks for, in the order they run. Validate `options` first.
pub fn plan_steps(options: &Options) -> Vec<StepKind> {
    if options.format == OutputFormat::Pkg {
        // Convert: the package is installed by the PS5's installer, never sent to a folder.
        let mut steps = vec![StepKind::Pack];
        if options.install {
            steps.push(StepKind::InstallPackage);
            if options.delete_packed {
                steps.push(StepKind::DeletePacked);
            }
        }
        return steps;
    }
    if options.install {
        return vec![StepKind::InstallPackage];
    }
    let mut steps = vec![StepKind::Pack];
    if options.delete_original {
        steps.push(StepKind::DeleteOriginal);
    }
    if options.send.is_some() {
        steps.push(StepKind::Send);
        steps.extend(options.dlc.iter().map(|pkg| StepKind::InstallDlc { pkg: pkg.clone() }));
        if options.delete_packed {
            steps.push(StepKind::DeletePacked);
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{Compression, Level, Options, OptionsError, OutputFormat, SendTarget};
    use std::path::PathBuf;
    use StepKind::*;

    fn opts(delete_original: bool, send: bool, delete_packed: bool, dlc: usize) -> Options {
        Options {
            format: OutputFormat::Folder,
            level: Level::Fast,
            output_dir: PathBuf::from("/out"),
            delete_original,
            send: send.then(|| SendTarget {
                console: "ps5".into(),
                destination: "/data/homebrew".into(),
            }),
            delete_packed,
            dlc: (0..dlc).map(|i| PathBuf::from(format!("/dlc/{i}.pkg"))).collect(),
            bundle: Default::default(),
            install: false,
            compression: Default::default(),
        }
    }

    /// A dump turned into a .fpkg: installing needs the PS5, so it brings `send`.
    fn convert(install: bool, delete_packed: bool) -> Options {
        let mut o = opts(false, install, delete_packed, 0);
        o.format = OutputFormat::Pkg;
        o.install = install;
        o
    }

    #[test]
    fn pack_only() {
        assert_eq!(plan_steps(&opts(false, false, false, 0)), vec![Pack]);
    }

    #[test]
    fn pack_and_delete_original() {
        assert_eq!(plan_steps(&opts(true, false, false, 0)), vec![Pack, DeleteOriginal]);
    }

    #[test]
    fn send_without_delete_packed() {
        assert_eq!(plan_steps(&opts(false, true, false, 0)), vec![Pack, Send]);
    }

    #[test]
    fn send_and_delete_packed() {
        assert_eq!(plan_steps(&opts(false, true, true, 0)), vec![Pack, Send, DeletePacked]);
    }

    #[test]
    fn every_toggle_with_two_dlc() {
        assert_eq!(
            plan_steps(&opts(true, true, true, 2)),
            vec![
                Pack,
                DeleteOriginal,
                Send,
                InstallDlc { pkg: "/dlc/0.pkg".into() },
                InstallDlc { pkg: "/dlc/1.pkg".into() },
                DeletePacked,
            ]
        );
    }

    #[test]
    fn delete_original_and_send() {
        assert_eq!(
            plan_steps(&opts(true, true, false, 1)),
            vec![Pack, DeleteOriginal, Send, InstallDlc { pkg: "/dlc/0.pkg".into() }]
        );
    }

    #[test]
    fn delete_packed_needs_send() {
        assert_eq!(
            opts(false, false, true, 0).validate(),
            Err(OptionsError::DeletePackedWithoutSend)
        );
    }

    #[test]
    fn dlc_needs_send() {
        assert_eq!(opts(false, false, false, 1).validate(), Err(OptionsError::DlcWithoutSend));
    }

    #[test]
    fn valid_options_pass() {
        assert_eq!(opts(true, true, true, 2).validate(), Ok(()));
    }

    #[test]
    fn levels_match_forge_presets() {
        assert_eq!(
            (Level::Fast.zstd_level(), Level::Medium.zstd_level(), Level::Max.zstd_level()),
            (5, 9, 19)
        );
    }

    #[test]
    fn step_state_json_shape() {
        let json = serde_json::to_string(&StepState::Failed { reason: "x".into() }).unwrap();
        assert_eq!(json, r#"{"state":"failed","reason":"x"}"#);
    }

    #[test]
    fn an_install_job_has_one_step() {
        let mut o = opts(true, true, true, 2);
        o.install = true;
        assert_eq!(plan_steps(&o), vec![InstallPackage]);
        assert_eq!(o.validate(), Ok(()));
        o.send = None;
        o.delete_packed = false;
        o.dlc.clear();
        assert_eq!(o.validate(), Err(OptionsError::InstallWithoutPs5));
    }

    #[test]
    fn convert_only() {
        assert_eq!(plan_steps(&convert(false, false)), vec![Pack]);
        assert_eq!(convert(false, false).validate(), Ok(()));
    }

    #[test]
    fn convert_and_install() {
        assert_eq!(plan_steps(&convert(true, false)), vec![Pack, InstallPackage]);
        assert_eq!(convert(true, false).validate(), Ok(()));
    }

    #[test]
    fn convert_install_and_delete() {
        assert_eq!(plan_steps(&convert(true, true)), vec![Pack, InstallPackage, DeletePacked]);
        assert_eq!(convert(true, true).validate(), Ok(()));
    }

    #[test]
    fn a_converted_package_is_deleted_only_after_installing() {
        let mut o = convert(false, true);
        o.send = Some(SendTarget { console: "ps5".into(), destination: "/data/homebrew".into() });
        assert_eq!(o.validate(), Err(OptionsError::DeletePackageWithoutInstall));
    }

    #[test]
    fn a_converted_package_is_never_sent_to_a_folder() {
        let mut o = convert(false, false);
        o.send = Some(SendTarget { console: "ps5".into(), destination: "/data/homebrew".into() });
        o.delete_original = true;
        assert_eq!(plan_steps(&o), vec![Pack]);
    }

    #[test]
    fn install_only_is_unchanged() {
        let mut o = opts(false, true, false, 0);
        o.install = true;
        assert_eq!(plan_steps(&o), vec![InstallPackage]);
    }

    #[test]
    fn a_package_is_compressed_fast_unless_asked_otherwise() {
        let o: Options = serde_json::from_value(serde_json::json!({
            "format": "pkg", "level": "fast", "output_dir": "/out", "delete_original": false,
            "send": null, "delete_packed": false, "dlc": []
        }))
        .unwrap();
        assert_eq!((o.format, o.compression), (OutputFormat::Pkg, Compression::Fast));
    }

    #[test]
    fn compression_levels_have_their_screen_names() {
        let names: Vec<String> = [Compression::Fast, Compression::Balanced, Compression::Smallest]
            .iter()
            .map(|c| serde_json::to_value(c).unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["fast", "balanced", "smallest"]);
    }

    #[test]
    fn a_package_is_never_stored_raw() {
        let raw: Result<Compression, _> = serde_json::from_value(serde_json::json!("off"));
        assert!(raw.is_err());
    }
}
