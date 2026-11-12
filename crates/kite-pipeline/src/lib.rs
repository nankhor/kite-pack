//! Kite Pack's pipeline: what a job is, which steps it runs, in which order, and the rules
//! that keep every delete behind the check before it.

mod job;
mod options;
mod snapshot;
mod steps;

pub use job::{Job, JobStore};
pub use options::{Level, Options, OptionsError, OutputFormat, SendTarget};
pub use snapshot::{Snapshot, SnapshotEntry};
pub use steps::{Step, StepKind, StepState, plan_steps};
