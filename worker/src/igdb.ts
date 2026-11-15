/**
 * The Worker's plain parts, with no network: what a request asks for, the IGDB queries, picking
 * the right game among IGDB's results, and the shape of what goes back to Kite Pack.
 */

/** IGDB's id for the PlayStation 5. */
export const PS5 = 167;
/** Longer than any game name; longer requests are refused. */
export const MAX_TEXT = 120;

export type Route =
  | { kind: "hello" }
  | { kind: "title"; title: string }
  | { kind: "id"; id: number }
  | { kind: "search"; q: string };

/** A game's name as IGDB writes it: no ™ ® ©, plain quotes, single spaces. */
export function cleanTitle(raw: string): string {
  return raw
    .replace(/[™®©]/g, "")
    .replace(/[‘’ʼ]/g, "'")
    .replace(/[“”]/g, '"')
    .replace(/\s+/g, " ")
    .trim();
}

/** For comparing names: lower case, no accents, letters and digits only. */
export function nameKey(name: string): string {
  return cleanTitle(name)
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

/** What a request asks for, or null when it's nothing this Worker answers. */
export function parseRoute(url: URL): Route | null {
  const path = url.pathname.replace(/\/+$/, "") || "/";
  if (path === "/") return { kind: "hello" };
  if (path === "/v1/game") {
    const title = cleanTitle(url.searchParams.get("title") ?? "");
    return title && title.length <= MAX_TEXT ? { kind: "title", title } : null;
  }
  const byId = /^\/v1\/game\/(\d{1,10})$/.exec(path);
  if (byId) return { kind: "id", id: Number(byId[1]) };
  if (path === "/v1/search") {
    const q = cleanTitle(url.searchParams.get("q") ?? "");
    return q && q.length <= MAX_TEXT ? { kind: "search", q } : null;
  }
  return null;
}

/** Where a route's answer is kept: the same game asked two ways is one entry. */
export function cacheKey(route: Exclude<Route, { kind: "hello" }>): string {
  switch (route.kind) {
    case "title":
      return `t:${nameKey(route.title)}`;
    case "id":
      return `i:${route.id}`;
    case "search":
      return `s:${nameKey(route.q)}`;
  }
}

const quoted = (s: string) => `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;

export const GAME_FIELDS = [
  "name",
  "summary",
  "first_release_date",
  "genres.name",
  "game_modes.name",
  "involved_companies.developer",
  "involved_companies.publisher",
  "involved_companies.company.name",
  "cover.image_id",
  "artworks.image_id",
  "screenshots.image_id",
].join(",");

/** A search by name; PS5 games only, unless that finds nothing. */
export function titleQuery(title: string, ps5Only: boolean): string {
  const where = ps5Only ? ` where platforms = (${PS5});` : "";
  return `search ${quoted(title)}; fields ${GAME_FIELDS};${where} limit 10;`;
}

export function idQuery(id: number): string {
  return `fields ${GAME_FIELDS}; where id = ${id}; limit 1;`;
}

/** For "Wrong game?": a few games of that name, any platform. */
export function searchQuery(q: string): string {
  return `search ${quoted(q)}; fields name,first_release_date,cover.image_id; limit 10;`;
}

interface Image {
  image_id?: string;
}

/** A game as IGDB answers, with only the fields asked for. */
export interface IgdbGame {
  id: number;
  name: string;
  summary?: string;
  first_release_date?: number;
  genres?: { name?: string }[];
  game_modes?: { name?: string }[];
  involved_companies?: { developer?: boolean; publisher?: boolean; company?: { name?: string } }[];
  cover?: Image;
  artworks?: Image[];
  screenshots?: Image[];
}

/** What Kite Pack gets for a game. Pictures are IGDB image ids; the app makes the links. */
export interface Details {
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

export interface Candidate {
  id: number;
  name: string;
  year: number | null;
  cover: string | null;
}

/** The game whose name matches, else one whose name starts the same way, else IGDB's first. */
export function pickMatch(games: IgdbGame[], title: string): IgdbGame | null {
  if (games.length === 0) return null;
  const want = nameKey(title);
  const exact = games.find((g) => nameKey(g.name) === want);
  if (exact) return exact;
  const close = games.find((g) => {
    const have = nameKey(g.name);
    return have.length >= 4 && (have.startsWith(want) || want.startsWith(have));
  });
  return close ?? games[0];
}

const imageId = (i?: Image) => (i?.image_id && /^[a-z0-9]+$/i.test(i.image_id) ? i.image_id : null);
const imageIds = (list: Image[] | undefined, max: number) =>
  (list ?? []).map(imageId).filter((id): id is string => id !== null).slice(0, max);
const names = (list: { name?: string }[] | undefined, max: number) =>
  (list ?? []).map((x) => x.name).filter((n): n is string => !!n).slice(0, max);

function company(game: IgdbGame, role: "developer" | "publisher"): string | null {
  return game.involved_companies?.find((c) => c[role] && c.company?.name)?.company?.name ?? null;
}

export function shapeGame(game: IgdbGame): Details {
  const summary = game.summary?.trim();
  return {
    id: game.id,
    name: game.name,
    summary: summary ? summary.slice(0, 1200) : null,
    released: game.first_release_date ?? null,
    genres: names(game.genres, 3),
    modes: names(game.game_modes, 2),
    developer: company(game, "developer"),
    publisher: company(game, "publisher"),
    cover: imageId(game.cover),
    artworks: imageIds(game.artworks, 6),
    screenshots: imageIds(game.screenshots, 12),
  };
}

export function shapeCandidate(game: IgdbGame): Candidate {
  return {
    id: game.id,
    name: game.name,
    year: game.first_release_date ? new Date(game.first_release_date * 1000).getUTCFullYear() : null,
    cover: imageId(game.cover),
  };
}
