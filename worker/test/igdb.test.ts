import { describe, expect, it } from "vitest";
import {
  cacheKey,
  cleanTitle,
  idQuery,
  nameKey,
  parseRoute,
  pickMatch,
  searchQuery,
  shapeCandidate,
  shapeGame,
  titleQuery,
  type IgdbGame,
} from "../src/igdb";

const url = (path: string) => new URL(`https://kite-details.example.workers.dev${path}`);

describe("cleanTitle", () => {
  it("drops trademark signs and curly quotes", () => {
    expect(cleanTitle("Marvel’s Spider-Man 2™")).toBe("Marvel's Spider-Man 2");
    expect(cleanTitle("  Ratchet &   Clank®: Rift Apart ")).toBe("Ratchet & Clank: Rift Apart");
  });
});

describe("nameKey", () => {
  it("matches names written differently", () => {
    expect(nameKey("Marvel’s Spider-Man 2")).toBe(nameKey("MARVEL'S SPIDER MAN 2"));
    expect(nameKey("Pokémon")).toBe("pokemon");
  });
});

describe("parseRoute", () => {
  it("reads the three questions", () => {
    expect(parseRoute(url("/"))).toEqual({ kind: "hello" });
    expect(parseRoute(url("/v1/game?title=Astro%20Bot%E2%84%A2"))).toEqual({ kind: "title", title: "Astro Bot" });
    expect(parseRoute(url("/v1/game/119133"))).toEqual({ kind: "id", id: 119133 });
    expect(parseRoute(url("/v1/search?q=returnal"))).toEqual({ kind: "search", q: "returnal" });
  });

  it("refuses anything else", () => {
    expect(parseRoute(url("/v1/game"))).toBeNull();
    expect(parseRoute(url(`/v1/game?title=${"a".repeat(121)}`))).toBeNull();
    expect(parseRoute(url("/v1/game/12a"))).toBeNull();
    expect(parseRoute(url("/v1/search?q=%20%20"))).toBeNull();
    expect(parseRoute(url("/v4/games"))).toBeNull();
  });
});

describe("cacheKey", () => {
  it("keeps one entry for one name written two ways", () => {
    expect(cacheKey({ kind: "title", title: "Marvel’s Spider-Man 2" })).toBe(cacheKey({ kind: "title", title: "marvel's spider-man 2" }));
    expect(cacheKey({ kind: "id", id: 7 })).toBe("i:7");
  });
});

describe("queries", () => {
  it("search PS5 games by name, with quotes made safe", () => {
    const q = titleQuery('Say "hi"', true);
    expect(q).toContain('search "Say \\"hi\\"";');
    expect(q).toContain("where platforms = (167);");
    expect(titleQuery("Astro Bot", false)).not.toContain("where");
  });

  it("asks one game by id, and a few names for Wrong game?", () => {
    expect(idQuery(42)).toContain("where id = 42; limit 1;");
    expect(searchQuery("astro")).toBe('search "astro"; fields name,first_release_date,cover.image_id; limit 10;');
  });
});

describe("pickMatch", () => {
  const games: IgdbGame[] = [
    { id: 1, name: "Marvel's Spider-Man 2: Digital Deluxe Edition" },
    { id: 2, name: "Marvel's Spider-Man 2" },
    { id: 3, name: "Marvel's Spider-Man: Miles Morales" },
  ];

  it("takes the same name first", () => {
    expect(pickMatch(games, "Marvel’s Spider-Man 2™")?.id).toBe(2);
  });

  it("then a name that starts the same way", () => {
    expect(pickMatch(games.slice(0, 1), "Marvel's Spider-Man 2")?.id).toBe(1);
    expect(pickMatch([{ id: 9, name: "Astro Bot" }], "Astro Bot Digital Deluxe")?.id).toBe(9);
  });

  it("else IGDB's first, or nothing", () => {
    expect(pickMatch([{ id: 5, name: "Something" }], "Other")?.id).toBe(5);
    expect(pickMatch([], "Other")).toBeNull();
  });
});

describe("shapeGame", () => {
  it("keeps what the page shows", () => {
    const shaped = shapeGame({
      id: 2,
      name: "Marvel's Spider-Man 2",
      summary: "  A story.  ",
      first_release_date: 1697760000,
      genres: [{ name: "Adventure" }, { name: "Action" }, { name: "Fighting" }, { name: "Extra" }],
      game_modes: [{ name: "Single player" }],
      involved_companies: [
        { publisher: true, company: { name: "Sony Interactive Entertainment" } },
        { developer: true, company: { name: "Insomniac Games" } },
      ],
      cover: { image_id: "co6zdl" },
      artworks: [{ image_id: "ar2x7k" }, { image_id: "bad id!" }],
      screenshots: [{ image_id: "sc1" }, {}],
    });
    expect(shaped).toEqual({
      id: 2,
      name: "Marvel's Spider-Man 2",
      summary: "A story.",
      released: 1697760000,
      genres: ["Adventure", "Action", "Fighting"],
      modes: ["Single player"],
      developer: "Insomniac Games",
      publisher: "Sony Interactive Entertainment",
      cover: "co6zdl",
      artworks: ["ar2x7k"],
      screenshots: ["sc1"],
    });
  });

  it("copes with a bare game", () => {
    expect(shapeGame({ id: 1, name: "Bare" })).toMatchObject({ summary: null, released: null, genres: [], developer: null, cover: null });
  });
});

describe("shapeCandidate", () => {
  it("gives the year and cover", () => {
    expect(shapeCandidate({ id: 3, name: "Returnal", first_release_date: 1619740800, cover: { image_id: "co2" } })).toEqual({
      id: 3,
      name: "Returnal",
      year: 2021,
      cover: "co2",
    });
  });
});
