import { describe, expect, it } from "vitest";
import { footerNote, formatSize, formatSpeed, minutesLeft, speed, stepRows, updateLabel } from "./model";
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
  recent: [],
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

  it("shows the speed for sending only", () => {
    const rows = stepRows(
      job([{ kind: "pack", state: "running", note: null }]),
      progress({
        done: 62,
        total: 100,
        startedAt: now - 60_000,
        startedDone: 0,
        recent: [
          { at: now - 2_000, done: 0 },
          { at: now, done: 2_000_000 },
        ],
      }),
      now,
    );
    expect(rows[0].status).toBe("62%, about 1 min left");
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

describe("speed", () => {
  it("is the rate over the recent progress", () => {
    const recent = [
      { at: 0, done: 0 },
      { at: 1_000, done: 30_000_000 },
      { at: 2_000, done: 76_400_000 },
    ];
    expect(speed(progress({ recent }))).toBe(38_200_000);
  });
  it("waits for a second of progress", () => {
    expect(speed(progress({ recent: [] }))).toBeNull();
    expect(speed(progress({ recent: [{ at: 0, done: 0 }] }))).toBeNull();
    expect(speed(progress({ recent: [{ at: 0, done: 0 }, { at: 900, done: 10 }] }))).toBeNull();
  });
  it("reads like the sizes, per second", () => {
    expect(formatSpeed(38_200_000)).toBe("38.2 MB/s");
    expect(formatSpeed(950_000)).toBe("950 KB/s");
    expect(formatSpeed(0)).toBe("0 bytes/s");
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

describe("sending", () => {
  const now = 1_000_000;

  it("describes each send state", () => {
    expect(stepRows(job([{ kind: "send", state: "pending", note: null }]), null, now)[0]).toEqual({
      label: "Send to PS5",
      state: "pending",
      status: "Starts when packing ends",
      bar: 0,
    });
    const running = stepRows(
      job([{ kind: "send", state: "running", note: null }]),
      progress({ done: 62, total: 100, startedAt: now - 60_000 * 13, startedDone: 0 }),
      now,
    )[0];
    expect(running).toEqual({ label: "Send to PS5", state: "active", status: "62%, about 8 min left", bar: 62 });
    const fast = stepRows(
      job([{ kind: "send", state: "running", note: null }]),
      progress({
        done: 62,
        total: 100,
        startedAt: now - 60_000 * 13,
        startedDone: 0,
        recent: [
          { at: now - 2_000, done: 0 },
          { at: now, done: 76_400_000 },
        ],
      }),
      now,
    )[0];
    expect(fast.status).toBe("62%, 38.2 MB/s, about 8 min left");
    const waiting = stepRows(job([{ kind: "send", state: "waiting", note: null }]), progress({ done: 38, total: 100 }), now)[0];
    expect(waiting).toEqual({ label: "Send to PS5", state: "paused", status: "Paused at 38%", bar: 38 });
    expect(stepRows(job([{ kind: "send", state: "done", note: "Sent and checked" }]), null, now)[0].status).toBe(
      "Every file checked",
    );
  });

  it("describes deleting the packed folder", () => {
    expect(stepRows(job([{ kind: "deletePacked", state: "pending", note: null }]), null, now)[0].status).toBe(
      "After the PS5 confirms every file",
    );
  });

  it("says what is happening while sending or waiting", () => {
    const sending = job([
      { kind: "pack", state: "done", note: "x" },
      { kind: "send", state: "running", note: null },
    ]);
    expect(footerNote({ kind: "running", job: sending })).toBe("Each file is checked on the PS5 as it lands.");
    const waiting = job([
      { kind: "pack", state: "done", note: "x" },
      { kind: "send", state: "waiting", note: null },
    ]);
    expect(footerNote({ kind: "running", job: waiting })).toBe(
      "The PS5 isn't reachable. Turn it on and start the Kite helper; sending picks up where it stopped.",
    );
  });

  it("says when sending starts by itself", () => {
    expect(footerNote({ kind: "ready", outputDir: "/Games/Packed", send: true })).toBe(
      "Packs into /Games/Packed, then sends to the PS5 by itself.",
    );
  });

  it("mentions the Trash when the packed folder went there", () => {
    const sent = job(
      [
        { kind: "send", state: "done", note: "x" },
        { kind: "deletePacked", state: "done", note: "x" },
      ],
      { state: "finished" },
    );
    expect(footerNote({ kind: "done", job: sent })).toBe(
      "Deleted folders stay in the Trash until you empty it. Play the game once first.",
    );
  });
});
