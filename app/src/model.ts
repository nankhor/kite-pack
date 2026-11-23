// The screen's words and numbers, worked out from plain data so they can be tested.

import { baseName, parentOf } from "./paths";
import type {
  AheadState,
  Format,
  GameInfo,
  HistoryEntry,
  JobView,
  Measured,
  ProgressState,
  Status,
  StepKind,
  StepView,
} from "./types";

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
  /** `alongside`: each part goes to the PS5 as soon as it is packed (a folder, helper 0.4+). */
  | { kind: "ready"; outputDir: string; send?: boolean; alongside?: boolean; bundled?: number }
  | { kind: "running" | "failed" | "done"; job: JobView };

/** Parts of a packed folder go to the PS5 while the rest packs: helper 0.4.0 and later. */
export function sendsAlongside(format: Format, status: Status | null): boolean {
  return format === "folder" && status?.state === "running" && versionAtLeast(status.version, 0, 4);
}

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
    case "pack": {
      const base =
        job.packedBytes !== null
          ? `${formatSize(job.packedBytes)} in ${folderName(job.outputDir)}`
          : "Packed and checked";
      // The packer's own line about what it folded in: kept in the job, so it still shows after a
      // restart, when the screen no longer has the picks.
      const bundled = step.note?.split(" · ").find((part) => part.startsWith("bundled: "));
      return bundled ? `${base} · ${bundled.slice("bundled: ".length)}` : base;
    }
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

/** Packing and sending, which run together: how far each has come. */
export interface PackSend {
  /** 0 to 100. */
  packed: number;
  /** 0 to 100: what the PS5 has of the packed copy. */
  onPs5: number;
  onPs5Bytes: number;
  /** What the packed copy comes to, or should (an estimate while packing); null while nothing says. */
  totalBytes: number | null;
  /** Parts are going (or went) to the PS5 while packing. */
  alongside: boolean;
  /** Until both are done: whichever ends last. Null while there is no rate. */
  minutes: number | null;
}

/** Minutes until the PS5 has `total` bytes, at the rate parts have been arriving. */
function sendMinutes(ahead: AheadState, total: number): number | null {
  const first = ahead.recent[0];
  const last = ahead.recent[ahead.recent.length - 1];
  if (!first || !last || last.at - first.at < 1_000 || last.bytes <= first.bytes) return null;
  const rate = (last.bytes - first.bytes) / (last.at - first.at);
  return Math.max(1, Math.ceil((total - ahead.onPs5) / rate / 60_000));
}

