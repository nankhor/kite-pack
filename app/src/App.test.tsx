import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { Backend, DropEvent } from "./backend";
import type {
  BundlePlan,
  Found,
  GameInfo,
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
    dlcTools: null,
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
            },
          ]
        : []),
    ],
    route: "inPack" as const,
    problem: null,
    emulatorFromSettings: false,
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
      bundle: { dlc: [], backport: null },
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
    expect(screen.getByText("Pack")).toBeInTheDocument();
    backend.emit({ type: "progress", id: "job-1", index: 0, done: 62, total: 100 });
    expect(screen.getByText(/^62%/)).toBeInTheDocument();
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
      bundle: { dlc: [], backport: null },
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
    expect(screen.getByText(/^38%/)).toBeInTheDocument();
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


describe("the PS5's library", () => {
  const INSTALLED: InstalledGame[] = [
    {
      titleId: "PPSA00001",
      title: "Ashen Tide",
      version: "01.050.000",
      updateInstalled: true,
      dlc: ["TIDEBREAKER00000", "SPDLC02"],
      external: false,
      found: "appmeta+app",
    },
    {
      titleId: "PPSA00002",
      title: "Second Wind",
      version: null,
      updateInstalled: false,
      dlc: [],
      external: true,
      found: "app",
    },
  ];

  async function openLibrary() {
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("button", { name: /See what/ }));
    return screen.findByRole("heading", { name: "On the PS5" });
  }

  it("lists what is installed with its update, add-ons and drive", async () => {
    backend.installed = INSTALLED;
    await openLibrary();

    expect(await screen.findByText("2 titles installed")).toBeInTheDocument();
    expect(screen.getByText("Ashen Tide")).toBeInTheDocument();
    expect(screen.getByText("PPSA00001")).toBeInTheDocument();
    expect(screen.getByText("2 DLC")).toBeInTheDocument();
    expect(screen.getByText("Extended storage")).toBeInTheDocument();
    expect(backend.library).toHaveBeenCalled();
  });

  it("measures a title's size only when asked", async () => {
    backend.installed = INSTALLED;
    await openLibrary();

    await userEvent.click(await screen.findAllByRole("button", { name: "Measure" }).then((b) => b[0]));
    expect(await screen.findByText("2.0 GB")).toBeInTheDocument();
    expect(backend.measureApp).toHaveBeenCalledWith("PPSA00001");
  });

  it("shows the room left where games go", async () => {
    backend.installed = INSTALLED;
    await openLibrary();
    expect(await screen.findByText(/480\.0 GB free where games go/)).toBeInTheDocument();
    expect(backend.freeSpace).toHaveBeenCalledWith("/data/homebrew");
  });

  it("filters by name or title id", async () => {
    backend.installed = INSTALLED;
    await openLibrary();

    await userEvent.type(screen.getByRole("searchbox", { name: "Search games" }), "second");
    expect(screen.getByText("Second Wind")).toBeInTheDocument();
    expect(screen.queryByText("Ashen Tide")).not.toBeInTheDocument();
  });

  it("says why it couldn't read the console", async () => {
    backend.library.mockRejectedValueOnce("The PS5 isn't reachable. Check it's on and the Kite helper is running.");
    render(<App backend={backend} />);
    await userEvent.click(await screen.findByRole("button", { name: /See what/ }));
    expect(await screen.findByText(/isn't reachable/)).toBeInTheDocument();
  });

  it("goes back to the drop screen", async () => {
    backend.installed = INSTALLED;
    await openLibrary();

    await userEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByText("Drop a game here")).toBeInTheDocument();
  });
});


describe("inside the packed copy", () => {
  async function withGame() {
    render(<App backend={backend} />);
    await screen.findByText("Drop a game here");
    await backend.drop("/Games/Ashen Tide");
    await screen.findByRole("heading", { name: "Ashen Tide" });
  }

  it("adds a DLC folder and says what the pack will carry", async () => {
    backend.pickDlcFolder.mockResolvedValueOnce("/Games/DLC/Tidebreaker");
    await withGame();

    await userEvent.click(screen.getByRole("button", { name: "Add DLC folder" }));

    expect(await screen.findByText("Tidebreaker")).toBeInTheDocument();
    expect(await screen.findByText("1 add-on will be inside the packed copy.")).toBeInTheDocument();
    expect(backend.bundlePlan).toHaveBeenLastCalledWith(
      "/Games/Ashen Tide",
      ["/Games/DLC/Tidebreaker"],
      null,
    );
  });

  it("keeps a DLC that cannot go in visible, with the packer's reason", async () => {
    backend.pickDlcPackage.mockResolvedValueOnce("/Games/DLC/other.pkg");
    const refusedPlan: BundlePlan = {
      items: [
        {
          path: "/Games/DLC/other.pkg",
          kind: "dlc",
          name: "other.pkg",
          titleId: "PPSA99999",
          contentId: null,
          label: null,
          bytes: 1,
          problem: "DLC for PPSA99999, not for PPSA01234",
        },
      ],
      route: "unavailable",
      problem: "DLC for PPSA99999, not for PPSA01234",
      emulatorFromSettings: false,
    };
    backend.bundlePlan.mockImplementation(async (_source, dlc) =>
      dlc.length > 0 ? refusedPlan : { items: [], route: "inPack", problem: null, emulatorFromSettings: false },
    );
    await withGame();

    await userEvent.click(screen.getByRole("button", { name: "Add DLC package" }));

    expect(await screen.findByText("DLC for PPSA99999, not for PPSA01234")).toBeInTheDocument();
  });

  it("adds a backport folder and removes it again", async () => {
    backend.pickBackport.mockResolvedValueOnce("/Games/Backport/v1.00");
    await withGame();

    await userEvent.click(screen.getByRole("button", { name: "Add backport" }));
    expect(await screen.findByText("v1.00")).toBeInTheDocument();
    expect(await screen.findByText("a backport will be inside the packed copy.")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Remove v1.00" }));
    expect(screen.queryByText("a backport will be inside the packed copy.")).not.toBeInTheDocument();
  });

  it("hands the picks to the job", async () => {
    backend.pickDlcFolder.mockResolvedValueOnce("/Games/DLC/Tidebreaker");
    await withGame();
    await userEvent.click(screen.getByRole("button", { name: "Add DLC folder" }));
    await screen.findByText("Tidebreaker");
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));

    expect(backend.start).toHaveBeenLastCalledWith(
      "/Games/Ashen Tide",
      expect.objectContaining({ bundle: { dlc: ["/Games/DLC/Tidebreaker"], backport: null } }),
    );
  });
});
