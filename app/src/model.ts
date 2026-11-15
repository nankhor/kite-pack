// The screen's words and numbers, worked out from plain data so they can be tested.

import { baseName } from "./paths";
import type { GameInfo, HistoryEntry, JobView, ProgressState, StepKind, StepView } from "./types";

export interface StepRow {
  label: string;
  status: string;
  state: "done" | "active" | "paused" | "pending" | "failed";
  /** 0 to 100 for steps that have a bar, null for the rest. */
  bar: number | null;
  /** For the DLC row: each package and how it stands. */
  items?: DlcItem[];
}

export interface DlcItem {
  name: string;
  state: "waiting" | "installing" | "installed" | "failed";
  /** Why it failed. */
  note: string | null;
}

/** The Pack tab's little note while a job is on: how far, or how it ended. */
export interface Badge {
  text: string;
  tone: "mint" | "amber";
}

export type NoteInput =
  | { kind: "ready"; outputDir: string; send?: boolean; bundled?: number }
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
  send: "Send to PS5",
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

/** Bytes a second over the recent progress; null until it spans a second. */
export function speed(p: ProgressState): number | null {
  const first = p.recent[0];
  const last = p.recent[p.recent.length - 1];
  if (!first || !last || last.at - first.at < 1_000) return null;
  return Math.max(0, Math.round(((last.done - first.done) / (last.at - first.at)) * 1_000));
}

/** "38.2 MB/s", in the same units as the sizes. */
export function formatSpeed(bytesPerSecond: number): string {
  return `${formatSize(bytesPerSecond)}/s`;
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
    const rate = step.kind === "send" ? speed(p) : null;
    const parts = [`${pct}%`];
    if (rate !== null) parts.push(formatSpeed(rate));
    if (mins !== null) parts.push(`about ${mins} min left`);
    return [parts.join(", "), hasBar ? pct : null];
  }
  if (step.kind === "deleteOriginal" || step.kind === "deletePacked") return ["Moving to the Trash", null];
  const words = p?.stage ? (STAGES[p.stage] ?? "Working") : "Starting";
  return [words, hasBar ? 0 : null];
}

function pendingStatus(kind: StepKind, withDlc: boolean): string {
  switch (kind) {
    case "deleteOriginal":
      return "After the packed copy is checked";
    case "deletePacked":
      return withDlc ? "After the PS5 confirms the game and every DLC" : "After the PS5 confirms every file";
    case "pack":
      return "Waiting to start";
    case "send":
      return "Starts when packing ends";
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
    case "send":
      return "Every file checked";
    default:
      return step.note ?? "Done";
  }
}

/** One row for one step. */
function stepRow(step: StepView, index: number, job: JobView, progress: ProgressState | null, now: number): StepRow {
  const label = LABELS[step.kind];
  const hasBar = step.kind === "pack" || step.kind === "send";
  switch (step.state) {
    case "done":
      return { label, state: "done", status: doneStatus(step, job), bar: hasBar ? 100 : null };
    case "failed":
      return { label, state: "failed", status: step.note ?? "Failed", bar: null };
    case "waiting": {
      // Paused where it stopped: the last progress this step reported.
      const p = progress && progress.index === index && progress.total > 0 ? progress : null;
      const pct = p ? Math.min(100, Math.floor((p.done / p.total) * 100)) : null;
      return {
        label,
        state: "paused",
        status: pct === null ? "Waiting for the PS5" : `Paused at ${pct}%`,
        bar: hasBar ? (pct ?? 0) : null,
      };
    }
    case "running": {
      const p = progress && progress.index === index ? progress : null;
      const [status, bar] = runningStatus(step, p, now);
      return { label, state: "active", status, bar };
    }
    default:
      return {
        label,
        state: "pending",
        status: pendingStatus(step.kind, job.steps.some((st) => st.kind === "installDlc")),
        bar: step.kind === "send" ? 0 : null,
      };
  }
}

const ITEM_STATES: Record<StepView["state"], DlcItem["state"]> = {
  pending: "waiting",
  running: "installing",
  waiting: "waiting",
  done: "installed",
  failed: "failed",
};

/** A DLC's name: from its package when known, else its file name. */
function dlcName(step: StepView, names: Record<string, string>): string {
  const pkg = step.pkg ?? "";
  return names[pkg] ?? (baseName(pkg).replace(/\.pkg$/i, "") || "DLC");
}

