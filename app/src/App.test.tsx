import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { Backend, DropEvent } from "./backend";
import type {
  BundlePlan,
  DlcFolder,
  DlcInfo,
  Found,
  GameInfo,
  HistoryEntry,
  InstalledGame,
  JobView,
  PackOptions,
  Settings,
  StartOutcome,
  Status,
  UiEvent,
} from "./types";

const GAME: GameInfo = {
  kind: "folder",
  title: "Ashen Tide",
  titleId: "PPSA01234",
  contentId: "UP0000-PPSA01234_00-ASHENTIDE0000000",
  version: "01.050.000",
  totalBytes: 84_200_000_000,
  dlcs: [
    { contentId: "x-TIDEBREAKER00000", label: "TIDEBREAKER00000", name: "Tidebreaker Expansion", bytes: 6_100_000_000 },
  ],
  iconDataUrl: null,
  findings: [],
};

const job = (over: Partial<JobView> = {}): JobView => ({
  id: "job-1",
  source: "/Games/Ashen Tide",
  outputDir: "/Games",
  deleteOriginal: false,
  packed: null,
  packedBytes: null,
  steps: [{ kind: "pack", state: "pending", note: null }],
  state: "running",
  rev: 0,
  ...over,
});

class FakeBackend implements Backend {
  games = new Map<string, GameInfo | string>([
    ["/Games/Ashen Tide", GAME],
    ["/Games/Tidebreaker.pkg", "This is an update or a DLC on its own, not a game"],
  ]);
  left: JobView | null = null;
  initial: string | null = null;
  cfg: Settings = {
    address: null,
    helperPort: 9130,
    loaderPort: 9021,
    destination: "/data/homebrew",
    packedFolder: null,
  };
  online = true;
  found: Found[] = [];
  installed: InstalledGame[] = [];
  private listeners: ((e: UiEvent) => void)[] = [];
  private droppers: ((d: DropEvent) => void)[] = [];

