use crate::options::Options;
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
    use crate::options::{Level, Options, OptionsError, OutputFormat, SendTarget};
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
        }
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
}