/** Every DLC step as one row, counting through them; the list holds each one. */
function dlcRow(steps: StepView[], names: Record<string, string>): StepRow {
  const label = LABELS.installDlc;
  const n = steps.length;
  const items = steps.map((st) => ({
    name: dlcName(st, names),
    state: ITEM_STATES[st.state],
    note: st.state === "failed" ? st.note : null,
  }));
  const done = steps.filter((st) => st.state === "done").length;
  const failed = steps.filter((st) => st.state === "failed").length;
  const failedNote = failed ? ` · ${failed} failed` : "";
  const bar = Math.floor(((done + failed) / n) * 100);
  const current = steps.findIndex((st) => st.state === "running" || st.state === "waiting");
  if (current >= 0) {
    const at = `${current + 1} of ${n}`;
    if (steps[current].state === "waiting") {
      return { label, state: "paused", status: `Waiting for the PS5 at ${at}${failedNote}`, bar, items };
    }
    return { label, state: "active", status: `${at}: ${items[current].name}${failedNote}`, bar, items };
  }
  if (done + failed === n) {
    if (failed) return { label, state: "failed", status: `${failed} of ${n} failed`, bar: null, items };
    return { label, state: "done", status: n === 1 ? "Installed" : `${n} installed`, bar: null, items };
  }
  if (done + failed > 0) {
    return { label, state: "pending", status: `${done} of ${n} installed${failedNote}`, bar, items };
  }
  return { label, state: "pending", status: `${n} DLC, after the game is on the PS5`, bar: null, items };
}

/**
 * One row per step of the job, in the order they run. The DLC steps share one row, however
 * many there are; `names` gives each package's name by its path.
 */
export function stepRows(
  job: JobView,
  progress: ProgressState | null,
  now: number,
  names: Record<string, string> = {},
): StepRow[] {
  const dlc = job.steps.filter((st) => st.kind === "installDlc");
  const rows: StepRow[] = [];
  job.steps.forEach((step, index) => {
    if (step.kind !== "installDlc") rows.push(stepRow(step, index, job, progress, now));
    else if (step === dlc[0]) rows.push(dlcRow(dlc, names));
  });
  return rows;
}

