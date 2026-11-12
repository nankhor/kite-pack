// The screen's state and how each event moves it.

import type { GameInfo, JobView, PackOptions, ProgressState, UiEvent } from "./types";

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
}

export type Action =
  | { type: "game"; game: GameInfo; source: string; outputDir: string }
  | { type: "message"; text: string | null }
  | { type: "options"; patch: Partial<PackOptions> }
  | { type: "started"; job: JobView }
  | { type: "restored"; job: JobView; game: GameInfo }
  | { type: "event"; event: UiEvent; now: number }
  | { type: "back" }
  | { type: "reset" }
  | { type: "drag"; on: boolean };

export const initial: State = {
  phase: "empty",
  game: null,
  source: null,
  message: null,
  options: { format: "folder", level: "fast", outputDir: "", deleteOriginal: false },
  job: null,
  progress: null,
  dragging: false,
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
  if (!s.job) return s;
  if (e.type === "job") {
    if (e.job.id !== s.job.id) return s;
    if (e.job.state === "cancelled") return { ...s, phase: s.game ? "ready" : "empty", job: null, progress: null };
    const running = e.job.steps.findIndex((st) => st.state === "running");
    const progress = s.progress && s.progress.index === running ? s.progress : null;
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
      return { ...s, message: a.text };
    case "options":
      return { ...s, options: { ...s.options, ...a.patch } };
    case "started":
      return { ...s, phase: phaseOf(a.job), job: a.job, progress: null, message: null };
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
      return { ...initial, options: { ...initial.options, outputDir: s.options.outputDir } };
    case "drag":
      return { ...s, dragging: a.on };
  }
}
