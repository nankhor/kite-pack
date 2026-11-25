import { afterEach, describe, expect, it } from "vitest";
import { applyTheme, readTheme, writeTheme } from "./theme";

/** A stand-in for the computer's dark-mode setting that tests can flip. */
function fakeSystem(dark: boolean) {
  const listeners = new Set<() => void>();
  const media = {
    matches: dark,
    addEventListener: (_: string, l: () => void) => listeners.add(l),
    removeEventListener: (_: string, l: () => void) => listeners.delete(l),
  };
  return {
    media: media as unknown as MediaQueryList,
    flip(to: boolean) {
      media.matches = to;
      listeners.forEach((l) => l());
    },
    listening: () => listeners.size,
  };
}

const dark = () => document.documentElement.classList.contains("dark");

afterEach(() => {
  document.documentElement.classList.remove("dark");
  localStorage.clear();
});

describe("theme", () => {
  it("follows the system until one is picked", () => {
    expect(readTheme()).toBe("system");
    applyTheme("system", fakeSystem(true).media);
    expect(dark()).toBe(true);
    applyTheme("light", fakeSystem(true).media);
    expect(dark()).toBe(false);
    applyTheme("dark", fakeSystem(false).media);
    expect(dark()).toBe(true);
  });

  it("follows a system switch live, and stops when asked", () => {
    const system = fakeSystem(false);
    const stop = applyTheme("system", system.media);
    expect(dark()).toBe(false);
    system.flip(true);
    expect(dark()).toBe(true);
    stop();
    expect(system.listening()).toBe(0);
  });

  it("remembers the choice", () => {
    writeTheme("dark");
    expect(localStorage.getItem("kite.theme")).toBe("dark");
    expect(readTheme()).toBe("dark");
    localStorage.setItem("kite.theme", "neon");
    expect(readTheme()).toBe("system");
  });
});

// The tokens as written in styles.css (a `?raw` import comes back empty in tests).
const fs = (await import(/* @vite-ignore */ `node:${"fs"}`)) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
// (A literal `new URL(..., import.meta.url)` would be rewritten by Vite into an asset URL.)
const here = import.meta.url;
const css = fs.readFileSync(new URL("./styles.css", here), "utf8");

function tokens(selector: string): Record<string, string> {
  const at = css.indexOf(`${selector} {`);
  const block = css.slice(at, css.indexOf("}", at));
  const out: Record<string, string> = {};
  for (const m of block.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-f]{6})/gi)) out[m[1]] = m[2].toLowerCase();
  return out;
}

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

const PAIRS: [string, string][] = [
  ["fg", "bg"],
  ["fg", "card"],
  ["muted", "bg"],
  ["muted", "card"],
  ["muted", "soft"],
  ["link", "card"],
  ["on-primary", "primary"],
  ["ok", "ok-bg"],
  ["warn", "warn-bg"],
  ["fail", "fail-bg"],
];

describe.each([":root", ".dark"])("%s tokens", (selector) => {
  it("read well: every text colour at 4.5:1 or more on its surface", () => {
    const t = tokens(selector);
    const weak = PAIRS.filter(([fg, bg]) => {
      expect(t[fg], `--${fg}`).toBeDefined();
      expect(t[bg], `--${bg}`).toBeDefined();
      return contrast(t[fg], t[bg]) < 4.5;
    }).map(([fg, bg]) => `${fg} on ${bg}: ${contrast(t[fg], t[bg]).toFixed(2)}`);
    expect(weak).toEqual([]);
  });
});
