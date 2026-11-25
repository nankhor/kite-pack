// The screen's state and how each event moves it.

import { pageOf } from "./model";
import type {
  BundlePlan,
  ConvertChoice,
  BundleSelection,
  GameInfo,
  InstalledGame,
  JobView,
  PackOptions,
  PickedDlc,
  ProgressState,
  Settings,
  Status,
  UiEvent,
  View,
} from "./types";

/** How far back the speed looks, in ms. */
const SPEED_WINDOW = 10_000;

export type Phase = "empty" | "ready" | "running" | "failed" | "done";

export interface State {
  phase: Phase;
  game: GameInfo | null;
  source: string | null;
  /** Why the last dropped or chosen item wasn't opened. */
  message: string | null;
  options: PackOptions;
  /** Convert's choices. */
  convert: ConvertChoice;
  job: JobView | null;
  progress: ProgressState | null;
  dragging: boolean;
  /** Pack was pressed and its reply hasn't come back: updates may arrive first. */
  starting: boolean;
  /** The tab shown, or Settings. */
  view: View;
  /** DLC picked to install on the PS5 after the game. */
  dlc: PickedDlc[];
  settings: Settings | null;
  /** How the PS5 stands; null until first checked. */
  status: Status | null;
  /** The game as the console has it, or null when it is not installed (or could not be read). */
  onPs5: InstalledGame | null;
  /** DLC and a backport to fold into the packed copy. */
  bundle: BundleSelection;
  /** Whether they can go in; null until the plan is read. */
  bundlePlan: BundlePlan | null;
}

export type Action =
  /** `packagesDir`: where Convert writes the .fpkg. */
  | { type: "game"; game: GameInfo; source: string; outputDir: string; packagesDir?: string }
  | { type: "message"; text: string | null }
  | { type: "options"; patch: Partial<PackOptions> }
  | { type: "convert"; patch: Partial<ConvertChoice> }
  | { type: "starting" }
  | { type: "started"; job: JobView }
  | { type: "restored"; job: JobView; game: GameInfo }
  | { type: "event"; event: UiEvent; now: number }
  | { type: "back" }
  | { type: "reset" }
  | { type: "drag"; on: boolean }
  | { type: "view"; view: View }
  | { type: "dlc"; dlc: PickedDlc[] }
  | { type: "settings"; settings: Settings }
  | { type: "status"; status: Status }
  | { type: "onPs5"; game: InstalledGame | null }
  | { type: "bundle"; bundle: BundleSelection }
  | { type: "plan"; plan: BundlePlan | null };

export const initial: State = {
  phase: "empty",
  game: null,
  source: null,
  message: null,
  options: {
    format: "folder",
    level: "fast",
    outputDir: "",
    deleteOriginal: false,
    send: false,
    deletePacked: false,
    bundle: { dlc: [], backport: null },
  },
  convert: { compression: "fast", outputDir: "", install: true, deleteAfter: false },
  job: null,
  progress: null,
  dragging: false,
  starting: false,
  view: "pack",
  dlc: [],
  settings: null,
  status: null,
  onPs5: null,
  bundle: { dlc: [], backport: null },
  bundlePlan: null,
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
  // Asked of the screen, not of the job: App answers it.
  if (e.type === "closeAsked") return s;
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
    let progress = s.progress && s.progress.index === active ? s.progress : null;
    // A paused send that moves again: time left and speed count from now, not across the pause.
    if (progress && s.job.steps[active]?.state === "waiting" && e.job.steps[active].state === "running") {
      progress = { ...progress, startedAt: null, startedDone: 0, recent: [] };
    }
    return { ...s, job: e.job, phase: phaseOf(e.job), progress };
  }
  if (e.id !== s.job.id) return s;
  const fresh: ProgressState = {
    index: e.index,
    stage: null,
    done: 0,
    total: 0,
    startedAt: null,
    startedDone: 0,
    recent: [],
  };
  const p = s.progress && s.progress.index === e.index ? s.progress : fresh;
  if (e.type === "stage") return { ...s, progress: { ...p, stage: e.stage } };
  if (e.type === "ahead") {
    const recent = [...(p.ahead?.recent ?? []).filter((r) => r.at >= now - SPEED_WINDOW), { at: now, bytes: e.onPs5 }];
    return { ...s, progress: { ...p, ahead: { sent: e.sent, sending: e.sending, onPs5: e.onPs5, estimate: e.estimate, recent } } };
  }
  const started = p.startedAt === null ? { startedAt: now, startedDone: e.done } : {};
  const recent = [...p.recent.filter((r) => r.at >= now - SPEED_WINDOW), { at: now, done: e.done }];
  return { ...s, progress: { ...p, ...started, done: e.done, total: e.total, recent } };
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
        convert: { ...s.convert, outputDir: a.packagesDir ?? a.outputDir },
        job: null,
        progress: null,
        dlc: [],
        onPs5: null,
        bundle: { dlc: [], backport: null },
        bundlePlan: null,
        view: "pack",
      };
    case "message":
      return { ...s, message: a.text, starting: false };
    case "starting":
      return { ...s, starting: true, message: null };
    case "options":
      return { ...s, options: { ...s.options, ...a.patch } };
    case "convert":
      return { ...s, convert: { ...s.convert, ...a.patch } };
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
        convert: pageOf(a.job) === "convert" ? { ...s.convert, outputDir: a.job.outputDir } : s.convert,
        job: a.job,
        phase: a.job.state === "failed" ? "failed" : "running",
        // A job left by a closed app shows on its own page.
        view: pageOf(a.job),
      };
    case "event":
      return onEvent(s, a.event, a.now);
    case "back":
      return { ...s, phase: s.game ? "ready" : "empty", job: null, progress: null };
    case "reset":
      return {
        ...initial,
        options: { ...initial.options, outputDir: s.options.outputDir },
        convert: s.convert,
        settings: s.settings,
        // Another game opens on the page this one was on.
        view: s.view,
        status: s.status,
      };
    case "drag":
      return { ...s, dragging: a.on };
    case "view":
      return { ...s, view: a.view };
    case "dlc":
      return { ...s, dlc: a.dlc };
    case "settings":
      return { ...s, settings: a.settings };
    case "status":
      return { ...s, status: a.status };
    case "onPs5":
      return { ...s, onPs5: a.game };
    case "bundle":
      return { ...s, bundle: a.bundle, bundlePlan: null, message: null };
    case "plan":
      return { ...s, bundlePlan: a.plan };
  }
}
