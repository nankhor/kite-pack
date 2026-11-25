// A stand-in for the app, to look at every page in a plain browser: `npm run dev`, then open
// http://localhost:5173/?demo (add `&open=pack`, `&open=install` or `&open=packing` to start
// with a game in front). Made-up games with drawn art; nothing here reaches a PS5. Loaded only by
// the dev server (main.tsx), never in a build.

import type { Backend, DropEvent } from "./backend";
import type { GameDetails } from "./details";
import type { UpdatePrefs } from "./updates";
import type { GameInfo, HistoryEntry, InstalledGame, JobView, Measured, Settings, StepView, UiEvent } from "./types";

type Palette = [string, string, string, string];

const GAMES: Record<string, { name: string; id: string; palette: Palette; story: string }> = {
  ashen: {
    name: "Ashen Tide",
    id: "PPSA01234",
    palette: ["#123f4e", "#f3e4c2", "#0f5163", "#07303b"],
    story:
      "A lighthouse keeper rows out into a sea of drifting ash to find where the tide went. Sail between burning islands, read the currents and bring the light back before the last ember goes out.",
  },
  ember: { name: "Ember Circuit", id: "PPSA04410", palette: ["#2a1440", "#ff9b4a", "#5b2a7a", "#170a24"], story: "Night races through a city that runs on stolen sparks." },
  hollow: { name: "Hollow Garden", id: "PPSA02288", palette: ["#1f3b2a", "#d9f2b4", "#2f5c3c", "#132619"], story: "Tend a garden that grows inside an abandoned train." },
  moons: { name: "Paper Moons", id: "PPSA07731", palette: ["#e8e1f5", "#7a5cc4", "#cfc3ea", "#b5a6de"], story: "Fold the night sky back together, one moon at a time." },
  iron: { name: "Iron Lantern", id: "PPSA03905", palette: ["#2b2b2f", "#f0c060", "#45454c", "#18181b"], story: "A lamplighter keeps a sleeping city safe from what walks in the fog." },
  north: { name: "Northbound", id: "PPSA05172", palette: ["#cfe0ee", "#ffffff", "#9cbad3", "#6e93b3"], story: "Drive a sled team to the top of the world before winter closes the pass." },
};