/** Pack and send for a job that does both; null for one that only packs. */
export function packSend(job: JobView, progress: ProgressState | null, now: number): PackSend | null {
  const pi = job.steps.findIndex((st) => st.kind === "pack");
  const si = job.steps.findIndex((st) => st.kind === "send");
  if (pi < 0 || si < 0) return null;
  const [pack, send] = [job.steps[pi], job.steps[si]];
  const pp = progress && progress.index === pi ? progress : null;
  const sp = progress && progress.index === si && progress.total > 0 ? progress : null;
  const ahead = job.aheadBytes ?? 0;
  const packed = pack.state === "done" ? 100 : pp && pp.total > 0 ? Math.min(100, Math.floor((pp.done / pp.total) * 100)) : 0;
  let onPs5Bytes = 0;
  let totalBytes: number | null = null;
  let minutes: number | null = null;
  if (send.state === "done") {
    totalBytes = job.packedBytes;
    onPs5Bytes = totalBytes ?? 0;
  } else if (sp) {
    // The last of it: what went while packing, and Send's own count of the rest.
    onPs5Bytes = ahead + sp.done;
    totalBytes = ahead + sp.total;
    minutes = minutesLeft(sp, now);
  } else if (pack.state === "done") {
    onPs5Bytes = ahead;
    totalBytes = job.packedBytes;
  } else if (pp) {
    onPs5Bytes = pp.ahead?.onPs5 ?? 0;
    totalBytes = pp.ahead?.estimate || null;
    const packing = minutesLeft(pp, now);
    const sending = pp.ahead && totalBytes ? sendMinutes(pp.ahead, totalBytes) : null;
    minutes = packing === null ? sending : sending === null ? packing : Math.max(packing, sending);
  }
  const onPs5 =
    send.state === "done" ? 100 : totalBytes ? Math.min(99, Math.floor((Math.min(onPs5Bytes, totalBytes) / totalBytes) * 100)) : 0;
  return { packed, onPs5, onPs5Bytes, totalBytes, alongside: ahead > 0 || !!pp?.ahead, minutes };
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

/** Where a measured title's size comes from: the dump it is mounted from, or only its launcher. */
export function dumpNote(m: Measured): string {
  if (m.dumps.length === 0) return "No dump found where games go";
  const folders = [...new Set(m.dumps.map(parentOf))];
  return folders.length === 1 ? `With its dump in ${folders[0]}` : `With its dumps in ${folders.length} folders`;
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
      if (!input.send) return `Packs into ${input.outputDir}${inside}.`;
      return input.alongside
        ? `Packs into ${input.outputDir}${inside} and sends each part to the PS5 as soon as it's packed.`
        : `Packs into ${input.outputDir}${inside}, then sends to the PS5 by itself.`;
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
      // The reason is above the bar already: this line says what to do about it.
      const step = input.job.steps.find((s) => s.state === "failed");
      if (!step) return "Something stopped the job.";
      if (step.note?.startsWith("Not enough free space") || step.note?.startsWith("The drive filled up")) {
        return `Free up space in ${input.job.outputDir} and Retry, or Cancel and pick another folder under Save to.`;
      }
      return step.note ? "Retry once that's fixed, or Cancel." : `${LABELS[step.kind]} failed with no reason given. Retry, or Cancel.`;
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
  /** 0 to 100; null while it runs with nothing to count (a move to the Trash). For Pack and
   *  send: what the PS5 has. */
  fill: number | null;
  /** Pack and send only: how much is packed, drawn lighter under `fill`. */
  under?: number;
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
  deleteOriginal: "Trash original",
  send: "Send to PS5",
  installDlc: "Install DLC",
  deletePacked: "Trash packed copy",
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
    // The pack step's own note already says what it bundled; the screen's count is only for
    // jobs whose packer said nothing (or older jobs).
    if (row.state === "done" && kind === "pack" && inside > 0 && !row.status.includes("·")) {
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
    } else if (row.state === "pending" && kind === "deleteOriginal") {
      note = "After packing";
    } else if (row.state === "pending" && kind === "deletePacked") {
      note = "At the end";
    }
    return { kind, label: PART_LABELS[kind], weight: PART_WEIGHTS[kind], fill, state: row.state, note, items: row.items };
  });

  // Packing and sending run together: one part for both, what is packed under what the PS5 has.
  const both = packSend(job, progress, now);
  if (both) {
    const pi = parts.findIndex((part) => part.kind === "pack");
    const si = parts.findIndex((part) => part.kind === "send");
    const combined = combine(parts[pi], parts[si], both, job, progress);
    parts.splice(si, 1);
    parts.splice(pi, 1, combined);
  }

  const total = parts.reduce((t, part) => t + part.weight, 0) || 1;
  // Pack and send counts as halfway between what is packed and what the PS5 has.
  const filled = parts.reduce((t, part) => t + part.weight * (part.under !== undefined ? ((part.fill ?? 0) + part.under) / 2 : (part.fill ?? 50)), 0);
  const overall = Math.round(filled / total);

  // The head follows the step running now, which may not be the first part still going: the
  // original goes to the Trash while Pack and send still has the rest to send.
  const runningKind = job.steps.find((st) => st.state === "running" || st.state === "waiting" || st.state === "failed")?.kind;
  const byStep = runningKind ? parts.findIndex((part) => part.kind === runningKind || (part.kind === "pack" && runningKind === "send" && both !== null)) : -1;
  const at = byStep >= 0 ? byStep : parts.findIndex((part) => part.state === "active" || part.state === "paused" || part.state === "failed");
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
  if (both && part.under !== undefined) {
    const head = packSendHead(job, both, progress, now);
    return { ...head, overall, parts: head.tone === "fail" ? saidOnce(parts, at) : parts };
  }
  const row = rows[kinds.indexOf(part.kind)];
  if (part.state === "failed") {
    const head = part.kind === "pack" ? "Packing failed" : `${part.label} failed`;
    return { head, detail: row.status, tone: "fail", overall, parts: saidOnce(parts, at) };
  }
  if (part.state === "paused") return { head: "Waiting for the PS5", detail: row.status, tone: "amber", overall, parts };
  // A move to the Trash has nothing to count: the head says it all.
  const detail = part.fill === null ? "" : row.status;
  return { head: DOING[part.kind], detail, tone: "plain", overall, parts };
}

/** The failed part says only that it failed: why is in the line above the bar. */
function saidOnce(parts: BarPart[], at: number): BarPart[] {
  return parts.map((part, i) => (i === at && !part.items ? { ...part, note: "Failed" } : part));
}

