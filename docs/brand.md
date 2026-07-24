# The mark

![The Apocrypha mark](images/logo-white.png#gh-dark-mode-only)
![The Apocrypha mark](images/logo-black.png#gh-light-mode-only)

Three bars arranged as an impossible triangle.

## Why this shape

A modded game is not the game that shipped. It looks whole, it plays like the
real thing, but it is an object that the people who built it never made. An
impossible triangle is the same claim in geometry: a form that reads as solid
and cannot exist. The name says the same thing again, since apocrypha are
writings set aside from the accepted canon.

It is one shape, so it works alone the way a mark should. No letter, no
wordmark, nothing to explain.

## Rules

- **One colour.** Black or white, inherited from whatever is behind it. Never
  the accent green, never two tones, never a gradient.
- **Solid, not outlined.** The interface icons are monoline; the mark is not.
  A brand mark and a toolbar icon do different jobs, which is the same split
  Apple keeps between its logo and SF Symbols.
- **Never redrawn.** Scale it, do not rebuild it. The geometry is exact and
  easy to break.
- **Clear space** of at least a quarter of the mark's height on every side.
- **Minimum size** 16px. It holds there, which is why this shape was chosen
  over the finer drawn version of the same idea.

## Geometry

One parallelogram, rotated 120 and 240 degrees about the centre of a 32 unit
grid. Every number below was verified rather than eyeballed, and an earlier
draft that looked correct in preview failed all of them.

| Property | Value |
| --- | --- |
| Grid | 32 x 32 |
| Centre | 16.000, 16.000 |
| Margins | 3.20 left and right, 4.84 top and bottom |
| Gaps between bars | 1.58, all three identical |
| Bar end angle | 60 degrees |
| Rotation | exactly 120 degrees apart |

## Files

| File | Use |
| --- | --- |
| `docs/images/logo.svg` | Source. Single colour, `currentColor`. |
| `docs/images/logo-white.png` | White on transparent, 512px. |
| `docs/images/logo-black.png` | Black on transparent, 512px. |
| `docs/images/logo-1024.png` | White on near black, 1024px. For submissions that ask for a logo legible on dark. |
| `apps/desktop/src-tauri/icons/` | Packaged application icons, 32 to 512. |

In the application the mark lives in `apps/desktop/src/components/icons.tsx` as
`Logo`, with `Lockup` stacking it above the name for the splash and anywhere
the brand leads rather than labels.

## Known resemblance

Three bars forming a triangle is not far from the Google Drive mark. In one
colour the resemblance is much weaker and the bar angles differ, but it is worth
knowing before someone else points it out.
