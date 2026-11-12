// The screen's state and how each event moves it.

import type { GameInfo, JobView, PackOptions, ProgressState, Settings, Status, UiEvent } from "./types";

export type Phase = "empty" | "ready" | "running" | "failed" | "done";

export interface State {
  phase: Phase;
  game: GameInfo | null;
  source: string | null;
  /** Why the last dropped or chosen item wasn't opened. */
  message: string | null;
  options: PackOptions;
  job: JobView | null;
  progress: ProgressState | null;
  dragging: boolean;
  /** Pack was pressed and its reply hasn't come back: updates may arrive first. */
  starting: boolean;
  /** The main screen or Settings. */
  view: "main" | "settings";
  settings: Settings | null;
  /** How the PS5 stands; null until first checked. */
  status: Status | null;
}

export type Action =
  | { type: "game"; game: GameInfo; source: string; outputDir: string }
  | { type: "message"; text: string | null }
  | { type: "options"; patch: Partial<PackOptions> }
  | { type: "starting" }
  | { type: "started"; job: JobView }
  | { type: "restored"; job: JobView; game: GameInfo }
  | { type: "event"; event: UiEvent; now: number }
  | { type: "back" }
  | { type: "reset" }
  | { type: "drag"; on: boolean }
  | { type: "view"; view: "main" | "settings" }
  | { type: "settings"; settings: Settings }
  | { type: "status"; status: Status };

export const initial: State = {
  phase: "empty",
  game: null,
  source: null,
  message: null,
  options: { format: "folder", level: "fast", outputDir: "", deleteOriginal: false, send: false, deletePacked: false },
  job: null,
  progress: null,
  dragging: false,
  starting: false,
  view: "main",
  settings: null,
  status: null,
};

function phaseOf(job: JobView): Phase {
  switch (job.state) {
    case "finished":
      return "done";
    case "failed":
      return "failed";
    default:
      return "running";
  }
}

function onEvent(s: State, e: UiEvent, now: number): State {
  if (!s.job) {
    // The job's first update can beat Pack's reply; take it as the job.
    if (s.starting && e.type === "job" && e.job.state !== "cancelled") {
      return { ...s, job: e.job, phase: phaseOf(e.job), progress: null };
    }
    return s;
  }
  if (e.type === "job") {
    if (e.job.id !== s.job.id || e.job.rev < s.job.rev) return s;
    if (e.job.state === "cancelled") return { ...s, phase: s.game ? "ready" : "empty", job: null, progress: null };
    // Keep the progress of the step that is running or waiting: a paused send shows where it stopped.
    const active = e.job.steps.findIndex((st) => st.state === "running" || st.state === "waiting");
    const progress = s.progress && s.progress.index === active ? s.progress : null;
    return { ...s, job: e.job, phase: phaseOf(e.job), progress };
  }
  if (e.id !== s.job.id) return s;
  const fresh: ProgressState = { index: e.index, stage: null, done: 0, total: 0, startedAt: null, startedDone: 0 };
  const p = s.progress && s.progress.index === e.index ? s.progress : fresh;
  if (e.type === "stage") return { ...s, progress: { ...p, stage: e.stage } };
  const started = p.startedAt === null ? { startedAt: now, startedDone: e.done } : {};
  return { ...s, progress: { ...p, ...started, done: e.done, total: e.total } };
}

export function reduce(s: State, a: Action): State {
  switch (a.type) {
    case "game":
      return {
        ...s,
        phase: "ready",
        game: a.game,
        source: a.source,
        message: null,
        options: { ...s.options, outputDir: a.outputDir },
        job: null,
        progress: null,
      };
    case "message":
      return { ...s, message: a.text, starting: false };
    case "starting":
      return { ...s, starting: true, message: null };
    case "options":
      return { ...s, options: { ...s.options, ...a.patch } };
    case "started": {
      // A command's reply is older than any update about the same job that came before it.
      const newer = s.job && s.job.id === a.job.id && s.job.rev > a.job.rev ? s.job : a.job;
      const progress = newer === s.job ? s.progress : null;
      return { ...s, phase: phaseOf(newer), job: newer, progress, message: null, starting: false };
    }
    case "restored":
      return {
        ...s,
        game: a.game,
        source: a.job.source,
        options: { ...s.options, outputDir: a.job.outputDir, deleteOriginal: a.job.deleteOriginal },
        job: a.job,
        phase: a.job.state === "failed" ? "failed" : "running",
      };
    case "event":
      return onEvent(s, a.event, a.now);
    case "back":
      return { ...s, phase: s.game ? "ready" : "empty", job: null, progress: null };
    case "reset":
      return {
        ...initial,
        options: { ...initial.options, outputDir: s.options.outputDir },
        settings: s.settings,
        status: s.status,
      };
    case "drag":
      return { ...s, dragging: a.on };
    case "view":
      return { ...s, view: a.view };
    case "settings":
      return { ...s, settings: a.settings };
    case "status":
      return { ...s, status: a.status };
  }
}
