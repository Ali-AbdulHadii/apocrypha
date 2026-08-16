/**
 * Icon set.
 *
 * Rules, applied uniformly:
 *   single colour  (inherits currentColor, never multi-tone)
 *   1.5 stroke     on a 24 grid, round caps and joins
 *   no fills       outlines only, so they read at 16px
 *   16px default   matching the --sp-4 rhythm
 *
 * Drawn inline rather than pulled from a package: no extra dependency, and the
 * whole set stays consistent because it is one file.
 */

import type { ReactNode } from "react";

export type IconProps = { size?: number; strokeWidth?: number };

function make(path: ReactNode) {
  return function IconComponent({ size = 16, strokeWidth = 1.5 }: IconProps) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        {path}
      </svg>
    );
  };
}

export const Icon = {
  /* navigation */
  library: make(
    <>
      <rect x="3" y="4" width="7" height="16" rx="1" />
      <rect x="14" y="4" width="7" height="9" rx="1" />
      <path d="M14 17h7" />
    </>,
  ),
  mods: make(
    <>
      <path d="M12 3 4 7l8 4 8-4-8-4Z" />
      <path d="m4 17 8 4 8-4" />
      <path d="m4 12 8 4 8-4" />
    </>,
  ),
  profiles: make(
    <>
      <path d="M4 20a6 6 0 0 1 12 0" />
      <circle cx="10" cy="8" r="4" />
      <path d="M18 14a5 5 0 0 1 3 4.5" />
    </>,
  ),
  /* Load order: a stack, and the arrow that moves a row through it. */
  order: make(
    <>
      <path d="M3 6h10" />
      <path d="M3 12h10" />
      <path d="M3 18h10" />
      <path d="M18 4v16" />
      <path d="m15 7 3-3 3 3" />
      <path d="m15 17 3 3 3-3" />
    </>,
  ),
  conflicts: make(
    <>
      <path d="M12 4 3 19h18L12 4Z" />
      <path d="M12 10v4" />
      <path d="M12 17h.01" />
    </>,
  ),
  downloads: make(
    <>
      <path d="M12 3v11" />
      <path d="m7.5 9.5 4.5 4.5 4.5-4.5" />
      <path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />
    </>,
  ),
  settings: make(
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1" />
    </>,
  ),

  /* actions */
  plus: make(
    <>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </>,
  ),
  account: make(
    <>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="10" r="3" />
      <path d="M6.2 18.4a6.5 6.5 0 0 1 11.6 0" />
    </>,
  ),
  check: make(<path d="m5 13 4 4L19 7" />),
  /** The "some, but not all" half of a select-all checkbox. */
  minus: make(<path d="M5 12h14" />),
  search: make(
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </>,
  ),
  refresh: make(
    <>
      <path d="M20 12a8 8 0 1 1-2.3-5.7" />
      <path d="M20 4v5h-5" />
    </>,
  ),
  apply: make(
    <>
      <path d="M12 3v13" />
      <path d="m7 11 5 5 5-5" />
      <path d="M4 21h16" />
    </>,
  ),
  undo: make(
    <>
      <path d="M4 8v5h5" />
      <path d="M20 16a8 8 0 0 0-14-5.3L4 13" />
    </>,
  ),
  preview: make(
    <>
      <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6Z" />
      <circle cx="12" cy="12" r="2.5" />
    </>,
  ),
  folder: make(
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" />,
  ),
  trash: make(
    <>
      <path d="M4 7h16" />
      <path d="M10 4h4" />
      <path d="M6 7v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7" />
      <path d="M10 11v6M14 11v6" />
    </>,
  ),
  grip: make(
    <>
      <path d="M9 6h.01M9 12h.01M9 18h.01" />
      <path d="M15 6h.01M15 12h.01M15 18h.01" />
    </>,
  ),

  /* state */
  info: make(
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5" />
      <path d="M12 8h.01" />
    </>,
  ),
  warning: make(
    <>
      <path d="M12 4 3 19h18L12 4Z" />
      <path d="M12 10v4" />
      <path d="M12 17h.01" />
    </>,
  ),
  sun: make(
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </>,
  ),
  moon: make(<path d="M20 13A8 8 0 1 1 11 4a6.5 6.5 0 0 0 9 9Z" />),
  chevronRight: make(<path d="m9 5 7 7-7 7" />),
  chevronDown: make(<path d="m5 9 7 7 7-7" />),
  close: make(
    <>
      <path d="M6 6l12 12" />
      <path d="M18 6 6 18" />
    </>,
  ),
  minimize: make(<path d="M5 12h14" />),
  maximize: make(<rect x="5" y="5" width="14" height="14" rx="1" />),
  restore: make(
    <>
      <rect x="4" y="8" width="12" height="12" rx="1" />
      <path d="M8 8V5a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1h-3" />
    </>,
  ),
  package: make(
    <>
      <path d="M4 8v9l8 4 8-4V8l-8-4-8 4Z" />
      <path d="m4 8 8 4 8-4" />
      <path d="M12 12v9" />
    </>,
  ),
};

export type IconName = keyof typeof Icon;

/**
 * The Apocrypha mark.
 *
 * Three bars in the arrangement of an impossible triangle: a form that reads as
 * a whole but cannot actually exist. A modded game is the same thing, no longer
 * the object that shipped, and "apocrypha" means writings set aside from the
 * accepted canon.
 *
 * Unlike the icons above it is solid rather than monoline, because a brand mark
 * and a toolbar icon do different jobs. It is one colour, inherits from context,
 * and is centred on the 32 grid to three decimal places with equal margins and
 * three identical 1.58 unit gaps. Those numbers are why it reads evenly at 16px.
 */
const MARK = [
  "M7.32 27.16 25.18 27.16 28.28 21.80 10.42 21.80Z",
  "M12.13 5.13 3.20 20.60 6.29 25.96 15.22 10.49Z",
  "M28.80 20.31 19.87 4.84 13.68 4.84 22.61 20.31Z",
];

export function Logo({ size = 32 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
    >
      {MARK.map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  );
}

/**
 * The mark stacked above the name, for the splash and anywhere the brand leads
 * rather than labels. The word is letterspaced and set lighter so the mark
 * carries the weight.
 */
export function Lockup({ size = 64 }: { size?: number }) {
  return (
    <span className="lockup" style={{ ["--lockup-size" as string]: `${size}px` }}>
      <Logo size={size} />
      <span className="lockup-word">Apocrypha</span>
    </span>
  );
}
