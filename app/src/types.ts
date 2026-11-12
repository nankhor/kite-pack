// The shapes the Rust side sends (kite-forge's GameInfo, kite-session's JobView and UiEvent).

export interface DlcInfo {
  contentId: string;
  label: string;
  name: string | null;
  bytes: number;
}

export interface GameInfo {
  kind: string;
  title: string | null;
  titleId: string | null;
  contentId: string | null;
  version: string | null;
  totalBytes: number;
  dlcs: DlcInfo[];
  iconDataUrl: string | null;
  findings: string[];
}

export type StepKind = "pack" | "deleteOriginal" | "send" | "installDlc" | "deletePacked";
export type StepState = "pending" | "running" | "waiting" | "done" | "failed";
export type JobState = "running" | "paused" | "finished" | "failed" | "cancelled";

export interface StepView {
  kind: StepKind;
  state: StepState;
  note: string | null;
}

export interface JobView {
  id: string;
  source: string;
  outputDir: string;
  deleteOriginal: boolean;
  packed: string | null;
  packedBytes: number | null;
  steps: StepView[];
  state: JobState;
  /** Grows with every view the app sends: the larger one is the newer. */
  rev: number;
}

export type UiEvent =
  | { type: "job"; job: JobView }
  | { type: "stage"; id: string; index: number; stage: string }
  | { type: "progress"; id: string; index: number; done: number; total: number };

export type Format = "folder" | "exfat" | "ffpkg" | "ffpfs";
export type Level = "fast" | "medium" | "max";

export interface PackOptions {
  format: Format;
  level: Level;
  outputDir: string;
  deleteOriginal: boolean;
  send: boolean;
  deletePacked: boolean;
}

/** The running step's progress, as the screen tracks it. */
export interface ProgressState {
  index: number;
  stage: string | null;
  done: number;
  total: number;
  /** When the first bytes arrived, and how many there were then (ms since epoch). */
  startedAt: number | null;
  startedDone: number;
}

/** The app's PS5 settings (kite-ps5's Settings). */
export interface Settings {
  address: string | null;
  helperPort: number;
  loaderPort: number;
  /** Where games go on the PS5. */
  destination: string;
  /** Where packed copies go on this computer; null for next to each game. */
  packedFolder: string | null;
}

export type Status =
  | { state: "notSetUp" }
  | { state: "notReachable" }
  | { state: "running"; version: string }
  | { state: "refused"; reason: string };

/** A Kite helper that answered on the network. */
export interface Found {
  address: string;
  port: number;
  version: string;
}

export type StartOutcome = "alreadyRunning" | "sent";
