import { describe, expect, it } from "vitest";
import { dominantHue, glowShadow } from "./glow";

/** A 24x24 RGBA image of one colour. */
function solid(r: number, g: number, b: number, a = 255): Uint8ClampedArray {
  const px = new Uint8ClampedArray(24 * 24 * 4);
  for (let i = 0; i < px.length; i += 4) px.set([r, g, b, a], i);
  return px;
}

function hue([r, g, b]: [number, number, number]): number {
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  const h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return h * 60;
}

describe("dominantHue", () => {
  it("finds the cover's own colour", () => {
    const c = dominantHue(solid(0xc2, 0x70, 0x46));
    expect(c).not.toBeNull();
    expect(Math.abs(hue(c!) - hue([0xc2, 0x70, 0x46]))).toBeLessThan(10);
  });

  it("gives nothing for a grey or empty cover", () => {
    expect(dominantHue(solid(0x80, 0x80, 0x80))).toBeNull();
    expect(dominantHue(solid(0xc2, 0x70, 0x46, 0))).toBeNull();
  });

  it("prefers the saturated colour over a grey majority", () => {
    const px = solid(0x80, 0x80, 0x80);
    for (let i = 0; i < 100 * 4; i += 4) px.set([0x20, 0x60, 0xe0, 255], i);
    const c = dominantHue(px)!;
    expect(Math.abs(hue(c) - hue([0x20, 0x60, 0xe0]))).toBeLessThan(10);
  });
});

describe("glowShadow", () => {
  it("falls back to the brand glow", () => {
    expect(glowShadow(null, false)).toBe("0 24px 80px -16px rgba(194, 112, 70, 0.38)");
  });
  it("glows stronger when done", () => {
    expect(glowShadow([10, 20, 30], true)).toBe("0 24px 90px -8px rgba(10, 20, 30, 0.6)");
  });
});
