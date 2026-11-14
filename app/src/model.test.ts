import { describe, expect, it } from "vitest";
import {
  footerNote,
  formatSize,
  formatSpeed,
  formatWhen,
  historyEntry,
  minutesLeft,
  packBadge,
  speed,
  stepRows,
  updateLabel,
} from "./model";
import type { GameInfo, JobView, ProgressState, StepView } from "./types";

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

describe("DLC steps", () => {
  const dlc = (state: StepView["state"], pkg: string, note: string | null = null): StepView => ({
    kind: "installDlc",
    state,
    note,
    pkg,
  });
  const before: StepView[] = [
    { kind: "pack", state: "done", note: "x" },
    { kind: "send", state: "done", note: null },
  ];
  const names = { "/dlc/a.pkg": "Suit Pack", "/dlc/b.pkg": "Arena Map", "/dlc/c.pkg": "Soundtrack" };

  it("shows every DLC as one row", () => {
    const steps = [...before, dlc("pending", "/dlc/a.pkg"), dlc("pending", "/dlc/b.pkg")];
    const rows = stepRows(job([...steps, { kind: "deletePacked", state: "pending", note: null }]), null, 0, names);
    expect(rows.map((r) => r.label)).toEqual(["Pack", "Send to PS5", "Install DLC", "Delete the packed folder"]);
    expect(rows[2]).toMatchObject({ state: "pending", status: "2 DLC, after the game is on the PS5", bar: null });
  });

  it("names the DLC installing now and counts through them", () => {
    const steps = [...before, dlc("done", "/dlc/a.pkg"), dlc("running", "/dlc/b.pkg"), dlc("pending", "/dlc/c.pkg")];
    const row = stepRows(job(steps), null, 0, names)[2];
    expect(row).toMatchObject({ state: "active", status: "2 of 3: Arena Map", bar: 33 });
    expect(row.items).toEqual([
      { name: "Suit Pack", state: "installed", note: null },
      { name: "Arena Map", state: "installing", note: null },
      { name: "Soundtrack", state: "waiting", note: null },
    ]);
  });

  it("keeps going past a failed DLC, and says so", () => {
    const steps = [...before, dlc("failed", "/dlc/a.pkg", "No room on the PS5"), dlc("running", "/dlc/b.pkg"), dlc("pending", "/dlc/c.pkg")];
    const row = stepRows(job(steps), null, 0, names)[2];
    expect(row.status).toBe("2 of 3: Arena Map · 1 failed");
    expect(row.items?.[0]).toEqual({ name: "Suit Pack", state: "failed", note: "No room on the PS5" });
  });

  it("ends failed when one didn't install", () => {
    const steps = [...before, dlc("failed", "/dlc/a.pkg", "x"), dlc("done", "/dlc/b.pkg"), dlc("done", "/dlc/c.pkg")];
    expect(stepRows(job(steps), null, 0, names)[2]).toMatchObject({ state: "failed", status: "1 of 3 failed", bar: null });
  });

  it("says when they're all in", () => {
    const steps = [...before, dlc("done", "/dlc/a.pkg"), dlc("done", "/dlc/b.pkg")];
    expect(stepRows(job(steps), null, 0, names)[2]).toMatchObject({ state: "done", status: "2 installed" });
    expect(stepRows(job([...before, dlc("done", "/dlc/a.pkg")]), null, 0, names)[2].status).toBe("Installed");
  });

  it("waits with the PS5", () => {
    const steps = [...before, dlc("done", "/dlc/a.pkg"), dlc("waiting", "/dlc/b.pkg"), dlc("pending", "/dlc/c.pkg")];
    expect(stepRows(job(steps), null, 0, names)[2]).toMatchObject({ state: "paused", status: "Waiting for the PS5 at 2 of 3" });
  });

  it("deletes the packed folder only after the DLC, and says so", () => {
    const steps = [...before, dlc("running", "/dlc/a.pkg"), { kind: "deletePacked" as const, state: "pending" as const, note: null }];
    expect(stepRows(job(steps), null, 0, names)[3].status).toBe("After the PS5 confirms the game and every DLC");
    expect(footerNote({ kind: "running", job: job(steps) })).toBe(
      "The PS5 installs each DLC with its own installer. If one fails, the rest still go.",
    );
  });

  it("uses the file name for a DLC it has no name for", () => {
    const row = stepRows(job([...before, dlc("running", "C:\\DLC\\Deluxe_Upgrade.pkg")]), null, 0, {})[2];
    expect(row.status).toBe("1 of 1: Deluxe_Upgrade");
  });
});

