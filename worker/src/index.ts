/**
 * kite-details: Kite Pack asks it for a game's story, studio, release date and pictures. It asks
 * IGDB with the Twitch credentials it holds as secrets, so they never ship in the app, and it keeps
 * each answer for 30 days so IGDB is asked about each game about once.
 *
 *   GET /v1/game?title=<name>  the best PS5 match for a name  → { found, game? }
 *   GET /v1/game/<igdb id>     one game, picked with "Wrong game?" → { found, game? }
 *   GET /v1/search?q=<name>    a few games of that name        → { results }
 */

import {
  cacheKey,
  idQuery,
  parseRoute,
  pickMatch,
  searchQuery,
  shapeCandidate,
  shapeGame,
  titleQuery,
  type IgdbGame,
  type Route,
} from "./igdb";

export interface Env {
  TWITCH_CLIENT_ID: string;
  TWITCH_CLIENT_SECRET: string;
  CACHE: KVNamespace;
  LIMIT?: RateLimit;
}

const DAY = 86_400;
const KEEP_FOUND = 30 * DAY;
const KEEP_MISSING = DAY;

const CORS = { "access-control-allow-origin": "*", "access-control-allow-methods": "GET, OPTIONS" };

function json(body: unknown, status = 200, maxAge = 0): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      ...CORS,
      "content-type": "application/json; charset=utf-8",
      "cache-control": maxAge ? `public, max-age=${maxAge}` : "no-store",
    },
  });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: { ...CORS, "access-control-max-age": String(DAY) } });
    }
    if (request.method !== "GET") return json({ error: "Only GET is answered." }, 405);
    const route = parseRoute(new URL(request.url));
    if (!route) return json({ error: "Not something this Worker answers." }, 404);
    if (route.kind === "hello") return json({ name: "kite-details", about: "Game details from IGDB for Kite Pack." });

    const key = cacheKey(route);
    const kept = await env.CACHE.get(key, "json");
    if (kept) return json(kept, 200, DAY);

    // Only lookups that reach IGDB count toward the limit.
    if (env.LIMIT) {
      const who = request.headers.get("cf-connecting-ip") ?? "anyone";
      const { success } = await env.LIMIT.limit({ key: who });
      if (!success) return json({ error: "Too many lookups. Try again in a minute." }, 429);
    }

    try {
      const body = await answer(route, env);
      const missing = "found" in body && body.found === false;
      await env.CACHE.put(key, JSON.stringify(body), { expirationTtl: missing ? KEEP_MISSING : KEEP_FOUND });
      return json(body, 200, DAY);
    } catch {
      return json({ error: "IGDB didn't answer. Try again later." }, 502);
    }
  },
} satisfies ExportedHandler<Env>;

async function answer(route: Exclude<Route, { kind: "hello" }>, env: Env): Promise<Record<string, unknown>> {
  switch (route.kind) {
    case "title": {
      let games = await igdb(env, titleQuery(route.title, true));
      if (games.length === 0) games = await igdb(env, titleQuery(route.title, false));
      const game = pickMatch(games, route.title);
      return game ? { found: true, game: shapeGame(game) } : { found: false };
    }
    case "id": {
      const [game] = await igdb(env, idQuery(route.id));
      return game ? { found: true, game: shapeGame(game) } : { found: false };
    }
    case "search":
      return { results: (await igdb(env, searchQuery(route.q))).map(shapeCandidate) };
  }
}

interface Token {
  token: string;
  /** Milliseconds since 1970. */
  until: number;
}

let memo: Token | null = null;

/** Twitch's app token: kept in memory and in KV, and asked for again an hour before it ends. */
async function appToken(env: Env, fresh: boolean): Promise<string> {
  const now = Date.now();
  if (!fresh && memo && memo.until > now) return memo.token;
  if (!fresh) {
    const kept = await env.CACHE.get<Token>("twitch-token", "json");
    if (kept && kept.until > now) {
      memo = kept;
      return kept.token;
    }
  }
  const url = new URL("https://id.twitch.tv/oauth2/token");
  url.search = new URLSearchParams({
    client_id: env.TWITCH_CLIENT_ID,
    client_secret: env.TWITCH_CLIENT_SECRET,
    grant_type: "client_credentials",
  }).toString();
  const res = await fetch(url, { method: "POST" });
  if (!res.ok) throw new Error(`Twitch answered ${res.status}`);
  const body = await res.json<{ access_token: string; expires_in: number }>();
  const life = Math.max(60, body.expires_in - 3600);
  memo = { token: body.access_token, until: now + life * 1000 };
  await env.CACHE.put("twitch-token", JSON.stringify(memo), { expirationTtl: life });
  return memo.token;
}

async function igdb(env: Env, query: string, retried = false): Promise<IgdbGame[]> {
  const res = await fetch("https://api.igdb.com/v4/games", {
    method: "POST",
    headers: {
      "Client-ID": env.TWITCH_CLIENT_ID,
      Authorization: `Bearer ${await appToken(env, retried)}`,
      Accept: "application/json",
    },
    body: query,
  });
  // A token Twitch has ended early: get a new one, once.
  if (res.status === 401 && !retried) return igdb(env, query, true);
  if (!res.ok) throw new Error(`IGDB answered ${res.status}`);
  return res.json<IgdbGame[]>();
}
