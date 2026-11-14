import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { Backend, DropEvent } from "./backend";
import type {
  DlcFolder,
  DlcInfo,
  Found,
  GameInfo,
  HistoryEntry,
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
  cfg: Settings = { address: null, helperPort: 9130, loaderPort: 9021, destination: "/data/homebrew", packedFolder: null };
  online = true;
  found: Found[] = [];
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
    expect(await screen.findByText("Set up your PS5 in the PS5 tab first.")).toBeInTheDocument();
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

  it("says when no PS5 is set up, and opens the PS5 tab", async () => {
    render(<App backend={backend} />);
    const chip = await screen.findByRole("button", { name: /Not set up/ });
    await userEvent.click(chip);
    expect(await screen.findByRole("heading", { name: "Your PS5" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "PS5" })).toHaveAttribute("aria-selected", "true");
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
    expect(await screen.findByRole("heading", { name: "Your PS5" })).toBeInTheDocument();
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
    expect(await screen.findByRole("heading", { name: "Your PS5" })).toBeInTheDocument();
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

describe("DLC", () => {
  const SUITS: DlcInfo = { contentId: "x-SUITS", label: "SUITS0000000000", name: "Suit Pack", bytes: 640_000_000, iconDataUrl: null };
  const MAP: DlcInfo = { contentId: "x-MAP", label: "MAP000000000000", name: "Arena Map", bytes: 410_000_000, iconDataUrl: null };

  async function readyToSend() {
    backend.cfg.address = "192.168.1.42";
    backend.dlc.set("/DLC/suits.pkg", SUITS).set("/DLC/map.pkg", MAP);
    await showGame();
    await screen.findByRole("button", { name: /Helper running/ });
    await userEvent.click(screen.getByRole("switch", { name: "Send to PS5" }));
  }

  it("waits for Send before taking DLC", async () => {
    await showGame();
    expect(screen.getByText("Turn on Send to PS5 to install DLC.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add packages" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add a folder" })).toBeDisabled();
  });

  it("adds packages, says which aren't DLC for the game, and lists them", async () => {
    await readyToSend();
    backend.dlcPicked = ["/DLC/suits.pkg", "/DLC/map.pkg", "/DLC/Patch.pkg"];
    await userEvent.click(screen.getByRole("button", { name: "Add packages" }));
    const summary = await screen.findByRole("button", { name: /2 DLC/ });
    expect(screen.getByText("Not DLC for this game: Patch.pkg.")).toBeInTheDocument();
    expect(screen.getByText("Packs into /Games, sends to the PS5, then installs 2 DLC.")).toBeInTheDocument();
    await userEvent.click(summary);
    const list = screen.getByRole("region", { name: "DLC to install" });
    expect(within(list).getByText("Suit Pack")).toBeInTheDocument();
    expect(within(list).getByText("Arena Map")).toBeInTheDocument();
    await userEvent.click(within(list).getByRole("button", { name: "Remove Suit Pack" }));
    expect(screen.getByRole("button", { name: /1 DLC/ })).toBeInTheDocument();
  });

  it("takes every DLC in a folder", async () => {
    await readyToSend();
    await userEvent.click(screen.getByRole("button", { name: "Add a folder" }));
    expect(await screen.findByRole("button", { name: /2 DLC/ })).toBeInTheDocument();
    expect(backend.dlcInFolder).toHaveBeenCalledWith("/Games/Ashen Tide", "PPSA01234");
    expect(screen.getByText("Not DLC for this game: Patch.pkg.")).toBeInTheDocument();
  });

  it("packs with the DLC, and shows its install as one line", async () => {
    await readyToSend();
    backend.dlcPicked = ["/DLC/suits.pkg", "/DLC/map.pkg"];
    await userEvent.click(screen.getByRole("button", { name: "Add packages" }));
    await screen.findByRole("button", { name: /2 DLC/ });
    await userEvent.click(screen.getByRole("button", { name: "Pack" }));
    expect(backend.start).toHaveBeenCalledWith(
      "/Games/Ashen Tide",
      expect.objectContaining({ send: true, dlc: ["/DLC/suits.pkg", "/DLC/map.pkg"] }),
    );
    backend.emit({
      type: "job",
      job: job({
        steps: [
          { kind: "pack", state: "done", note: "x" },
          { kind: "send", state: "done", note: null },
          { kind: "installDlc", state: "done", note: null, pkg: "/DLC/suits.pkg" },
          { kind: "installDlc", state: "running", note: null, pkg: "/DLC/map.pkg" },
        ],
      }),
    });
    expect(screen.getAllByText("Install DLC")).toHaveLength(1);
    expect(screen.getByText("2 of 2: Arena Map")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "See all 2" }));
    const each = screen.getByRole("region", { name: "Each DLC" });
    expect(within(each).getByText("Installed")).toBeInTheDocument();
    expect(within(each).getByText("Installing")).toBeInTheDocument();
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
