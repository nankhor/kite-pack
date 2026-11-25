// A game's story, studio and pictures from IGDB, through Kite Pack's own Worker (worker/ in the
// repo), which holds the IGDB keys. Each game is looked up once and kept on this computer.

/** Where the Worker (worker/) answers. Empty: no lookups, and the page shows the game's own icon. */
export const DETAILS_URL = "https://kite-details.nankhor.workers.dev";

/** One game, as the Worker answers. Pictures are IGDB image ids: see igdbImage. */
export interface GameDetails {
  id: number;
  name: string;
  summary: string | null;
  /** Unix seconds. */
  released: number | null;
  genres: string[];
  modes: string[];
  developer: string | null;
  publisher: string | null;
  cover: string | null;
  artworks: string[];
  screenshots: string[];
}

/** A game "Wrong game?" can pick instead. */
export interface Candidate {
  id: number;
  name: string;
  year: number | null;
  cover: string | null;
}

export type ImageSize = "cover_small" | "cover_big" | "screenshot_med" | "screenshot_big" | "720p" | "1080p";

/** IGDB's picture links need no key. */
export function igdbImage(id: string, size: ImageSize): string {
  // An IGDB id never holds a ":"; a whole address (the demo's drawings) is used as it is.
  if (id.includes(":")) return id;
  return `https://images.igdb.com/igdb/image/upload/t_${size}/${id}.jpg`;
}

/** The wide picture behind the title: the game's art, else its first screenshot. */
export function heroImage(d: GameDetails): string | null {
  const id = d.artworks[0] ?? d.screenshots[0] ?? null;
  return id ? igdbImage(id, "1080p") : null;
}

/** "20 October 2023". */
export function releaseDate(seconds: number): string {
  return new Date(seconds * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
}

/** "Insomniac Games · Sony Interactive Entertainment · 20 October 2023": who made it and when. */
export function makers(d: GameDetails): string {
  const who = [d.developer, d.publisher !== d.developer ? d.publisher : null].filter((x): x is string => !!x);
  if (d.released) who.push(releaseDate(d.released));
  return who.join(" · ");
}

/** Genres, then how it's played: the tags under the art. */
export function tags(d: GameDetails): string[] {
  return [...d.genres, ...d.modes];
}

/** Where details are kept: localStorage in the app, a Map in tests. */
export interface KeptStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

interface Kept {
  /** Milliseconds since 1970. */
  at: number;
  /** The IGDB game picked with "Wrong game?", or null for the best match. */
  pick: number | null;
  details: GameDetails | null;
}

const KEEP_MS = 30 * 24 * 3600 * 1000;
const keyOf = (title: string, titleId: string | null) => titleId ?? title.toLowerCase();

/** Looks games up through the Worker, keeping each answer for 30 days. */
export function detailsClient(base: string, get: typeof fetch, store: KeptStore, now: () => number = Date.now) {
  const keptKey = (k: string) => `kite.details.${k}`;
  const pickKey = (k: string) => `kite.details.pick.${k}`;

  function readKept(k: string): Kept | null {
    try {
      const raw = store.getItem(keptKey(k));
      return raw ? (JSON.parse(raw) as Kept) : null;
    } catch {
      return null;
    }
  }

  function picked(k: string): number | null {
    const raw = store.getItem(pickKey(k));
    const id = raw ? Number(raw) : NaN;
    return Number.isInteger(id) && id > 0 ? id : null;
  }

  async function ask<T>(path: string): Promise<T> {
    if (!base) throw new Error("Game details aren't set up");
    const res = await get(`${base}${path}`);
    if (!res.ok) throw new Error(`The details server answered ${res.status}`);
    return (await res.json()) as T;
  }

  return {
    /** The game's details, null when IGDB doesn't know it; rejects when the Worker can't be reached and nothing is kept. */
    async details(title: string, titleId: string | null): Promise<GameDetails | null> {
      const k = keyOf(title, titleId);
      const pick = picked(k);
      const kept = readKept(k);
      if (kept && kept.pick === pick && now() - kept.at < KEEP_MS) return kept.details;
      try {
        const path = pick ? `/v1/game/${pick}` : `/v1/game?title=${encodeURIComponent(title)}`;
        const body = await ask<{ found: boolean; game?: GameDetails }>(path);
        const details = body.found && body.game ? body.game : null;
        store.setItem(keptKey(k), JSON.stringify({ at: now(), pick, details } satisfies Kept));
        return details;
      } catch (e) {
        // Offline: an old answer for the same pick beats none.
        if (kept && kept.pick === pick) return kept.details;
        throw e;
      }
    },

    async search(q: string): Promise<Candidate[]> {
      const body = await ask<{ results: Candidate[] }>(`/v1/search?q=${encodeURIComponent(q)}`);
      return body.results;
    },

    /** Shows `id` for this game from now on; null goes back to the best match. */
    pick(title: string, titleId: string | null, id: number | null): void {
      const k = keyOf(title, titleId);
      if (id === null) store.removeItem(pickKey(k));
      else store.setItem(pickKey(k), String(id));
    },
  };
}

const OFF_KEY = "kite.details.off";

/** Whether game details are on (they are unless switched off in Settings). */
export function detailsOn(store: KeptStore): boolean {
  return store.getItem(OFF_KEY) !== "1";
}

export function setDetailsOn(store: KeptStore, on: boolean): void {
  if (on) store.removeItem(OFF_KEY);
  else store.setItem(OFF_KEY, "1");
}