  inspect = vi.fn(async (path: string) => {
    const found = this.games.get(path);
    if (found === undefined) throw "Not a PS5 game";
    if (typeof found === "string") throw found;
    return found;
  });
  start = vi.fn(async (_source: string, _options: PackOptions) => job());
  cancel = vi.fn(async () => {});
  carryOn = vi.fn(async (id: string) => job({ id }));
  discard = vi.fn(async (_id: string) => {});
  unfinished = vi.fn(async () => this.left);
  initialPath = vi.fn(async () => this.initial);
  pickFolder = vi.fn(async () => "/Games/Ashen Tide");
  pickFile = vi.fn(async () => null);
  reveal = vi.fn(async (_path: string) => {});
  settings = vi.fn(async () => ({ ...this.cfg }));
  saveSettings = vi.fn(async (s: Settings) => {
    this.cfg = { ...s };
  });
  ps5Status = vi.fn(async (): Promise<Status> => {
    if (!this.cfg.address) return { state: "notSetUp" };
    return this.online ? { state: "running", version: "0.1.0-alpha.3" } : { state: "notReachable" };
  });
  discover = vi.fn(async () => this.found);
  dlcEmuVersion = vi.fn(async () => "0.3");
  bundlePlan = vi.fn(async (_source: string, dlc: string[], backport: string | null): Promise<BundlePlan> => ({
    items: [
      ...dlc.map((path) => ({
        path,
        kind: "dlc" as const,
        name: path.split("/").pop() ?? path,
        titleId: "PPSA01234",
        contentId: "UP0000-PPSA01234_00-TIDEBREAKER00000",
        label: "TIDEBREAKER00000",
        bytes: 6_100_000_000,
        problem: null,
        route: "inPack" as const,
        iconDataUrl: null,
      })),
      ...(backport
        ? [
            {
              path: backport,
              kind: "backport" as const,
              name: backport.split("/").pop() ?? backport,
              titleId: "PPSA01234",
              contentId: null,
              label: null,
              bytes: 2_000_000,
              problem: null,
              route: "inPack" as const,
              iconDataUrl: null,
            },
          ]
        : []),
    ],
    route: "inPack" as const,
    problem: null,
    emulatorFrom: dlc.length > 0 ? ("builtin" as const) : ("none" as const),
  }));
  pickDlcFolder = vi.fn(async (): Promise<string | null> => null);
  pickDlcPackage = vi.fn(async (): Promise<string | null> => null);
  pickBackport = vi.fn(async (): Promise<string | null> => null);
  library = vi.fn(async () => ({ games: this.installed, truncated: false }));
  installedTitle = vi.fn(async (_titleId: string): Promise<InstalledGame | null> => null);
  libraryIcons = vi.fn(async (ids: string[]) =>
    ids.includes("PPSA00001") ? [{ titleId: "PPSA00001", dataUrl: "data:image/png;base64,AA==" }] : [],
  );
  measureApp = vi.fn(async () => ({
    bytes: 2_000_000_000,
    appBytes: 1_000_000_000,
    updateBytes: 500_000_000,
    dlcBytes: 500_000_000,
    partial: false,
  }));
  freeSpace = vi.fn(async () => ({ total: 1_000_000_000_000, free: 500_000_000_000, usable: 480_000_000_000, reserve: 10_000_000_000 }));
  startHelper = vi.fn(async (): Promise<StartOutcome> => "sent");
  /** DLC packages for Ashen Tide, by path; anything else isn't its DLC. */
  dlc = new Map<string, DlcInfo>();
  dlcPicked: string[] = [];
  checkDlc = vi.fn(async (path: string, _titleId: string) => {
    const info = this.dlc.get(path);
    if (!info) throw "This isn't DLC for PPSA01234";
    return info;
  });
  dlcInFolder = vi.fn(
    async (_path: string, _titleId: string): Promise<DlcFolder> => ({
      found: [...this.dlc].map(([path, info]) => ({ path, info })),
      skipped: ["Patch.pkg"],
    }),
  );
  pickDlcFiles = vi.fn(async () => this.dlcPicked);
  past: HistoryEntry[] = [];
  history = vi.fn(async () => this.past);
  addHistory = vi.fn(async (entry: HistoryEntry) => {
    this.past = [entry, ...this.past.filter((e) => e.id !== entry.id)];
  });
  clearHistory = vi.fn(async () => {
    this.past = [];
  });
  appVersion = vi.fn(async () => "0.1.3");
  onEvent(cb: (e: UiEvent) => void) {
    this.listeners.push(cb);
    return () => {};
  }
  onDrop(cb: (d: DropEvent) => void) {
    this.droppers.push(cb);
    return () => {};
  }

  emit(e: UiEvent) {
    act(() => this.listeners.forEach((l) => l(e)));
  }
  async drop(...paths: string[]) {
    await act(async () => this.droppers.forEach((d) => d({ type: "drop", paths })));
  }
}

let backend: FakeBackend;

beforeEach(() => {
  backend = new FakeBackend();
  localStorage.clear();
});

async function showGame() {
  render(<App backend={backend} />);
  await screen.findByText("Drop a game here");
  await backend.drop("/Games/Ashen Tide");
  await screen.findByRole("heading", { name: "Ashen Tide" });
}

describe("before a game", () => {
  it("asks for a game", async () => {
    render(<App backend={backend} />);
    expect(await screen.findByText("Drop a game here")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose folder" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose file" })).toBeInTheDocument();
  });

  it("opens a game chosen with Choose folder", async () => {
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("button", { name: "Choose folder" }));
    expect(await screen.findByRole("heading", { name: "Ashen Tide" })).toBeInTheDocument();
  });

  it("shows why a dropped file can't be packed", async () => {
    render(<App backend={backend} />);
    await screen.findByText("Drop a game here");
    await backend.drop("/Games/Tidebreaker.pkg");
    expect(await screen.findByText("This is an update or a DLC on its own, not a game")).toBeInTheDocument();
  });

  it("takes one game at a time", async () => {
    render(<App backend={backend} />);
    await screen.findByText("Drop a game here");
    await backend.drop("/Games/Ashen Tide", "/Games/Other");
    expect(await screen.findByText("Drop one game at a time.")).toBeInTheDocument();
    expect(backend.inspect).not.toHaveBeenCalled();
  });
});

