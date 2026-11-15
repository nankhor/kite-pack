import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Env } from "../src/index";

/** A KV store in memory, with only what the Worker uses. */
function fakeKv() {
  const data = new Map<string, string>();
  const kv = {
    data,
    get: vi.fn(async (key: string, type?: string) => {
      const v = data.get(key);
      return v === undefined ? null : type === "json" ? JSON.parse(v) : v;
    }),
    put: vi.fn(async (key: string, value: string) => void data.set(key, value)),
  };
  return kv;
}

const SPIDER = {
  id: 2,
  name: "Marvel's Spider-Man 2",
  summary: "A story.",
  genres: [{ name: "Adventure" }],
  screenshots: [{ image_id: "sc1" }],
};

// A fresh copy of the Worker for each test, so no token is left in memory from the last.
let worker: (typeof import("../src/index"))["default"];
let kv: ReturnType<typeof fakeKv>;
let env: Env;
let calls: { url: string; body?: string; auth?: string }[];
let igdbAnswers: { status: number; body: unknown }[];

function call(path: string, init?: RequestInit) {
  return worker.fetch(new Request(`https://kite-details.example.workers.dev${path}`, init), env);
}

beforeEach(async () => {
  vi.resetModules();
  worker = (await import("../src/index")).default;
  kv = fakeKv();
  env = { TWITCH_CLIENT_ID: "id", TWITCH_CLIENT_SECRET: "secret", CACHE: kv as unknown as KVNamespace };
  calls = [];
  igdbAnswers = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, body: init?.body as string | undefined, auth: (init?.headers as Record<string, string> | undefined)?.Authorization });
      if (url.startsWith("https://id.twitch.tv/")) {
        return new Response(JSON.stringify({ access_token: `tok${calls.length}`, expires_in: 5_000_000 }));
      }
      const next = igdbAnswers.shift() ?? { status: 200, body: [] };
      return new Response(JSON.stringify(next.body), { status: next.status });
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

describe("the Worker", () => {
  it("answers a name with the best PS5 match, and keeps it", async () => {
    igdbAnswers.push({ status: 200, body: [SPIDER] });
    const res = await call("/v1/game?title=Marvel%E2%80%99s%20Spider-Man%202");
    expect(res.status).toBe(200);
    expect(res.headers.get("access-control-allow-origin")).toBe("*");
    const body = (await res.json()) as { found: boolean; game: { name: string; screenshots: string[] } };
    expect(body.found).toBe(true);
    expect(body.game.name).toBe("Marvel's Spider-Man 2");
    expect(body.game.screenshots).toEqual(["sc1"]);
    expect(calls[1].body).toContain("where platforms = (167);");
    expect(calls[1].auth).toBe("Bearer tok1");
    expect(kv.data.has("t:marvel s spider man 2")).toBe(true);

    // Asked again: from KV, IGDB isn't asked.
    const again = await call("/v1/game?title=marvel's%20spider-man%202");
    expect(((await again.json()) as { found: boolean }).found).toBe(true);
    expect(calls).toHaveLength(2);
  });

  it("tries all platforms when no PS5 game has the name", async () => {
    igdbAnswers.push({ status: 200, body: [] }, { status: 200, body: [SPIDER] });
    const body = (await (await call("/v1/game?title=Spider")).json()) as { found: boolean };
    expect(body.found).toBe(true);
    expect(calls[2].body).not.toContain("where");
  });

  it("says when nothing is found", async () => {
    const body = await (await call("/v1/game?title=Nothing%20Like%20It")).json();
    expect(body).toEqual({ found: false });
  });

  it("gets a new token when Twitch ended the old one", async () => {
    igdbAnswers.push({ status: 401, body: {} }, { status: 200, body: [SPIDER] });
    const res = await call("/v1/game/2");
    expect(res.status).toBe(200);
    const auths = calls.filter((c) => c.url.includes("igdb")).map((c) => c.auth);
    expect(auths).toEqual(["Bearer tok1", "Bearer tok3"]);
  });

  it("searches for Wrong game?", async () => {
    igdbAnswers.push({ status: 200, body: [{ id: 2, name: "Marvel's Spider-Man 2", first_release_date: 1697760000 }] });
    const body = await (await call("/v1/search?q=spider-man")).json();
    expect(body).toEqual({ results: [{ id: 2, name: "Marvel's Spider-Man 2", year: 2023, cover: null }] });
  });

  it("says when IGDB doesn't answer, and keeps nothing", async () => {
    igdbAnswers.push({ status: 500, body: {} });
    const res = await call("/v1/game?title=Astro%20Bot");
    expect(res.status).toBe(502);
    expect(kv.put.mock.calls.filter(([k]) => k !== "twitch-token")).toHaveLength(0);
  });

  it("refuses other methods and paths, and answers CORS checks", async () => {
    expect((await call("/v1/game?title=x", { method: "POST" })).status).toBe(405);
    expect((await call("/v4/games")).status).toBe(404);
    const pre = await call("/v1/game?title=x", { method: "OPTIONS" });
    expect(pre.status).toBe(204);
    expect(pre.headers.get("access-control-allow-methods")).toContain("GET");
  });

  it("limits lookups that reach IGDB", async () => {
    env.LIMIT = { limit: vi.fn(async () => ({ success: false })) } as unknown as RateLimit;
    const res = await call("/v1/game?title=Astro%20Bot", { headers: { "cf-connecting-ip": "1.2.3.4" } });
    expect(res.status).toBe(429);
    expect(calls).toHaveLength(0);
  });
});