const svg = (body: string, w: number, h: number) =>
  `data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}">${body}</svg>`)}`;

function art(key: string, w = 1600, h = 900): string {
  const [b, l, m, d] = GAMES[key].palette;
  return svg(
    `<rect width="${w}" height="${h}" fill="${b}"/><circle cx="${w * 0.72}" cy="${h * 0.32}" r="${h * 0.16}" fill="${l}"/>` +
      `<path d="M0 ${h * 0.62} C${w * 0.18} ${h * 0.52} ${w * 0.32} ${h * 0.78} ${w * 0.5} ${h * 0.64} S${w * 0.8} ${h * 0.52} ${w} ${h * 0.66} V${h} H0Z" fill="${m}"/>` +
      `<path d="M0 ${h * 0.82} C${w * 0.2} ${h * 0.75} ${w * 0.4} ${h * 0.92} ${w * 0.6} ${h * 0.81} S${w * 0.85} ${h * 0.77} ${w} ${h * 0.85} V${h} H0Z" fill="${d}"/>`,
    w,
    h,
  );
}

function icon(key: string): string {
  const [b, l] = GAMES[key].palette;
  return svg(`<rect width="96" height="96" fill="${b}"/><circle cx="60" cy="36" r="18" fill="${l}"/><path d="M8 68c16-10 28 6 44-2s24-8 36-2v32H8z" fill="${l}" opacity=".85"/>`, 96, 96);
}

function game(key: string, extra: Partial<GameInfo> = {}): GameInfo {
  const g = GAMES[key];
  return {
    kind: "folder",
    title: g.name,
    titleId: g.id,
    contentId: `UP0000-${g.id}_00-${g.name.replace(/\W/g, "").toUpperCase().padEnd(16, "0").slice(0, 16)}`,
    version: "01.050.000",
    totalBytes: 84_200_000_000,
    dlcs: [],
    iconDataUrl: icon(key),
    findings: [],
    ...extra,
  };
}

const PACKAGE = game("ashen", {
  kind: "pkg",
  totalBytes: 84_300_000_000,
  package: {
    platform: "ps5",
    kind: "game",
    titleId: "PPSA01234",
    contentId: "UP0000-PPSA01234_00-ASHENTIDE0000000",
    title: "Ashen Tide",
    version: "01.050.000",
    bytes: 84_300_000_000,
    iconDataUrl: icon("ashen"),
    packable: true,
  },
});

const ITEMS: Record<string, GameInfo> = {
  "/Games/Ashen Tide": game("ashen", { dlcs: [{ contentId: "x-TIDEBREAKER00000", label: "TIDEBREAKER00000", name: "Tidebreaker Expansion", bytes: 1_200_000_000 }] }),
  "/Games/Hollow Garden": game("hollow", { totalBytes: 31_900_000_000, version: "01.020.000" }),
  "/Games/Ashen Tide.fpkg": PACKAGE,
};

const INSTALLED: InstalledGame[] = ["ashen", "ember", "hollow", "iron", "moons", "north"].map((k, i) => ({
  titleId: GAMES[k].id,
  title: GAMES[k].name,
  version: "01.000.000",
  updateInstalled: i % 2 === 0,
  dlc: k === "ashen" ? ["TIDEBREAKER00000"] : [],
  external: i >= 4,
  found: "appmeta+app",
}));

const HISTORY: HistoryEntry[] = [
  { id: "h1", title: "Ember Circuit", titleId: "PPSA04410", finishedAt: Date.now() - 6 * 3600_000, sourceBytes: 52_700_000_000, packedBytes: 31_200_000_000, sent: true, packed: null, dlc: 0, iconDataUrl: icon("ember") },
  { id: "h2", title: "Iron Lantern", titleId: "PPSA03905", finishedAt: Date.now() - 3 * 86400_000, sourceBytes: 96_100_000_000, packedBytes: 58_300_000_000, sent: false, packed: "/Games/Packed/Iron Lantern", dlc: 0, iconDataUrl: icon("iron") },
  { id: "h3", title: "Paper Moons", titleId: "PPSA07731", finishedAt: Date.now() - 5 * 86400_000, sourceBytes: 12_400_000_000, packedBytes: null, sent: true, packed: null, dlc: 0, iconDataUrl: icon("moons") },
];

function details(key: string): GameDetails {
  const g = GAMES[key];
  return {
    id: 1,
    name: g.name,
    summary: g.story,
    released: 1_700_000_000,
    genres: ["Adventure"],
    modes: ["Single player"],
    developer: "Driftwood Studio",
    publisher: "Driftwood Studio",
    cover: icon(key),
    artworks: [art(key)],
    screenshots: [art(key, 1280, 720), art(key === "ashen" ? "ember" : "ashen", 1280, 720), art("north", 1280, 720)],
  };
}

const keyOf = (title: string) => Object.keys(GAMES).find((k) => GAMES[k].name === title) ?? "ashen";

export function demoBackend(params: URLSearchParams): Backend {
  const listeners: ((e: UiEvent) => void)[] = [];
  let cfg: Settings = { address: "192.168.1.42", helperPort: 9130, loaderPort: 9021, destination: "/data/homebrew", packedFolder: "/Games/Packed" };
  let prefs: UpdatePrefs = { check: true, auto: false };
  let history = [...HISTORY];
  let timer: ReturnType<typeof setInterval> | null = null;
  const emit = (e: UiEvent) => listeners.forEach((l) => l(e));

  /** Runs a made-up job: each step fills in a few seconds. */
  function run(source: string, steps: StepView["kind"][]): JobView {
    const job: JobView = {
      id: `demo-${Date.now()}`,
      source,
      outputDir: "/Games/Packed",
      deleteOriginal: false,
      packed: null,
      packedBytes: null,
      steps: steps.map((kind) => ({ kind, state: "pending", note: null })),
      state: "running",
      rev: 0,
    };
    let at = 0;
    let done = 0;
    const total = 84_200_000_000;
    if (timer) clearInterval(timer);
    timer = setInterval(() => {
      const step = job.steps[at];
      if (!step) return;
      if (step.state === "pending") {
        step.state = "running";
        emit({ type: "job", job: { ...job, steps: job.steps.map((st) => ({ ...st })), rev: ++job.rev } });
      }
      done = Math.min(total, done + total / 40);
      emit({ type: "progress", id: job.id, index: at, done, total });
      if (done >= total && params.get("open") !== "packing") {
        step.state = "done";
        step.note = step.kind === "installPackage" ? "Installed and confirmed" : null;
        at += 1;
        done = 0;
        const last = at >= job.steps.length;
        if (last) {
          job.state = "finished";
          job.packedBytes = 51_600_000_000;
          if (timer) clearInterval(timer);
        }
        emit({ type: "job", job: { ...job, steps: job.steps.map((st) => ({ ...st })), rev: ++job.rev } });
      }
    }, 400);
    return job;
  }

  const open = params.get("open");
  const first = open === "install" ? "/Games/Ashen Tide.fpkg" : open ? "/Games/Ashen Tide" : null;

  return {
    inspect: async (path) => {
      const found = ITEMS[path];
      if (!found) throw "Not a PS5 game";
      return found;
    },
    start: async (source, options) => run(source, options.send ? ["pack", "send"] : ["pack"]),
    install: async (source) => run(source, ["installPackage"]),
    quit: async () => {},
    cancel: async () => {
      if (timer) clearInterval(timer);
    },
    carryOn: async (id) => ({ ...run("/Games/Ashen Tide", ["pack", "send"]), id }),
    discard: async () => {},
    unfinished: async () => null,
    initialPath: async () => first,
    pickFolder: async () => "/Games/Ashen Tide",
    pickFile: async () => "/Games/Ashen Tide.fpkg",
    reveal: async () => {},
    settings: async () => ({ ...cfg }),
    saveSettings: async (s) => {
      cfg = { ...s };
    },
    ps5Status: async () => ({ state: "running", version: "0.4.0" }),
    discover: async () => [{ address: "192.168.1.42", port: 9130, version: "0.4.0" }],
    startHelper: async () => "alreadyRunning",
    checkDlc: async () => {
      throw "This isn't DLC for this game";
    },
    dlcInFolder: async () => ({ found: [], skipped: [] }),
    pickDlcFiles: async () => [],
    pickDlcFolder: async () => null,
    pickBackport: async () => null,
    bundlePlan: async () => ({ items: [], route: "inPack", problem: null, emulatorFrom: "none" }),
    dlcEmuVersion: async () => "0.3",
    smpStatus: async () => ({ host: "192.168.1.42", port: 10101, version: "1.6", apiVersion: 2, installed: false, baseInstalled: false, sourceType: "folder" }),
    installBackport: async () => {},
    library: async () => ({ games: INSTALLED, truncated: false }),
    installedTitle: async (id) => INSTALLED.find((g) => g.titleId === id) ?? null,
    libraryIcons: async (ids) => ids.map((id) => ({ titleId: id, dataUrl: icon(Object.keys(GAMES).find((k) => GAMES[k].id === id) ?? "ashen") })),
    measureApp: async (): Promise<Measured> => ({ bytes: 84_200_000_000, appBytes: 71_400_000_000, updateBytes: 11_600_000_000, dlcBytes: 1_200_000_000, dumpBytes: 0, dumps: [], partial: false }),
    freeSpace: async (path) =>
      path.includes("ext")
        ? { total: 2_000_000_000_000, free: 1_200_000_000_000, usable: 1_190_000_000_000, reserve: 10_000_000_000 }
        : { total: 825_000_000_000, free: 318_600_000_000, usable: 308_000_000_000, reserve: 10_000_000_000 },
    history: async () => history,
    addHistory: async (entry) => {
      history = [entry, ...history.filter((e) => e.id !== entry.id)];
    },
    clearHistory: async () => {
      history = [];
    },
    appVersion: async () => "0.2.0",
    gameDetails: async (title) => details(keyOf(title)),
    searchGames: async () => [],
    pickGame: async () => {},
    detailsOn: async () => true,
    setDetailsOn: async () => {},
    checkUpdate: async () => null,
    installUpdate: async () => {},
    restartApp: async () => {},
    onUpdateProgress: () => () => {},
    updatePrefs: async () => prefs,
    setUpdatePrefs: async (p) => {
      prefs = p;
    },
    onEvent: (cb) => {
      listeners.push(cb);
      return () => {};
    },
    onDrop: (_cb: (d: DropEvent) => void) => () => {},
  };
}