describe("a game the console already has", () => {
  it("says so, and marks the DLC that is already there", async () => {
    backend.installedTitle.mockResolvedValueOnce({
      titleId: "PPSA01234",
      title: "Ashen Tide",
      version: "01.050.000",
      updateInstalled: true,
      dlc: ["TIDEBREAKER00000"],
      external: false,
      found: "appmeta+app",
    });
    await showGame();

    expect(await screen.findByText("Already on the PS5")).toBeInTheDocument();
    expect(screen.getByText("Update installed")).toBeInTheDocument();
    expect(screen.getByText("installed")).toBeInTheDocument();
    expect(backend.installedTitle).toHaveBeenCalledWith("PPSA01234");
  });

  it("says nothing when the game is not installed", async () => {
    await showGame();
    expect(screen.queryByText("Already on the PS5")).not.toBeInTheDocument();
  });
});

describe("ready", () => {
  it("shows the game and what it includes", async () => {
    await showGame();
    expect(screen.getByText("PPSA01234")).toBeInTheDocument();
    expect(screen.getByText("84.2 GB")).toBeInTheDocument();
    expect(screen.getByText("Update 1.05")).toBeInTheDocument();
    expect(screen.getByText(/Tidebreaker Expansion, 6\.1 GB/)).toBeInTheDocument();
  });

  it("offers the formats and levels, Folder and Fast first", async () => {
    await showGame();
    const format = screen.getByRole("group", { name: "Save as" });
    expect(within(format).getByRole("button", { name: "Folder" })).toHaveAttribute("aria-pressed", "true");
    for (const name of ["exFAT", ".ffpkg", ".ffpfs"]) {
      expect(within(format).getByRole("button", { name })).toHaveAttribute("aria-pressed", "false");
    }
    const level = screen.getByRole("group", { name: "Level" });
    expect(within(level).getByRole("button", { name: "Fast" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("/Games")).toBeInTheDocument();
    expect(screen.getByText("Packs into /Games.")).toBeInTheDocument();
  });

  it("keeps Send off until a PS5 is set up", async () => {
    await showGame();
    expect(await screen.findByText("Set up your PS5 in Settings first.")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Send to PS5" })).toBeDisabled();
    expect(screen.queryByText("Delete the packed folder")).not.toBeInTheDocument();
  });

  it("keeps the game when a dropped file is refused", async () => {
    await showGame();
    await backend.drop("/Games/Tidebreaker.pkg");
    expect(await screen.findByText("This is an update or a DLC on its own, not a game")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ashen Tide" })).toBeInTheDocument();
  });

  it("packs with the chosen options", async () => {
    await showGame();
    await userEvent.click(within(screen.getByRole("group", { name: "Save as" })).getByRole("button", { name: ".ffpkg" }));
    await userEvent.click(within(screen.getByRole("group", { name: "Level" })).getByRole("button", { name: "Max" }));
    await userEvent.click(screen.getByRole("switch", { name: "Delete the original" }));
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    expect(backend.start).toHaveBeenCalledWith("/Games/Ashen Tide", {
      format: "ffpkg",
      level: "max",
      outputDir: "/Games",
      deleteOriginal: true,
      send: false,
      deletePacked: false,
      dlc: [],
    });
  });

  it("keeps the folder chosen with Change for next time", async () => {
    backend.pickFolder = vi.fn(async () => "/Packed");
    await showGame();
    await userEvent.click(screen.getByRole("button", { name: "Change" }));
    expect(await screen.findByText("Packs into /Packed.")).toBeInTheDocument();
    expect(backend.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ packedFolder: "/Packed" }));
  });

  it("packs into the saved folder", async () => {
    backend.cfg.packedFolder = "/Packed";
    await showGame();
    expect(await screen.findByText("Packs into /Packed.")).toBeInTheDocument();
  });
});

describe("packing", () => {
  async function packing() {
    await showGame();
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    backend.emit({ type: "job", job: job({ steps: [{ kind: "pack", state: "running", note: null }] }) });
  }

  it("shows the step, its progress and Cancel", async () => {
    await packing();
    expect(within(screen.getByRole("list", { name: "Steps" })).getByText("Pack")).toBeInTheDocument();
    backend.emit({ type: "progress", id: "job-1", index: 0, done: 62, total: 100 });
    expect(within(screen.getByRole("list", { name: "Steps" })).getByText(/^62%/)).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /^Pack\s*62%$/ })).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Pack" })).toHaveAttribute("aria-valuenow", "62");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(backend.cancel).toHaveBeenCalled();
  });

  it("shows the running step when its update beats Pack's reply", async () => {
    await showGame();
    backend.start = vi.fn(async () => {
      backend.emit({ type: "job", job: job({ rev: 2, steps: [{ kind: "pack", state: "running", note: null }] }) });
      return job({ rev: 1 });
    });
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    expect(await screen.findByText("Starting")).toBeInTheDocument();
    expect(screen.queryByText("Waiting to start")).not.toBeInTheDocument();
  });

  it("shows the running step when its update beats Retry's reply", async () => {
    await packing();
    backend.emit({
      type: "job",
      job: job({ rev: 5, state: "failed", steps: [{ kind: "pack", state: "failed", note: "x" }] }),
    });
    backend.carryOn = vi.fn(async () => {
      backend.emit({ type: "job", job: job({ rev: 7, steps: [{ kind: "pack", state: "running", note: null }] }) });
      return job({ rev: 6 });
    });
    await userEvent.click(await screen.findByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Starting")).toBeInTheDocument();
  });

  it("goes back to Ready when cancelled", async () => {
    await packing();
    backend.emit({ type: "job", job: job({ state: "cancelled" }) });
    expect(await screen.findByRole("button", { name: "Pack" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ashen Tide" })).toBeInTheDocument();
  });

  it("refuses a new game while packing", async () => {
    await packing();
    await backend.drop("/Games/Ashen Tide");
    expect(await screen.findByText("A game is already being packed.")).toBeInTheDocument();
    expect(backend.inspect).toHaveBeenCalledTimes(1);
  });

  it("ends with what was kept and where", async () => {
    await packing();
    backend.emit({
      type: "job",
      job: job({
        state: "finished",
        packed: "/Games/[Ashen Tide]-[PPSA01234]",
        packedBytes: 51_600_000_000,
        steps: [{ kind: "pack", state: "done", note: "checked" }],
      }),
    });
    expect(await screen.findByText("Packed", { selector: "p" })).toBeInTheDocument();
    expect(screen.getByText("51.6 GB")).toBeInTheDocument();
    expect(screen.getByText("32.6 GB")).toBeInTheDocument();
    expect(screen.getByText("Nothing was deleted.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Show folder" }));
    expect(backend.reveal).toHaveBeenCalledWith("/Games/[Ashen Tide]-[PPSA01234]");
    await userEvent.click(screen.getByRole("button", { name: "Pack another game" }));
    expect(await screen.findByText("Drop a game here")).toBeInTheDocument();
  });

  it("offers Retry when a step fails", async () => {
    await packing();
    backend.emit({
      type: "job",
      job: job({ state: "failed", steps: [{ kind: "pack", state: "failed", note: "No space left on the drive" }] }),
    });
    expect(await screen.findByText("Pack failed: No space left on the drive")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(backend.carryOn).toHaveBeenCalledWith("job-1");
  });

  it("discards a failed job on Cancel", async () => {
    await packing();
    backend.emit({ type: "job", job: job({ state: "failed", steps: [{ kind: "pack", state: "failed", note: "x" }] }) });
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(backend.discard).toHaveBeenCalledWith("job-1");
    expect(await screen.findByRole("button", { name: "Pack" })).toBeInTheDocument();
  });
});

describe("on launch", () => {
  it("carries on a job left by a closed app", async () => {
    backend.left = job({ state: "paused" });
    render(<App backend={backend} />);
    await waitFor(() => expect(backend.carryOn).toHaveBeenCalledWith("job-1"));
    expect(await screen.findByRole("heading", { name: "Ashen Tide" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("shows a job that failed before it closed, without running it", async () => {
    backend.left = job({ state: "failed", steps: [{ kind: "pack", state: "failed", note: "Disk full" }] });
    render(<App backend={backend} />);
    expect(await screen.findByText("Pack failed: Disk full")).toBeInTheDocument();
    expect(backend.carryOn).not.toHaveBeenCalled();
  });

  it("opens a game given when the app starts", async () => {
    backend.initial = "/Games/Ashen Tide";
    render(<App backend={backend} />);
    expect(await screen.findByRole("heading", { name: "Ashen Tide" })).toBeInTheDocument();
  });

  it("names a game without a title by its title ID", async () => {
    backend.games.set("/Games/x", { ...GAME, title: null, iconDataUrl: null });
    backend.initial = "/Games/x";
    render(<App backend={backend} />);
    expect(await screen.findByRole("heading", { name: "PPSA01234" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "No icon" })).toBeInTheDocument();
  });
});

describe("PS5", () => {
  const ADDRESS = "192.168.1.42";

  it("says when no PS5 is set up, and opens Settings", async () => {
    render(<App backend={backend} />);
    const chip = await screen.findByRole("button", { name: /Not set up/ });
    await userEvent.click(chip);
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Find my PS5" })).toBeInTheDocument();
  });

  it("shows a running helper", async () => {
    backend.cfg.address = ADDRESS;
    render(<App backend={backend} />);
    expect(await screen.findByRole("button", { name: /Helper running/ })).toBeInTheDocument();
  });

  it("shows an unreachable PS5", async () => {
    backend.cfg.address = ADDRESS;
    backend.online = false;
    render(<App backend={backend} />);
    expect(await screen.findByRole("button", { name: /Not reachable/ })).toBeInTheDocument();
  });

  it("picks the one PS5 it finds on the network", async () => {
    backend.found = [{ address: ADDRESS, port: 9130, version: "0.1.0-alpha.3" }];
    render(<App backend={backend} />);
    expect(await screen.findByRole("button", { name: /Helper running/ })).toBeInTheDocument();
    expect(backend.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ address: ADDRESS }));
  });

  it("doesn't choose between several PS5s by itself", async () => {
    backend.found = [
      { address: ADDRESS, port: 9130, version: "1" },
      { address: "192.168.1.43", port: 9130, version: "1" },
    ];
    render(<App backend={backend} />);
    await waitFor(() => expect(backend.discover).toHaveBeenCalled());
    expect(await screen.findByRole("button", { name: /Not set up/ })).toBeInTheDocument();
    expect(backend.saveSettings).not.toHaveBeenCalled();
  });

  it("sends, with the third toggle lined up with Send", async () => {
    backend.cfg.address = ADDRESS;
    await showGame();
    await screen.findByRole("button", { name: /Helper running/ });
    const send = screen.getByRole("switch", { name: "Send to PS5" });
    expect(send).toBeEnabled();
    expect(screen.getByText("Starts by itself when packing ends, into /data/homebrew.")).toBeInTheDocument();
    await userEvent.click(send);
    const third = screen.getByRole("switch", { name: "Delete the packed folder" });
    // Same row style as Send, so both switches sit on the same left edge.
    expect(third.closest(".toggle")!.className).toBe(send.closest(".toggle")!.className);
    await userEvent.click(screen.getByRole("switch", { name: "Delete the original" }));
    await userEvent.click(third);
    expect(screen.getByText("Only the copy on the PS5 will be left.")).toBeInTheDocument();
    expect(screen.getByText("Packs into /Games, then sends to the PS5 by itself.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    expect(backend.start).toHaveBeenCalledWith("/Games/Ashen Tide", {
      format: "folder",
      level: "fast",
      outputDir: "/Games",
      deleteOriginal: true,
      send: true,
      deletePacked: true,
      dlc: [],
    });
  });

  it("lets Send wait for a PS5 that isn't reachable", async () => {
    backend.cfg.address = ADDRESS;
    backend.online = false;
    await showGame();
    expect(await screen.findByText("The PS5 isn't reachable right now. Sending waits for it.")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Send to PS5" })).toBeEnabled();
  });

  it("shows the send step through to the PS5", async () => {
    backend.cfg.address = ADDRESS;
    await showGame();
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    const steps = (send: "running" | "waiting" | "done") =>
      job({
        steps: [
          { kind: "pack", state: "done", note: "x" },
          { kind: "send", state: send, note: send === "done" ? "Sent and checked" : null },
        ],
        state: send === "done" ? "finished" : "running",
        packed: "/Games/[Ashen Tide]-[PPSA01234]",
        packedBytes: 51_600_000_000,
      });
    backend.emit({ type: "job", job: steps("running") });
    backend.emit({ type: "progress", id: "job-1", index: 1, done: 38, total: 100 });
    expect(screen.getByText("Send to PS5")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Steps" })).getByText(/^38%/)).toBeInTheDocument();
    backend.emit({ type: "job", job: steps("waiting") });
    expect(screen.getByText("Paused at 38%")).toBeInTheDocument();
    expect(
      screen.getByText("The PS5 isn't reachable. Turn it on and start the Kite helper; sending picks up where it stopped."),
    ).toBeInTheDocument();
    backend.emit({ type: "job", job: steps("done") });
    expect(await screen.findByText("Every file checked")).toBeInTheDocument();
    expect(screen.getByText("On the PS5", { selector: "p" })).toBeInTheDocument();
  });
});

describe("tabs", () => {
  it("moves between Pack, PS5 and History", async () => {
    render(<App backend={backend} />);
    await screen.findByText("Drop a game here");
    expect(screen.getByRole("tab", { name: "Pack" })).toHaveAttribute("aria-selected", "true");
    await userEvent.click(screen.getByRole("tab", { name: "PS5" }));
    expect(await screen.findByRole("heading", { name: "On the PS5" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "History" }));
    expect(await screen.findByText("Nothing here yet. Each game you pack shows up here once it's done.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Pack" }));
    expect(await screen.findByText("Drop a game here")).toBeInTheDocument();
  });

  it("moves with the arrow keys", async () => {
    render(<App backend={backend} />);
    await screen.findByText("Drop a game here");
    screen.getByRole("tab", { name: "Pack" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(await screen.findByRole("heading", { name: "On the PS5" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "PS5" })).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(await screen.findByRole("tab", { name: "History" })).toHaveAttribute("aria-selected", "true");
  });

  it("opens Settings from the gear, with the versions", async () => {
    backend.cfg.address = "192.168.1.42";
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(await screen.findByText("0.1.3")).toBeInTheDocument();
    expect(await screen.findByText("0.1.0-alpha.3")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps a running job in sight from the other tabs", async () => {
    await showGame();
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    backend.emit({ type: "job", job: job({ steps: [{ kind: "pack", state: "running", note: null }] }) });
    backend.emit({ type: "progress", id: "job-1", index: 0, done: 62, total: 100 });
    await userEvent.click(screen.getByRole("tab", { name: "History" }));
    expect(screen.getByRole("tab", { name: /^Pack\s*62%$/ })).toBeInTheDocument();
  });

  it("opens a dropped game on the Pack tab", async () => {
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("tab", { name: "History" }));
    await backend.drop("/Games/Ashen Tide");
    expect(await screen.findByRole("heading", { name: "Ashen Tide" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Pack" })).toHaveAttribute("aria-selected", "true");
  });
});

describe("inside the packed copy", () => {
  async function addPackages(...paths: string[]) {
    backend.dlcPicked = paths;
    await userEvent.click(screen.getByRole("button", { name: /Add DLC/ }));
    await userEvent.click(screen.getByRole("menuitem", { name: /Packages/ }));
  }

  async function addFolder(path: string) {
    backend.pickDlcFolder.mockResolvedValueOnce(path);
    await userEvent.click(screen.getByRole("button", { name: /Add DLC/ }));
    await userEvent.click(screen.getByRole("menuitem", { name: /Folder/ }));
  }

  it("adds packages from the one Add DLC button and lists them", async () => {
    await showGame();
    await addPackages("/DLC/suits.pkg", "/DLC/map.pkg");

    expect(await screen.findByRole("button", { name: /2 add-ons/ })).toBeInTheDocument();
    expect(screen.getByText("Folded into the packed copy before it is written.")).toBeInTheDocument();
    expect(screen.getByText("Packs into /Games with 2 add-ons inside.")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /2 add-ons/ }));
    const list = screen.getByRole("region", { name: "What goes inside the packed copy" });
    expect(within(list).getByText("suits.pkg")).toBeInTheDocument();
    expect(within(list).getByText("map.pkg")).toBeInTheDocument();
    expect(within(list).getAllByText("In the pack")).toHaveLength(2);
  });

  it("adds a DLC folder from the menu", async () => {
    await showGame();
    await addFolder("/DLC/Tidebreaker");

    expect(await screen.findByRole("button", { name: /1 add-on/ })).toBeInTheDocument();
    expect(backend.bundlePlan).toHaveBeenLastCalledWith("/Games/Ashen Tide", ["/DLC/Tidebreaker"], null);
  });

  it("adds a backport, and it is listed as one", async () => {
    backend.pickBackport.mockResolvedValueOnce("/Backports/v1.00");
    await showGame();
    await userEvent.click(screen.getByRole("button", { name: "Add backport" }));

    expect(await screen.findByRole("button", { name: /1 backport/ })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /1 backport/ }));
    const list = screen.getByRole("region", { name: "What goes inside the packed copy" });
    expect(within(list).getByText("Backport")).toBeInTheDocument();
    expect(within(list).getByText("v1.00")).toBeInTheDocument();
  });

  it("leaves out an add-on the console already has", async () => {
    backend.installedTitle.mockResolvedValueOnce({
      titleId: "PPSA01234",
      title: "Ashen Tide",
      version: "01.050.000",
      updateInstalled: true,
      dlc: ["TIDEBREAKER00000"],
      external: false,
      found: "appmeta+app",
    });
    await showGame();
    await addPackages("/DLC/suits.pkg");
    await screen.findByRole("button", { name: /1 add-on/ });

    await userEvent.click(screen.getByRole("button", { name: /1 add-on/ }));
    expect(screen.getByText("On the PS5")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    expect(backend.start).toHaveBeenCalledWith(
      "/Games/Ashen Tide",
      expect.objectContaining({ bundle: undefined }),
    );
  });

  it("packs what goes in, and the footer counts it", async () => {
    await showGame();
    await addPackages("/DLC/suits.pkg", "/DLC/map.pkg");
    await screen.findByRole("button", { name: /2 add-ons/ });
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));

    expect(backend.start).toHaveBeenCalledWith(
      "/Games/Ashen Tide",
      expect.objectContaining({ bundle: { dlc: ["/DLC/suits.pkg", "/DLC/map.pkg"], backport: null } }),
    );
  });

  it("starts a new game with nothing picked", async () => {
    await showGame();
    await addPackages("/DLC/suits.pkg");
    await screen.findByRole("button", { name: /1 add-on/ });

    await backend.drop("/Games/Tidebreaker.pkg");
    await backend.drop("/Games/Ashen Tide");

    expect(screen.queryByRole("button", { name: /add-on/ })).not.toBeInTheDocument();
  });

  it("removes one, then all", async () => {
    await showGame();
    await addPackages("/DLC/suits.pkg", "/DLC/map.pkg");
    await userEvent.click(await screen.findByRole("button", { name: /2 add-ons/ }));
    await userEvent.click(screen.getByRole("button", { name: "Remove suits.pkg" }));
    expect(await screen.findByRole("button", { name: /1 add-on/ })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Remove all" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: /add-on/ })).not.toBeInTheDocument());
  });
});

describe("what's installed on the PS5", () => {
  const ASHEN: InstalledGame = {
    titleId: "PPSA00001",
    title: "Ashen Tide",
    version: "01.050.000",
    updateInstalled: true,
    dlc: ["TIDEBREAKER00000", "SPDLC02"],
    external: false,
    found: "appmeta+app",
  };

  it("lists what the console has as soon as the PS5 tab opens", async () => {
    backend.installed = [ASHEN];
    backend.cfg.address = "192.168.1.42";
    render(<App backend={backend} />);
    await screen.findByText("Drop a game here");
    expect(backend.library).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("tab", { name: "PS5" }));

    const row = (await screen.findByText("Ashen Tide")).closest("tr")!;
    expect(within(row).getByText("PPSA00001")).toBeInTheDocument();
    expect(within(row).getByText("1.05")).toBeInTheDocument();
    expect(within(row).getByText("Installed")).toBeInTheDocument();
    expect(within(row).getByText("Internal")).toBeInTheDocument();
    expect(screen.getByText("2 DLC")).toBeInTheDocument();
    expect(backend.library).toHaveBeenCalled();
    expect(await screen.findByText(/free where games go/)).toBeInTheDocument();
  });

  it("measures a title's size when asked", async () => {
    backend.installed = [ASHEN];
    backend.cfg.address = "192.168.1.42";
    render(<App backend={backend} />);
    await screen.findByText("Drop a game here");
    await userEvent.click(screen.getByRole("tab", { name: "PS5" }));
    await userEvent.click(await screen.findByRole("button", { name: "Measure" }));

    expect(await screen.findByText("2.0 GB")).toBeInTheDocument();
    expect(backend.measureApp).toHaveBeenCalledWith("PPSA00001");
  });

  it("opens the PS5 tab from the start screen's link", async () => {
    backend.cfg.address = "192.168.1.42";
    backend.installed = [ASHEN];
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("button", { name: /See what/ }));
    expect(await screen.findByRole("heading", { name: "On the PS5" })).toBeInTheDocument();
    expect(await screen.findByText("Ashen Tide")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "PS5" })).toHaveAttribute("aria-selected", "true");
  });

  it("asks for the PS5 in Settings first, without reading anything", async () => {
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("tab", { name: "PS5" }));
    expect(
      await screen.findByText("No PS5 set up yet. Choose yours in Settings, and its games show up here."),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(backend.library).not.toHaveBeenCalled();
  });
});

describe("History", () => {
  it("keeps a finished job and lists it", async () => {
    await showGame();
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    backend.emit({
      type: "job",
      job: job({
        steps: [{ kind: "pack", state: "done", note: "x" }],
        state: "finished",
        packed: "/Games/[Ashen Tide]-[PPSA01234]",
        packedBytes: 51_600_000_000,
      }),
    });
    await waitFor(() => expect(backend.addHistory).toHaveBeenCalledTimes(1));
    expect(backend.addHistory).toHaveBeenCalledWith(
      expect.objectContaining({ id: "job-1", title: "Ashen Tide", sent: false, packed: "/Games/[Ashen Tide]-[PPSA01234]" }),
    );
    await userEvent.click(screen.getByRole("tab", { name: /History/ }));
    expect(await screen.findByText("Ashen Tide")).toBeInTheDocument();
    expect(screen.getByText("32.6 GB saved")).toBeInTheDocument();
    expect(screen.getByText("Packed copy kept")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Show folder" }));
    expect(backend.reveal).toHaveBeenCalledWith("/Games/[Ashen Tide]-[PPSA01234]");
  });

  it("clears the history", async () => {
    backend.past = [
      {
        id: "a",
        title: "Ashen Tide",
        titleId: "PPSA01234",
        finishedAt: Date.now(),
        sourceBytes: 84_200_000_000,
        packedBytes: 51_600_000_000,
        sent: true,
        packed: null,
        dlc: 2,
      },
    ];
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("tab", { name: "History" }));
    expect(await screen.findByText("With 2 DLC, installed on the PS5")).toBeInTheDocument();
    expect(screen.getByText("On the PS5")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Clear history" }));
    expect(await screen.findByText("Nothing here yet. Each game you pack shows up here once it's done.")).toBeInTheDocument();
    expect(backend.clearHistory).toHaveBeenCalled();
  });
});