/** The Pack tab's note: shown from the other tabs too, so a running job is never out of sight. */
export function packBadge(job: JobView | null, progress: ProgressState | null): Badge | null {
  if (!job || job.state === "cancelled") return null;
  if (job.state === "finished") return { text: "Done", tone: "mint" };
  if (job.state === "failed") return { text: "Failed", tone: "amber" };
  const index = job.steps.findIndex((st) => st.state === "running" || st.state === "waiting");
  if (index < 0) return job.state === "paused" ? { text: "Paused", tone: "amber" } : null;
  const step = job.steps[index];
  if (step.state === "waiting") return { text: "Paused", tone: "amber" };
  if (step.kind === "installDlc") {
    const dlc = job.steps.filter((st) => st.kind === "installDlc");
    return { text: `DLC ${dlc.indexOf(step) + 1}/${dlc.length}`, tone: "mint" };
  }
  if (progress && progress.index === index && progress.total > 0) {
    return { text: `${Math.min(100, Math.floor((progress.done / progress.total) * 100))}%`, tone: "mint" };
  }
  return { text: "Working", tone: "mint" };
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "Today, 10:42", "Yesterday, 23:05", then "12 Nov", with the year when it isn't this one. */
export function formatWhen(at: number, now: number): string {
  const d = new Date(at);
  const n = new Date(now);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((day(n) - day(d)) / 86_400_000);
  const time = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (days === 0) return `Today, ${time}`;
  if (days === 1) return `Yesterday, ${time}`;
  const date = `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return d.getFullYear() === n.getFullYear() ? date : `${date} ${d.getFullYear()}`;
}

/** What History keeps of a finished job. */
export function historyEntry(job: JobView, game: GameInfo, now: number, bundled = 0): HistoryEntry {
  const done = (kind: StepKind) => job.steps.some((st) => st.kind === kind && st.state === "done");
  return {
    id: job.id,
    title: game.title ?? game.titleId ?? baseName(job.source),
    titleId: game.titleId,
    finishedAt: now,
    sourceBytes: game.totalBytes,
    packedBytes: job.packedBytes,
    sent: done("send"),
    packed: job.packed && !done("deletePacked") ? job.packed : null,
    dlc: bundled + job.steps.filter((st) => st.kind === "installDlc" && st.state === "done").length,
    iconDataUrl: game.iconDataUrl,
  };
}

/** The line beside the buttons. */
export function footerNote(input: NoteInput): string {
  switch (input.kind) {
    case "ready": {
      const inside = input.bundled ? ` with ${input.bundled} ${input.bundled === 1 ? "add-on" : "add-ons"} inside` : "";
      return input.send
        ? `Packs into ${input.outputDir}${inside}, then sends to the PS5 by itself.`
        : `Packs into ${input.outputDir}${inside}.`;
    }
    case "running": {
      const steps = input.job.steps;
      if (steps.some((s) => s.state === "waiting")) {
        return "The PS5 isn't reachable. Turn it on and start the Kite helper; sending picks up where it stopped.";
      }
      if (steps.some((s) => s.kind === "send" && s.state === "running")) {
        return "Each file is checked on the PS5 as it lands.";
      }
      if (steps.some((s) => s.kind === "installDlc" && s.state === "running")) {
        return "The PS5 installs each DLC with its own installer. If one fails, the rest still go.";
      }
      return "You can leave this running. If you close Kite Pack, it carries on next time.";
    }
    case "failed": {
      const step = input.job.steps.find((s) => s.state === "failed");
      return step ? `${LABELS[step.kind]} failed: ${step.note ?? "no reason given"}` : "Something stopped the job.";
    }
    case "done": {
      if (input.job.steps.some((s) => s.kind === "deletePacked" && s.state === "done")) {
        return "Deleted folders stay in the Trash until you empty it. Play the game once first.";
      }
      return input.job.deleteOriginal
        ? "The original is in the Trash until you empty it. Play the game once first."
        : "Nothing was deleted.";
    }
  }
}

/** One part of the job's bar: one step, or all the DLC together. */
export interface BarPart {
  kind: StepKind;
  label: string;
  /** Its share of the bar: about how long the step takes, never too thin to read. */
  weight: number;
  /** 0 to 100; null while it runs with nothing to count (a move to the Trash). */
  fill: number | null;
  state: StepRow["state"];
  /** The short line under the part. */
  note: string;
  /** For the DLC part: each package and how it stands. */
  items?: DlcItem[];
}

/** The job as one bar: the line above it, and its parts. */
export interface JobBar {
  head: string;
  detail: string;
  tone: "plain" | "amber" | "mint" | "fail";
  /** The whole job, 0 to 100. */
  overall: number;
  parts: BarPart[];
}

const PART_LABELS: Record<StepKind, string> = {
  pack: "Pack",
  deleteOriginal: "Original to the Trash",
  send: "Send to PS5",
  installDlc: "Install DLC",
  deletePacked: "Packed copy to the Trash",
};

const PART_WEIGHTS: Record<StepKind, number> = { pack: 38, deleteOriginal: 10, send: 34, installDlc: 16, deletePacked: 10 };

const DOING: Record<StepKind, string> = {
  pack: "Packing",
  deleteOriginal: "Moving the original to the Trash",
  send: "Sending to PS5",
  installDlc: "Installing DLC",
  deletePacked: "Moving the packed copy to the Trash",
};

/**
 * The job as one bar, its parts the job's own steps (the DLC as one). `names` gives each DLC
 * package's name by path; `inside` is how many add-ons were folded into the packed copy.
 */
export function jobBar(
  job: JobView,
  progress: ProgressState | null,
  now: number,
  { names = {}, inside = 0 }: { names?: Record<string, string>; inside?: number } = {},
): JobBar {
  const rows = stepRows(job, progress, now, names);
  // The kind of each row: the steps in order, the DLC steps counted once, as stepRows does.
  const kinds = job.steps.map((st) => st.kind).filter((k, i, all) => k !== "installDlc" || all.indexOf(k) === i);
  const parts: BarPart[] = rows.map((row, i) => {
    const kind = kinds[i];
    const index = job.steps.findIndex((st) => st.kind === kind);
    const p = progress && progress.index === index && progress.total > 0 ? progress : null;
    let fill: number | null = row.state === "done" || row.state === "failed" ? 100 : (row.bar ?? 0);
    let note = row.status;
    if (row.state === "done" && kind === "pack" && inside > 0) {
      note = `${row.status}, ${inside} ${inside === 1 ? "add-on" : "add-ons"} inside`;
    } else if (row.state === "active") {
      if (kind === "pack") note = p ? `${row.bar}% packed` : "";
      else if (kind === "send") note = p ? `${formatSize(p.done)} of ${formatSize(p.total)}` : "";
      else if (kind === "deleteOriginal" || kind === "deletePacked") {
        fill = null;
        note = "";
      }
    } else if (row.state === "paused") {
      note = "Paused";
    }
    return { kind, label: PART_LABELS[kind], weight: PART_WEIGHTS[kind], fill, state: row.state, note, items: row.items };
  });

  const total = parts.reduce((t, part) => t + part.weight, 0) || 1;
  const filled = parts.reduce((t, part) => t + part.weight * (part.fill ?? 50), 0);
  const overall = Math.round(filled / total);

  const at = parts.findIndex((part) => part.state === "active" || part.state === "paused" || part.state === "failed");
  if (job.state === "finished") {
    const sent = job.steps.some((st) => st.kind === "send" && st.state === "done");
    return {
      head: sent ? "On the PS5" : "Packed",
      detail: sent ? "Sent and checked" : "Packed and checked",
      tone: "mint",
      overall: 100,
      parts,
    };
  }
  if (at < 0) return { head: "Starting", detail: "", tone: "plain", overall, parts };
  const part = parts[at];
  const row = rows[at];
  if (part.state === "failed") return { head: `${part.label} failed`, detail: row.status, tone: "fail", overall, parts };
  if (part.state === "paused") return { head: "Waiting for the PS5", detail: row.status, tone: "amber", overall, parts };
  return { head: DOING[part.kind], detail: row.status, tone: "plain", overall, parts };
}
