// Kite Pack's own updates: what the app asks GitHub, and the two Settings switches.

/** A newer Kite Pack, as the app's check_update answers (kite-update's Update). */
export interface UpdateInfo {
  version: string;
  tag: string;
  /** The release notes. */
  notes: string;
  page: string;
  /** This computer's file. */
  file: string;
  url: string;
  size: number;
  sumsUrl: string | null;
}

export interface UpdatePrefs {
  /** Look for a newer version when the app opens. */
  check: boolean;
  /** Download and install it without asking; the restart is still asked. */
  auto: boolean;
}

export const DEFAULT_PREFS: UpdatePrefs = { check: true, auto: false };

/** Where the update stands, for the banner under the tabs. */
export type UpdateView =
  | { state: "none" }
  | { state: "available"; info: UpdateInfo }
  | { state: "downloading"; info: UpdateInfo; done: number; total: number }
  | { state: "ready"; info: UpdateInfo }
  | { state: "failed"; info: UpdateInfo; reason: string };

interface Store {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const KEY = "kite.updates";

export function readPrefs(store: Store): UpdatePrefs {
  try {
    const raw = store.getItem(KEY);
    if (!raw) return DEFAULT_PREFS;
    const p = JSON.parse(raw) as Partial<UpdatePrefs>;
    return { check: p.check !== false, auto: p.auto === true };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function writePrefs(store: Store, prefs: UpdatePrefs): void {
  store.setItem(KEY, JSON.stringify(prefs));
}

/** "42%" of a download, or nothing before the size is known. */
export function percent(done: number, total: number): string {
  if (total <= 0) return "";
  return `${Math.min(100, Math.floor((done / total) * 100))}%`;
}

/** Release notes as lines to show: list marks and headings dropped, blank lines skipped. */
export function noteLines(notes: string): string[] {
  return notes
    .split("\n")
    .map((l) => l.replace(/^\s*(?:[-*]|#+)\s*/, "").trim())
    .filter((l) => l.length > 0);
}
