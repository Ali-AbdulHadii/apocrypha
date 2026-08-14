/**
 * The colour a game already has.
 *
 * Steam caches cover art for every installed game and the app already loads it
 * as a data URI, so a game's palette is on hand without asking anyone to choose
 * one and without a network request. That is what the Play button is tinted
 * with: Cyberpunk comes out yellow, Monster Hunter comes out gold-brown,
 * whatever ships next comes out whatever it looks like.
 *
 * The alternative was a colour written into each profile TOML. It would have
 * been reviewable, but it makes every new game wait for somebody to have an
 * opinion, and a hand-picked hex is a second source of truth about what a game
 * looks like when the artwork is right there.
 *
 * Everything here runs on a canvas in the webview against a data URI that is
 * already in memory. Nothing is fetched and nothing is written.
 */

/** A colour, and a foreground that can be read against it. */
export interface Accent {
  /** `h s% l%`, ready for a CSS colour function. */
  hsl: string;
  /** Black or white, whichever the label survives on. */
  ink: string;
}

/**
 * How coarsely the image is quantised before counting.
 *
 * Cover art is photographic: without bucketing, a thousand near-identical
 * shades of the same sky each count once and the winner is decided by noise.
 * Five bits collapses those into one bucket while still telling apart colours
 * anybody would call different.
 */
const BUCKET_BITS = 3;
/** Longest edge the image is scaled to before reading pixels. */
const SAMPLE_EDGE = 96;

/**
 * Pull the most representative colour out of an image.
 *
 * Resolves `null` when there is nothing usable — no art, a decode failure, or
 * an image with no colour in it at all. Every caller has to have an answer for
 * that anyway, because a game Steam has never cached art for is normal.
 */
export async function accentFromImage(src: string): Promise<Accent | null> {
  const img = await load(src);
  if (!img) return null;

  const scale = Math.min(1, SAMPLE_EDGE / Math.max(img.width, img.height));
  const w = Math.max(1, Math.round(img.width * scale));
  const h = Math.max(1, Math.round(img.height * scale));

  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;

  ctx.drawImage(img, 0, 0, w, h);
  let data: Uint8ClampedArray;
  try {
    data = ctx.getImageData(0, 0, w, h).data;
  } catch {
    // A tainted canvas. Not possible for a data URI, but reading pixels is the
    // kind of call that should not be able to take the screen down with it.
    return null;
  }

  const counts = new Map<number, { n: number; r: number; g: number; b: number }>();
  for (let i = 0; i < data.length; i += 4) {
    const [r, g, b, a] = [data[i], data[i + 1], data[i + 2], data[i + 3]];
    if (a < 200) continue;

    const { s, l } = rgbToHsl(r, g, b);
    // Near-black, near-white and near-grey are skipped. Cover art is mostly
    // dark background, so counting every pixel returns "dark grey" for almost
    // every game — technically the most common colour and useless as an
    // identity. What is wanted is the colour somebody would name.
    if (l < 0.18 || l > 0.9 || s < 0.22) continue;

    const key =
      ((r >> (8 - BUCKET_BITS)) << (BUCKET_BITS * 2)) |
      ((g >> (8 - BUCKET_BITS)) << BUCKET_BITS) |
      (b >> (8 - BUCKET_BITS));
    const seen = counts.get(key);
    if (seen) {
      seen.n += 1;
      seen.r += r;
      seen.g += g;
      seen.b += b;
    } else {
      counts.set(key, { n: 1, r, g, b });
    }
  }
  if (counts.size === 0) return null;

  // Weighted by how saturated the bucket is as well as how common it is. A
  // muted colour covering slightly more of the image should not beat the one
  // the artwork is actually built around.
  let best: { n: number; r: number; g: number; b: number } | null = null;
  let bestScore = -1;
  for (const c of counts.values()) {
    const r = c.r / c.n;
    const g = c.g / c.n;
    const b = c.b / c.n;
    const { s } = rgbToHsl(r, g, b);
    const score = c.n * (0.5 + s);
    if (score > bestScore) {
      bestScore = score;
      best = { n: c.n, r, g, b };
    }
  }
  if (!best) return null;

  return toAccent(best.r, best.g, best.b);
}

/**
 * Bring a sampled colour into a range a button can actually use.
 *
 * Artwork is not designed to be a UI surface: cover art is full of colours that
 * are too dark to look deliberate or too pale to carry white text. Hue and the
 * character of the saturation are kept, lightness is pulled into a band that
 * reads as a button, and the label is then whichever of black or white has the
 * better contrast against the result.
 */
function toAccent(r: number, g: number, b: number): Accent {
  const { h, s, l } = rgbToHsl(r, g, b);
  const sat = clamp(s, 0.35, 0.85);
  const light = clamp(l, 0.34, 0.62);
  return {
    hsl: `${Math.round(h)} ${Math.round(sat * 100)}% ${Math.round(light * 100)}%`,
    ink: contrastInk(h, sat, light),
  };
}

/**
 * Black or white on top, by relative luminance rather than by lightness.
 *
 * HSL lightness is not perceptual: a saturated yellow at 55% and a saturated
 * blue at 55% are nothing alike to read against, and choosing by `l` alone puts
 * white text on the yellow.
 */
function contrastInk(h: number, s: number, l: number): string {
  const [r, g, b] = hslToRgb(h, s, l);
  const lin = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  const luminance = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  // 0.179 is where contrast against white and against black are equal.
  return luminance > 0.179 ? "#0b0b0c" : "#ffffff";
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function load(src: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = src;
  });
}

function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };

  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === rn) h = ((gn - bn) / d + (gn < bn ? 6 : 0)) * 60;
  else if (max === gn) h = ((bn - rn) / d + 2) * 60;
  else h = ((rn - gn) / d + 4) * 60;
  return { h, s, l };
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  const [r1, g1, b1] =
    hp < 1
      ? [c, x, 0]
      : hp < 2
        ? [x, c, 0]
        : hp < 3
          ? [0, c, x]
          : hp < 4
            ? [0, x, c]
            : hp < 5
              ? [x, 0, c]
              : [c, 0, x];
  const m = l - c / 2;
  return [
    Math.round((r1 + m) * 255),
    Math.round((g1 + m) * 255),
    Math.round((b1 + m) * 255),
  ];
}
