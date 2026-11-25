import { describe, expect, it, vi } from "vitest";
import {
  detailsClient,
  detailsOn,
  heroImage,
  igdbImage,
  makers,
  releaseDate,
  setDetailsOn,
  tags,
  type GameDetails,
  type KeptStore,
} from "./details";

const SPIDER: GameDetails = {
  id: 2,
  name: "Marvel's Spider-Man 2",
  summary: "A story.",
  released: 1697760000,
  genres: ["Adventure"],
  modes: ["Single player"],
  developer: "Insomniac Games",
  publisher: "Sony Interactive Entertainment",
  cover: "co1",
  artworks: ["ar1"],
  screenshots: ["sc1", "sc2"],
};

function memoryStore(): KeptStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
    removeItem: (k) => void data.delete(k),
  };
}

const answer = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });

describe("pictures and words", () => {
  it("links IGDB pictures by size", () => {
    expect(igdbImage("sc1", "screenshot_med")).toBe("https://images.igdb.com/igdb/image/upload/t_screenshot_med/sc1.jpg");
    // A picture given as a whole address (the demo's drawings) is used as it is.
    expect(igdbImage("data:image/svg+xml;utf8,x", "1080p")).toBe("data:image/svg+xml;utf8,x");
  });

  it("puts the game's art behind the title, else a screenshot, else nothing", () => {
    expect(heroImage(SPIDER)).toContain("/t_1080p/ar1.jpg");
    expect(heroImage({ ...SPIDER, artworks: [] })).toContain("/t_1080p/sc1.jpg");
    expect(heroImage({ ...SPIDER, artworks: [], screenshots: [] })).toBeNull();
  });

  it("says who made it and when", () => {
    expect(releaseDate(1697760000)).toBe("20 October 2023");
    expect(makers(SPIDER)).toBe("Insomniac Games · Sony Interactive Entertainment · 20 October 2023");
    expect(makers({ ...SPIDER, publisher: "Insomniac Games", released: null })).toBe("Insomniac Games");
    expect(makers({ ...SPIDER, developer: null, publisher: null, released: null })).toBe("");
  });

  it("tags genres, then how it's played", () => {
    expect(tags(SPIDER)).toEqual(["Adventure", "Single player"]);
  });
});

describe("detailsClient", () => {
  const BASE = "https://kite-details.example.workers.dev";

  function setup() {
    const store = memoryStore();
    let t = 1_000_000;
    const get = vi.fn(async (_url: string | URL | Request) => answer({ found: true, game: SPIDER }));
    const client = detailsClient(BASE, get as unknown as typeof fetch, store, () => t);
    return { store, get, client, later: (ms: number) => (t += ms) };
  }

  it("looks a game up by name once, then keeps it", async () => {
    const { get, client } = setup();
    expect(await client.details("Marvel’s Spider-Man 2", "PPSA08338")).toEqual(SPIDER);
    expect(get).toHaveBeenCalledWith(`${BASE}/v1/game?title=Marvel%E2%80%99s%20Spider-Man%202`);
    expect(await client.details("Marvel’s Spider-Man 2", "PPSA08338")).toEqual(SPIDER);
    expect(get).toHaveBeenCalledTimes(1);
  });

  it("asks again after 30 days", async () => {
    const { get, client, later } = setup();
    await client.details("Astro Bot", "PPSA01325");
    later(31 * 24 * 3600 * 1000);
    await client.details("Astro Bot", "PPSA01325");
    expect(get).toHaveBeenCalledTimes(2);
  });

  it("keeps 'not found' too", async () => {
    const { get, client } = setup();
    get.mockResolvedValueOnce(answer({ found: false }));
    expect(await client.details("Homebrew", null)).toBeNull();
    expect(await client.details("Homebrew", null)).toBeNull();
    expect(get).toHaveBeenCalledTimes(1);
  });

  it("asks for the picked game, and the best match again once the pick is cleared", async () => {
    const { get, client } = setup();
    await client.details("Spider-Man 2", "PPSA08338");
    client.pick("Spider-Man 2", "PPSA08338", 1234);
    await client.details("Spider-Man 2", "PPSA08338");
    expect(get).toHaveBeenLastCalledWith(`${BASE}/v1/game/1234`);
    client.pick("Spider-Man 2", "PPSA08338", null);
    await client.details("Spider-Man 2", "PPSA08338");
    expect(get).toHaveBeenLastCalledWith(`${BASE}/v1/game?title=Spider-Man%202`);
  });

  it("offline, gives the old answer if it has one, else fails", async () => {
    const { get, client, later } = setup();
    await client.details("Astro Bot", "PPSA01325");
    later(31 * 24 * 3600 * 1000);
    get.mockRejectedValue(new TypeError("Failed to fetch"));
    expect(await client.details("Astro Bot", "PPSA01325")).toEqual(SPIDER);
    await expect(client.details("Returnal", "PPSA01284")).rejects.toThrow("Failed to fetch");
  });

  it("fails on a server error, and when no server is set", async () => {
    const { get, client } = setup();
    get.mockResolvedValueOnce(answer({ error: "x" }, 502));
    await expect(client.details("Astro Bot", null)).rejects.toThrow("502");
    await expect(detailsClient("", get as unknown as typeof fetch, memoryStore()).details("Astro Bot", null)).rejects.toThrow("set up");
  });

  it("searches for Wrong game?", async () => {
    const { get, client } = setup();
    get.mockResolvedValueOnce(answer({ results: [{ id: 2, name: "Marvel's Spider-Man 2", year: 2023, cover: null }] }));
    expect(await client.search("spider man")).toEqual([{ id: 2, name: "Marvel's Spider-Man 2", year: 2023, cover: null }]);
    expect(get).toHaveBeenCalledWith(`${BASE}/v1/search?q=spider%20man`);
  });
});

describe("the details switch", () => {
  it("is on until switched off", () => {
    const store = memoryStore();
    expect(detailsOn(store)).toBe(true);
    setDetailsOn(store, false);
    expect(detailsOn(store)).toBe(false);
    setDetailsOn(store, true);
    expect(detailsOn(store)).toBe(true);
  });
});
