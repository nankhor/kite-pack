//! Kite Pack's pipeline: what a job is, which steps it runs, in which order, and the rules
//! that keep every delete behind the check before it.

mod job;
mod options;
mod ports;
mod runner;
mod snapshot;
mod steps;
mod system_trash;

pub use ports::{
    InstallError, Installer, PackError, PackOutcome, PackRequest, Packer, Progress, SendError,
    Sender, Sleeper, ThreadSleeper, Trash, Unavailable,
};
pub use runner::{RunEvent, RunOutcome, Runner};
pub use system_trash::SystemTrash;
pub use job::{Job, JobStore};
pub use options::{Bundle, Level, Options, OptionsError, OutputFormat, SendTarget};
pub use snapshot::{Snapshot, SnapshotEntry};
pub use steps::{Step, StepKind, StepState, plan_steps};
