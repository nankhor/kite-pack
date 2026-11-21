import { describe, expect, it } from "vitest";
import { initial, reduce, type State } from "./state";
import type { JobView, StepState, UiEvent } from "./types";

const sendJob = (send: StepState, rev: number): JobView => ({
  id: "j1",
  source: "/Games/Ashen Tide",
  outputDir: "/Games/Packed",
  deleteOriginal: false,
  packed: "/Games/Packed/Ashen Tide",
  packedBytes: null,
  steps: [
    { kind: "pack", state: "done", note: "x" },
    { kind: "send", state: send, note: null },
  ],
  state: "running",
  rev,
});

const sent = (done: number): UiEvent => ({ type: "progress", id: "j1", index: 1, done, total: 1_000 });
const on = (s: State, event: UiEvent, now: number): State => reduce(s, { type: "event", event, now });

describe("progress", () => {
  const sending: State = { ...initial, phase: "running", job: sendJob("running", 1) };

  it("keeps the last ten seconds for the speed", () => {
    let s = sending;
    for (let t = 0; t <= 15_000; t += 1_000) s = on(s, sent(t / 100), t);
    const recent = s.progress?.recent ?? [];
    expect(recent[0]).toEqual({ at: 5_000, done: 50 });
    expect(recent[recent.length - 1]).toEqual({ at: 15_000, done: 150 });
  });

  it("counts time left and speed from when a paused send moves again", () => {
    let s = on(sending, sent(100), 0);
    s = on(s, sent(200), 1_000);
    s = on(s, { type: "job", job: sendJob("waiting", 2) }, 1_500);
    expect(s.progress).toMatchObject({ done: 200, startedAt: 0 });

    s = on(s, { type: "job", job: sendJob("running", 3) }, 60_000);
    expect(s.progress).toMatchObject({ done: 200, startedAt: null, recent: [] });

    s = on(s, sent(300), 61_000);
    expect(s.progress).toMatchObject({ startedAt: 61_000, startedDone: 300, recent: [{ at: 61_000, done: 300 }] });
  });

  it("keeps the parts already sent beside the pack's own progress", () => {
    const packing: State = {
      ...initial,
      phase: "running",
      job: { ...sendJob("pending", 1), steps: [{ kind: "pack", state: "running", note: null }, { kind: "send", state: "pending", note: null }] },
    };
    let s = on(packing, { type: "progress", id: "j1", index: 0, done: 10, total: 100 }, 0);
    s = on(s, { type: "ahead", id: "j1", index: 0, sent: 1, sending: true }, 500);
    s = on(s, { type: "progress", id: "j1", index: 0, done: 20, total: 100 }, 1_000);
    expect(s.progress).toMatchObject({ index: 0, done: 20, ahead: { sent: 1, sending: true } });
    // Another job's count is not this one's.
    s = on(s, { type: "ahead", id: "other", index: 0, sent: 5, sending: false }, 1_500);
    expect(s.progress?.ahead).toEqual({ sent: 1, sending: true });
  });

  it("keeps the estimate going while the send keeps running", () => {
    let s = on(sending, sent(100), 0);
    s = on(s, { type: "job", job: sendJob("running", 2) }, 1_000);
    expect(s.progress).toMatchObject({ startedAt: 0, recent: [{ at: 0, done: 100 }] });
  });
});

describe("picks for the packed copy", () => {
  it("clear the last note when they change", () => {
    const noted = reduce(initial, { type: "message", text: "None of the 2 packages in that folder is DLC for Ashen Tide." });
    const picked = reduce(noted, { type: "bundle", bundle: { dlc: ["/DLC/suits.pkg"], backport: null } });
    expect(picked.message).toBeNull();
    expect(picked.bundle.dlc).toEqual(["/DLC/suits.pkg"]);
  });
});
