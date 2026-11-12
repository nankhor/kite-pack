// The soft glow around the game icon, in the icon's own colour.
//
// The colour picking is ported from PS5 Dump Forge's app/src/glow.ts (quer3q, GPL-3.0):
// the most weighted saturated hue of a 24x24 copy of the icon, made vivid but not neon.

import { useEffect, useState } from "react";

export type Rgb = [number, number, number];

const SIZE = 24;
const BINS = 12; // hue buckets, 30 degrees each
const BRAND: Rgb = [194, 112, 70];

/** The icon's glow colour, or null when it has none worth using (grey, empty). */
export function dominantHue(px: Uint8ClampedArray): Rgb | null {
  const bins = Array.from({ length: BINS }, () => ({ w: 0, r: 0, g: 0, b: 0 }));
  const sum = [0, 0, 0];
  let n = 0;
  for (let p = 0; p < px.length; p += 4) {
    if (px[p + 3] < 128) continue;
    const c: Rgb = [px[p], px[p + 1], px[p + 2]];
    sum[0] += c[0];
    sum[1] += c[1];
    sum[2] += c[2];
    n++;
    const [h, s, l] = toHsl(c);
    if (s < 0.25 || l < 0.12 || l > 0.9) continue; // grey, near-black, near-white
    const w = s * (1 - Math.abs(2 * l - 1));
    const bin = bins[Math.floor(h * BINS) % BINS];
    bin.w += w;
    bin.r += c[0] * w;
    bin.g += c[1] * w;
    bin.b += c[2] * w;
  }
  if (n === 0) return null;
  const best = bins.reduce((a, b) => (b.w > a.w ? b : a));
  if (best.w === 0) {
    const avg: Rgb = [sum[0] / n, sum[1] / n, sum[2] / n];
    return toHsl(avg)[1] < 0.12 ? null : round(tame(avg));
  }
  return round(tame([best.r / best.w, best.g / best.w, best.b / best.w]));
}

/** The icon's box-shadow: the board's two glows, in `rgb` or the brand colour. */
export function glowShadow(rgb: Rgb | null, done: boolean): string {
  const [r, g, b] = rgb ?? BRAND;
  return done ? `0 24px 90px -8px rgba(${r}, ${g}, ${b}, 0.6)` : `0 24px 80px -16px rgba(${r}, ${g}, ${b}, 0.38)`;
}

const cache = new Map<string, Rgb | null>();

/** The glow colour of an icon given as a data: URL; null until read, or when it has none. */
export function useIconGlow(dataUrl: string | null): Rgb | null {
  const [rgb, setRgb] = useState<Rgb | null>(() => (dataUrl ? (cache.get(dataUrl) ?? null) : null));
  useEffect(() => {
    if (!dataUrl) {
      setRgb(null);
      return;
    }
    if (cache.has(dataUrl)) {
      setRgb(cache.get(dataUrl) ?? null);
      return;
    }
    let live = true;
    const img = new Image();
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = SIZE;
      const ctx = canvas.getContext("2d");
      let found: Rgb | null = null;
      if (ctx) {
        ctx.drawImage(img, 0, 0, SIZE, SIZE);
        found = dominantHue(ctx.getImageData(0, 0, SIZE, SIZE).data);
      }
      cache.set(dataUrl, found);
      if (live) setRgb(found);
    };
    img.src = dataUrl;
    return () => {
      live = false;
    };
  }, [dataUrl]);
  return rgb;
}

function round(c: Rgb): Rgb {
  return c.map(Math.round) as Rgb;
}

/** A glow colour: vivid but not neon, mid lightness. */
function tame(c: Rgb): Rgb {
  const [h, s, l] = toHsl(c);
  return fromHsl(h, Math.min(0.95, Math.max(0.6, s)), Math.min(0.58, Math.max(0.45, l)));
}

function toHsl([r, g, b]: Rgb): [number, number, number] {
  const [x, y, z] = [r / 255, g / 255, b / 255];
  const max = Math.max(x, y, z);
  const min = Math.min(x, y, z);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return [0, 0, l];
  const s = d / (1 - Math.abs(2 * l - 1));
  const h = max === x ? ((y - z) / d + 6) % 6 : max === y ? (z - x) / d + 2 : (x - y) / d + 4;
  return [h / 6, s, l];
}

function fromHsl(h: number, s: number, l: number): Rgb {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = h * 6;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  const [r, g, b] =
    hp < 1 ? [c, x, 0] : hp < 2 ? [x, c, 0] : hp < 3 ? [0, c, x] : hp < 4 ? [0, x, c] : hp < 5 ? [x, 0, c] : [c, 0, x];
  const m = l - c / 2;
  return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}
