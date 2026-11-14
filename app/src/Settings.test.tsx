import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { Backend } from "./backend";
import type { Found, Settings, StartOutcome, Status } from "./types";

class FakeBackend implements Backend {
  cfg: Settings = {
    address: null,
    helperPort: 9130,
    loaderPort: 9021,
    destination: "/data/homebrew",
    packedFolder: null,
    dlcTools: null,
  };
  status: Status = { state: "notSetUp" };
  found: Found[] = [];
  inspect = vi.fn(async () => {
    throw "x";
  });
  start = vi.fn();
  cancel = vi.fn(async () => {});
  carryOn = vi.fn();
  discard = vi.fn(async () => {});
  unfinished = vi.fn(async () => null);
  initialPath = vi.fn(async () => null);
  pickFolder = vi.fn(async () => "/Packed");
  pickFile = vi.fn(async () => null);
  reveal = vi.fn(async () => {});
  settings = vi.fn(async () => ({ ...this.cfg }));
  saveSettings = vi.fn(async (s: Settings) => {
    this.cfg = { ...s };
  });
  ps5Status = vi.fn(async () => this.status);
  discover = vi.fn(async () => this.found);
  startHelper = vi.fn(async (): Promise<StartOutcome> => "sent");
  library = vi.fn(async () => ({ games: [], truncated: false }));
  bundlePlan = vi.fn(async () => ({ items: [], route: "inPack" as const, problem: null, emulatorFromSettings: false }));
  pickDlcFolder = vi.fn(async () => null);
  pickDlcPackage = vi.fn(async () => null);
  pickBackport = vi.fn(async () => null);
  installedTitle = vi.fn(async () => null);
  libraryIcons = vi.fn(async () => []);
  measureApp = vi.fn();
  freeSpace = vi.fn();
  onEvent() {
    return () => {};
  }
  onDrop() {
    return () => {};
  }
}

let backend: FakeBackend;

beforeEach(() => {
  backend = new FakeBackend();
});

async function openSettings() {
  render(<App backend={backend} />);
  await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
  await screen.findByRole("heading", { name: "Settings" });
}

describe("Settings", () => {
  it("finds the PS5 on the network and uses it", async () => {
    await openSettings();
    backend.found = [{ address: "192.168.1.42", port: 9130, version: "0.1.0-alpha.3" }, { address: "192.168.1.43", port: 9130, version: "0.1.0-alpha.3" }];
    await userEvent.click(screen.getByRole("button", { name: "Find my PS5" }));
    expect(await screen.findByText("192.168.1.42, Kite helper 0.1.0-alpha.3")).toBeInTheDocument();
    await userEvent.click(screen.getAllByRole("button", { name: "Use this PS5" })[1]);
    expect(backend.saveSettings).toHaveBeenLastCalledWith(expect.objectContaining({ address: "192.168.1.43" }));
  });

  it("says when nothing answers", async () => {
    await openSettings();
    await userEvent.click(screen.getByRole("button", { name: "Find my PS5" }));
    expect(
      await screen.findByText("No Kite helper found on your network. Start it on the PS5, or type its address."),
    ).toBeInTheDocument();
  });

  it("saves a typed address", async () => {
    await openSettings();
    await userEvent.type(screen.getByLabelText("Address"), "192.168.1.50");
    await userEvent.tab();
    expect(backend.saveSettings).toHaveBeenLastCalledWith(expect.objectContaining({ address: "192.168.1.50" }));
  });

  it("starts the Kite helper", async () => {
    backend.cfg.address = "192.168.1.42";
    backend.status = { state: "notReachable" };
    await openSettings();
    await userEvent.click(screen.getByRole("button", { name: "Start the Kite helper" }));
    expect(backend.startHelper).toHaveBeenCalled();
    expect(await screen.findByText("Sent to the PS5. It starts in a few seconds.")).toBeInTheDocument();
  });

  it("says when the helper is already running", async () => {
    backend.cfg.address = "192.168.1.42";
    backend.startHelper = vi.fn(async (): Promise<StartOutcome> => "alreadyRunning");
    await openSettings();
    await userEvent.click(screen.getByRole("button", { name: "Start the Kite helper" }));
    expect(await screen.findByText("It's already running.")).toBeInTheDocument();
  });

  it("shows why starting failed", async () => {
    backend.cfg.address = "192.168.1.42";
    backend.startHelper = vi.fn(async () => {
      throw "The PS5's ELF loader isn't answering on port 9021.";
    });
    await openSettings();
    await userEvent.click(screen.getByRole("button", { name: "Start the Kite helper" }));
    expect(await screen.findByText("The PS5's ELF loader isn't answering on port 9021.")).toBeInTheDocument();
  });

  it("shows why the helper refused this computer", async () => {
    backend.cfg.address = "192.168.1.42";
    backend.status = { state: "refused", reason: "The Kite helper only accepts computers on your home network." };
    await openSettings();
    expect(
      await screen.findByText("The Kite helper only accepts computers on your home network."),
    ).toBeInTheDocument();
  });

  it("saves where games go on the PS5", async () => {
    await openSettings();
    const field = screen.getByLabelText("Games go to");
    await userEvent.clear(field);
    await userEvent.type(field, "/mnt/ext0/homebrew");
    await userEvent.tab();
    expect(backend.saveSettings).toHaveBeenLastCalledWith(expect.objectContaining({ destination: "/mnt/ext0/homebrew" }));
  });

  it("goes back with Done", async () => {
    await openSettings();
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(await screen.findByText("Drop a game here")).toBeInTheDocument();
  });
});
