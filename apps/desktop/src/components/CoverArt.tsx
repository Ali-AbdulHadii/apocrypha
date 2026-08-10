/**
 * Procedural cover art for a catalogue card.
 *
 * The service has no mod images yet, and a grid of cards with nothing in them
 * reads as broken rather than as empty. This gives each mod a quiet gradient
 * tinted by a hue derived from its own slug, so cards are distinguishable at a
 * glance and a given mod always looks the same — without inventing artwork
 * nobody supplied.
 *
 * It is deliberately the same construction the website uses, so the two do not
 * drift into looking like different products, and so that when real images
 * arrive this is exactly the element they replace.
 *
 * Only the hue is set here. Lightness, saturation, the mark and the sheen all
 * come from theme tokens, because fixed near-black values put a dark slab on
 * every card in light mode.
 */

import type { CSSProperties } from "react";
import { Logo } from "./icons";

/** Stable small hash → hue, so a given slug always gets the same cover. */
function hueFrom(seed: string): number {
  let h = 0;
  for (let i = 0; i < seed.length; i++) {
    h = (h * 31 + seed.charCodeAt(i)) % 360;
  }
  return h;
}

export function CoverArt({ seed, mark = 96 }: { seed: string; mark?: number }) {
  const hue = hueFrom(seed);
  const style = {
    "--cover-h": hue,
    // A second hue a fifth of the wheel away, so the gradient has somewhere to
    // travel to. Any further apart and the cards start reading as a rainbow.
    "--cover-h2": (hue + 46) % 360,
  } as CSSProperties;

  return (
    <div className="cover" style={style} aria-hidden>
      <span className="cover-mark">
        <Logo size={mark} />
      </span>
      <span className="cover-sheen" />
    </div>
  );
}
