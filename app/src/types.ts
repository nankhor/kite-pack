// The shapes the Rust side sends (kite-forge's GameInfo, kite-session's JobView and UiEvent).

export interface DlcInfo {
  contentId: string;
  label: string;
  name: string | null;
  bytes: number;
  /** The package's own icon; none for DLC found inside a dump, and for some small DLC. */
  iconDataUrl?: string | null;
}

/** A DLC package picked to install after the game. */
export interface PickedDlc {
  path: string;
  info: DlcInfo;
}

/** What a chosen folder holds for the game. */
export interface DlcFolder {
  found: PickedDlc[];
  /** File names of the packages that aren't DLC for this game. */
  skipped: string[];
}

/** A package as Sony's installer takes it, read from its header. */
export interface PackageInfo {
  /** "ps5" or "ps4". */
  platform: string;
  /** "game", "update" or "dlc". */
  kind: string;
  titleId: string;
  contentId: string;
  title: string | null;
  version: string | null;
  bytes: number;
  iconDataUrl: string | null;
  /** A PS5 game package Forge can also pack. */
  packable: boolean;
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
  /** Set when the item is a package: it can be installed on the PS5 as it is. */
  package?: PackageInfo | null;
}

export type StepKind = "pack" | "deleteOriginal" | "send" | "installDlc" | "deletePacked" | "installPackage";
export type StepState = "pending" | "running" | "waiting" | "done" | "failed";
export type JobState = "running" | "paused" | "finished" | "failed" | "cancelled";

export interface StepView {
  kind: StepKind;
  state: StepState;
  note: string | null;
  /** The package an installDlc step installs. */
  pkg?: string | null;
}

export interface JobView {
  id: string;
  source: string;
  outputDir: string;
  deleteOriginal: boolean;
  packed: string | null;
  packedBytes: number | null;
  /** Bytes of the packed copy that went to the PS5 while it was packed (Send leaves them out). */
  aheadBytes?: number;
  steps: StepView[];
  state: JobState;
  /** Grows with every view the app sends: the larger one is the newer. */
  rev: number;
}

export type UiEvent =
  | { type: "job"; job: JobView }
  | { type: "stage"; id: string; index: number; stage: string }
  | { type: "progress"; id: string; index: number; done: number; total: number }
  /** While Pack runs: parts of the packed copy already on the PS5, one on its way, the bytes
   *  on the PS5, and what the packed copy should come to (0 until a first part says). */
  | { type: "ahead"; id: string; index: number; sent: number; sending: boolean; onPs5: number; estimate: number }
  /** The window was asked to close while an install runs: ask first. */
  | { type: "closeAsked" };

export type Format = "folder" | "exfat" | "ffpkg" | "ffpfs";
export type Level = "fast" | "medium" | "max";

export interface PackOptions {
  format: Format;
  level: Level;
  outputDir: string;
  deleteOriginal: boolean;
  send: boolean;
  deletePacked: boolean;
  /** DLC packages to install after the game; only sent along when sending. */
  dlc?: string[];
  /** DLC and a backport folded into the packed copy, before it is written. */
  bundle?: BundleSelection;
}

/** Parts of the packed copy that went to the PS5 while it was packed. */
export interface AheadState {
  /** Parts the PS5 has whole, and whether the next is on its way. */
  sent: number;
  sending: boolean;
  /** Bytes on the PS5, the part on its way included. */
  onPs5: number;
  /** What the packed copy should come to; 0 while nothing says. */
  estimate: number;
  /** The last few seconds of `onPs5` (ms since epoch), for the sending speed. */
  recent: { at: number; bytes: number }[];
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
  /** While packing: what has gone to the PS5 alongside. */
  ahead?: AheadState;
}

/** ShadowMount+'s answer about this console, and the game's base (kite-ps5's SmpStatus). */
export interface SmpStatus {
  host: string;
  port: number;
  version: string;
  apiVersion: number;
  /** The base is installed as a native package: ShadowMount+ installs DLC for it. */
  installed: boolean;
  /** The base is only registered from a folder or image. */
  baseInstalled: boolean;
  sourceType: string;
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
  /** ShadowMount+'s HTTP API port; 10101 unless its config says otherwise. */
  smpPort?: number;
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
  /** The dump the PS5 mounts the game from (ShadowMount+), in the folders games go to. */
  dumpBytes: number;
  /** Where those dumps are on the console. */
  dumps: string[];
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
  /** `inPack`, `install` (needs the PS5's installer, which the helper cannot drive yet) or `blocked`. */
  route: "inPack" | "install" | "blocked";
  /** The package's or folder's own tile, when it has one. */
  iconDataUrl?: string | null;
}

/** Whether the picked DLC and backport can go into the packed copy. */
export interface BundlePlan {
  items: BundleItem[];
  /** Where the whole selection goes: `inPack`, `install` (no folder to fold into) or `unavailable`. */
  route: "inPack" | "install" | "unavailable";
  problem: string | null;
  /** Where the DLC emulator comes from: the dump, the app's own copy, or a picked folder. */
  emulatorFrom: "dump" | "builtin" | "picked" | "none";
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

/** A finished job, for the History tab (the app's history.rs Entry). */
export interface HistoryEntry {
  id: string;
  title: string;
  titleId: string | null;
  /** ms since 1970. */
  finishedAt: number;
  sourceBytes: number;
  packedBytes: number | null;
  sent: boolean;
  /** The packed copy, when it was kept on this computer. */
  packed: string | null;
  dlc: number;
  iconDataUrl?: string | null;
}

/** The app's tabs, and Settings behind the gear. */
export type View = "pack" | "ps5" | "history" | "settings";