/** The one part for packing and sending, from their two. */
function combine(pack: BarPart, send: BarPart, both: PackSend, job: JobView, progress: ProgressState | null): BarPart {
  const st = (kind: StepKind) => job.steps.find((s) => s.kind === kind)!.state;
  const [ps, ss] = [st("pack"), st("send")];
  const state: BarPart["state"] =
    ps === "failed" || ss === "failed"
      ? "failed"
      : ss === "waiting"
        ? "paused"
        : ps === "done" && ss === "done"
          ? "done"
          : ps === "pending" && ss === "pending"
            ? "pending"
            : "active";
  const onPs5 = (bytes: number) => `${formatSize(bytes)} on the PS5`;
  let note: string;
  if (state === "failed") note = ps === "failed" ? pack.note : send.note;
  else if (state === "done") {
    // "51.6 GB in Packed, sent and checked", and after it what the pack folded in, if anything.
    const [base, ...extra] = pack.note.split(" · ");
    const sent = base === "Packed and checked" ? "Packed, sent and checked" : `${base}, sent and checked`;
    note = [sent, ...extra].join(" · ");
  } else if (state === "paused") note = "Paused";
  else if (state === "pending") note = "Waiting to start";
  else if (ss === "running") {
    note = both.totalBytes && both.onPs5Bytes > 0 ? `${formatSize(both.onPs5Bytes)} of ${onPs5(both.totalBytes)}` : "Packed, starting to send";
  }
  else if (ps === "done") note = both.onPs5Bytes > 0 ? `${onPs5(both.onPs5Bytes)}, the rest next` : "Packed, sending next";
  else {
    const packing = progress && progress.total > 0 ? `${both.packed}% packed` : "";
    note = both.onPs5Bytes > 0 ? `${packing}, ${onPs5(both.onPs5Bytes)}` : packing;
  }
  return {
    kind: "pack",
    label: "Pack and send",
    weight: pack.weight + send.weight,
    // Done and failed fill the part whole (mint or red), as every other step's does.
    fill: state === "done" || state === "failed" ? 100 : both.onPs5,
    under: both.packed,
    state,
    note,
  };
}

/** The line above the bar while packing and sending. */
function packSendHead(job: JobView, both: PackSend, progress: ProgressState | null, now: number): Omit<JobBar, "overall" | "parts"> {
  const pack = job.steps.find((s) => s.kind === "pack")!;
  const send = job.steps.find((s) => s.kind === "send")!;
  if (pack.state === "failed") return { head: "Packing failed", detail: pack.note ?? "Failed", tone: "fail" };
  if (send.state === "failed") return { head: "Sending failed", detail: send.note ?? "Failed", tone: "fail" };
  if (send.state === "waiting") return { head: "Waiting for the PS5", detail: `Paused at ${both.onPs5}% on the PS5`, tone: "amber" };
  const left = both.minutes === null ? null : `about ${both.minutes} min left`;
  if (send.state === "running") {
    const index = job.steps.indexOf(send);
    const rate = progress && progress.index === index ? speed(progress) : null;
    const parts = [`${both.onPs5}% on the PS5`];
    if (rate !== null) parts.push(formatSpeed(rate));
    if (left) parts.push(left);
    const ahead = (job.aheadBytes ?? 0) > 0;
    return { head: ahead ? "Sending the rest" : "Sending to PS5", detail: parts.join(", "), tone: "plain" };
  }
  const index = job.steps.indexOf(pack);
  const p = progress && progress.index === index && progress.total > 0 ? progress : null;
  if (!p) return { head: "Packing", detail: "Starting", tone: "plain" };
  if (both.alongside) {
    const parts = [`${both.packed}% packed`];
    if (both.onPs5Bytes > 0) parts.push(both.totalBytes ? `${both.onPs5}% on the PS5` : `${formatSize(both.onPs5Bytes)} on the PS5`);
    if (left) parts.push(left);
    return { head: "Packing and sending", detail: parts.join(", "), tone: "plain" };
  }
  // Nothing has gone yet: an image, an older helper, or the first part still packing.
  const parts = [`${both.packed}% packed`];
  if (both.minutes !== null) parts.push(`about ${both.minutes} min to pack`);
  void now;
  return { head: "Packing", detail: parts.join(", "), tone: "plain" };
}

/** What adding a folder of packages did: how many DLC went in, and what was left out. Null when all of it went in. */
export function folderNote(added: number, skipped: string[], game: string): string | null {
  const n = skipped.length;
  if (added === 0) {
    return n === 1
      ? `The package in that folder isn't DLC for ${game}.`
      : `None of the ${n} packages in that folder is DLC for ${game}.`;
  }
  if (n === 0) return null;
  const shown = skipped.slice(0, 3).join(", ");
  const rest = n > 3 ? ` and ${n - 3} more` : "";
  const files = n === 1 ? "1 file that isn't" : `${n} files that aren't`;
  return `Added ${added} DLC from the folder. Left out ${files} DLC for this game: ${shown}${rest}.`;
}

/** Whether a version string is at least `major.minor` (for the helper's streamed install). */
export function versionAtLeast(version: string | null, major: number, minor: number): boolean {
  const m = version?.match(/^(\d+)\.(\d+)/);
  if (!m) return false;
  const [a, b] = [Number(m[1]), Number(m[2])];
  return a > major || (a === major && b >= minor);
}
