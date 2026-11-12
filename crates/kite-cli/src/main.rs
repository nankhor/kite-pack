//! `kite-pack`: pack a PS5 game with zstd from the command line, with the same saved,
//! resumable pipeline the app uses. Sending to the PS5 arrives with the Kite helper.

use clap::{Parser, Subcommand, ValueEnum};
use kite_forge::{ForgePacker, inspect_game};
use kite_pipeline::{
    Job, JobStore, Level, Options, OutputFormat, Progress, RunEvent, RunOutcome, Runner,
    StepKind, StepState, SystemTrash, ThreadSleeper, Unavailable,
};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "kite-pack", version, about = "Pack a PS5 game with zstd")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what a game folder, image, package or archive holds.
    Inspect {
        path: PathBuf,
        /// Print it as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Pack a game with zstd.
    Pack {
        source: PathBuf,
        /// Folder the packed copy goes in.
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = FormatArg::Folder)]
        format: FormatArg,
        #[arg(long, value_enum, default_value_t = LevelArg::Fast)]
        level: LevelArg,
        /// Move the original to the Trash once the packed copy is checked.
        #[arg(long)]
        delete_original: bool,
    },
    /// List jobs that haven't finished.
    Jobs,
    /// Carry on with an unfinished job from its first open step.
    Resume { id: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum FormatArg {
    Folder,
    Exfat,
    Ffpkg,
    Ffpfs,
}

#[derive(Clone, Copy, ValueEnum)]
enum LevelArg {
    Fast,
    Medium,
    Max,
}

impl From<FormatArg> for OutputFormat {
    fn from(f: FormatArg) -> OutputFormat {
        match f {
            FormatArg::Folder => OutputFormat::Folder,
            FormatArg::Exfat => OutputFormat::Exfat,
            FormatArg::Ffpkg => OutputFormat::Ffpkg,
            FormatArg::Ffpfs => OutputFormat::Ffpfs,
        }
    }
}

impl From<LevelArg> for Level {
    fn from(l: LevelArg) -> Level {
        match l {
            LevelArg::Fast => Level::Fast,
            LevelArg::Medium => Level::Medium,
            LevelArg::Max => Level::Max,
        }
    }
}

/// Exit code for a source or job the tool refuses.
const REFUSED: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect { path, json } => inspect(path, json),
        Command::Pack { source, out, format, level, delete_original } => {
            pack(source, out, format.into(), level.into(), delete_original)
        }
        Command::Jobs => jobs(),
        Command::Resume { id } => resume(&id),
    }
}

fn store() -> Result<JobStore, ExitCode> {
    let data = std::env::var_os("KITE_PACK_DATA")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join("kite-pack")))
        .unwrap_or_else(|| PathBuf::from(".kite-pack"));
    JobStore::open(data.join("jobs")).map_err(|e| {
        eprintln!("Couldn't open the jobs folder: {e}");
        ExitCode::FAILURE
    })
}

fn inspect(path: PathBuf, json: bool) -> ExitCode {
    let info = match inspect_game(&path) {
        Ok(info) => info,
        Err(problem) => {
            eprintln!("{}", problem.message());
            return ExitCode::from(REFUSED);
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&info).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    println!("{}", info.title.as_deref().unwrap_or("Untitled game"));
    println!("Title ID  {}", info.title_id.as_deref().unwrap_or("unknown"));
    println!("Version   {}", info.version.as_deref().unwrap_or("unknown"));
    println!("Size      {}", size(info.total_bytes));
    if info.dlcs.is_empty() {
        println!("DLC       none inside");
    }
    for dlc in &info.dlcs {
        println!("DLC       {}, {}", dlc.name.as_deref().unwrap_or(&dlc.label), size(dlc.bytes));
    }
    for finding in &info.findings {
        println!("Note      {finding}");
    }
    ExitCode::SUCCESS
}

fn pack(source: PathBuf, out: PathBuf, format: OutputFormat, level: Level, delete_original: bool) -> ExitCode {
    if let Err(problem) = inspect_game(&source) {
        eprintln!("{}", problem.message());
        return ExitCode::from(REFUSED);
    }
    let options = Options {
        format,
        level,
        output_dir: out,
        delete_original,
        send: None,
        delete_packed: false,
        dlc: vec![],
    };
    let mut job = match Job::new(source, options) {
        Ok(job) => job,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(REFUSED);
        }
    };
    let store = match store() {
        Ok(s) => s,
        Err(code) => return code,
    };
    if let Err(e) = store.save(&job) {
        eprintln!("Couldn't save the job: {e}");
        return ExitCode::FAILURE;
    }
    run(&store, &mut job)
}

