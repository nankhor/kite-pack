import { describe, expect, it } from "vitest";
import { footerNote, formatSize, minutesLeft, stepRows, updateLabel } from "./model";
import type { JobView, ProgressState, StepView } from "./types";

const job = (steps: StepView[], extra: Partial<JobView> = {}): JobView => ({
  id: "j1",
  source: "/Games/Ashen Tide",
  outputDir: "/Games/Packed",
  deleteOriginal: false,
  packed: null,
  packedBytes: null,
  steps,
  state: "running",
  rev: 0,
  ...extra,
});

const progress = (p: Partial<ProgressState>): ProgressState => ({
  index: 0,
  stage: null,
  done: 0,
  total: 0,
  startedAt: null,
  startedDone: 0,
  ...p,
});

describe("formatSize", () => {
  it("uses decimal units with one decimal", () => {
    expect(formatSize(84_200_000_000)).toBe("84.2 GB");
    expect(formatSize(51_600_000_000)).toBe("51.6 GB");
    expect(formatSize(716_978)).toBe("717 KB");
    expect(formatSize(6_100_000_000)).toBe("6.1 GB");
    expect(formatSize(0)).toBe("0 bytes");
  });
});

describe("updateLabel", () => {
  it("names the update a dump already contains", () => {
    expect(updateLabel("01.050.000")).toBe("Update 1.05");
    expect(updateLabel("02.000.000")).toBe("Update 2.00");
  });
  it("says when there is none", () => {
    expect(updateLabel("01.000.000")).toBe("Base game, no update");
    expect(updateLabel(null)).toBe("Base game, no update");
  });
});

describe("stepRows", () => {
  const now = 1_000_000;

  it("shows a running pack with its percentage and time left", () => {
    const rows = stepRows(
      job([{ kind: "pack", state: "running", note: null }]),
      progress({ done: 62, total: 100, startedAt: now - 60_000, startedDone: 0 }),
      now,
    );
    expect(rows).toEqual([{ label: "Pack", state: "active", status: "62%, about 1 min left", bar: 62 }]);
  });

  it("says what the pack is doing before any bytes", () => {
    const rows = stepRows(job([{ kind: "pack", state: "running", note: null }]), progress({ stage: "scan" }), now);
    expect(rows[0].status).toBe("Reading the game");
    expect(rows[0].bar).toBe(0);
  });

  it("shows where a finished pack went", () => {
    const rows = stepRows(
      job([{ kind: "pack", state: "done", note: "3 files" }], { packedBytes: 51_600_000_000, state: "finished" }),
      null,
      now,
    );
    expect(rows[0]).toEqual({ label: "Pack", state: "done", status: "51.6 GB in Packed", bar: 100 });
  });

  it("describes deleting the original", () => {
    const pending = stepRows(
      job([
        { kind: "pack", state: "running", note: null },
        { kind: "deleteOriginal", state: "pending", note: null },
      ]),
      null,
      now,
    );
    expect(pending[1]).toEqual({
      label: "Delete the original",
      state: "pending",
      status: "After the packed copy is checked",
      bar: null,
    });
    const done = stepRows(job([{ kind: "deleteOriginal", state: "done", note: "Moved to the Trash" }]), null, now);
    expect(done[0].status).toBe("In the Trash");
  });

  it("shows why a step failed", () => {
    const rows = stepRows(job([{ kind: "pack", state: "failed", note: "No space left on the drive" }]), null, now);
    expect(rows[0].state).toBe("failed");
    expect(rows[0].status).toBe("No space left on the drive");
  });
});

describe("minutesLeft", () => {
  it("projects from the rate since the first bytes, at least 1", () => {
    expect(minutesLeft(progress({ done: 50, total: 100, startedAt: 0, startedDone: 0 }), 600_000)).toBe(10);
    expect(minutesLeft(progress({ done: 99, total: 100, startedAt: 0, startedDone: 0 }), 60_000)).toBe(1);
    expect(minutesLeft(progress({ done: 0, total: 100, startedAt: null }), 60_000)).toBeNull();
  });
});

describe("footerNote", () => {
  const packJob = job([{ kind: "pack", state: "running", note: null }]);
  it("covers every state", () => {
    expect(footerNote({ kind: "ready", outputDir: "/Games/Packed" })).toBe("Packs into /Games/Packed.");
    expect(footerNote({ kind: "running", job: packJob })).toBe(
      "You can leave this running. If you close Kite Pack, it carries on next time.",
    );
    expect(
      footerNote({ kind: "failed", job: job([{ kind: "pack", state: "failed", note: "No space left" }], { state: "failed" }) }),
    ).toBe("Pack failed: No space left");
    expect(footerNote({ kind: "done", job: job([], { deleteOriginal: true, state: "finished" }) })).toBe(
      "The original is in the Trash until you empty it. Play the game once first.",
    );
    expect(footerNote({ kind: "done", job: job([], { state: "finished" }) })).toBe("Nothing was deleted.");
  });
});
