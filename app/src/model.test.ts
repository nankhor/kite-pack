import { describe, expect, it } from "vitest";
import {
  folderNote,
  footerNote,
  formatSize,
  formatSpeed,
  formatWhen,
  historyEntry,
  jobBar,
  minutesLeft,
  packBadge,
  sendsAlongside,
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
    ).toBe("Retry once that's fixed, or Cancel.");
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

  it("says what to do when the drive is too full", () => {
    const full = job(
      [{ kind: "pack", state: "failed", note: "Not enough free space in Games: needs up to 592.4 GB, 322.1 GB free" }],
      { state: "failed", outputDir: "C:\\Users\\parwe\\Documents\\Games" },
    );
    expect(footerNote({ kind: "failed", job: full })).toBe(
      "Free up space in C:\\Users\\parwe\\Documents\\Games and Retry, or Cancel and pick another folder under Save to.",
    );
    const silent = job([{ kind: "send", state: "failed", note: null }], { state: "failed" });
    expect(footerNote({ kind: "failed", job: silent })).toBe("Send to PS5 failed with no reason given. Retry, or Cancel.");
  });

  it("says when sending starts by itself", () => {
    expect(footerNote({ kind: "ready", outputDir: "/Games/Packed", send: true })).toBe(
      "Packs into /Games/Packed, then sends to the PS5 by itself.",
    );
  });

  it("says each part goes as soon as it's packed when the helper can take it", () => {
    expect(footerNote({ kind: "ready", outputDir: "/Games/Packed", send: true, alongside: true })).toBe(
      "Packs into /Games/Packed and sends each part to the PS5 as soon as it's packed.",
    );
    expect(footerNote({ kind: "ready", outputDir: "/Games", send: true, alongside: true, bundled: 2 })).toBe(
      "Packs into /Games with 2 add-ons inside and sends each part to the PS5 as soon as it's packed.",
    );
    // Not sending: packing alone, whatever the helper could do.
    expect(footerNote({ kind: "ready", outputDir: "/Games/Packed", send: false, alongside: true })).toBe("Packs into /Games/Packed.");
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

describe("footerNote with DLC inside the pack", () => {
  it("counts the add-ons the packed copy will carry", () => {
    expect(footerNote({ kind: "ready", outputDir: "/Games", send: true, bundled: 20 })).toBe(
      "Packs into /Games with 20 add-ons inside, then sends to the PS5 by itself.",
    );
    expect(footerNote({ kind: "ready", outputDir: "/Games", send: false, bundled: 20 })).toBe(
      "Packs into /Games with 20 add-ons inside.",
    );
    expect(footerNote({ kind: "ready", outputDir: "/Games", bundled: 1 })).toBe("Packs into /Games with 1 add-on inside.");
  });
});

describe("jobBar", () => {
  const st = (kind: StepView["kind"], state: StepView["state"], extra: Partial<StepView> = {}): StepView => ({
    kind,
    state,
    note: null,
    ...extra,
  });
  const dlc = (state: StepView["state"], pkg: string) => st("installDlc", state, { pkg });

  it("has one part per step the job has, packing and sending as one, the DLC as one", () => {
    const bar = jobBar(
      job([
        st("pack", "done"),
        st("deleteOriginal", "done"),
        st("send", "done"),
        dlc("done", "/d/a.pkg"),
        dlc("running", "/d/b.pkg"),
        dlc("pending", "/d/c.pkg"),
        st("deletePacked", "pending"),
      ]),
      null,
      0,
      { names: { "/d/a.pkg": "Suit Pack", "/d/b.pkg": "Arena Map" } },
    );
    expect(bar.parts.map((p) => p.label)).toEqual(["Pack and send", "Trash original", "Install DLC", "Trash packed copy"]);
    expect(bar.parts.map((p) => p.state)).toEqual(["done", "done", "active", "pending"]);
    expect(bar.parts[2]).toMatchObject({ note: "2 of 3: Arena Map", fill: 33 });
    expect(bar.parts[2].items).toHaveLength(3);
    expect(bar.parts[3].note).toBe("At the end");
    expect(bar).toMatchObject({ head: "Installing DLC", detail: "2 of 3: Arena Map", tone: "plain" });
  });

  it("leaves out what the job doesn't do", () => {
    expect(jobBar(job([st("pack", "done"), st("send", "running")]), null, 0).parts.map((p) => p.label)).toEqual([
      "Pack and send",
    ]);
    expect(jobBar(job([st("pack", "running")]), null, 0).parts.map((p) => p.label)).toEqual(["Pack"]);
    expect(jobBar(job([st("pack", "running"), st("deleteOriginal", "pending")]), null, 0).parts.map((p) => p.label)).toEqual([
      "Pack",
      "Trash original",
    ]);
  });

  it("packs alone on one plain bar", () => {
    const bar = jobBar(job([st("pack", "running")]), progress({ index: 0, done: 62, total: 100 }), 0);
    expect(bar).toMatchObject({ head: "Packing", tone: "plain", overall: 62 });
    expect(bar.detail).toMatch(/^62%/);
    expect(bar.parts).toHaveLength(1);
    expect(bar.parts[0]).toMatchObject({ label: "Pack", state: "active", fill: 62, note: "62% packed" });
    expect(bar.parts[0].under).toBeUndefined();
  });

  it("packs under what the PS5 has, before any part has gone", () => {
    const packing = job([st("pack", "running"), st("send", "pending")]);
    const bar = jobBar(packing, progress({ index: 0, done: 62, total: 100 }), 0);
    expect(bar).toMatchObject({ head: "Packing", detail: "62% packed", tone: "plain" });
    expect(bar.parts[0]).toMatchObject({ label: "Pack and send", state: "active", fill: 0, under: 62, note: "62% packed" });
    // Halfway between what is packed (62) and what the PS5 has (0).
    expect(bar.overall).toBe(31);
  });

  it("says what the PS5 has while it still packs", () => {
    const packing = job([st("pack", "running"), st("deleteOriginal", "pending"), st("send", "pending")]);
    const at = (ahead: ProgressState["ahead"]) => jobBar(packing, progress({ index: 0, done: 62, total: 100, ahead }), 0);
    const first = at({ sent: 0, sending: true, onPs5: 0, estimate: 0, recent: [] });
    expect(first).toMatchObject({ head: "Packing and sending", detail: "62% packed" });
    expect(first.parts[0]).toMatchObject({ fill: 0, under: 62, note: "62% packed" });

    const some = at({ sent: 1, sending: true, onPs5: 14_200_000_000, estimate: 71_000_000_000, recent: [] });
    expect(some).toMatchObject({ head: "Packing and sending", detail: "62% packed, 20% on the PS5" });
    expect(some.parts[0]).toMatchObject({ fill: 20, under: 62, note: "62% packed, 14.2 GB on the PS5" });
    expect(some.parts.map((p) => p.label)).toEqual(["Pack and send", "Trash original"]);

    // No size to go by yet: the bytes, not a share.
    const unsized = at({ sent: 1, sending: false, onPs5: 4_000_000_000, estimate: 0, recent: [] });
    expect(unsized.detail).toBe("62% packed, 4.0 GB on the PS5");
    expect(unsized.parts[0].fill).toBe(0);
  });

  it("gives the time left by whichever of packing and sending ends last", () => {
    const packing = job([st("pack", "running"), st("send", "pending")]);
    const ahead = {
      sent: 3,
      sending: true,
      onPs5: 14_200_000_000,
      estimate: 71_000_000_000,
      // 10 GB a minute: 56.8 GB more to go takes 6 minutes.
      recent: [
        { at: 0, bytes: 4_200_000_000 },
        { at: 60_000, bytes: 14_200_000_000 },
      ],
    };
    // Packing: 50 of 100 in 10 minutes, so 10 more.
    const slowPack = progress({ index: 0, done: 50, total: 100, startedAt: 0, startedDone: 0, ahead });
    expect(jobBar(packing, slowPack, 600_000).detail).toBe("50% packed, 20% on the PS5, about 10 min left");
    // Packing: 90 of 100 in 9 minutes, so 1 more; sending still needs 6.
    const fastPack = progress({ index: 0, done: 90, total: 100, startedAt: 0, startedDone: 0, ahead });
    expect(jobBar(packing, fastPack, 540_000).detail).toBe("90% packed, 20% on the PS5, about 6 min left");
  });

  it("counts what the PS5 has of the whole, the parts sent while packing included", () => {
    const rest = job([st("pack", "done"), st("send", "running")], { packedBytes: 71_200_000_000, aheadBytes: 20_000_000_000 });
    const bar = jobBar(rest, progress({ index: 1, done: 24_100_000_000, total: 51_200_000_000 }), 0);
    expect(bar.head).toBe("Sending the rest");
    expect(bar.detail).toMatch(/^61% on the PS5/);
    expect(bar.parts[0]).toMatchObject({ state: "active", fill: 61, under: 100, note: "44.1 GB of 71.2 GB on the PS5" });

    const all = job([st("pack", "done"), st("send", "running")], { packedBytes: 71_200_000_000 });
    const sending = jobBar(all, progress({ index: 1, done: 44_100_000_000, total: 71_200_000_000 }), 0);
    expect(sending.head).toBe("Sending to PS5");
    expect(sending.parts[0].note).toBe("44.1 GB of 71.2 GB on the PS5");
  });

  it("says what is on the PS5 while the original goes to the Trash", () => {
    const between = job([st("pack", "done"), st("deleteOriginal", "running"), st("send", "pending")], {
      packedBytes: 71_200_000_000,
      aheadBytes: 60_200_000_000,
    });
    const bar = jobBar(between, null, 0);
    expect(bar.head).toBe("Moving the original to the Trash");
    expect(bar.parts[0]).toMatchObject({ label: "Pack and send", state: "active", fill: 84, note: "60.2 GB on the PS5, the rest next" });
  });

  it("starts with nothing to count yet", () => {
    const bar = jobBar(job([st("pack", "running")]), null, 0);
    expect(bar).toMatchObject({ head: "Packing", detail: "Starting" });
    expect(bar.parts[0]).toMatchObject({ fill: 0, note: "" });
    const both = jobBar(job([st("pack", "running"), st("send", "pending")]), null, 0);
    expect(both).toMatchObject({ head: "Packing", detail: "Starting" });
    expect(both.parts[0]).toMatchObject({ fill: 0, under: 0 });
  });

  it("turns amber while it waits for the PS5", () => {
    const bar = jobBar(job([st("pack", "done"), st("send", "waiting")]), progress({ index: 1, done: 38, total: 100 }), 0);
    expect(bar).toMatchObject({ head: "Waiting for the PS5", detail: "Paused at 38% on the PS5", tone: "amber" });
    expect(bar.parts[0]).toMatchObject({ state: "paused", fill: 38, note: "Paused" });
  });

  it("runs a light through a move to the Trash, with nothing to count", () => {
    const bar = jobBar(job([st("pack", "done"), st("deleteOriginal", "running"), st("send", "pending")]), null, 0);
    expect(bar.head).toBe("Moving the original to the Trash");
    expect(bar.detail).toBe("");
    expect(bar.parts[1]).toMatchObject({ label: "Trash original", fill: null, note: "" });
  });

  it("says which step failed and why", () => {
    const sent = job([st("pack", "done"), st("send", "failed", { note: "No room on the PS5" })], { state: "failed" });
    const bar = jobBar(sent, null, 0);
    expect(bar).toMatchObject({ head: "Sending failed", detail: "No room on the PS5", tone: "fail" });
    // Why is said once, above the bar.
    expect(bar.parts[0]).toMatchObject({ state: "failed", fill: 100, note: "Failed" });

    const packed = job([st("pack", "failed", { note: "Disk full" }), st("send", "pending")], { state: "failed" });
    expect(jobBar(packed, null, 0)).toMatchObject({ head: "Packing failed", detail: "Disk full", tone: "fail" });
    const alone = job([st("pack", "failed", { note: "Disk full" })], { state: "failed" });
    const bar2 = jobBar(alone, null, 0);
    expect(bar2).toMatchObject({ head: "Packing failed", detail: "Disk full", tone: "fail" });
    expect(bar2.parts[0].note).toBe("Failed");
  });

  it("ends mint, whole", () => {
    const sent = job([st("pack", "done"), st("send", "done")], { state: "finished", packedBytes: 51_600_000_000 });
    const bar = jobBar(sent, null, 0);
    expect(bar).toMatchObject({ head: "On the PS5", detail: "Sent and checked", tone: "mint", overall: 100 });
    expect(bar.parts[0]).toMatchObject({ state: "done", fill: 100, note: "51.6 GB in Packed, sent and checked" });
    const kept = jobBar(job([st("pack", "done")], { state: "finished" }), null, 0);
    expect(kept).toMatchObject({ head: "Packed", detail: "Packed and checked", tone: "mint" });
  });

  it("says what was folded into the packed copy", () => {
    const packed = job([st("pack", "done"), st("deleteOriginal", "running")], { packedBytes: 71_200_000_000 });
    expect(jobBar(packed, null, 0, { inside: 21 }).parts[0].note).toBe("71.2 GB in Packed, 21 add-ons inside");
    expect(jobBar(packed, null, 0, { inside: 1 }).parts[0].note).toBe("71.2 GB in Packed, 1 add-on inside");
    const sent = job([st("pack", "done"), st("send", "done")], { state: "finished", packedBytes: 71_200_000_000 });
    expect(jobBar(sent, null, 0, { inside: 21 }).parts[0].note).toBe("71.2 GB in Packed, 21 add-ons inside, sent and checked");
  });
});

describe("folderNote", () => {
  it("says nothing when every package went in", () => {
    expect(folderNote(3, [], "Ashen Tide")).toBeNull();
  });

  it("names what was left out, up to three", () => {
    expect(folderNote(2, ["Patch.pkg"], "Ashen Tide")).toBe(
      "Added 2 DLC from the folder. Left out 1 file that isn't DLC for this game: Patch.pkg.",
    );
    expect(folderNote(1, ["a.pkg", "b.pkg", "c.pkg", "d.pkg", "e.pkg"], "Ashen Tide")).toBe(
      "Added 1 DLC from the folder. Left out 5 files that aren't DLC for this game: a.pkg, b.pkg, c.pkg and 2 more.",
    );
  });

  it("says when none of them is DLC for the game", () => {
    expect(folderNote(0, ["Other.pkg"], "Ashen Tide")).toBe("The package in that folder isn't DLC for Ashen Tide.");
    expect(folderNote(0, ["a.pkg", "b.pkg"], "Ashen Tide")).toBe("None of the 2 packages in that folder is DLC for Ashen Tide.");
  });
});


describe("the pack row's add-on count", () => {
  it("is said once, even when the pack step already says it", () => {
    const bar = jobBar(
      job([
        { kind: "pack", state: "done", note: "612 files, 80 GB, checked · bundled: 60 DLC inside, DLC emulator added" },
        { kind: "deleteOriginal", state: "running", note: null },
      ]),
      null,
      0,
      { inside: 60 },
    );
    const pack = bar.parts.find((p) => p.kind === "pack")!;
    expect(pack.note).toBe("Packed and checked · 60 DLC inside, DLC emulator added");
  });

  it("stays after the send, once", () => {
    const bar = jobBar(
      job(
        [
          { kind: "pack", state: "done", note: "612 files, 80 GB, checked · bundled: 60 DLC inside, DLC emulator added" },
          { kind: "send", state: "done", note: null },
        ],
        { state: "finished" },
      ),
      null,
      0,
      { inside: 60 },
    );
    expect(bar.parts[0].note).toBe("Packed, sent and checked · 60 DLC inside, DLC emulator added");
  });
});

describe("sendsAlongside", () => {
  const running = (version: string) => ({ state: "running" as const, version });
  it("sends a packed folder part by part with helper 0.4.0 and later", () => {
    expect(sendsAlongside("folder", running("0.4.0"))).toBe(true);
    expect(sendsAlongside("folder", running("0.5.1"))).toBe(true);
    expect(sendsAlongside("folder", running("1.0.0"))).toBe(true);
  });

  it("waits for the end of packing otherwise", () => {
    expect(sendsAlongside("folder", running("0.3.0"))).toBe(false);
    expect(sendsAlongside("exfat", running("0.4.0"))).toBe(false);
    expect(sendsAlongside("ffpkg", running("0.4.0"))).toBe(false);
    expect(sendsAlongside("folder", { state: "notReachable" })).toBe(false);
    expect(sendsAlongside("folder", null)).toBe(false);
  });
});
