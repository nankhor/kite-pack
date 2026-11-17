import postcss from "postcss";
import { describe, expect, it } from "vitest";

// The files as written (a `?raw` import comes back empty in tests). Node's fs is loaded by name
// so the app's TypeScript setup doesn't need Node's types.
const fs = (await import(/* @vite-ignore */ `node:${"fs"}`)) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
// (A literal `new URL(..., import.meta.url)` would be rewritten by Vite into an asset URL.)
const here = import.meta.url;
const read = (file: string) => fs.readFileSync(new URL(file, here), "utf8");

// A rule left open swallows the rules after it, and browsers drop them without a word: the DLC
// lists once lost their window style that way. The app's CSS has no nesting, so a rule inside a
// plain rule, or a block still open at the end, always means a missing "}".
describe.each(["screen.css", "styles.css"])("%s", (file) => {
  it("closes every rule before the next one starts", () => {
    const css = read(file);
    expect(css.length).toBeGreaterThan(1000);
    let root: postcss.Root | null = null;
    expect(() => (root = postcss.parse(css))).not.toThrow();
    const nested: string[] = [];
    root!.walkRules((rule) => {
      if (rule.parent?.type === "rule") nested.push(`${rule.selector} (inside ${(rule.parent as postcss.Rule).selector})`);
    });
    expect(nested).toEqual([]);
  });
});
