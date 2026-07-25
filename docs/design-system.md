# Apocrypha Design System

The visual and interaction language of the Apocrypha desktop app: a native, Linux-first
mod manager for games (first target: Monster Hunter Wilds).

**Stack:** Tauri v2 + React 18 + TypeScript + Vite. Plain CSS with custom properties.
No Tailwind, no component library, no icon package. framer-motion for animation only.

**Source of truth:**

```
apps/desktop/src/styles/theme.css     tokens, reset, ambient layer
apps/desktop/src/styles/app.css       every component class
apps/desktop/src/components/icons.tsx the whole icon set and the logo
apps/desktop/src/components/ui.tsx    Switch, Segmented, Chip, Spinner, toasts, pageMotion
apps/desktop/src/lib/theme.tsx        dark / light / system resolution
```

If this document and those files disagree, the files win. Update this document in the
same commit.

Two audiences are served here. A contributor building new UI reads sections 1 to 9. A
designer or an AI design assistant producing new screens reads sections 1 to 7 plus
section 10, which explains exactly what to paste and what to ask for.

---

## Table of contents

1. [Design principles](#1-design-principles)
2. [Token reference](#2-token-reference)
3. [Colour palette and alternative palettes](#3-colour-palette-and-alternative-palettes)
4. [Layout and grid](#4-layout-and-grid)
5. [Iconography and logo](#5-iconography-and-logo)
6. [Component inventory](#6-component-inventory)
7. [Motion](#7-motion)
8. [Accessibility](#8-accessibility)
9. [How to extend](#9-how-to-extend)
10. [Using this document with an AI design tool](#10-using-this-document-with-an-ai-design-tool)
11. [Appendix: paste-ready token block](#11-appendix-paste-ready-token-block)

---

## 1. Design principles

The reference is Apple's desktop design language: macOS System Settings, Finder,
and the Human Interface Guidelines. In practice that means deference (quiet chrome,
loud content), inset grouped sections rather than loose controls, hierarchy from
typography rather than borders, one accent used sparingly, standard controls, and
short physical motion. Apocrypha departs from it in four deliberate ways: it is dark
first, it is denser because mod lists are long, it draws its own window chrome
because Linux desktops disagree about titlebars, and its accent is green.

Five rules. Everything else in this document is a consequence of one of them.

### 1.1 Content over chrome

The interesting thing on screen is always the user's data: their games, their mods,
their file lists, their conflicts. Chrome recedes. Surfaces are near-black or near-white,
borders are one pixel, elevation comes from a hairline plus a soft shadow. There are no
button glows anywhere in the app. There is no decorative gradient except the single
accent gradient, and that only appears on things the user is about to press or is
waiting on.

Practical test: if you remove a visual element and nothing about the data becomes harder
to read, the element should not exist.

### 1.2 Reversibility is visible

The engine underneath is journaled and hash-guarded: originals are copied into a
content-addressed vault before anything is replaced, every operation is flushed to an
append-only journal as it happens, and rollback refuses to delete a file whose bytes
changed since deploy. The UI must make that safety legible, not hide it.

That is why the dry run is a first-class screen, not a modal warning. It is why the
progress dialog names its phases in plain language ("Saving a record so this can be
undone"). It is why the deploy bar always carries a state dot and a sentence about
whether what is on disk matches what is on screen. Destructive-looking actions are shown
next to their undo.

### 1.3 Density without noise

This is a desktop tool for people with a hundred mods. Rows are compact, numbers are
tabular, the mod list groups into collapsible categories and supports search, filter and
sort. Density is achieved by removing separators, not by adding them: grouping comes from
a shared surface and a hairline, not from stripes, shadows or alternating rows.

Practical test: at 1280 x 820 the mods screen should show at least eight mod rows without
scrolling, and no two adjacent elements should be separated by more than one visual
device.

### 1.4 One accent

There is exactly one accent hue in the product, a deep desaturated green, deliberately
not neon. It marks the primary action, the current selection, and the focus ring. Nothing
else is allowed to be that colour. Semantic colours (success, warning, danger, info) are
reserved for state and never used decoratively.

Practical test: count the accent-coloured pixels on a screen. If there is more than one
"most important thing", the screen is wrong.

### 1.5 Motion that explains

Animation exists to answer a question the user would otherwise have to ask: where did
that come from, what is still selected, is anything happening. The nav pill and the
segmented pill slide between positions with `layoutId` so identity is preserved. Steps
in the wizard slide horizontally in the direction of travel. Toasts fly in from the edge
they live on. Nothing spins or pulses for decoration, apart from the one indeterminate
spinner and the skeleton shimmer, which both mean "waiting". Everything is switched off
under `prefers-reduced-motion`.

---

## 2. Token reference

Every value in the app comes from a custom property. Components never hard-code a colour,
size, radius or duration. This is what lets Settings > Appearance retheme the entire
application at runtime by overriding a handful of variables on `:root`.

### 2.1 Colour: dark theme

Applied on `:root` and `:root[data-theme="dark"]`. `color-scheme: dark`.

| Variable | Value | Hex | Role |
| --- | --- | --- | --- |
| `--bg-sunken` | `hsl(190 14% 3%)` | `#070809` | Lowest layer: titlebar, rail, group headers, inputs, dialog footers. Reads as "carved in". |
| `--bg-base` | `hsl(190 12% 5%)` | `#0B0E0E` | Window background, scroll area, wizard body. |
| `--bg-surface` | `hsl(190 12% 8%)` | `#121617` | Cards, stats, mod groups, option sets, deploy bar, dialogs. |
| `--bg-raised` | `hsl(190 11% 11%)` | `#191E1F` | Things that sit on a surface: mod rows, option cards, default buttons, segmented pill. |
| `--bg-overlay` | `hsl(190 11% 13%)` | `#1E2425` | Floating elements over content: toasts. |
| `--bg-hover` | `hsl(190 11% 15%)` | `#22292A` | Hover state for any interactive surface. |
| `--bg-active` | `hsl(190 11% 18%)` | `#293133` | Pressed state, nav pill, neutral chip fill, progress track, switch track (off). |
| `--border` | `hsl(190 10% 17%)` | `#272E30` | Default 1px hairline on every container. |
| `--border-strong` | `hsl(190 10% 26%)` | `#3C4749` | Hover borders, dialog and wizard edges, unchecked marks, scrollbar thumb. |
| `--text-primary` | `hsl(160 14% 94%)` | `#EEF2F0` | Body text, headings, values. 17.2:1 on `--bg-base`. |
| `--text-secondary` | `hsl(165 8% 68%)` | `#A7B4B1` | Supporting text, inactive nav labels, ghost buttons. 9.1:1. |
| `--text-tertiary` | `hsl(165 7% 48%)` | `#72837F` | Metadata, placeholders, counts, labels, disabled icons. 4.9:1. |
| `--accent` | `hsl(var(--accent-h) var(--accent-s) 48%)` | `#4CA987` | The one accent. Focus ring, active step text, logo mark, selected mark fill, spinner head. 6.8:1. |
| `--accent-hover` | `hsl(var(--accent-h) var(--accent-s) 55%)` | `#61B898` | Primary button hover. |
| `--accent-muted` | `hsl(var(--accent-h) 28% 13%)` | `#182A24` | Tinted accent background: accent chip, active wizard step, info notice, text selection. |
| `--accent-contrast` | `hsl(160 30% 96%)` | `#F2F8F6` | Text and glyphs drawn on top of the accent gradient. |
| `--accent-border` | `hsl(var(--accent-h) 26% 28%)` | `#355A4C` | Border on selected or focused containers (option cards, inputs, dragging rows). |
| `--accent-gradient` | `linear-gradient(135deg, hsl(H S 44%), hsl(H2 S-4 38%))` | `#469B7C` to `#408254` | Primary buttons, switch (on), progress fill, active step index, selected marks, splash bar. |
| `--success` | `hsl(152 58% 55%)` | `#4ACF91` | Confirmed good state: applied, detected, completed phase. |
| `--success-bg` | `hsl(152 40% 12%)` | `#122B1F` | Fill behind success text. 7.6:1 pairing. |
| `--warning` | `hsl(38 82% 60%)` | `#EDAF45` | Pending, not applied, not registered, unsaved changes. |
| `--warning-bg` | `hsl(38 45% 12%)` | `#2C2211` | Fill behind warning text. 8.1:1 pairing. |
| `--danger` | `hsl(2 70% 62%)` | `#E25F5A` | Failure, destructive action, close button hover. |
| `--danger-bg` | `hsl(2 42% 13%)` | `#2F1413` | Fill behind danger text. 4.9:1 pairing. |
| `--info` | `hsl(198 70% 60%)` | `#52B6E0` | Neutral informational accent. Rarely used; prefer `--accent`. |
| `--shadow-sm` | `0 1px 2px hsl(190 30% 1% / .5)` | | Barely-there lift. |
| `--shadow` | `0 2px 8px hsl(190 30% 1% / .5)` | | Standard lift. |
| `--shadow-lg` | `0 8px 32px hsl(190 30% 1% / .6)` | | Modals: wizard, dialog, toast. |
| `--scrim` | `hsl(190 30% 2% / .72)` | | Backdrop behind modals and the lightbox. |
| `--glow-a` | `hsl(var(--accent-h) 50% 30% / .16)` | | Top-left lobe of the ambient wash. |
| `--glow-b` | `hsl(190 40% 30% / .1)` | | Bottom-right lobe of the ambient wash. |

### 2.2 Colour: light theme

Applied on `:root[data-theme="light"]`. `color-scheme: light`. The ramp does not simply
invert: in light mode `--bg-surface`, `--bg-raised` and `--bg-overlay` are all pure white
and separation comes from borders, while `--bg-sunken` is the tinted one.

| Variable | Value | Hex | Role |
| --- | --- | --- | --- |
| `--bg-base` | `hsl(0 0% 100%)` | `#FFFFFF` | Window background. |
| `--bg-sunken` | `hsl(160 12% 97%)` | `#F6F8F8` | Titlebar, rail, group headers, inputs, dialog footers. |
| `--bg-surface` | `hsl(0 0% 100%)` | `#FFFFFF` | Cards, dialogs, toolbars. |
| `--bg-raised` | `hsl(0 0% 100%)` | `#FFFFFF` | Rows, option cards, default buttons. |
| `--bg-overlay` | `hsl(0 0% 100%)` | `#FFFFFF` | Toasts. |
| `--bg-hover` | `hsl(160 14% 96%)` | `#F3F6F5` | Hover. |
| `--bg-active` | `hsl(160 14% 93%)` | `#EBF0EE` | Pressed, nav pill, neutral chip, progress track. |
| `--border` | `hsl(160 12% 90%)` | `#E2E9E7` | Default hairline. Carries most of the structure in light mode. |
| `--border-strong` | `hsl(160 10% 78%)` | `#C1CDC9` | Hover borders, modal edges, unchecked marks. |
| `--text-primary` | `hsl(190 20% 8%)` | `#101718` | Body text. 18.1:1 on white. |
| `--text-secondary` | `hsl(185 8% 36%)` | `#546263` | Supporting text. 6.4:1. |
| `--text-tertiary` | `hsl(185 7% 52%)` | `#7C8C8D` | Metadata and placeholders. 3.5:1, so it is used only at 12px to 13px for non-essential text. |
| `--accent` | `hsl(var(--accent-h) 46% 30%)` | `#297056` | The accent, darkened for a white background. 5.9:1. |
| `--accent-hover` | `hsl(var(--accent-h) 46% 24%)` | `#215945` | Primary button hover. |
| `--accent-muted` | `hsl(var(--accent-h) 34% 95%)` | `#EEF7F3` | Tinted accent background. |
| `--accent-contrast` | `hsl(0 0% 100%)` | `#FFFFFF` | Text on the accent gradient. 5.9:1 to 7.7:1. |
| `--accent-border` | `hsl(var(--accent-h) 26% 78%)` | `#B8D5CB` | Selected borders. |
| `--accent-gradient` | `linear-gradient(135deg, hsl(H 46% 30%), hsl(H2 42% 26%))` | `#297056` to `#265E37` | Same uses as dark. |
| `--success` | `hsl(152 60% 28%)` | `#1D724A` | 5.3:1 on `--success-bg`. |
| `--success-bg` | `hsl(152 50% 94%)` | `#E8F7F0` | |
| `--warning` | `hsl(32 80% 36%)` | `#A56112` | 4.4:1 on `--warning-bg`. |
| `--warning-bg` | `hsl(38 86% 94%)` | `#FDF3E3` | |
| `--danger` | `hsl(2 66% 44%)` | `#BA2B26` | 5.4:1 on `--danger-bg`. |
| `--danger-bg` | `hsl(2 78% 96%)` | `#FDEDED` | |
| `--info` | `hsl(200 72% 38%)` | `#1B78A7` | |
| `--shadow-sm` | `0 1px 2px hsl(190 16% 40% / .08)` | | |
| `--shadow` | `0 2px 8px hsl(190 16% 40% / .08)` | | |
| `--shadow-lg` | `0 8px 32px hsl(190 16% 30% / .14)` | | |
| `--scrim` | `hsl(190 16% 20% / .32)` | | Lighter scrim; the blur does the separating. |
| `--glow-a` | `hsl(var(--accent-h) 60% 60% / .1)` | | |
| `--glow-b` | `hsl(200 50% 60% / .07)` | | |

### 2.3 Accent inputs

Three variables generate every accent value in both themes. These are the ones the
Appearance panel writes, and the ones to change first when reskinning.

| Variable | Value | Purpose |
| --- | --- | --- |
| `--accent-h` | `158` | Accent hue. Drives `--accent`, `--accent-hover`, `--accent-muted`, `--accent-border`, gradient stop 1, `--glow-a`. |
| `--accent-s` | `38%` | Accent saturation in dark mode. Deliberately below 45% so the accent never reads as neon. |
| `--accent-h2` | `138` | Hue of the second gradient stop. Offset from `--accent-h` by roughly 20 degrees to give accent surfaces depth. |

### 2.4 Typography

SF Pro Display is loaded through `@font-face` with `local()` from the user's installed
fonts, falling back to Inter and then system UI. Nothing is redistributed with the app.
Only three upright faces exist (Regular, Medium, Bold), so the app uses **only weights
400, 500 to 600, and 700**. Never specify 300, 800 or 900: they will synthesise badly.

| Variable | Value | Purpose |
| --- | --- | --- |
| `--font-display` | `"Apocrypha Display", "SF Pro Display", -apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", system-ui, sans-serif` | Everything. |
| `--font-mono` | `"SF Mono", "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace` | Paths, file lists, hashes, commands. Applied via `.mono` at `0.92em`, `letter-spacing: -0.01em`. |
| `--text-base` | `14px` | Body size on `<body>`. The single knob Appearance scales from. |
| `--text-xs` | `12px` | Chips, metadata, counts, option descriptions, file lists, uppercase labels. |
| `--text-sm` | `13px` | Secondary UI: titlebar title, hints, selects, small buttons, phases, toasts, steps. |
| `--text-md` | `14px` | Default body, buttons, nav items, card titles, mod names, inputs. |
| `--text-lg` | `16px` | Rail brand, dialog title, wizard title, game name. |
| `--text-xl` | `20px` | Screen title in the topbar, stat values, splash wordmark. |
| `--text-2xl` | `24px` | Reserved for a future hero. Not currently used. |
| `--text-3xl` | `32px` | Reserved. Not currently used. |
| `--leading-tight` | `1.25` | Headings and any two-line stack of labels. |
| `--leading` | `1.5` | Body text, descriptions, notices. |

Weight and tracking are paired to size, always:

| Role | Size | Weight | Letter-spacing | Notes |
| --- | --- | --- | --- | --- |
| Screen title (`.topbar h1`) | 20px | 600 | `-0.02em` | `--leading-tight`. |
| Stat value | 20px | 600 | `-0.02em` | `font-variant-numeric: tabular-nums`. |
| Dialog / wizard title | 16px | 600 | `-0.02em` | |
| Rail brand | 16px | 600 | `-0.02em` | |
| Card title, mod name, option name | 14px / 13px | 600 | `-0.01em` | |
| Body, nav item, button | 14px | 400 / 500 | 0 | Buttons are 500, primary buttons 600. |
| Section label (`.stat-label`, `.option-set-title`) | 12px | 600 | `+0.06em`, uppercase | The only uppercase in the app apart from the splash. |
| Splash wordmark | 20px | 600 | `+0.08em`, uppercase | |
| Metadata | 12px to 13px | 400 | 0 | `--text-tertiary`. |

Rules: negative tracking tightens as size grows, positive tracking only ever appears with
uppercase, and any number a user might compare against another number gets
`font-variant-numeric: tabular-nums`.

### 2.5 Spacing

Powers of two. There is no 12, no 20, no 24. If a gap feels wrong, the fix is a different
step, not a new value.

| Variable | Value | Typical use |
| --- | --- | --- |
| `--sp-1` | `2px` | Hairline offsets, switch thumb inset, segmented inner padding, chip vertical padding, step list gap. |
| `--sp-2` | `4px` | Tight gaps: label to value, rail item gaps, group body gaps, small button vertical padding. |
| `--sp-3` | `8px` | The workhorse. Icon to label, button gaps, row gaps, card internals, list gaps. |
| `--sp-4` | `16px` | Component padding: cards, mod rows, option cards, dialogs, stack gaps. |
| `--sp-5` | `32px` | Page padding: content scroll area, topbar horizontal, wizard body, modal insets. |
| `--sp-6` | `64px` | Empty-state vertical padding. Large breathing room. |
| `--sp-7` | `128px` | Reserved for a future marketing or onboarding surface. |

### 2.6 Radius

| Variable | Value | Applies to |
| --- | --- | --- |
| `--radius-xs` | `4px` | Small interactive squares: window buttons, wizard steps, step index, drag handle, checkbox mark, thumbnails, skeletons, segmented inner buttons, focus outline. |
| `--radius-sm` / `--radius` | `8px` | Default. Buttons, inputs, selects, mod rows, option cards, stats, toasts, notices, file lists, dialog sub-elements. |
| `--radius-lg` / `--radius-xl` | `16px` | Containers: cards, mod groups, option sets, dialogs, the wizard, the preview hero. |
| `--radius-pill` | `999px` | Fully round: chips, switches, dots, progress bars, badges, swatches, scrollbar thumb, lightbox caption. |

Nesting rule: a child's radius is one step below its parent's. A 16px card contains 8px
rows which contain 4px thumbnails. Never nest equal radii.

### 2.7 Motion

| Variable | Value | Purpose |
| --- | --- | --- |
| `--ease-out` | `cubic-bezier(0.16, 1, 0.3, 1)` | The house curve. Fast start, long settle. Used for essentially every transition and every framer-motion tween. |
| `--ease-in-out` | `cubic-bezier(0.65, 0, 0.35, 1)` | Symmetric moves where nothing is entering or leaving. Rare. |
| `--dur-fast` | `120ms` | Hover, press, colour and border changes. |
| `--dur` | `200ms` | State changes with a position or size component: chevron rotation, group expand, page change, theme switch. |
| `--dur-slow` | `360ms` | Full-screen changes: splash fade out. |

### 2.8 Layout

| Variable | Value | Purpose |
| --- | --- | --- |
| `--titlebar-h` | `40px` | Height of the custom window chrome row. |
| `--rail-w` | `224px` | Fixed width of the navigation rail. |

Non-tokenised layout constants worth knowing (they are literal in `app.css` because they
are structural, not thematic):

| Constant | Value | Where |
| --- | --- | --- |
| Wizard size | `min(1120px, 100%)` x `min(760px, 100%)` | `.wizard` |
| Wizard step column | `240px` | `.wizard` grid column 1 |
| Dialog width | `min(480px, 100%)` | `.dialog` |
| Stat column min | `144px` | `.stat-grid` auto-fit |
| Option column min | `224px` | `.option-grid` auto-fill |
| Search min width | `200px` | `.search` |
| Key column in `.kv` | `176px` | definition lists |
| Thumbnail height | `128px` | `.option-thumb` |
| Hero and file list cap | `256px` | `.preview-hero`, `.file-list` |
| Window default | `1280 x 820`, min `940 x 620` | `tauri.conf.json` |

### 2.9 Elevation and the ambient layer

Elevation is expressed by three things in this order: background step, hairline border,
then shadow. Shadows are only used on things that float over content (wizard, dialog,
toast). Cards and rows get no shadow at all.

Behind everything sits `.ambient`, a `position: fixed`, `pointer-events: none`,
`z-index: 0` layer painting two very soft radial gradients:

```css
radial-gradient(60% 50% at 12% 0%,   var(--glow-a) 0%, transparent 70%),
radial-gradient(50% 45% at 100% 100%, var(--glow-b) 0%, transparent 72%)
```

Top-left lobe carries the accent hue, bottom-right lobe a cool neutral. Both are under
16% alpha. It should read as atmosphere, never as a shape. `--ambient-opacity` can dim it.

Stacking order used across the app:

| z-index | Layer |
| --- | --- |
| 0 | `.ambient` |
| 1 | `.app-body` (rail plus content) |
| 2 | `.titlebar`, dragging mod row |
| 50 | `.overlay` (wizard, apply dialog) |
| 80 | `.lightbox` |
| 100 | `.toast-wrap`, `.splash` |

---

## 3. Colour palette and alternative palettes

### 3.1 Rebuilding the palette elsewhere

Everything below is derived, not arbitrary. If you are rebuilding this in Figma, a
Penpot library, or another codebase, these are the literal values.

**Dark ramp** (background hue 190, saturation 10% to 14%, lightness climbing 3, 5, 8, 11,
13, 15, 18):

```
sunken   hsl(190 14% 3%)    #070809
base     hsl(190 12% 5%)    #0B0E0E
surface  hsl(190 12% 8%)    #121617
raised   hsl(190 11% 11%)   #191E1F
overlay  hsl(190 11% 13%)   #1E2425
hover    hsl(190 11% 15%)   #22292A
active   hsl(190 11% 18%)   #293133
border   hsl(190 10% 17%)   #272E30
strong   hsl(190 10% 26%)   #3C4749
```

**Dark text** (hue drifts warm-green so it never looks blue against the cool ramp):

```
primary    hsl(160 14% 94%)  #EEF2F0
secondary  hsl(165 8% 68%)   #A7B4B1
tertiary   hsl(165 7% 48%)   #72837F
```

**Dark accent** (hue 158, saturation 38%):

```
accent          hsl(158 38% 48%)  #4CA987
accent-hover    hsl(158 38% 55%)  #61B898
accent-muted    hsl(158 28% 13%)  #182A24
accent-contrast hsl(160 30% 96%)  #F2F8F6
accent-border   hsl(158 26% 28%)  #355A4C
gradient        135deg #469B7C -> #408254
```

**Dark semantics:**

```
success #4ACF91 on #122B1F
warning #EDAF45 on #2C2211
danger  #E25F5A on #2F1413
info    #52B6E0
```

**Light ramp:**

```
base/surface/raised/overlay  #FFFFFF
sunken   hsl(160 12% 97%)  #F6F8F8
hover    hsl(160 14% 96%)  #F3F6F5
active   hsl(160 14% 93%)  #EBF0EE
border   hsl(160 12% 90%)  #E2E9E7
strong   hsl(160 10% 78%)  #C1CDC9
```

**Light text and accent:**

```
primary    hsl(190 20% 8%)   #101718
secondary  hsl(185 8% 36%)   #546263
tertiary   hsl(185 7% 52%)   #7C8C8D
accent          hsl(158 46% 30%)  #297056
accent-hover    hsl(158 46% 24%)  #215945
accent-muted    hsl(158 34% 95%)  #EEF7F3
accent-border   hsl(158 26% 78%)  #B8D5CB
accent-contrast #FFFFFF
gradient        135deg #297056 -> #265E37
```

**Light semantics:**

```
success #1D724A on #E8F7F0
warning #A56112 on #FDF3E3
danger  #BA2B26 on #FDEDED
info    #1B78A7
```

### 3.2 How to build an alternative palette

Only four things need to change to reskin the app, and they change in this order:

1. **`--accent-h`** (hue, 0 to 360). The single largest decision.
2. **`--accent-s`** (saturation). Keep it between 30% and 46% in dark mode. Above 50% the
   accent starts to glow against the near-black ramp and violates principle 1.4.
3. **`--accent-h2`** (second gradient stop hue). Offset `--accent-h` by 15 to 30 degrees.
   Larger offsets read as a rainbow, smaller offsets read as a flat fill.
4. **The background ramp hue.** Move it to within roughly 30 degrees of `--accent-h`, or
   leave it neutral. The ramp keeps its lightness steps (3, 5, 8, 11, 13, 15, 18) and its
   low saturation (10% to 14% in dark). Never change the lightness steps: they are what
   makes elevation legible.

**The trap: lightness is not perceptual.** `hsl(158 38% 48%)` and `hsl(276 32% 48%)` have
the same L but the violet is far darker to the eye. If you change the hue and keep the
lightness, blues and violets will fail contrast and yellows will burn. So after changing
hue, retune lightness until the ratios below hold.

**Targets to hold, in both themes:**

| Pairing | Minimum | Comfortable |
| --- | --- | --- |
| `--text-primary` on `--bg-base` | 7:1 | 15:1 or better |
| `--text-secondary` on `--bg-base` | 4.5:1 | 6:1 |
| `--text-tertiary` on `--bg-surface` | 3:1 (12px to 13px non-essential text only) | 4.5:1 |
| `--accent` on `--bg-base` and `--bg-surface` | 4.5:1 | 6 to 7:1 |
| `--accent` on `--accent-muted` | 4.5:1 | 5:1 |
| `--accent-contrast` on both gradient stops | 3:1 | 4.5:1 |
| Each semantic colour on its own `-bg` | 4.5:1 | 5:1 |
| `--border` against its parent surface | visible at 100% zoom | |

**Known deviation to be aware of.** In the shipped dark theme, `--accent-contrast`
(`#F2F8F6`) on the lighter gradient stop (`#469B7C`) measures 3.13:1, and 4.30:1 on the
darker stop. That clears the 3:1 bar for UI components and large text but not the 4.5:1
bar for 14px body text. If you need strict AA on primary button labels, set the stops to
`hsl(H S 35%)` and `hsl(H2 S-4 37%)`, which measures 4.68:1 and 4.53:1. Every
alternative palette below lists both a "matched" gradient (same visual weight as today)
and a "strict" gradient (AA on the label).

**Do not change** the semantic colours when reskinning. Green success, amber warning and
red danger are conventions, and they stay legible whatever the accent is. The only case
for touching them is when the accent hue lands on top of one of them: if you pick a red
accent, shift `--danger` toward hue 355 and raise its saturation so the two do not
collide, and lean harder on the warning icon to carry meaning.

### 3.3 Worked alternative palettes

Each block is paste-ready. Drop it after the existing theme blocks in `theme.css`, or
paste it into the browser devtools on `:root` to preview. All values were measured, not
guessed.

#### Cobalt

Cold, technical, closest in feel to the default. Good if green reads too "eco".

```css
:root {
  --accent-h: 212;
  --accent-s: 42%;
  --accent-h2: 232;
}

:root[data-theme="dark"] {
  --bg-sunken:  hsl(215 14% 3%);   /* #070709 */
  --bg-base:    hsl(215 12% 5%);   /* #0B0C0E */
  --bg-surface: hsl(215 12% 8%);   /* #121417 */
  --bg-raised:  hsl(215 11% 11%);  /* #191C1F */
  --bg-overlay: hsl(215 11% 13%);  /* #1E2125 */
  --bg-hover:   hsl(215 11% 15%);  /* #22262A */
  --bg-active:  hsl(215 11% 18%);  /* #292D33 */
  --border:        hsl(215 10% 17%); /* #272B30 */
  --border-strong: hsl(215 10% 26%); /* #3C4149 */

  --accent:        hsl(212 42% 62%); /* #759BC7, 6.8:1 on base */
  --accent-hover:  hsl(212 42% 69%); /* #8FAED1 */
  --accent-muted:  hsl(212 28% 15%); /* accent on it: 5.6:1 */
  --accent-border: hsl(212 26% 28%); /* #35465A */
  /* matched weight */
  --accent-gradient: linear-gradient(135deg, hsl(212 42% 58%) 0%, hsl(232 38% 52%) 100%);
  /* strict AA on the label, swap in if you prefer:
  --accent-gradient: linear-gradient(135deg, hsl(212 42% 47%) 0%, hsl(232 38% 55%) 100%); */
  --glow-b: hsl(215 40% 30% / 0.1);
}

:root[data-theme="light"] {
  --accent:        hsl(212 46% 30%); /* #294A70, 9.1:1 on white */
  --accent-hover:  hsl(212 46% 24%);
  --accent-muted:  hsl(212 34% 95%); /* #EEF2F7 */
  --accent-border: hsl(212 26% 78%); /* #B8C6D5 */
  --accent-gradient: linear-gradient(135deg, hsl(212 46% 30%) 0%, hsl(232 42% 26%) 100%);
}
```

#### Ember

Warm copper. Bear in mind it sits nearer `--warning`, so keep warning chips iconed.

```css
:root {
  --accent-h: 24;
  --accent-s: 44%;
  --accent-h2: 8;
}

:root[data-theme="dark"] {
  --bg-sunken:  hsl(22 14% 3%);   /* #090707 */
  --bg-base:    hsl(22 12% 5%);   /* #0E0C0B */
  --bg-surface: hsl(22 12% 8%);   /* #171412 */
  --bg-raised:  hsl(22 11% 11%);  /* #1F1B19 */
  --bg-overlay: hsl(22 11% 13%);  /* #25201E */
  --bg-hover:   hsl(22 11% 15%);  /* #2A2522 */
  --bg-active:  hsl(22 11% 18%);  /* #332D29 */
  --border:        hsl(22 10% 17%); /* #302A27 */
  --border-strong: hsl(22 10% 26%); /* #49413C */

  --accent:        hsl(24 44% 59%); /* #C48D68, 6.8:1 on base */
  --accent-hover:  hsl(24 44% 66%); /* #CEA182 */
  --accent-muted:  hsl(24 28% 15%);
  --accent-border: hsl(24 26% 28%); /* #5A4435 */
  --accent-gradient: linear-gradient(135deg, hsl(24 44% 55%) 0%, hsl(8 40% 49%) 100%);
  /* strict: hsl(24 44% 43%) -> hsl(8 40% 49%) */
  --warning: hsl(48 84% 62%);       /* push warning yellow-ward so it stays distinct */
  --glow-b: hsl(22 40% 30% / 0.1);
}

:root[data-theme="light"] {
  --accent:        hsl(24 46% 30%); /* #704529, 8.2:1 on white */
  --accent-hover:  hsl(24 46% 24%);
  --accent-muted:  hsl(24 34% 95%); /* #F7F1EE */
  --accent-border: hsl(24 26% 78%); /* #D5C4B8 */
  --accent-gradient: linear-gradient(135deg, hsl(24 46% 30%) 0%, hsl(8 42% 26%) 100%);
}
```

#### Amethyst

Quieter and more "software". Note the lower saturation: violet at 40% reads synthetic.

```css
:root {
  --accent-h: 276;
  --accent-s: 32%;
  --accent-h2: 300;
}

:root[data-theme="dark"] {
  --bg-sunken:  hsl(268 14% 3%);   /* #080709 */
  --bg-base:    hsl(268 12% 5%);   /* #0D0B0E */
  --bg-surface: hsl(268 12% 8%);   /* #141217 */
  --bg-raised:  hsl(268 11% 11%);  /* #1C191F */
  --bg-overlay: hsl(268 11% 13%);  /* #211E25 */
  --bg-hover:   hsl(268 11% 15%);  /* #26222A */
  --bg-active:  hsl(268 11% 18%);  /* #2E2933 */
  --border:        hsl(268 10% 17%); /* #2B2730 */
  --border-strong: hsl(268 10% 26%); /* #423C49 */

  --accent:        hsl(276 32% 66%); /* #AE8DC4, 6.9:1 on base */
  --accent-hover:  hsl(276 32% 73%); /* #BFA4D0 */
  --accent-muted:  hsl(276 28% 17%); /* accent on it: 5.5:1 */
  --accent-border: hsl(276 26% 28%); /* #4B355A */
  --accent-gradient: linear-gradient(135deg, hsl(276 32% 62%) 0%, hsl(300 28% 56%) 100%);
  /* strict: hsl(276 32% 52%) -> hsl(300 28% 48%) */
  --glow-b: hsl(268 40% 30% / 0.1);
}

:root[data-theme="light"] {
  --accent:        hsl(276 46% 30%); /* #542970, 10.9:1 on white */
  --accent-hover:  hsl(276 46% 24%);
  --accent-muted:  hsl(276 34% 95%); /* #F3EEF7 */
  --accent-border: hsl(276 26% 78%); /* #CAB8D5 */
  --accent-gradient: linear-gradient(135deg, hsl(276 46% 30%) 0%, hsl(300 42% 26%) 100%);
}
```

#### Bone

Near-monochrome. The accent is a warm grey and the semantic colours become the only
chroma on screen, which makes state extremely loud. Good accessibility profile.

```css
:root {
  --accent-h: 200;
  --accent-s: 6%;
  --accent-h2: 200;
}

:root[data-theme="dark"] {
  --bg-sunken:  hsl(210 6% 3%);   /* #070808 */
  --bg-base:    hsl(210 4% 5%);   /* #0C0D0D */
  --bg-surface: hsl(210 4% 8%);   /* #141415 */
  --bg-raised:  hsl(210 3% 11%);  /* #1B1C1D */
  --bg-overlay: hsl(210 3% 13%);  /* #202122 */
  --bg-hover:   hsl(210 3% 15%);  /* #252627 */
  --bg-active:  hsl(210 3% 18%);  /* #2D2E2F */
  --border:        hsl(210 2% 17%); /* #2A2B2C */
  --border-strong: hsl(210 2% 26%); /* #414244 */

  --accent:        hsl(200 6% 60%); /* #939B9F, 6.9:1 on base */
  --accent-hover:  hsl(200 6% 67%); /* #A6ADB0 */
  --accent-muted:  hsl(200 10% 16%);
  --accent-border: hsl(200 8% 30%);
  --accent-gradient: linear-gradient(135deg, hsl(200 6% 56%) 0%, hsl(200 2% 50%) 100%);
  /* strict: hsl(200 6% 44%) -> hsl(200 2% 44%) */
  --glow-a: hsl(200 12% 40% / 0.1);
  --glow-b: hsl(200 10% 40% / 0.06);
}

:root[data-theme="light"] {
  --accent:        hsl(200 20% 26%);
  --accent-hover:  hsl(200 20% 20%);
  --accent-muted:  hsl(200 14% 95%);
  --accent-border: hsl(200 12% 78%);
  --accent-gradient: linear-gradient(135deg, hsl(200 20% 26%) 0%, hsl(200 14% 22%) 100%);
}
```

### 3.4 Checking a palette you invented

Run every pairing in the table in 3.2 through a contrast checker. A 20-line script is
enough: convert HSL to sRGB, linearise each channel
(`c <= 0.03928 ? c/12.92 : ((c+0.055)/1.055)^2.4`), take
`0.2126R + 0.7152G + 0.0722B`, then `(lighter + 0.05) / (darker + 0.05)`.

Then look at it in both themes with the Appearance toggle. Two failure modes are not
caught by numbers: an accent that vibrates against the background (saturation too high),
and a background ramp whose middle steps collapse into each other (saturation too low for
the hue, so `--bg-raised` and `--bg-overlay` stop being distinguishable).

---

## 4. Layout and grid

### 4.1 Window and shell

The OS decorations are replaced by custom chrome, so the window reads as an application
rather than a browser frame. Outermost structure:

```
.app                       grid-template-rows: var(--titlebar-h) 1fr; height 100%; isolation: isolate
├── .titlebar              40px, --bg-sunken, 1px bottom border, z-index 2
└── .app-body              grid-template-columns: auto 1fr; z-index 1
    ├── .rail              224px, --bg-sunken, 1px right border
    └── .content           flex column, min-width 0, min-height 0
        ├── .topbar        padding 16px 32px, 1px bottom border
        ├── .scroll        flex 1, overflow-y auto, padding 32px
        └── .deploybar     padding 8px 32px, 1px top border, --bg-surface
```

`.ambient` is fixed behind all of it at `z-index: 0`. `body` has `overflow: hidden`; the
only scroll container in the shell is `.scroll` (and `.wizard-body` inside the modal).
Every flex or grid child that can hold long text carries `min-width: 0` so truncation
works instead of overflow.

### 4.2 The power-of-two rhythm

Vertical rhythm is not a baseline grid, it is a nesting rule. Each level of containment
steps down one notch on the spacing scale:

```
page padding      32   (--sp-5)
between cards     16   (--sp-4)   .stack
card padding      16   (--sp-4)
inside a card     8    (--sp-3)   .stack.tight, .row
label to value    4    (--sp-2)
optical nudge     2    (--sp-1)
```

The same idea governs radius (16 -> 8 -> 4) and type (20 -> 16 -> 14 -> 13 -> 12). If a
new design needs a value between two steps, the containment is wrong, not the scale.

### 4.3 Content grids

Only three grid patterns exist. Reuse them rather than inventing a fourth.

| Pattern | Definition | Used by |
| --- | --- | --- |
| Stat grid | `repeat(auto-fit, minmax(144px, 1fr))`, gap 8 | `.stat-grid` |
| Option grid | `repeat(auto-fill, minmax(224px, 1fr))`, gap 8 | `.option-grid` |
| Key/value | `176px 1fr`, gap `8px 16px` | `.kv` |

`auto-fit` for stats (they stretch to fill, so four stats always span the width);
`auto-fill` for options (a lone option card should not become 900px wide).

### 4.4 Responsive behaviour

The app is a resizable desktop window, minimum 940 x 620. There are no media queries.
Responsiveness comes from four mechanisms:

1. **Intrinsic grids.** The stat and option grids reflow by themselves. Stats go 4 up,
   then 3, then 2. Options go 4 up down to 1.
2. **`flex-wrap` on the toolbar.** Search keeps `flex: 1` with a 200px floor; the three
   selects wrap onto a second line when the window narrows.
3. **`min()` on modals.** The wizard is `min(1120px, 100%)` by `min(760px, 100%)` inside
   a 32px-inset overlay, so it shrinks with the window and never clips.
4. **Truncation, not reflow, in rows.** Mod names use `.truncate`; chips and buttons have
   `white-space: nowrap` and never shrink. A narrow window loses characters from the mod
   name, never the switch or the Configure button.

The rail is fixed at 224px and does not collapse today. If a collapsed rail is added, it
should animate width only, keep the icons in place, and keep `--rail-w` as the knob.

---

## 5. Iconography and logo

### 5.1 Icon rules

The entire icon set lives in one file, `src/components/icons.tsx`, drawn inline. No icon
package. One file is what keeps the set visually consistent.

Every icon obeys all of these:

| Rule | Value |
| --- | --- |
| Grid | `viewBox="0 0 24 24"` |
| Stroke | `1.5`, overridable via a `strokeWidth` prop |
| Caps and joins | `round` / `round` |
| Fill | `none`, always. Outlines only. |
| Colour | `stroke="currentColor"`, single colour, never multi-tone |
| Default size | `16` (`size` prop) |
| A11y | `aria-hidden="true"`, `focusable="false"`; the label lives on the parent control |

Sizes actually used: 11 to 14px inside chips, marks and phase dots; 16px default; 18px in
the rail and for lead icons; 20px in dialog headers; 32 to 40px at `strokeWidth={1}` for
empty-state illustrations. Larger icons drop the stroke weight so they do not look heavy.

Current set, grouped as in the file:

```
navigation  library  mods  downloads  profiles  conflicts  settings
actions     plus  check  search  refresh  apply  undo  preview  folder  grip
state       info  warning  sun  moon  chevronRight  chevronDown  close
            minimize  maximize  restore  package
```

Drawing a new icon: build it from the same primitives already in the set (a 7x16
rectangle at x=3, a circle at r=7 or r=9, chevrons on a 7-unit run), keep every terminal
on a whole or half unit, and check it at 16px before committing. If it needs a fill or a
second colour to read, redraw it.

### 5.2 The logo

"Apocrypha" means hidden or set-aside writings, so the mark is a **sealed codex**: a
closed book with a spine fold and a single short seal line across the cover.

```
viewBox 0 0 32 32, stroke 1.75, round caps, no fill, currentColor
codex body  M7 5.5h13.5A3.5 3.5 0 0 1 24 9v17.5H10.5A3.5 3.5 0 0 1 7 23V5.5Z
spine fold  M7 23a3.5 3.5 0 0 1 3.5-3.5H24
seal        M14 11h4
```

Rules:

- One colour, always `currentColor`. It is painted `--accent` in the titlebar, the rail
  and the splash, and inherits text colour anywhere else.
- Never place it on the accent gradient, never emboss it, never add a glow.
- Sizes in use: 18px (titlebar), 32px (rail), 48px (splash).
- Below 16px, drop the seal line rather than shrinking the whole mark.
- The wordmark is the product name in the display face at weight 600. On the splash it is
  uppercase at `+0.08em`; everywhere else it is sentence case at `-0.02em`.

---

## 6. Component inventory

Every component is a CSS class in `app.css` plus, where it needs state, a small React
component. There are no styled-components, no CSS modules, no utility classes beyond
`.mono`, `.truncate`, `.visually-hidden`, `.stack`, `.row` and `.divider`.

For each entry below: **anatomy** is the DOM, **states** are every visual state that
exists, **spacing** is the literal token usage.

---

### 6.1 Titlebar

**Class:** `.titlebar`, `.titlebar-drag`, `.titlebar-title`, `.titlebar-sep`,
`.win-controls`, `.win-btn`, `.win-btn.close`
**Component:** `components/TitleBar.tsx`

**Anatomy.** A 40px row on `--bg-sunken` with a 1px bottom border. Left to right: the
logo at 18px in `--accent`, the product name at 13px / 500 in `--text-secondary`, an
optional `/` separator in `--text-tertiary` at 50% opacity, an optional subtitle (the
active game) that truncates. Everything left of the buttons is inside `.titlebar-drag`,
which carries `data-tauri-drag-region` and a double-click handler that toggles maximise.
On the right, three 32 x 28 buttons: minimise (14px), maximise or restore (13px), close
(14px).

**States.** Button rest: `--text-tertiary`, transparent. Hover: `--bg-hover` fill,
`--text-primary` glyph, `--radius-xs`, `--dur-fast`. Close hover is the exception:
`--danger` fill with a white glyph. The maximise button swaps to the restore glyph when
the window is maximised, tracked through the Tauri `onResized` event.

**Spacing.** Padding `0 --sp-2 0 --sp-4` (asymmetric: the buttons carry their own inset).
Gap `--sp-3` inside the drag region, `--sp-1` between window buttons. `user-select: none`.

**Note.** `decorations` is currently `true` in `tauri.conf.json`, so this bar coexists
with the OS frame during development. Set it to `false` to ship the intended chrome.

---

### 6.2 Rail navigation

**Class:** `.rail`, `.rail-brand`, `.rail-mark`, `.rail-title`, `.rail-sub`, `.nav-item`,
`.nav-item.active`, `.nav-pill`, `.nav-icon`, `.nav-badge`, `.rail-spacer`

**Anatomy.** A 224px column on `--bg-sunken` with a 1px right border. Top: `.rail-brand`,
a 32px accent-coloured logo next to a two-line stack (`.rail-title` 16px / 600 /
`-0.02em`, `.rail-sub` 12px `--text-tertiary`), both at `--leading-tight`. Then the nav
list: six items (Library, Mods, Downloads, Profiles, Changes, Settings), each an 18px icon
in a 16px box, a 14px / 500 label, and an optional right-aligned `.nav-badge` count. Then
`.rail-spacer` (`flex: 1`) pushes the theme toggle to the bottom, which is itself a
`.nav-item`.

**States.** Rest: `--text-secondary`, no fill. Hover: `--bg-hover`, `--text-primary`,
`--dur-fast`. Active: `--text-primary` plus `.nav-pill`, an absolutely positioned
`--bg-active` rectangle at `--radius-sm` behind the content, animated between items with
framer-motion `layoutId="nav-pill"` (spring, stiffness 520, damping 40). Content sits at
`z-index: 1` above the pill. `aria-current="page"` is set on the active item.

**Spacing.** Rail padding `--sp-4 --sp-3`, item gap `--sp-2` (2px between nav buttons in
practice), item padding `--sp-3` all round, icon-to-label gap `--sp-3`. Badge:
`--radius-pill`, 12px tabular, `--bg-active`, padding `0 --sp-3`, `line-height: 18px`.

**Rule.** The pill is the only element that animates position in the rail. Do not add a
left indicator bar, a colour change on the icon, or a bold weight on the active label:
one signal is enough.

---

### 6.3 Topbar

**Class:** `.topbar`, `.topbar h1`, `.topbar-sub`, `.topbar-actions`

**Anatomy.** A single row under the titlebar, inside the content column, with a 1px
bottom border and no fill (it sits on `--bg-base`). Left: an `h1` at 20px / 600 /
`-0.02em` naming the screen, and a `.topbar-sub` at 13px `--text-tertiary` naming the
active game and its detection state. Right, pushed by `margin-left: auto`: an optional
spinner while a background call is running, then a default button (Detect) and the
primary button (Add mod).

**States.** The whole bar has no hover state. Buttons carry their own. When `busy` is
true the action buttons disable and the spinner appears.

**Spacing.** Padding `--sp-4 --sp-5`. Gaps `--sp-3` throughout.

**Rule.** Exactly one primary button per screen, and it lives here or in the deploy bar,
never both.

---

### 6.4 Card

**Class:** `.card`, `.card-title`, `.card-hint`; variants `.game-card`, `.game-art`

**Anatomy.** `--bg-surface`, 1px `--border`, `--radius-lg`, padding `--sp-4`. Usually
composed with `.stack` so children space at `--sp-4`, or `.stack.tight` at `--sp-3`.
`.card-title` is 14px / 600 / `-0.01em`. `.card-hint` is 13px `--text-tertiary` at
`--leading`, used for the explanatory sentence under a title.

**States.** A plain card is not interactive and has no hover. `.game-card` is the
interactive variant: `display: flex`, gap `--sp-4`, a 48px `.game-art` tile
(`--radius-sm`, `--bg-sunken`, 1px border, centred glyph), then a text block, then a
trailing chevron. Hover raises the border to `--border-strong` and the fill to
`--bg-raised` over `--dur`, plus a framer-motion `whileHover={{ y: -2 }}` and
`whileTap={{ scale: 0.995 }}` spring. Selected sets `border-color: --accent-border`.

**Spacing.** Card padding `--sp-4`; never `--sp-5` (that is page padding) and never
`--sp-3` (that is inside-card spacing).

**Rule.** Cards do not nest inside cards. If you need a second level, use a `.divider`
and a `.card-title`, or promote the inner block to its own card in the same `.stack`.

---

### 6.5 Stat

**Class:** `.stat-grid`, `.stat`, `.stat-label`, `.stat-value`

**Anatomy.** A compact metric tile: `--bg-surface`, 1px border, `--radius-sm` (one step
below a card), padding `--sp-3 --sp-4`. Two lines: `.stat-label` at 12px / 600 uppercase
with `+0.06em` tracking in `--text-tertiary`, then `.stat-value` at 20px / 600 /
`-0.02em` with `tabular-nums`, offset by `margin-top: --sp-1`.

**States.** None. Stats are read-only. If a stat needs a state, it is a chip, not a stat.

**Spacing.** Grid `repeat(auto-fit, minmax(144px, 1fr))`, gap `--sp-3`. Used in rows of
four: Engine / Load order / Mods / Enabled on Library, Method / New files / Replaced /
Total size on the deployment screen.

---

### 6.6 Button

**Class:** `.btn`, `.btn.primary`, `.btn.danger`, `.btn.ghost`, `.btn.sm`, `.btn.icon`

**Anatomy.** `inline-flex`, centred, gap `--sp-3` between icon and label, padding
`--sp-3 --sp-4`, `--radius-sm`, 14px / 500, `line-height: 1`, `white-space: nowrap`, 1px
`--border`. Icons sit left of the label at 14 to 15px.

| Variant | Rest | Hover | Notes |
| --- | --- | --- | --- |
| default `.btn` | `--bg-raised`, `--border`, `--text-primary` | `--bg-hover`, `--border-strong` | Active: `--bg-active`. The workhorse. |
| `.primary` | `--accent-gradient`, transparent border, `--accent-contrast`, weight 600 | flat `--accent-hover` | One per screen. |
| `.danger` | transparent fill, `--danger` text, `--border` | `--danger-bg` fill, `--danger` border | Text stays `--danger` at rest so it is never mistaken for the primary. |
| `.ghost` | transparent fill and border, `--text-secondary` | `--bg-hover`, `--text-primary` | Cancel, dismiss, tertiary actions. |
| `.sm` | padding `--sp-2 --sp-3`, 13px | as parent variant | Inline actions inside rows and cards. |
| `.icon` | padding `--sp-3`, `aspect-ratio: 1` | as parent variant | Square, icon only. Requires `aria-label`. |

**States.** Disabled is `opacity: 0.4` plus `cursor: not-allowed`, and hover styles are
suppressed via `:not(:disabled)`. Focus uses the global `:focus-visible` ring. Transitions
are `--dur-fast` on background and border, linear on opacity.

**Rule.** `.primary` never carries a shadow or a glow. The gradient is the only signal it
needs.

---

### 6.7 Chip

**Class:** `.chip`, `.chip.ok`, `.chip.warn`, `.chip.bad`, `.chip.accent`, `.dot`
**Component:** `Chip` in `ui.tsx`

**Anatomy.** A pill label: `inline-flex`, gap `--sp-2`, padding `--sp-1 --sp-3`,
`--radius-pill`, 12px / 500, `line-height: 18px`, `white-space: nowrap`. Optionally leads
with a `.dot`, a 6px `currentColor` circle, when the chip describes a live state.

| Variant | Fill | Text | Meaning |
| --- | --- | --- | --- |
| default | `--bg-active` | `--text-secondary` | Neutral fact: version number, "Off". |
| `.ok` | `--success-bg` | `--success` | Confirmed: detected, in game, active, chosen. |
| `.warn` | `--warning-bg` | `--warning` | Attention: not applied, not registered, still in game. |
| `.bad` | `--danger-bg` | `--danger` | Failure. |
| `.accent` | `--accent-muted` | `--accent` | Classification, not state: installer model, "loader active". |

**States.** Chips are not interactive. They never receive hover, focus or click handlers.
If it needs to be clickable it is a `.btn.sm`.

**Rule.** A chip is at most three words. Longer text belongs in `.mod-meta` or a notice.

---

### 6.8 Switch

**Class:** `.switch`, `.switch[data-on="true"]`, `.switch-thumb`
**Component:** `Switch` in `ui.tsx`

**Anatomy.** A 36 x 20 `<button type="button" role="switch" aria-checked>` with
`--radius-pill`, a 1px `--border-strong`, and a 14 x 14 thumb inset 2px from the top and
left. It requires a `label` prop, which becomes `aria-label`.

**States.** Off: `--bg-active` track, `--border-strong` border, `--bg-surface` thumb. On:
`--accent-gradient` track, transparent border, and in dark mode the thumb switches to
`--accent-contrast`. Disabled: `opacity: 0.4`, `cursor: not-allowed`. The thumb animates
`x: 0 -> 16` with a framer-motion spring (stiffness 700, damping 40); the track colour
crossfades over `--dur`.

**Rule.** The switch means "this thing is on now". For a choice between two named things
use `.segmented`. For a selection inside a set use `.mark`.

---

### 6.9 Segmented control

**Class:** `.segmented`, `.segmented button`, `.segmented button.active`, `.seg-pill`
**Component:** `Segmented<T>` in `ui.tsx`

**Anatomy.** `inline-flex` on `--bg-sunken` with a 1px border, `--radius-sm`, padding
`--sp-1`, gap `--sp-1`. Each option is a `<button role="tab" aria-selected>` at 13px /
500, padding `--sp-2 --sp-4`, `--radius-xs`. The container has `role="tablist"`.

**States.** Inactive: `--text-secondary`. Active: `--text-primary` plus `.seg-pill`, an
absolutely positioned `--bg-raised` rectangle with a 1px border that slides between
options via a framer-motion `layoutId` built from the `idPrefix` prop (spring, stiffness
500, damping 40). That prop must be unique per control on screen, otherwise two
controls will animate into each other.

**Rule.** Two to four options, each one or two words. More than four is a `<select>`.

---

### 6.10 Search and select toolbar

**Class:** `.toolbar`, `.search`, `.search input`, `.search-icon`, `.select`

**Anatomy.** `.toolbar` is a `flex-wrap` row with gap `--sp-3`. `.search` is
`position: relative`, `flex: 1`, `min-width: 200px`, containing a full-width input with
asymmetric padding (`--sp-3 --sp-4 --sp-3 36px`) to clear the icon, and `.search-icon`, a
14px glyph absolutely positioned at `left: --sp-3` and vertically centred with
`translateY(-50%)`, in `--text-tertiary`, `pointer-events: none`. `.select` is a native
`<select>` at 13px, padding `--sp-3`, `--bg-sunken`, 1px border, `--radius-sm`.

**States.** Both use `--bg-sunken` at rest so inputs read as recessed against
`--bg-base`. Focus removes the default outline and sets `border-color: --accent-border`
over `--dur-fast`. Placeholder is `--text-tertiary`.

**Spacing.** In the mods screen the toolbar carries one search plus three selects (state,
category, sort). When the window narrows, the selects wrap to a second line while the
search keeps the first.

**Rule.** Native `<select>` is used deliberately: it gets the platform's keyboard
handling, typeahead and scroll behaviour for free. Every control needs an `aria-label`
because the toolbar has no visible labels.

---

### 6.11 Mod group (collapsible category)

**Class:** `.mod-group`, `.mod-group-head`, `.mod-group-name`, `.mod-group-count`,
`.mod-group-body`, `.chevron`, `.chevron.open`
**Component:** `ModsScreen.tsx`

**Anatomy.** A `<section>` with `--bg-surface`, 1px border, `--radius-lg` and
`overflow: hidden` (so the header's fill is clipped to the corner radius). The header is
a full-width `<button>` on `--bg-sunken` with a 1px bottom border, padding
`--sp-3 --sp-4`, containing a 14px chevron, the category name at 13px / 600 / `+0.01em`,
and a count at 12px `--text-tertiary` with tabular figures ("3 of 7 enabled"). The body
is a flex column of mod rows with gap `--sp-2` and padding `--sp-3`.

**States.** Header hover: `--bg-hover`. Open: the chevron rotates 90 degrees over `--dur`
with `--ease-out`; the body animates `height: 0 -> auto` and `opacity: 0 -> 1` over 200ms
with `overflow: hidden`. Collapsed state is per-category and held in a `Set` in component
state.

**Rule.** Categories come from mod metadata; anything without one lands in
"Uncategorised". Groups are always sorted alphabetically, so the list does not reshuffle
when a mod is toggled.

---

### 6.12 Mod row

**Class:** `.mod-row`, `.mod-row.disabled`, `.mod-row.dragging`, `.drag-handle`,
`.mod-name`, `.mod-meta`, `.mod-order`
**Component:** `ModRow` in `ModsScreen.tsx`

**Anatomy.** Left to right, gap `--sp-4`:

1. `.drag-handle` (grip icon, 14px) when reordering is possible, otherwise `.mod-order`, a
   right-aligned tabular priority number with `min-width: 24px`.
2. The `Switch`.
3. A flexible text block with `min-width: 0`: a row of `.mod-name` (14px / 600 /
   `-0.01em`, `.truncate`), an optional version `Chip`, and a status chip; then
   `.mod-meta` at 13px `--text-tertiary` with author, selected options out of total,
   file count and size, separated by middots.
4. A `.btn.sm` (Configure).

Container: `--bg-raised`, 1px `--border`, `--radius-sm`, padding `--sp-4`.

**Status chip logic:**

| Enabled | On disk | Chip |
| --- | --- | --- |
| yes | yes | `.ok` with a dot, "In game" |
| yes | no | `.warn`, "Not applied" |
| no | yes | `.warn`, "Still in game" |
| no | no | default, "Off" |

This is principle 1.2 in one component: the row states what is actually on disk, not just
what the user intends.

**States.** Hover raises the border to `--border-strong`. Disabled (mod off) drops
`.mod-name` and `.mod-meta` to 45% opacity while leaving the switch and button at full
strength. Dragging sets `--accent-border` plus `--bg-hover` and lifts to `z-index: 2`,
with `whileDrag={{ scale: 1.01 }}`. Enter and exit are springs (stiffness 420, damping 34)
with `layout` so neighbours slide rather than jump.

**Drag handle.** `cursor: grab`, `:active` becomes `grabbing`, `touch-action: none`,
padding `--sp-2` with `margin: calc(var(--sp-2) * -1)` so the hit target grows without
changing layout. Hover fills `--bg-hover` at `--radius-xs`. Reordering uses framer-motion
`Reorder.Group` / `Reorder.Item` with `dragListener={false}` and explicit `dragControls`,
so only the grip starts a drag. Dragging is disabled unless the list is in load order with
no search and no filters, and a `.card-hint` says so when it is unavailable.

---

### 6.13 Option card

**Class:** `.option`, `.option.selected`, `.option.locked`, `.option.has-thumb`,
`.mark`, `.mark.radio`, `.mark.check`, `.mark-inner`, `.option-name`, `.option-desc`,
`.option-meta`, `.option-thumb`, `.thumb-empty`, `.zoom-hint`
**Component:** `OptionCard` in `InstallWizard.tsx`, `OptionThumb` in `Preview.tsx`

**Anatomy.** A `<button>` with `--bg-raised`, 1px border, `--radius-sm`, padding
`--sp-4`, `display: flex`, gap `--sp-3`, `align-items: flex-start`. Contents: the mark,
then a text column with `.option-name` (13px / 600 / `-0.01em`), `.option-desc` (12px
`--text-secondary`, `white-space: pre-wrap`, `margin-top: --sp-2`), and `.option-meta`
(12px `--text-tertiary`, tabular, `margin-top: --sp-3`) reading "required" plus file count
plus size.

**Four roles**, derived from mod metadata rather than hardcoded:

| Role | Mark | Interaction | Visual |
| --- | --- | --- | --- |
| `forced` | `.mark.check`, pre-checked | `disabled`, `.locked`, `cursor: default` | Always selected; meta leads with "required". |
| `exclusive` (radio) | `.mark.radio` | `role="radio"`, one per radio set | Picking one clears its siblings in the same set only. |
| `stackable` (check) | `.mark.check` | `role="checkbox"`, independent | Layered over the chosen base variant. |
| `info` | none | not a button at all | Rendered as a `.notice` plus optional `.preview-hero`, never installed. |

**Mark.** 16 x 16, 1px `--border-strong`, centred content, `--accent-contrast` glyph
colour. `.radio` is `--radius-pill`, `.check` is `--radius-xs`. Selected sets
`border-color: --accent` and `background: --accent-gradient`. The radio fill is a 6px
`.mark-inner` dot springing in (stiffness 620, damping 28); the check is a 13px check icon
scaling in over 140ms.

**Thumbnail variant.** When the option has a preview image the card gets `.has-thumb`,
which flips it to `flex-direction: column` with padding `--sp-3`, and the mark moves to
`position: absolute` at `top/right: --sp-3` with a `--bg-surface` backing so it stays
readable over the image. `.option-thumb` is a full-width 128px box, `--radius-xs`,
`--bg-sunken`, 1px border, with `object-fit: contain` so images are letterboxed and
**never cropped**. A `.zoom-hint` pill fades in on hover. Clicking the thumbnail opens the
lightbox and stops the click from toggling the option. While loading, the box shows
`.skeleton.thumb-empty`.

**States.** Hover (not disabled): `--border-strong` plus `--bg-hover`. Selected:
`--accent-border` plus `--accent-muted` fill. Locked: no hover, no pointer.

---

### 6.14 Option set

**Class:** `.option-set`, `.option-set-title`, `.option-grid`

**Anatomy.** The container that groups option cards into one decision. `--bg-surface`,
1px border, `--radius-lg`, padding `--sp-4`. The title row is 12px / 600 uppercase with
`+0.06em` tracking in `--text-tertiary`, `display: flex`, gap `--sp-3`, and
`margin-bottom: --sp-4`. It typically holds a label, a lowercase non-uppercase suffix
explaining the rule ("select one", "always installed", "optional, layered over the choice
above"), and a right-aligned chip showing whether the set is resolved (`.ok` "chosen" or
`.warn` "not chosen"). The grid is `repeat(auto-fill, minmax(224px, 1fr))`, gap `--sp-3`.

**Ordering inside a wizard step**, which mirrors the order the deployment engine layers
files:

1. Notices (`info` options): cover art and warnings, full width.
2. Required (`forced`): always installed.
3. One block per radio set: the base "pick one" choices. Independent sets stay separate
   even when they share a group.
4. Add-ons (`stackable`): they override on top of whatever base was chosen.

**Rule.** One set is one decision. If a user cannot state in a sentence what the set is
asking, split it.

---

### 6.15 Wizard shell

**Class:** `.overlay`, `.wizard`, `.wizard-header`, `.wizard-steps`, `.step`,
`.step.active`, `.step-index`, `.step-check`, `.wizard-body`, `.wizard-footer`
**Component:** `InstallWizard.tsx`

**Anatomy.**

```
.overlay                fixed inset 0, --scrim, backdrop-filter blur(4px),
                        grid place-items center, padding --sp-5, z-index 50
└── .wizard             min(1120px,100%) x min(760px,100%)
                        grid-template-columns: 240px 1fr
                        grid-template-rows: auto 1fr auto
                        --bg-base, 1px --border-strong, --radius-lg, --shadow-lg
    ├── .wizard-header  spans both columns; --bg-surface, bottom border,
    │                   padding --sp-4 --sp-5
    ├── .wizard-steps   column 1; --bg-sunken, right border, padding --sp-3,
    │                   gap --sp-1, scrolls
    ├── .wizard-body    column 2; padding --sp-5, scrolls
    └── .wizard-footer  spans both columns; --bg-surface, top border,
                        padding --sp-4 --sp-5, gap --sp-3
```

Header: mod name at 16px / 650, version chip, installer-model accent chip, a `.card-hint`
with author and option count, and a right-aligned running total (selected bytes at weight
650 tabular, plus "N files selected" beneath).

Step list: each step is a full-width button, padding `--sp-3`, `--radius-xs`, 13px / 500,
with a 20 x 20 `.step-index` square (`--radius-xs`, `--bg-active`, `--text-tertiary`,
12px / 600), a truncating label, and a `.step-check` (success-coloured check, pushed right
with `margin-left: auto`) once every radio set in that step has a choice.

Footer: `.btn.ghost` Cancel on the left, a flexible spacer, an optional `.card-hint`
counting unresolved steps, then Back and either Next or the confirm button. The confirm
button carries the count ("Install, 412 files") and disables at zero files.

**States.** Step rest `--text-secondary`; hover `--bg-hover` plus `--text-primary`; active
`--accent-muted` fill with `--accent` text and the index square filled with
`--accent-gradient` in `--accent-contrast`. Body content crossfades and slides
horizontally on step change (`x: 14 -> 0 -> -14`, 200ms, `--ease-out`).

**Modal behaviour.** `role="dialog"`, `aria-modal="true"`, `aria-label` naming the mod.
Clicking the scrim cancels unless `busy`. The whole modal enters with a spring (stiffness
380, damping 34) from `scale: 0.97, y: 12`.

---

### 6.16 Dialog

**Class:** `.dialog`, `.dialog-head`, `.dialog-title`, `.dialog-body`, `.dialog-foot`
**Component:** `ApplyDialog.tsx`

**Anatomy.** `min(480px, 100%)`, `--bg-surface`, 1px `--border-strong`, `--radius-lg`,
`--shadow-lg`, `overflow: hidden`, inside the same `.overlay`. Head: padding
`--sp-4 --sp-4 0` with a status glyph (spinner, check, or warning, coloured `--accent`,
`--success` or `--danger`) beside a 16px / 600 / `-0.02em` title. Body: padding `--sp-4`,
flex column, gap `--sp-4`. Foot: padding `--sp-4`, `--bg-sunken`, 1px top border, gap
`--sp-3`, actions right-aligned with a flexible spacer.

**States.** The apply dialog has three: running (spinner, accent, phase list, no footer),
succeeded (check, success, summary hints, Done button), failed (warning, danger, a
`.notice` reading "Nothing was changed" plus the error, Done button). The footer only
appears once the operation is finished, so there is no way to dismiss a running deploy.

**Rule.** A dialog states an outcome or asks one question. Anything with more than one
decision is a wizard.

---

### 6.17 Progress and phases

**Class:** `.progress-track`, `.progress-fill`, `.phase-list`, `.phase`, `.phase.active`,
`.phase.done`, `.phase-dot`

**Anatomy.** `.progress-track` is a 4px `--bg-active` bar at `--radius-pill` with
`overflow: hidden`; `.progress-fill` is `--accent-gradient` at full height, animated by
width. `.phase-list` is a flex column with gap `--sp-3`; each `.phase` is a row with gap
`--sp-3` at 13px, leading with a 16px `.phase-dot` circle (1px `--border-strong`).

**States.** Phase pending: `--text-tertiary`, empty dot. Active: `--text-primary`, dot
border becomes `--accent`. Done: `--text-secondary`, dot fills `--success` with an 11px
white check. The fill animates width over 300ms with `--ease-out`.

**Rule.** Phases are named in plain language describing what is happening to the user's
files, not internal stage names. The shipped set is: "Checking what changed", "Removing
the previous set", "Copying files into the game", "Saving a record so this can be undone".
Because the backend applies a deployment as one blocking call, progress is expressed as
discrete phases, never as a fake byte counter.

**Indeterminate variant.** `.progress-track.indeterminate` drops the fill and slides a 32%
wide `--accent-gradient` block across the track on a 1.4s `--ease-in-out` loop. It is used
in exactly one situation: a download whose server sent no `Content-Length`, so the total
is genuinely unknown. It says "working" without claiming a percentage that would be
invented. Under `prefers-reduced-motion` the animation stops and the bar becomes a static
40% opacity fill. Never reach for it as decoration on something whose size is known.

---

### 6.18 Toast

**Class:** `.toast-wrap`, `.toast`, `.toast.ok`, `.toast.bad`
**Component:** `ToastProvider` and `useToast()` in `ui.tsx`

**Anatomy.** `.toast-wrap` is fixed at `bottom: --sp-4; right: --sp-4`, a flex column with
gap `--sp-3`, `z-index: 100`, `pointer-events: none`. Each toast is a row with gap
`--sp-3`, padding `--sp-3 --sp-4`, `--radius-sm`, `--bg-overlay`, 1px `--border-strong`,
`--shadow-lg`, 13px, `max-width: 400px`, leading with a status icon coloured `--success`,
`--danger` or `--accent`.

**States.** `.ok` and `.bad` swap the border colour to `--success` and `--danger`. Dismiss
is automatic: 4000ms for info and ok, 7000ms for bad. Enter is a spring (stiffness 460,
damping 34) from `opacity 0, y 16, scale 0.96`; exit slides right (`x: 24`) and fades.
`layout` plus `AnimatePresence mode="popLayout"` keeps the stack tidy as toasts expire.

**Rule.** Toasts report things that already happened. Anything the user must act on is a
`.notice` or a dialog. Anything that must not be missed (a failed apply) stays on screen
in the dialog instead.

---

### 6.19 Splash

**Class:** `.splash`, `.splash-inner`, `.splash-mark`, `.splash-name`, `.splash-bar`
**Component:** `Splash.tsx`

**Anatomy.** Fixed full-screen `--bg-base` at `z-index: 100`, contents centred. A flex
column with gap `--sp-4`: the 48px logo in `--accent`, the wordmark at 20px / 600
uppercase with `+0.08em` tracking, and a 128 x 2 `--bg-active` pill containing a 60%-wide
`--accent-gradient` span.

**States.** Mark rises from `y: 8` over 500ms; wordmark fades in at 80ms; bar appears at
160ms; the fill sweeps `x: -100% -> 100%` over 1.1s on a loop. The whole layer fades out
over 320ms rather than cutting, so the app does not flash.

**Rule.** Shown only while the first game and settings queries resolve. No tips, no
version string, no progress percentage.

---

### 6.20 Lightbox

**Class:** `.lightbox`, `.lightbox img`, `.lightbox-caption`
**Component:** `LightboxProvider` in `Preview.tsx`

**Anatomy.** Fixed full-screen `--scrim` with `backdrop-filter: blur(8px)` (double the
wizard's blur, because the layer above it is a single image), `z-index: 80`, padding
`--sp-5`, `cursor: zoom-out`. The image is `object-fit: contain`, capped at 100% of the
viewport in both axes, `--radius-sm`. The caption is a pill at `bottom: --sp-5`, centred
with `translateX(-50%)`, `--bg-surface`, 1px border, padding `--sp-2 --sp-4`, 13px
`--text-secondary`.

**States.** Fades in over 160ms; the image scales from 0.98 over 200ms. Closes on any
click and on Escape (a `keydown` listener mounted only while open).

**Rule.** Images are never cropped anywhere in the app. Thumbnails letterbox, the hero
letterboxes, the lightbox contains. Mod authors' screenshots are content.

---

### 6.21 Empty state

**Class:** `.empty`, `.empty-icon`, `.empty-title`

**Anatomy.** A centred flex column, gap `--sp-3`, padding `--sp-6 --sp-4`,
`text-align: center`, base colour `--text-tertiary`. Three or four parts: a large icon
(32 to 40px at `strokeWidth={1}`) in `--empty-icon` at 50% opacity with
`margin-bottom: --sp-2`; an `.empty-title` at 14px / 600 in `--text-secondary`; a sentence
of body text saying what to do; and optionally one `.btn.primary` with `margin-top: 8px`.

**States.** None. Every empty state names the cause and offers the action that resolves
it: no mods yet (Add mod), nothing matches the filters (clear them), no dry run yet
(run one).

**Rule.** Never show a bare "No data". Say what is missing and how to get it.

---

### 6.22 File list

**Class:** `.file-list`, `.file-list div`

**Anatomy.** A scrolling monospace block for paths: `--font-mono` at 12px,
`line-height: 1.8`, `--text-secondary`, `--bg-sunken`, 1px border, `--radius-sm`, padding
`--sp-3`, `max-height: 256px`, `overflow: auto`. Each entry is a `<div>` with
`white-space: nowrap`, so long paths scroll horizontally instead of wrapping.

**Usage.** Three lists on the deployment screen: file conflicts (path, arrow, winning
option in bold, contenders at 60% opacity), files that will be replaced, and new files.
Long lists are truncated in code at 400 entries with a dimmed "and N more" line.

**Rule.** Paths are always monospace and never wrapped or middle-ellipsised inside a file
list. The user needs to compare them character by character.

---

### 6.23 Skeleton

**Class:** `.skeleton`

**Anatomy.** A shimmering placeholder: a 90-degree gradient
`--bg-raised 25%, --bg-hover 50%, --bg-raised 75%` at `background-size: 200% 100%`,
animated with `@keyframes shimmer` to `background-position: -200% 0` over 1.4s
`ease-in-out` on a loop, `--radius-xs`.

**Usage.** Size it inline to match the thing it replaces: `height: 78px` for a pending
mod row, the full 128px thumbnail box, the 256px hero. Three skeletons is the standard
loading state for a list.

**Rule.** Skeletons are for content whose shape is known. For an action whose duration is
unknown, use the `.spinner` (14px, 2px `--border-strong` ring with an `--accent` top edge,
0.6s linear spin).

---

### 6.24 Download row

**Class:** `.download-row`, `.download-row.active`, `.download-icon` (+ `.ready`,
`.failed`, `.cancelled`), `.download-main`, `.download-actions`, `.section-head`,
`.section-title`, `.section-count`
**Component:** `DownloadsScreen.tsx`

**Anatomy.** Left to right, gap `--sp-4`, on the same shell as `.mod-row` (`--bg-raised`,
1px `--border`, `--radius-sm`, padding `--sp-4`):

1. `.download-icon`, a 32px `--radius-xs` tile on `--bg-sunken` holding a `Spinner` while
   running, `Icon.package` when ready, `Icon.warning` otherwise. Its colour is the whole
   state signal: `--success` ready, `--danger` failed, `--warning` stopped.
2. `.download-main`, `flex: 1; min-width: 0`, column at gap `--sp-2`: a `.mod-name` that
   truncates, an optional `Chip`, then `.mod-meta` in `tabular-nums`, then the
   `.progress-track` while running.
3. `.download-actions`: Stop while running; otherwise Install (`.btn.sm.primary`) when
   ready, plus a `.btn.sm.icon` trash.

**Grouping.** Rows are bucketed under `.section-head` labels in a fixed order:
Downloading, Ready to install, Did not finish. Empty groups are not rendered. This puts
what needs an action above what is already settled.

**Meta line.** Running: received of total, rate, time left, middot separated, with any
unknown part omitted rather than shown as a placeholder. Failed: the actual error text.
Ready: size and where it came from.

**States.** `.download-row.active` takes `--accent-border`. Rows animate with `layout` so
a finished download slides from one group to the next rather than disappearing and
reappearing.

**Installed state.** A download whose archive path matches an imported mod shows an `ok`
chip reading "in your library", swaps its icon to a check, and drops the button from
`.primary` to plain with the label "Install again". It stays clickable on purpose:
re-importing the same archive replaces the existing mod rather than duplicating it,
because the mod id is derived from the archive hash, so this is also how the user reopens
the option wizard.

**Rule.** A finished download never opens a dialog by itself. It waits with an Install
button. Fetching a file and choosing to install it are separate decisions, and a transfer
that completes in the background must not take the window.

**Rule.** A row states what is true of the file on disk, never what the app would prefer
to show. If it is installed it says so, and it says what it was installed as.

---

### 6.25 Also in the sheet

Smaller pieces that follow directly from the tokens:

| Class | What it is |
| --- | --- |
| `.deploybar` | Persistent bottom bar: state dot (`--warning` when dirty, `--success` when clean), a sentence of status, then Roll back, Dry run, and the primary Apply. Padding `--sp-3 --sp-5` on `--bg-surface`. |
| `.notice` | Inline message block: gap `--sp-3`, padding `--sp-4`, `--radius-sm`, `--warning-bg` with a `--warning` border by default; `.notice.info` swaps to `--accent-muted` with `--accent-border`. `.notice-title` 13px / 600, `.notice-body` 13px `--text-secondary`, `pre-wrap`. |
| `.preview-hero` | Wide cover art: full width, `max-height: 256px`, top corners `--radius-lg`, `--bg-sunken`, no bottom border so a `.notice` can butt against it. `cursor: zoom-in`. |
| `.kv` | Definition list on a `176px 1fr` grid, gap `8px 16px`, 13px. `dt` is `--text-tertiary`, `dd` is `--text-primary` with `overflow-wrap: anywhere` for long paths. |
| `.field` / `.input` | Labelled text input: column, gap `--sp-2`, label 13px / 500; input padding `--sp-3`, `--bg-sunken`, 1px border, `--radius-sm`, 14px, `--accent-border` on focus. |
| `.divider` | 1px `--border` rule. The only permitted separator inside a card. |
| `.swatches` / `.swatch` | Appearance accent picker: 32px circles, 2px transparent border that becomes `--text-primary` when active. |
| `.range` | Native slider with `accent-color: var(--accent)`. |
| `.stack` / `.stack.tight` / `.row` | Layout helpers: column at `--sp-4` / `--sp-3`, row at `--sp-3`. |
| `.mono` / `.truncate` / `.visually-hidden` | Monospace at `0.92em`, single-line ellipsis, screen-reader-only text. |

---

## 7. Motion

### 7.1 Tween or spring

Use a **CSS transition** (tween) when a property changes but nothing moves: colour,
border colour, opacity, background. `--dur-fast` with `--ease-out` covers almost all of
these.

Use a **framer-motion spring** when something changes position or size and the user needs
to track its identity: the nav pill, the segmented pill, the switch thumb, mod rows
reordering, modal entrances, toast arrivals, the radio dot.

Use a **framer-motion tween** with `ease: [0.16, 1, 0.3, 1]` for content swaps where
nothing is being tracked: page transitions, wizard step transitions, group expansion,
image fades, progress width.

### 7.2 The actual values

| Where | Type | Values |
| --- | --- | --- |
| Hover, press, border, colour | CSS | `--dur-fast` (120ms), `--ease-out` |
| Chevron rotation, theme swap, group expand | CSS / tween | `--dur` (200ms), `--ease-out` |
| Page transition (`pageMotion`) | tween | 180ms, `[0.16, 1, 0.3, 1]`, `opacity 0 -> 1`, `y 4 -> 0 -> -4` |
| Wizard step change | tween | 200ms, `x 14 -> 0 -> -14` |
| Group expand / collapse | tween | 200ms, `height 0 <-> auto` with `opacity` |
| Theme icon swap | tween | 220ms, rotate 90 degrees with scale 0.6 |
| Nav pill | spring | stiffness 520, damping 40, `layoutId="nav-pill"` |
| Segmented pill | spring | stiffness 500, damping 40, `layoutId` per `idPrefix` |
| Switch thumb | spring | stiffness 700, damping 40, `x 0 -> 16` |
| Radio dot | spring | stiffness 620, damping 28, `scale 0 -> 1` |
| Checkbox tick | tween | 140ms, `scale` and `opacity` |
| Mod row enter / exit / reorder | spring | stiffness 420, damping 34, with `layout` |
| Game card hover | spring | stiffness 400, damping 30, `y: -2`; tap `scale: 0.995` |
| Toast | spring | stiffness 460, damping 34, in from `y 16`, out to `x 24` |
| Dialog | spring | stiffness 420, damping 36, from `scale 0.98, y 8` |
| Wizard modal | spring | stiffness 380, damping 34, from `scale 0.97, y 12` |
| Overlay scrim | tween | 180ms opacity |
| Splash fade out | tween | 320ms, `--ease-out` |
| Splash bar sweep | loop | 1.1s `easeInOut`, `x -100% -> 100%` |
| Spinner | CSS keyframes | 0.6s linear, infinite |
| Skeleton shimmer | CSS keyframes | 1.4s `ease-in-out`, infinite |

### 7.3 Rules

- Nothing animates for longer than 360ms. If it needs longer, it is a loading state and
  should show a spinner or a skeleton.
- Nothing animates on first paint except the splash. `AnimatePresence` gets
  `initial={false}` wherever a list is already populated.
- Never animate `width`, `height`, `top` or `left` on hover. Transform and opacity only.
- Two things never animate simultaneously in the same region. If the nav pill is moving,
  the page transition waits (`AnimatePresence mode="wait"`).
- `prefers-reduced-motion: reduce` collapses every CSS transition and animation to
  0.001ms globally in `theme.css`. When adding a framer-motion animation that conveys
  meaning (not just polish), also gate it on `useReducedMotion()`.

---

## 8. Accessibility

### 8.1 Focus

A single global rule in `theme.css`:

```css
:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
  border-radius: var(--radius-xs);
}
```

Never remove it. Where a component overrides the outline for aesthetic reasons (the
search input and `.select` do, on `:focus`), it must substitute an equally visible signal:
in both cases the border becomes `--accent-border`. `:focus-visible` still applies for
keyboard users.

### 8.2 Contrast targets

Body text meets 4.5:1 and interactive borders meet 3:1 against their own background, in
both themes. Measured pairings are in sections 2.1, 2.2 and 3.2. Two deliberate
exceptions, both documented rather than hidden:

- `--text-tertiary` on `--bg-base` is 4.9:1 in dark but 3.5:1 in light. It is restricted
  to 12 to 13px non-essential metadata (counts, placeholders, hints) that is always
  duplicated or inferable elsewhere. Never use it for a value the user must read.
- `--accent-contrast` on the lighter gradient stop is 3.13:1 in dark. See section 3.2 for
  the strict alternative.

### 8.3 Reduced motion

`@media (prefers-reduced-motion: reduce)` sets `animation-duration`, `transition-duration`
to `0.001ms` and `animation-iteration-count` to 1 for every element. Layout and colour
still change, they just change instantly. The spinner and skeleton stop moving, which is
correct: their meaning is carried by their presence, not their motion.

### 8.4 Real semantics

Controls use the real role, not a div with a click handler:

| Component | Semantics |
| --- | --- |
| `Switch` | `<button type="button" role="switch" aria-checked>` plus a required `aria-label` |
| `Segmented` | container `role="tablist"`, options `role="tab" aria-selected` |
| Option card (exclusive) | `role="radio" aria-checked` with `aria-label` from the option name |
| Option card (stackable) | `role="checkbox" aria-checked` |
| Option card (forced) | `disabled` plus `aria-checked` so it is announced as locked-on |
| Nav item | `<button>` with `aria-current="page"` when active |
| Wizard, apply dialog | `role="dialog" aria-modal="true"` plus `aria-label` |
| Icons | `aria-hidden="true" focusable="false"`; the label is on the control |
| Icon-only buttons | always carry an `aria-label` (window controls, drag handle, theme toggle) |
| Toolbar controls | `aria-label` on every input and select, because labels are visual-free |
| Spinner | `aria-label="Loading"` |

### 8.5 Keyboard

Everything interactive is a real `<button>`, `<input>` or `<select>`, so tab order follows
the DOM and Enter or Space activates without extra code. Specific behaviours already
implemented: Escape closes the lightbox; Enter in the new-profile field submits; the
wizard scrim is clickable but the same action is always reachable through the Cancel
button in the footer.

Known gaps to close when touched: the wizard does not trap focus or restore it on close,
radio groups do not implement arrow-key roving tabindex, and drag reordering has no
keyboard alternative (add "move up" and "move down" to a row menu when the mod list is
next revised).

---

## 9. How to extend

### 9.1 Adding a new screen

1. Add the id to the `Screen` union in `App.tsx` and an entry to the `NAV` array with a
   label and an icon name. If the icon does not exist, draw it in `icons.tsx` first,
   following section 5.1.
2. Add a title to the `titles` record in `TopBar` (screen titles are nouns: Library,
   Mods, Profiles, Deployment, Settings).
3. Write the screen as a function component returning a `.stack`. The shell already
   provides the scroll container, the page padding (`--sp-5`) and the page transition.
   Do not add your own padding to the outermost element.
4. Compose from existing classes. In order of preference: an existing component, an
   existing component with a new modifier class, then and only then a new class.
5. Every screen needs an empty state (section 6.21) and a loading state (three
   `.skeleton` blocks sized like the real content).

### 9.2 Adding a token

Only add a token when the value is thematic (something the Appearance panel might
reasonably want to change) or used in three or more places. Otherwise use a literal and
leave a comment.

1. Define it in `:root` in `theme.css` if it is theme-independent (spacing, radius,
   duration, layout).
2. If it is a colour, define it in **both** the dark and the light block. A colour that
   exists in only one theme is a bug.
3. Derive it from an existing token where possible: `hsl(var(--accent-h) ...)` rather
   than a fresh hex.
4. Add a row to the relevant table in section 2 of this document, in the same commit.
5. Name it by role, not by appearance: `--bg-raised`, not `--grey-800`;
   `--accent-border`, not `--green-line`.

### 9.3 Theming rules

- **Never hard-code a colour.** No hex, no `rgb()`, no named colour in a component or in
  `app.css` outside the two theme blocks. There is exactly one intentional exception in
  the codebase: `color: #fff` on the close button hover, because it sits on `--danger` in
  both themes. Anything else is a bug.
- **Never hard-code a spacing, radius or duration.** Use the scale.
- **Inline styles are for layout only.** `style={{ minWidth: 0, flex: 1 }}` is fine.
  `style={{ color: "var(--accent)" }}` is acceptable when the colour is genuinely dynamic
  (a status glyph that switches between accent, success and danger). A literal colour in
  an inline style is not.
- **Both themes, every time.** Build in dark, then toggle to light before committing. The
  light ramp is not an inversion, so a change that looks right in dark can flatten
  completely in light where separation comes from borders.
- **The accent is not decoration.** If you find yourself reaching for `--accent` to make
  something look nicer, use `--text-primary` or a border instead.
- **No new shadows.** The three shadow tokens are the complete set, and they belong to
  floating layers only.

### 9.4 Adding a component

Put the class in `app.css` under the matching banner comment, with the same banner style
already used there. Put stateful behaviour in `ui.tsx` if it is generic, or next to its
screen if it is not. Then add an entry to section 6 with anatomy, states and spacing. A
component that is not in section 6 will be reinvented by the next contributor.

---

## 10. Using this document with an AI design tool

> For day to day work use [design-brief-paste.md](./design-brief-paste.md). It is a
> single self-contained file holding the tokens, the component vocabulary, the
> constraints and the prompt, so you can attach it in one go instead of collecting
> sections from this document. Read on for the reasoning behind it.

If you have never handed a design system to an AI before, this section is the whole
procedure. It works with Claude (including Artifacts and Claude Code), and the same shape
works with other assistants and with design tools that accept a written brief.

### 10.1 The idea in one paragraph

An AI design assistant is not reading your repository. It only knows what you paste into
the conversation. So the job is: paste the rules, state the constraints, describe the one
screen you want, and ask for a self-contained HTML file you can look at. Then iterate in
small steps. If you skip the rules, you will get a generic purple-gradient dashboard that
looks nothing like Apocrypha.

### 10.2 What to paste

Paste these three things, in this order, in a single message:

1. **The token block** from section 11 of this document. It is compact and complete: the
   whole palette in both themes, the type scale, spacing, radius and motion. This is the
   single most important thing to include.
2. **The relevant component entries** from section 6. Do not paste all of them. If you
   are asking for a mod list screen, paste 6.10 (toolbar), 6.11 (mod group), 6.12 (mod
   row), 6.7 (chip), 6.8 (switch) and 6.6 (button). Six entries is plenty.
3. **The five design principles** from section 1, at least the headings and one sentence
   each.

If the conversation supports file attachments, attaching this whole document works too,
and then you just say "use the design system in the attached file".

### 10.3 What to ask for

Ask for **one screen at a time**, as **a single self-contained HTML file**, with **all
CSS inline in a `<style>` block**, using **the CSS variables by name**. Asking for React
components is usually a mistake at the design stage: you want something you can open and
look at, not something you have to build.

Here is a prompt you can copy, fill in the bracket, and send after the paste:

```
You are designing a new screen for Apocrypha, a Linux-first desktop mod manager.
The design system is above. Follow it exactly.

Build: [describe the screen in two or three sentences. For example: "a Conflicts
screen showing which mod wins each contested file, with a filter for resolved
versus unresolved, and a per-file list of losing contenders."]

Constraints, all of which are hard requirements:
- Output ONE self-contained .html file. All CSS in a <style> block. No frameworks,
  no Tailwind, no CDN links, no external fonts, no images.
- Use the CSS custom properties from the token block by name. Never write a literal
  colour, spacing value, radius or duration anywhere in the CSS.
- Use only the spacing scale (2, 4, 8, 16, 32, 64) and only radius 4, 8, 16 or pill.
- Use only font weights 400, 500, 600 and 700.
- Reuse the existing component classes (.card, .btn, .chip, .stat, .mod-row, and so
  on) before inventing new ones. If you must invent a class, name it by role and
  list it at the end of your reply with a one-line description.
- Exactly one primary action on the screen. One accent colour, used only for the
  primary action, the current selection, and the focus ring.
- Include the app shell around it: 40px titlebar, 224px rail with the five nav
  items (Library, Mods, Profiles, Conflicts, Settings), topbar with title and
  subtitle, 32px page padding, deploy bar at the bottom.
- Include the empty state and the loading state for this screen as separate blocks
  further down the same file, each labelled with a comment.
- Icons: inline SVG only, 24 viewBox, 1.5 stroke, round caps, no fill,
  stroke="currentColor", 16px default.
- Render in dark theme by default and make sure it also works when
  data-theme="light" is set on the root element.
- No shadows except on floating layers (modals, toasts). No glows on buttons.
```

### 10.4 Constraints worth restating every time

AI assistants drift back to their defaults over a long conversation. These five are the
ones that slip first, so repeat them in follow-up messages:

1. **No literal colours.** Every colour is `var(--something)`.
2. **Powers of two only.** No 12px, no 20px, no 24px padding.
3. **One accent.** No second brand colour, no purple gradient, no coloured icons.
4. **No component library.** No Tailwind classes, no Bootstrap, no Material.
5. **Only three font weights.** 400, 500 or 600, and 700.

A short correction like this works well: "Close, but you used `#2ecc71` in two places and
a 12px gap. Replace them with `var(--success)` and `var(--sp-3)`, and keep everything else
the same."

### 10.5 Iterating

Change one thing per message and keep the file. Good follow-ups:

- "Same file, but show the loading state with three skeleton rows instead of the list."
- "The stat row should be four tiles using the `.stat` pattern, not four cards."
- "Add the collapsible category grouping from section 6.11 around the rows."
- "Now show me the same screen in light theme, nothing else changed."
- "Give me a version where the rail is collapsed to icons only, 64px wide."

Bad follow-ups, which cause the assistant to rebuild from scratch and lose your details:
"make it nicer", "modernise it", "add some polish".

### 10.6 Bringing a design back into the app

When a generated screen is right, port it rather than pasting it:

1. Move any new class into `apps/desktop/src/styles/app.css`, under the correct banner
   comment. Delete every class that duplicates something already there.
2. Convert the markup to a React component in `apps/desktop/src/components/`. Static
   markup becomes JSX, repeated blocks become `.map()`.
3. Replace inline SVGs with entries in `icons.tsx`, or reuse existing icons.
4. Wire state through the existing patterns: `useState` in the screen, `api.*` calls from
   `lib/api.ts`, `useToast()` for outcomes.
5. Check it in both themes and at the 940 x 620 minimum window size.
6. Add the new component to section 6 of this document.

### 10.7 What to hand a human designer

The same paste, plus three extras: the hex list in section 3.1 (so they can build the
palette in their tool), the layout constants in section 2.8, and a screenshot of an
existing screen for reference. Tell them the constraints that are non-negotiable
(power-of-two spacing, three font weights, one accent, monoline 1.5-stroke icons on a 24
grid) and which are open (illustration style, empty-state copy, iconography for new
concepts).

---

## 11. Appendix: paste-ready token block

Copy everything below into a conversation with an AI design assistant, or into a new
project to bootstrap the same look.

```css
/* Apocrypha design tokens. Dark is the default; light is opt-in via
   data-theme="light" on the root element. Every component reads these
   variables and never hard-codes a value. */

:root {
  /* type */
  --font-display: "SF Pro Display", -apple-system, BlinkMacSystemFont, "Inter",
    "Segoe UI", system-ui, sans-serif;
  --font-mono: "SF Mono", "JetBrains Mono", ui-monospace, Menlo, monospace;
  --text-base: 14px;
  --text-xs: 12px;   /* chips, metadata, uppercase labels */
  --text-sm: 13px;   /* secondary UI, hints, small buttons */
  --text-md: 14px;   /* body, buttons, nav, mod names */
  --text-lg: 16px;   /* dialog titles, brand */
  --text-xl: 20px;   /* screen titles, stat values */
  --text-2xl: 24px;
  --text-3xl: 32px;
  --leading-tight: 1.25;
  --leading: 1.5;
  /* only weights 400, 500-600 and 700 exist in this system */

  /* spacing: powers of two */
  --sp-1: 2px;
  --sp-2: 4px;
  --sp-3: 8px;
  --sp-4: 16px;
  --sp-5: 32px;
  --sp-6: 64px;
  --sp-7: 128px;

  /* radius: 4 inside 8 inside 16 */
  --radius-xs: 4px;
  --radius-sm: 8px;
  --radius: 8px;
  --radius-lg: 16px;
  --radius-xl: 16px;
  --radius-pill: 999px;

  /* motion */
  --ease-out: cubic-bezier(0.16, 1, 0.3, 1);
  --ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);
  --dur-fast: 120ms;
  --dur: 200ms;
  --dur-slow: 360ms;

  /* layout */
  --titlebar-h: 40px;
  --rail-w: 224px;

  /* accent inputs: change these three to reskin */
  --accent-h: 158;
  --accent-s: 38%;
  --accent-h2: 138;
}

:root,
:root[data-theme="dark"] {
  color-scheme: dark;
  --bg-sunken:  hsl(190 14% 3%);   /* #070809 titlebar, rail, inputs */
  --bg-base:    hsl(190 12% 5%);   /* #0B0E0E window */
  --bg-surface: hsl(190 12% 8%);   /* #121617 cards */
  --bg-raised:  hsl(190 11% 11%);  /* #191E1F rows, buttons */
  --bg-overlay: hsl(190 11% 13%);  /* #1E2425 toasts */
  --bg-hover:   hsl(190 11% 15%);  /* #22292A */
  --bg-active:  hsl(190 11% 18%);  /* #293133 pressed, pills, chips */
  --border:        hsl(190 10% 17%); /* #272E30 */
  --border-strong: hsl(190 10% 26%); /* #3C4749 */
  --text-primary:   hsl(160 14% 94%); /* #EEF2F0 */
  --text-secondary: hsl(165 8% 68%);  /* #A7B4B1 */
  --text-tertiary:  hsl(165 7% 48%);  /* #72837F */
  --accent:         hsl(var(--accent-h) var(--accent-s) 48%); /* #4CA987 */
  --accent-hover:   hsl(var(--accent-h) var(--accent-s) 55%);
  --accent-muted:   hsl(var(--accent-h) 28% 13%);
  --accent-contrast: hsl(160 30% 96%);
  --accent-border:  hsl(var(--accent-h) 26% 28%);
  --accent-gradient: linear-gradient(135deg,
    hsl(var(--accent-h) var(--accent-s) 44%) 0%,
    hsl(var(--accent-h2) calc(var(--accent-s) - 4%) 38%) 100%);
  --success: hsl(152 58% 55%);  --success-bg: hsl(152 40% 12%);
  --warning: hsl(38 82% 60%);   --warning-bg: hsl(38 45% 12%);
  --danger:  hsl(2 70% 62%);    --danger-bg:  hsl(2 42% 13%);
  --info:    hsl(198 70% 60%);
  --shadow-sm: 0 1px 2px hsl(190 30% 1% / 0.5);
  --shadow:    0 2px 8px hsl(190 30% 1% / 0.5);
  --shadow-lg: 0 8px 32px hsl(190 30% 1% / 0.6);
  --scrim: hsl(190 30% 2% / 0.72);
  --glow-a: hsl(var(--accent-h) 50% 30% / 0.16);
  --glow-b: hsl(190 40% 30% / 0.1);
}

:root[data-theme="light"] {
  color-scheme: light;
  --bg-base: hsl(0 0% 100%);
  --bg-sunken: hsl(160 12% 97%);
  --bg-surface: hsl(0 0% 100%);
  --bg-raised: hsl(0 0% 100%);
  --bg-overlay: hsl(0 0% 100%);
  --bg-hover: hsl(160 14% 96%);
  --bg-active: hsl(160 14% 93%);
  --border: hsl(160 12% 90%);
  --border-strong: hsl(160 10% 78%);
  --text-primary: hsl(190 20% 8%);
  --text-secondary: hsl(185 8% 36%);
  --text-tertiary: hsl(185 7% 52%);
  --accent: hsl(var(--accent-h) 46% 30%);
  --accent-hover: hsl(var(--accent-h) 46% 24%);
  --accent-muted: hsl(var(--accent-h) 34% 95%);
  --accent-contrast: hsl(0 0% 100%);
  --accent-border: hsl(var(--accent-h) 26% 78%);
  --accent-gradient: linear-gradient(135deg,
    hsl(var(--accent-h) 46% 30%) 0%,
    hsl(var(--accent-h2) 42% 26%) 100%);
  --success: hsl(152 60% 28%);  --success-bg: hsl(152 50% 94%);
  --warning: hsl(32 80% 36%);   --warning-bg: hsl(38 86% 94%);
  --danger:  hsl(2 66% 44%);    --danger-bg:  hsl(2 78% 96%);
  --info:    hsl(200 72% 38%);
  --shadow-sm: 0 1px 2px hsl(190 16% 40% / 0.08);
  --shadow:    0 2px 8px hsl(190 16% 40% / 0.08);
  --shadow-lg: 0 8px 32px hsl(190 16% 30% / 0.14);
  --scrim: hsl(190 16% 20% / 0.32);
  --glow-a: hsl(var(--accent-h) 60% 60% / 0.1);
  --glow-b: hsl(200 50% 60% / 0.07);
}

/* Global focus ring. Never remove it. */
:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
  border-radius: var(--radius-xs);
}

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.001ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.001ms !important;
  }
}
```

### Summary card for a design brief

```
Product      Apocrypha, a Linux-first desktop mod manager (Tauri + React, no Tailwind)
Mood         Quiet, dense, technical. A tool, not a storefront.
Themes       Dark (green on near-black) and light (green on white), runtime switchable
Accent       Deep desaturated green, hsl(158 38% 48%) dark / hsl(158 46% 30%) light
Gradient     135deg, hue 158 to hue 138, used only on primary buttons, switches,
             progress fills, step indices and selected marks
Type         SF Pro Display / Inter fallback, weights 400 / 500-600 / 700 only
             12, 13, 14, 16, 20, 24, 32 px
Spacing      2, 4, 8, 16, 32, 64, 128
Radius       4 inside 8 inside 16, plus a pill
Icons        Monoline, 1.5 stroke, 24 grid, round caps, no fill, one colour, 16px default
Logo         A sealed codex: closed book, spine fold, one seal line. Monoline, one colour.
Elevation    Background step, then a 1px border, then a shadow. Shadows only on
             floating layers. No glows anywhere.
Motion       cubic-bezier(.16,1,.3,1) at 120 / 200 / 360ms, springs for anything
             that moves and keeps its identity, all of it off under reduced motion
Shell        40px custom titlebar, 224px rail, topbar, 32px page padding, deploy bar
```

---

## Appendix B: current implementation notes

Honest state of the code as of this writing, so nobody wastes time hunting for a bug that
is really a to-do:

- `App.tsx` still renders an older inline mod list and does not yet mount `TitleBar.tsx`,
  `Splash.tsx`, `ApplyDialog.tsx` or the richer `ModsScreen.tsx`. Those components are
  complete and match `app.css`; wiring them into the shell is outstanding work.
- `InstallWizard.tsx` uses the class names `.wizard-backdrop` and `.radio-set`, while
  `app.css` defines the equivalents as `.overlay` and `.option-set`. The CSS is the
  intended naming; the component should be renamed to match.
- `App.tsx` references `Icon.chevron` and `Icon.rocket`, which do not exist in
  `icons.tsx`. Use `Icon.chevronRight` and `Icon.apply`, or draw the missing icons per
  section 5.1.
- `tauri.conf.json` sets `decorations: true`, so the custom titlebar currently sits under
  an OS frame. Ship with `false`.
- The Appearance panel exposes only the theme mode today. The swatch and range styles
  (`.swatches`, `.swatch`, `.range`) exist for the accent and text-size controls described
  in section 3, which are not yet built.
