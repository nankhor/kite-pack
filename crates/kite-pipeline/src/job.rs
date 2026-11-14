use crate::options::{Options, OptionsError};
use crate::snapshot::Snapshot;
use crate::steps::{Step, StepState, plan_steps};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// One game's trip: its source, the options chosen, and where each step stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub source: PathBuf,
    pub options: Options,
    pub steps: Vec<Step>,
    /// The packed output, once Pack is done.
    pub packed: Option<PathBuf>,
    /// The original as it was before packing, when it is to be deleted.
    pub snapshot: Option<Snapshot>,
    pub created_unix: u64,
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Job {
    pub fn new(source: PathBuf, options: Options) -> Result<Job, OptionsError> {
        options.validate()?;
        let steps = plan_steps(&options)
            .into_iter()
            .map(|kind| Step { kind, state: StepState::Pending })
            .collect();
        Ok(Job {
            id: uuid::Uuid::new_v4().simple().to_string(),
            source,
            options,
            steps,
            packed: None,
            snapshot: None,
            created_unix: now_unix(),
        })
    }

    /// The first step that is not done.
    pub fn next_open(&self) -> Option<usize> {
        self.steps.iter().position(|s| !matches!(s.state, StepState::Done { .. }))
    }

    pub fn is_finished(&self) -> bool {
        self.next_open().is_none()
    }

    /// Puts step `index`, and every failed step after it, back to Pending.
    pub fn retry_from(&mut self, index: usize) {
        for (i, step) in self.steps.iter_mut().enumerate().skip(index) {
            if i == index || matches!(step.state, StepState::Failed { .. }) {
                step.state = StepState::Pending;
            }
        }
    }
}

/// Jobs saved as `<id>.json`, one file each, written through a temporary file and a rename.
#[derive(Debug, Clone)]
pub struct JobStore {
    dir: PathBuf,
}

impl JobStore {
    pub fn open(dir: PathBuf) -> io::Result<JobStore> {
        std::fs::create_dir_all(&dir)?;
        Ok(JobStore { dir })
    }