fn jobs() -> ExitCode {
    let store = match store() {
        Ok(s) => s,
        Err(code) => return code,
    };
    let jobs = store.unfinished();
    if jobs.is_empty() {
        println!("No unfinished jobs");
    }
    for job in jobs {
        let next = job.next_open().map(|i| {
            let step = &job.steps[i];
            format!("{}: {}", step_name(&step.kind), state_word(&step.state))
        });
        println!("{}  {}  {}", job.id, job.source.display(), next.unwrap_or_default());
    }
    ExitCode::SUCCESS
}

fn resume(id: &str) -> ExitCode {
    let store = match store() {
        Ok(s) => s,
        Err(code) => return code,
    };
    let Some(mut job) = store.unfinished().into_iter().find(|j| j.id == id) else {
        eprintln!("No unfinished job {id}");
        return ExitCode::from(REFUSED);
    };
    if let Some(next) = job.next_open() {
        job.retry_from(next);
    }
    run(&store, &mut job)
}

fn run(store: &JobStore, job: &mut Job) -> ExitCode {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let _ = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst));

    let names: Vec<String> = job.steps.iter().map(|s| step_name(&s.kind)).collect();
    let runner = Runner {
        packer: &ForgePacker { full_verify: false },
        trash: &SystemTrash,
        sender: &Unavailable,
        installer: &Unavailable,
        sleeper: &ThreadSleeper,
        store,
        retry_every: Duration::from_secs(5),
    };
    let outcome = runner.run(job, &cancel, &|event| match event {
        RunEvent::StepStarted { index } => println!("{}: started", names[index]),
        RunEvent::Progress { progress: Progress::Stage(stage), .. } => println!("  {stage}"),
        RunEvent::Progress { .. } => {}
        RunEvent::StepWaiting { index, .. } => println!("{}: waiting for the PS5", names[index]),
        RunEvent::StepDone { index, note } => println!("{}: done, {note}", names[index]),
        RunEvent::StepFailed { index, reason } => eprintln!("{}: failed, {reason}", names[index]),
        RunEvent::Cancelled { index } => eprintln!("{}: cancelled", names[index]),
        RunEvent::Finished => println!("Finished"),
    });
    match outcome {
        RunOutcome::Finished => {
            if let Some(packed) = &job.packed {
                println!("Packed copy: {}", packed.display());
            }
            ExitCode::SUCCESS
        }
        RunOutcome::Failed { .. } | RunOutcome::Cancelled { .. } => {
            eprintln!("Job {} can carry on later: kite-pack resume {}", job.id, job.id);
            ExitCode::FAILURE
        }
    }
}

fn step_name(kind: &StepKind) -> String {
    match kind {
        StepKind::Pack => "Pack".into(),
        StepKind::DeleteOriginal => "Delete the original".into(),
        StepKind::Send => "Send to the PS5".into(),
        StepKind::InstallDlc { pkg } => format!(
            "Install DLC {}",
            pkg.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        ),
        StepKind::DeletePacked => "Delete the packed folder".into(),
    }
}

fn state_word(state: &StepState) -> String {
    match state {
        StepState::Pending => "not started".into(),
        StepState::Running => "running".into(),
        StepState::Waiting { .. } => "waiting for the PS5".into(),
        StepState::Done { note } => format!("done, {note}"),
        StepState::Failed { reason } => format!("failed, {reason}"),
    }
}

/// Decimal sizes, as Forge prints them.
fn size(bytes: u64) -> String {
    const UNITS: [(&str, f64); 3] = [("GB", 1e9), ("MB", 1e6), ("KB", 1e3)];
    for (unit, scale) in UNITS {
        if bytes as f64 >= scale {
            return format!("{:.1} {unit}", bytes as f64 / scale);
        }
    }
    format!("{bytes} bytes")
}