describe("packBadge", () => {
  const packing = job([{ kind: "pack", state: "running", note: null }]);

  it("is empty without a job", () => {
    expect(packBadge(null, null)).toBeNull();
  });

  it("shows how far the running step is", () => {
    expect(packBadge(packing, progress({ index: 0, done: 62, total: 100 }))).toEqual({ text: "62%", tone: "mint" });
    expect(packBadge(packing, null)).toEqual({ text: "Working", tone: "mint" });
  });

  it("counts through the DLC", () => {
    const steps: StepView[] = [
      { kind: "send", state: "done", note: null },
      { kind: "installDlc", state: "done", note: null, pkg: "/a.pkg" },
      { kind: "installDlc", state: "running", note: null, pkg: "/b.pkg" },
      { kind: "installDlc", state: "pending", note: null, pkg: "/c.pkg" },
    ];
    expect(packBadge(job(steps), null)).toEqual({ text: "DLC 2/3", tone: "mint" });
  });

  it("says when it waits, failed or is done", () => {
    expect(packBadge(job([{ kind: "send", state: "waiting", note: null }]), null)).toEqual({ text: "Paused", tone: "amber" });
    expect(packBadge(job([], { state: "failed" }), null)).toEqual({ text: "Failed", tone: "amber" });
    expect(packBadge(job([], { state: "finished" }), null)).toEqual({ text: "Done", tone: "mint" });
  });
});

describe("formatWhen", () => {
  const now = new Date(2026, 10, 14, 15, 0).getTime();

  it("says today and yesterday with the time, then the date", () => {
    expect(formatWhen(new Date(2026, 10, 14, 10, 42).getTime(), now)).toBe("Today, 10:42");
    expect(formatWhen(new Date(2026, 10, 13, 23, 5).getTime(), now)).toBe("Yesterday, 23:05");
    expect(formatWhen(new Date(2026, 10, 12, 9, 0).getTime(), now)).toBe("12 Nov");
    expect(formatWhen(new Date(2025, 10, 12, 9, 0).getTime(), now)).toBe("12 Nov 2025");
  });
});

describe("historyEntry", () => {
  const game: GameInfo = {
    kind: "folder",
    title: "Ashen Tide",
    titleId: "PPSA01234",
    contentId: null,
    version: null,
    totalBytes: 84_200_000_000,
    dlcs: [],
    iconDataUrl: "data:image/png;base64,AAAA",
    findings: [],
  };

  it("records what was packed, sent and kept", () => {
    const finished = job(
      [
        { kind: "pack", state: "done", note: null },
        { kind: "send", state: "done", note: null },
        { kind: "installDlc", state: "done", note: null, pkg: "/a.pkg" },
        { kind: "installDlc", state: "done", note: null, pkg: "/b.pkg" },
        { kind: "deletePacked", state: "done", note: null },
      ],
      { state: "finished", packed: "/Games/Packed/ashen", packedBytes: 51_600_000_000 },
    );
    expect(historyEntry(finished, game, 1_000)).toEqual({
      id: "j1",
      title: "Ashen Tide",
      titleId: "PPSA01234",
      finishedAt: 1_000,
      sourceBytes: 84_200_000_000,
      packedBytes: 51_600_000_000,
      sent: true,
      packed: null,
      dlc: 2,
      iconDataUrl: "data:image/png;base64,AAAA",
    });
  });

  it("keeps where the packed copy is when it stayed", () => {
    const kept = job([{ kind: "pack", state: "done", note: null }], { state: "finished", packed: "/Games/Packed/ashen" });
    const entry = historyEntry(kept, { ...game, title: null }, 1);
    expect(entry).toMatchObject({ title: "PPSA01234", sent: false, packed: "/Games/Packed/ashen", dlc: 0 });
  });
});

describe("footerNote with DLC", () => {
  it("counts the DLC it will install", () => {
    expect(footerNote({ kind: "ready", outputDir: "/Games", send: true, dlc: 20 })).toBe(
      "Packs into /Games, sends to the PS5, then installs 20 DLC.",
    );
    expect(footerNote({ kind: "ready", outputDir: "/Games", send: false, dlc: 20 })).toBe("Packs into /Games.");
  });
});
