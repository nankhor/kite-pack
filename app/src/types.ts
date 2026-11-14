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
  /** DLC and a backport to fold in before packing. */
  bundle: BundleSelection;
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
  /** The last few seconds of progress (ms since epoch, bytes done), for the speed. */
  recent: { at: number; done: number }[];
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
  /** The unzipped dlc_emu release, for dumps that carry no DLC emulator of their own. */
  dlcTools: string | null;
}

export type Status =
  | { state: "notSetUp" }
  | { state: "notReachable" }
  | { state: "running"; version: string }
  | { state: "refused"; reason: string };

/** One title installed on the PS5 (kite-ps5's InstalledGame). */
export interface InstalledGame {
  titleId: string;
  title: string;
  /** `contentVersion` from the console's param.json: the base content, not the update. */
  version: string | null;
  updateInstalled: boolean;
  /** One label per installed add-on, sorted. */
  dlc: string[];
  /** Its content folder is on an extended drive. */
  external: boolean;
  /** `appmeta`, `app`, or both, joined by `+`. */
  found: string;
}

/** What the PS5 has installed, and whether the list is the whole console. */
export interface Library {
  games: InstalledGame[];
  truncated: boolean;
}

/** One title's tile as a data: URL. */
export interface GameIcon {
  titleId: string;
  dataUrl: string;
}

/** What a title takes on the console (kite-ps5's Measured). */
export interface Measured {
  bytes: number;
  appBytes: number;
  updateBytes: number;
  dlcBytes: number;
  partial: boolean;
}

/** Room on the volume games go to. */
export interface FreeSpace {
  total: number;
  free: number;
  usable: number;
  reserve: number;
}

/** One thing picked to go inside the packed copy (kite-forge's BundleItem). */
export interface BundleItem {
  path: string;
  kind: "dlc" | "backport";
  name: string;
  titleId: string | null;
  contentId: string | null;
  label: string | null;
  bytes: number;
  /** Why it can't go in, in the screen's words; null when it can. */
  problem: string | null;
}

/** Whether the picked DLC and backport can go into the packed copy. */
export interface BundlePlan {
  items: BundleItem[];
  route: "inPack" | "unavailable";
  problem: string | null;
  /** The DLC emulator will come from the folder picked in Settings. */
  emulatorFromSettings: boolean;
}

/** What the screen picked to put inside the pack. */
export interface BundleSelection {
  dlc: string[];
  backport: string | null;
}

/** A Kite helper that answered on the network. */
export interface Found {
  address: string;
  port: number;
  version: string;
}

export type StartOutcome = "alreadyRunning" | "sent";
