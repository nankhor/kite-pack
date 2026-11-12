// The screen's words and numbers, worked out from plain data so they can be tested.

import type { JobView, ProgressState, StepKind, StepView } from "./types";

export interface StepRow {
  label: string;
  status: string;
  state: "done" | "active" | "paused" | "pending" | "failed";
  /** 0 to 100 for steps that have a bar, null for the rest. */
  bar: number | null;
}

export type NoteInput =
  | { kind: "ready"; outputDir: string }
  | { kind: "running" | "failed" | "done"; job: JobView };

/** Decimal sizes, as drives and Forge count them: "84.2 GB", "717 KB". */
export function formatSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${Math.round(bytes / 1e3)} KB`;
  return `${bytes} bytes`;
}

/** `contentVersion` "01.050.000" → "Update 1.05"; 1.00 or nothing → no update. */
export function updateLabel(version: string | null): string {
  const m = version?.match(/^(\d+)\.(\d+)/);
  if (!m) return "Base game, no update";
  const major = Number(m[1]);
  const minor = m[2].slice(0, 2).padEnd(2, "0");
  if (major <= 1 && Number(minor) === 0) return "Base game, no update";
  return `Update ${major}.${minor}`;
}

const LABELS: Record<StepKind, string> = {
  pack: "Pack",
  deleteOriginal: "Delete the original",
  send: "Send to the PS5",
  installDlc: "Install DLC",
  deletePacked: "Delete the packed folder",
};

/** Forge's stage names, in the screen's words. */
const STAGES: Record<string, string> = {
  scan: "Reading the game",
  preflight: "Checking",
  measure: "Measuring",
  write: "Writing",
  pack: "Packing",
  verify: "Checking the packed copy",
  finalize: "Finishing",
};

/** Minutes left at the rate since the first bytes arrived; null before there is a rate. */
export function minutesLeft(p: ProgressState, now: number): number | null {
  if (p.startedAt === null || p.total <= 0) return null;
  const elapsed = now - p.startedAt;
  const moved = p.done - p.startedDone;
  if (elapsed <= 0 || moved <= 0) return null;
  const msLeft = (p.total - p.done) / (moved / elapsed);
  return Math.max(1, Math.ceil(msLeft / 60_000));
}

function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function runningStatus(step: StepView, p: ProgressState | null, now: number): [string, number | null] {
  const hasBar = step.kind === "pack" || step.kind === "send";
  if (p && p.total > 0) {
    const pct = Math.min(100, Math.floor((p.done / p.total) * 100));
    const mins = minutesLeft(p, now);
    return [mins === null ? `${pct}%` : `${pct}%, about ${mins} min left`, hasBar ? pct : null];
  }
  if (step.kind === "deleteOriginal" || step.kind === "deletePacked") return ["Moving to the Trash", null];
  const words = p?.stage ? (STAGES[p.stage] ?? "Working") : "Starting";
  return [words, hasBar ? 0 : null];
}

function pendingStatus(kind: StepKind): string {
  switch (kind) {
    case "deleteOriginal":
      return "After the packed copy is checked";
    case "deletePacked":
      return "After the PS5 confirms every file";
    case "pack":
      return "Waiting to start";
    default:
      return "After packing";
  }
}

function doneStatus(step: StepView, job: JobView): string {
  switch (step.kind) {
    case "pack":
      return job.packedBytes !== null
        ? `${formatSize(job.packedBytes)} in ${folderName(job.outputDir)}`
        : "Packed and checked";
    case "deleteOriginal":
    case "deletePacked":
      return "In the Trash";
    default:
      return step.note ?? "Done";
  }
}

/** One row per step of the job, in the order they run. */
export function stepRows(job: JobView, progress: ProgressState | null, now: number): StepRow[] {
  return job.steps.map((step, index) => {
    const label = LABELS[step.kind];
    const hasBar = step.kind === "pack" || step.kind === "send";
    switch (step.state) {
      case "done":
        return { label, state: "done", status: doneStatus(step, job), bar: hasBar ? 100 : null };
      case "failed":
        return { label, state: "failed", status: step.note ?? "Failed", bar: null };
      case "waiting":
        return { label, state: "paused", status: "Waiting for the PS5", bar: null };
      case "running": {
        const p = progress && progress.index === index ? progress : null;
        const [status, bar] = runningStatus(step, p, now);
        return { label, state: "active", status, bar };
      }
      default:
        return { label, state: "pending", status: pendingStatus(step.kind), bar: null };
    }
  });
}

/** The line beside the buttons. */
export function footerNote(input: NoteInput): string {
  switch (input.kind) {
    case "ready":
      return `Packs into ${input.outputDir}.`;
    case "running":
      return "You can leave this running. If you close Kite Pack, it carries on next time.";
    case "failed": {
      const step = input.job.steps.find((s) => s.state === "failed");
      return step ? `${LABELS[step.kind]} failed: ${step.note ?? "no reason given"}` : "Something stopped the job.";
    }
    case "done":
      return input.job.deleteOriginal
        ? "The original is in the Trash until you empty it. Play the game once first."
        : "Nothing was deleted.";
  }
}