    pub fn save(&self, job: &Job) -> io::Result<()> {
        let path = self.dir.join(format!("{}.json", job.id));
        let tmp = self.dir.join(format!("{}.json.tmp", job.id));
        let bytes = serde_json::to_vec_pretty(job).map_err(io::Error::other)?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)
    }

    pub fn load(&self, id: &str) -> io::Result<Job> {
        let bytes = std::fs::read(self.dir.join(format!("{id}.json")))?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    /// Every job not finished yet, newest first, with steps that were running or waiting
    /// when the app stopped put back to Pending. Files that don't read as a job are skipped.
    pub fn unfinished(&self) -> Vec<Job> {
        let Ok(dir) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut jobs: Vec<Job> = dir
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| std::fs::read(p).ok())
            .filter_map(|bytes| serde_json::from_slice::<Job>(&bytes).ok())
            .filter(|job| !job.is_finished())
            .map(|mut job| {
                for step in &mut job.steps {
                    if matches!(step.state, StepState::Running | StepState::Waiting { .. }) {
                        step.state = StepState::Pending;
                    }
                }
                job
            })
            .collect();
        jobs.sort_by(|a, b| b.created_unix.cmp(&a.created_unix));
        jobs
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        std::fs::remove_file(self.dir.join(format!("{id}.json")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{Level, OutputFormat, SendTarget};
    use crate::steps::{StepKind, StepState, plan_steps};
    use std::fs;

    fn options(delete_original: bool, send: bool) -> Options {
        Options {
            format: OutputFormat::Folder,
            level: Level::Fast,
            output_dir: PathBuf::from("/out"),
            delete_original,
            send: send.then(|| SendTarget { console: "ps5".into(), destination: "/data/homebrew".into() }),
            delete_packed: false,
            dlc: vec![],
            bundle: Default::default(),
        }
    }

    fn states(job: &Job) -> Vec<StepState> {
        job.steps.iter().map(|s| s.state.clone()).collect()
    }

    #[test]
    fn new_job_has_pending_steps() {
        let o = options(true, true);
        let job = Job::new(PathBuf::from("/games/ashen"), o.clone()).unwrap();
        assert_eq!(job.steps.len(), plan_steps(&o).len());
        assert!(job.steps.iter().all(|s| s.state == StepState::Pending));
        assert_eq!(job.steps[0].kind, StepKind::Pack);
        assert_eq!(job.packed, None);
        assert_eq!(job.next_open(), Some(0));
    }

    #[test]
    fn invalid_options_are_refused() {
        let mut o = options(false, false);
        o.delete_packed = true;
        assert_eq!(
            Job::new(PathBuf::from("/g"), o).unwrap_err(),
            OptionsError::DeletePackedWithoutSend
        );
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JobStore::open(tmp.path().join("jobs")).unwrap();
        let job = Job::new(PathBuf::from("/games/ashen"), options(true, true)).unwrap();
        store.save(&job).unwrap();
        assert_eq!(store.load(&job.id).unwrap(), job);
    }

    #[test]
    fn save_leaves_no_tmp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JobStore::open(tmp.path().to_path_buf()).unwrap();
        let job = Job::new(PathBuf::from("/g"), options(false, false)).unwrap();
        store.save(&job).unwrap();
        store.save(&job).unwrap();
        let names: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![format!("{}.json", job.id)]);
    }

    #[test]
    fn unfinished_resets_running_and_waiting() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JobStore::open(tmp.path().to_path_buf()).unwrap();
        let mut job = Job::new(PathBuf::from("/g"), options(true, true)).unwrap();
        job.steps[0].state = StepState::Done { note: "ok".into() };
        job.steps[1].state = StepState::Running;
        job.steps[2].state = StepState::Waiting { since_unix: 7 };
        store.save(&job).unwrap();
        let loaded = store.unfinished();
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            states(&loaded[0]),
            vec![StepState::Done { note: "ok".into() }, StepState::Pending, StepState::Pending]
        );
    }

    #[test]
    fn unfinished_skips_finished_jobs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JobStore::open(tmp.path().to_path_buf()).unwrap();
        let mut job = Job::new(PathBuf::from("/g"), options(false, false)).unwrap();
        job.steps[0].state = StepState::Done { note: "ok".into() };
        assert!(job.is_finished());
        store.save(&job).unwrap();
        assert!(store.unfinished().is_empty());
    }

    #[test]
    fn unfinished_skips_corrupt_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JobStore::open(tmp.path().to_path_buf()).unwrap();
        let job = Job::new(PathBuf::from("/g"), options(false, false)).unwrap();
        store.save(&job).unwrap();
        fs::write(tmp.path().join("bad.json"), "{not json").unwrap();
        fs::write(tmp.path().join("newer.json"), r#"{"id":"x","format_from_the_future":1}"#).unwrap();
        let loaded = store.unfinished();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, job.id);
    }

    #[test]
    fn retry_from_resets_failed_steps() {
        let mut o = options(true, true);
        o.dlc = vec![PathBuf::from("/dlc/0.pkg")];
        let mut job = Job::new(PathBuf::from("/g"), o).unwrap();
        let failed = StepState::Failed { reason: "x".into() };
        job.steps[0].state = StepState::Done { note: "ok".into() };
        job.steps[1].state = failed.clone();
        job.steps[3].state = failed;
        job.retry_from(1);
        assert_eq!(
            states(&job),
            vec![
                StepState::Done { note: "ok".into() },
                StepState::Pending,
                StepState::Pending,
                StepState::Pending
            ]
        );
    }

    #[test]
    fn remove_deletes_the_job_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JobStore::open(tmp.path().to_path_buf()).unwrap();
        let job = Job::new(PathBuf::from("/g"), options(false, false)).unwrap();
        store.save(&job).unwrap();
        store.remove(&job.id).unwrap();
        assert!(store.load(&job.id).is_err());
    }
}
