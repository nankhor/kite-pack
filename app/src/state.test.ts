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

  it("keeps the estimate going while the send keeps running", () => {
    let s = on(sending, sent(100), 0);
    s = on(s, { type: "job", job: sendJob("running", 2) }, 1_000);
    expect(s.progress).toMatchObject({ startedAt: 0, recent: [{ at: 0, done: 100 }] });
  });
});
