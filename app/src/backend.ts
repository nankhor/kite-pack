// Everything the screen asks of the app, behind one interface so tests can stand in for it.

import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { readPrefs, writePrefs, type UpdateInfo, type UpdatePrefs } from "./updates";
import { DETAILS_URL, detailsClient, detailsOn, setDetailsOn, type Candidate, type GameDetails } from "./details";
import type {
  BundlePlan,
  DlcFolder,
  DlcInfo,
  Found,
  FreeSpace,
  GameIcon,
  GameInfo,
  HistoryEntry,
  InstalledGame,
  JobView,
  Library,
  Measured,
  PackOptions,
  Settings,
  StartOutcome,
  Status,
  UiEvent,
} from "./types";

export type DropEvent = { type: "over" } | { type: "leave" } | { type: "drop"; paths: string[] };

export interface Backend {
  /** Rejects with the reason, in the screen's words, when it isn't a game Kite Pack can pack. */
  inspect(path: string): Promise<GameInfo>;
  start(source: string, options: PackOptions): Promise<JobView>;
  cancel(): Promise<void>;
  carryOn(id: string): Promise<JobView>;
  discard(id: string): Promise<void>;
  unfinished(): Promise<JobView | null>;
  /** A game given to the app when it started ("Open with", or dropped on its icon). */
  initialPath(): Promise<string | null>;
  pickFolder(): Promise<string | null>;
  pickFile(): Promise<string | null>;
  reveal(path: string): Promise<void>;
  settings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<void>;
  /** Connects to the PS5's helper and says how it stands (takes up to a few seconds). */
  ps5Status(): Promise<Status>;
  /** Kite helpers that answer on the home network (about 2 s). */
  discover(): Promise<Found[]>;
  /** Sends the Kite helper to the PS5's ELF loader; rejects with the reason. */
  startHelper(): Promise<StartOutcome>;
  /** Reads a DLC package for the game `titleId`; rejects when it isn't DLC for it. */
  checkDlc(path: string, titleId: string): Promise<DlcInfo>;
  /** The DLC for `titleId` among the packages in a folder. */
  dlcInFolder(path: string, titleId: string): Promise<DlcFolder>;
  /** DLC packages, several at once; empty when nothing was picked. */
  pickDlcFiles(): Promise<string[]>;
  /** A DLC dump folder, for folding into the packed copy. */
  pickDlcFolder(): Promise<string | null>;
  /** A backport folder, for folding into the packed copy. */
  pickBackport(): Promise<string | null>;
  /** What the picked DLC and backport would add, and where each one goes. */
  bundlePlan(source: string, dlc: string[], backport: string | null): Promise<BundlePlan>;
  /** The DLC emulator release the app carries. */
  dlcEmuVersion(): Promise<string>;
  /** What the PS5 has installed (reads the console; takes a couple of seconds). */
  library(): Promise<Library>;
  /** One title on the console by id, or null when it is not installed. */
  installedTitle(titleId: string): Promise<InstalledGame | null>;
  /** The tiles of the given titles, as data: URLs. */
  libraryIcons(ids: string[]): Promise<GameIcon[]>;
  /** App, update and DLC bytes of one installed title. */
  measureApp(titleId: string): Promise<Measured>;
  /** The room left where games go. */
  freeSpace(path: string): Promise<FreeSpace>;
  /** Finished jobs, newest first. */
  history(): Promise<HistoryEntry[]>;
  addHistory(entry: HistoryEntry): Promise<void>;
  clearHistory(): Promise<void>;
  appVersion(): Promise<string>;
  /** The game's story, studio and pictures from IGDB; null when IGDB doesn't know it; rejects when offline. */
  gameDetails(title: string, titleId: string | null): Promise<GameDetails | null>;
  /** Games of a name on IGDB, for "Wrong game?". */
  searchGames(q: string): Promise<Candidate[]>;
  /** Shows IGDB game `igdbId` for this game from now on; null goes back to the best match. */
  pickGame(title: string, titleId: string | null, igdbId: number | null): Promise<void>;
  /** Whether game details are looked up (Settings). */
  detailsOn(): Promise<boolean>;
  setDetailsOn(on: boolean): Promise<void>;
  /** A newer Kite Pack on GitHub, or null. */
  checkUpdate(): Promise<UpdateInfo | null>;
  /** Downloads it, checks it and puts it in place for the next launch; progress comes to onUpdateProgress. */
  installUpdate(update: UpdateInfo): Promise<void>;
  /** Starts the new version; rejects while a game packs or sends. */
  restartApp(): Promise<void>;
  onUpdateProgress(cb: (p: { done: number; total: number }) => void): () => void;
  updatePrefs(): Promise<UpdatePrefs>;
  setUpdatePrefs(prefs: UpdatePrefs): Promise<void>;
  /** Returns a function that stops listening. */
  onEvent(cb: (e: UiEvent) => void): () => void;
  onDrop(cb: (d: DropEvent) => void): () => void;
}

