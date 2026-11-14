// Everything the screen asks of the app, behind one interface so tests can stand in for it.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type {
  Found,
  FreeSpace,
  GameIcon,
  GameInfo,
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
  /** What the PS5 has installed (reads the console; takes a couple of seconds). */
  library(): Promise<Library>;
  /** One title on the console by id, or null when it is not installed. */
  installedTitle(titleId: string): Promise<InstalledGame | null>;
  /** The tiles of those titles, as data: URLs. Titles with no icon are left out. */
  libraryIcons(ids: string[]): Promise<GameIcon[]>;
  /** App, update and DLC bytes of one installed title (walks the console's folders). */
  measureApp(titleId: string): Promise<Measured>;
  /** The room left where games go. */
  freeSpace(path: string): Promise<FreeSpace>;
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
  library: () => invoke("library"),
  installedTitle: (titleId) => invoke("installed_title", { titleId }),
  libraryIcons: (ids) => invoke("library_icons", { ids }),
  measureApp: (titleId) => invoke("measure_app", { titleId }),
  freeSpace: (path) => invoke("free_space", { path }),
  startHelper: () => invoke("start_helper"),
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
