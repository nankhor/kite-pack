import { describe, expect, it } from "vitest";
import { DEFAULT_PREFS, noteLines, percent, readPrefs, writePrefs } from "./updates";

function store() {
  const data = new Map<string, string>();
  return { data, getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
}

describe("update prefs", () => {
  it("look by default and don't install by themselves", () => {
    expect(readPrefs(store())).toEqual(DEFAULT_PREFS);
    expect(DEFAULT_PREFS).toEqual({ check: true, auto: false });
  });

  it("keep what was set, and shrug off junk", () => {
    const s = store();
    writePrefs(s, { check: false, auto: true });
    expect(readPrefs(s)).toEqual({ check: false, auto: true });
    s.data.set("kite.updates", "{not json");
    expect(readPrefs(s)).toEqual(DEFAULT_PREFS);
  });
});

describe("percent", () => {
  it("rounds down, and says nothing without a size", () => {
    expect(percent(33_640_000, 84_100_000)).toBe("40%");
    expect(percent(84_100_000, 84_100_000)).toBe("100%");
    expect(percent(5, 0)).toBe("");
  });
});

describe("noteLines", () => {
  it("drops list marks, headings and blank lines", () => {
    expect(noteLines("## Fixed\n\n- One.\n* Two.\n   Three.\n")).toEqual(["Fixed", "One.", "Two.", "Three."]);
  });
});