/** Wraps Tauri's async unlisten in a plain function that also works before it resolves. */
function unlistener(pending: Promise<() => void>): () => void {
  let stop: (() => void) | null = null;
  let gone = false;
  void pending.then((un) => {
    if (gone) un();
    else stop = un;
  });
  return () => {
    gone = true;
    stop?.();
  };
}

let client: ReturnType<typeof detailsClient> | null = null;
const details = () => (client ??= detailsClient(DETAILS_URL, (...args) => fetch(...args), localStorage));

const single = (picked: string | string[] | null): string | null => (Array.isArray(picked) ? (picked[0] ?? null) : picked);

export const tauriBackend: Backend = {
  inspect: (path) => invoke("inspect", { path }),
  start: (source, options) => invoke("start", { source, options }),
  cancel: () => invoke("cancel"),
  carryOn: (id) => invoke("carry_on", { id }),
  discard: (id) => invoke("discard", { id }),
  unfinished: () => invoke("unfinished"),
  initialPath: () => invoke("initial_path"),
  pickFolder: async () => single(await open({ directory: true, multiple: false })),
  pickFile: async () =>
    single(
      await open({
        multiple: false,
        filters: [{ name: "Game images and archives", extensions: ["exfat", "ffpkg", "ffpfs", "ffpfsc", "pkg", "zip", "7z", "rar"] }],
      }),
    ),
  reveal: (path) => revealItemInDir(path),
  settings: () => invoke("settings"),
  saveSettings: (settings) => invoke("save_settings", { settings }),
  ps5Status: () => invoke("ps5_status"),
  discover: () => invoke("discover"),
  dlcEmuVersion: () => invoke("dlc_emu_version"),
  bundlePlan: (source, dlc, backport) => invoke("bundle_plan", { source, dlc, backport }),
  pickDlcFolder: async () => single(await open({ directory: true, multiple: false })),
  pickBackport: async () => single(await open({ directory: true, multiple: false })),
  library: () => invoke("library"),
  installedTitle: (titleId) => invoke("installed_title", { titleId }),
  libraryIcons: (ids) => invoke("library_icons", { ids }),
  measureApp: (titleId) => invoke("measure_app", { titleId }),
  freeSpace: (path) => invoke("free_space", { path }),
  startHelper: () => invoke("start_helper"),
  checkDlc: (path, titleId) => invoke("check_dlc", { path, titleId }),
  dlcInFolder: (path, titleId) => invoke("dlc_in_folder", { path, titleId }),
  pickDlcFiles: async () => {
    const picked = await open({ multiple: true, filters: [{ name: "DLC packages", extensions: ["pkg"] }] });
    if (picked === null) return [];
    return Array.isArray(picked) ? picked : [picked];
  },
  history: () => invoke("load_history"),
  addHistory: (entry) => invoke("add_history", { entry }),
  clearHistory: () => invoke("clear_history"),
  appVersion: () => getVersion(),
  gameDetails: (title, titleId) => details().details(title, titleId),
  searchGames: (q) => details().search(q),
  pickGame: async (title, titleId, igdbId) => details().pick(title, titleId, igdbId),
  detailsOn: async () => detailsOn(localStorage),
  setDetailsOn: async (on) => setDetailsOn(localStorage, on),
  checkUpdate: () => invoke("check_update"),
  installUpdate: (update) => invoke("install_update", { update }),
  restartApp: () => invoke("restart_app"),
  onUpdateProgress: (cb) => unlistener(listen<{ done: number; total: number }>("kite://update", (e) => cb(e.payload))),
  updatePrefs: async () => readPrefs(localStorage),
  setUpdatePrefs: async (prefs) => writePrefs(localStorage, prefs),
  onEvent: (cb) => unlistener(listen<UiEvent>("kite://event", (e) => cb(e.payload))),
  onDrop: (cb) =>
    unlistener(
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "drop") cb({ type: "drop", paths: p.paths });
        else if (p.type === "leave") cb({ type: "leave" });
        else cb({ type: "over" });
      }),
    ),
};
