import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { Backend, DropEvent } from "./backend";
import type { GameInfo, JobView, PackOptions, UiEvent } from "./types";

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
  ...over,
});

class FakeBackend implements Backend {
  games = new Map<string, GameInfo | string>([
    ["/Games/Ashen Tide", GAME],
    ["/Games/Tidebreaker.pkg", "This is an update or a DLC on its own, not a game"],
  ]);
  left: JobView | null = null;
  initial: string | null = null;
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

  it("keeps Send off until the Kite helper exists", async () => {
    await showGame();
    expect(screen.getByRole("switch", { name: "Send to PS5" })).toBeDisabled();
    expect(screen.getByText("Needs the Kite helper on your PS5. It comes in the next version.")).toBeInTheDocument();
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
    });
  });

  it("uses the last folder chosen with Change", async () => {
    backend.pickFolder = vi.fn(async () => "/Packed");
    await showGame();
    await userEvent.click(screen.getByRole("button", { name: "Change" }));
    expect(await screen.findByText("Packs into /Packed.")).toBeInTheDocument();
    expect(localStorage.getItem("kite.outputDir")).toBe("/Packed");
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
