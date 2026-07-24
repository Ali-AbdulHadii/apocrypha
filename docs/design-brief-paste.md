# Design brief: paste this

A single self-contained brief for designing Apocrypha screens with an AI design
assistant. Everything an assistant needs is in this one file, so you can attach
it or paste it whole and start working.

The full system is in [design-system.md](./design-system.md), which is the
reference when you need detail. This file is the working copy. For the app mark
specifically, see [logo-brief.md](./logo-brief.md).

---

## How to use this

1. Attach this file, or paste sections 1 to 4 into a new conversation.
2. Add the prompt in section 5, with your screen described in the bracket.
3. You get back one HTML file. Open it in a browser and look at it.
4. Iterate one change at a time. Never say "make it nicer".
5. When it is right, port it using section 6.

Ask for **one screen at a time** and for **a self-contained HTML file**, not
React. You want something you can open and judge, not something you have to
build first.

---

## 1. What Apocrypha is

A mod manager for Linux games. It installs and removes game modifications
safely: every change is recorded and can be undone file by file. The people
using it are comfortable with computers, are modding a game they care about, and
are afraid of breaking their install. The interface exists to make them
confident that nothing irreversible just happened.

It is a tool, not a storefront and not a launcher. It should sit comfortably
beside a terminal and a code editor.

### The reference is Apple

Apple's desktop design language is the reference: macOS System Settings, Finder,
and the Human Interface Guidelines. Not the marketing pages, not iOS, and not a
skeuomorphic pastiche. The parts that matter here:

- **Deference.** Chrome is quiet so content is loud. The sidebar, toolbar and
  bottom bar are neutral surfaces. Nothing in the frame competes with the list.
- **Inset grouped sections.** Related settings and details sit inside a rounded
  container with a title above it, the way macOS System Settings groups rows.
  That is what `.card` is for. Do not scatter loose controls on a background.
- **Generous, consistent padding.** Apple's density comes from alignment, not
  from cramming. 16px inside containers, 32px around the page.
- **A single accent used sparingly.** Apple uses one system accent for the
  active selection, the primary action, and focus. Everything else is neutral.
  Coloured icons and multi-hue palettes are the opposite of this.
- **Typography carries hierarchy, not boxes.** Size and weight separate a title
  from a subtitle. Do not add a border or a background to make something look
  important.
- **Left-aligned labels, right-aligned values.** Detail rows read as a two
  column list, which is what `.kv` does.
- **Controls people already know.** Switches for on and off, segmented controls
  for exclusive choices, plain buttons for actions. No invented widgets.
- **Motion is physical and short.** Things move the way a real object with mass
  would. Springs, not bounces. Nothing lingers.
- **Optical alignment over mathematical alignment.** If a glyph looks off-centre
  at the same measured position, move it.

**Where Apocrypha deliberately differs.** It is dark first, because people mod
games at night and beside a terminal. It is denser than a Mac utility, because
mod lists run to hundreds of rows. It draws its own window chrome, because it
must work across Linux desktop environments that do not agree on titlebars. And
the accent is green rather than the system blue.

### Design principles

**Content over chrome.** The mod list is the product. Frames, dividers and
decoration earn their place or they go.

**Reversibility is visible.** The interface always shows what is about to
change, what already changed, and how to undo it. Preview before Apply is not a
power-user feature, it is the default path.

**Density without noise.** These are long lists. Rows are compact and scannable,
but never cramped or grey-on-grey.

**One accent.** A single green. It marks the primary action, the current
selection, and the focus ring. Nothing else is coloured for decoration. Status
uses semantic colours only when the status is real.

**Motion that explains.** Animation shows where something came from or where it
went. Nothing bounces for personality. Everything respects reduced motion.

---

## 2. Tokens

Use these by name. Never write a literal colour, size, radius or duration.

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

---

## 3. Component vocabulary

Reuse these classes before inventing anything. If you must add a class, name it
by role and list it at the end of your reply.

| Class | What it is |
| --- | --- |
| `.app` | Root shell. Grid: 40px titlebar, then body. Rounded, clipped. |
| `.titlebar` | Custom window chrome. Drag region plus minimise, maximise, close. |
| `.rail` | 224px left nav. `.nav-item`, `.nav-icon`, `.nav-badge`, `.nav-pill` for the active pill. |
| `.topbar` | Page heading. `h1` plus `.topbar-sub`, actions pushed right by `.topbar-actions`. |
| `.scroll` | Scrolling content area, 32px padding. |
| `.stack` / `.stack.tight` | Vertical flow, 16px or 8px gaps. |
| `.row` | Horizontal flow, 8px gap, centred. |
| `.card` | Surface with border and 16px padding. `.card-title`, `.card-hint`. |
| `.stat` | Metric tile. `.stat-label` uppercase 12px, `.stat-value` 20px tabular. Grid them with `.stat-grid`. |
| `.btn` | Button. Modifiers: `.primary`, `.danger`, `.ghost`, `.sm`, `.icon`. |
| `.chip` | Small status pill. Modifiers: `.ok`, `.warn`, `.bad`, `.accent`. Often holds a `.dot`. |
| `.switch` | On/off toggle with `.switch-thumb`. |
| `.segmented` | Segmented control, `.seg-pill` slides behind the active item. |
| `.toolbar` | Search plus filters. Holds `.search` (with `.search-icon`) and `.select`. |
| `.mod-group` | Collapsible category. `.mod-group-head`, `.mod-group-name`, `.mod-group-count`, `.mod-group-body`, `.chevron`. |
| `.mod-row` | One mod. `.drag-handle` grip, switch, `.mod-name`, `.mod-meta`, chips, action. `.disabled` dims it. |
| `.option` | Wizard choice card. `.selected`, `.locked`, `.has-thumb`. Contains `.mark.radio` or `.mark.check`, `.option-name`, `.option-desc`, `.option-meta`. |
| `.option-set` | Group of options with `.option-set-title` and `.option-grid`. |
| `.option-thumb` / `.preview-hero` | Preview images. Contained, never cropped. |
| `.wizard` | Modal installer. Header, 240px step list, body, footer. Steps use `.step`, `.step-index`, `.step-check`. |
| `.dialog` | Small modal. `.dialog-head`, `.dialog-body`, `.dialog-foot`. |
| `.progress-track` / `.progress-fill` | Progress bar. `.phase` list for named stages. |
| `.notice` / `.notice.info` | Inline callout, warning or informational. |
| `.deploybar` | Persistent bottom bar. Status dot, summary, actions right. |
| `.toast` | Transient message, bottom right. `.ok`, `.bad`. |
| `.empty` | Empty state. `.empty-icon`, `.empty-title`, then a sentence and one action. |
| `.file-list` | Monospace scrolling path list. |
| `.skeleton` | Shimmer placeholder. |
| `.kv` | Definition grid, 176px label column. |
| `.overlay` | Full-screen scrim for modals. |
| `.mono` | Monospace run, for paths and versions. |

### The shell, always present

```
titlebar   40px, custom chrome
rail       224px, nav: Library, Mods, Profiles, Changes, Settings
topbar     h1 plus subtitle, actions right
content    32px padding, scrolls
deploybar  status left, Undo all / Preview changes / Apply right
```

### Existing screens, for consistency

| Screen | Contains |
| --- | --- |
| Library | Game cards, stat row, a details card with a `.kv` grid and loader setup. |
| Mods | Toolbar, collapsible categories, draggable mod rows with status chips. |
| Profiles | Create form, list of profiles with an in-use chip. |
| Changes | Stat row, conflicts, replaced files, new files, all as `.file-list`. |
| Settings | Appearance panel, downloads panel, storage details. |

---

## 4. Hard constraints

These are the ones that slip in a long conversation. Restate them when correcting.

1. **No literal values.** Every colour, space, radius and duration is a `var()`.
2. **Powers of two only.** 2, 4, 8, 16, 32, 64, 128. No 12px, no 20px, no 24px.
3. **Radius 4, 8, 16 or pill.** Nothing else.
4. **Three font weights.** 400, 500 or 600, and 700. Nothing else exists in the font.
5. **One accent.** No second brand colour, no purple gradient, no rainbow icons.
6. **No component library.** No Tailwind, no Bootstrap, no Material, no CDN links.
7. **Icons are inline SVG.** 24 viewBox, 1.5 stroke, round caps and joins, `fill="none"`, `stroke="currentColor"`, 16px default. One colour. No filled shapes.
8. **No glows.** Elevation is borders and, on floating layers only, a soft shadow.
9. **Both themes.** Dark by default, and correct with `data-theme="light"` on the root.
10. **One primary action per screen.**

---

## 5. The prompt

Paste this after the brief, with the bracket filled in.

```
You are designing a screen for Apocrypha, a Linux-first desktop mod manager.
The design system is above. Follow it exactly.

The visual reference is Apple's desktop design language: macOS System Settings
and Finder. Quiet chrome, inset grouped sections inside rounded containers with
a title above, generous consistent padding, hierarchy from type rather than
boxes, one accent used only for the primary action and the current selection,
and standard controls. Dark first, and denser than a Mac utility because the
lists are long.

Build: [describe the screen in two or three sentences. Say what the user is
trying to do, what they need to see, and what the one primary action is.]

Hard requirements:
- Output ONE self-contained .html file. All CSS in a <style> block. No
  frameworks, no CDN links, no external fonts, no images.
- Use the CSS custom properties by name. Never write a literal colour, spacing
  value, radius or duration anywhere.
- Spacing only from 2, 4, 8, 16, 32, 64. Radius only 4, 8, 16 or pill.
- Font weights only 400, 500, 600, 700.
- Reuse the component classes listed above before inventing new ones. List any
  new class at the end of your reply with a one-line description of its role.
- Exactly one primary action. One accent colour, used only for that action, the
  current selection, and the focus ring.
- Include the full app shell: titlebar, rail with the five nav items, topbar,
  padded content, deploy bar.
- Include the empty state and the loading state as separate blocks lower in the
  same file, each behind an HTML comment saying which it is.
- Icons: inline SVG, 24 viewBox, 1.5 stroke, round caps, no fill,
  stroke="currentColor".
- Dark theme by default, and correct when data-theme="light" is set on <html>.
- No shadows except on floating layers. No glows.
- Nothing that reads as a web dashboard: no hero banners, no coloured stat
  cards, no icon circles, no gradient headers, no card grids of equal squares.

Before you write the file, tell me in three lines: the layout you chose, the one
primary action, and anything in my description you had to guess.
```

That last paragraph is worth keeping. It surfaces misunderstandings before you
have a whole file built on them.

### Good follow-ups

Change one thing per message and keep the file:

- "Same file, but show the loading state with three skeleton rows."
- "Make the stat row four `.stat` tiles rather than four cards."
- "Wrap the rows in the collapsible `.mod-group` pattern."
- "Show me the same screen with `data-theme=\"light\"`, nothing else changed."
- "You used `#2ecc71` and a 12px gap. Replace with `var(--success)` and
  `var(--sp-3)`, keep everything else."

Avoid "make it nicer", "modernise it", "add polish". Those cause a rebuild from
scratch and you lose the details you already fixed.

---

## 6. Bringing a design back into the app

1. New classes go into `apps/desktop/src/styles/app.css`, under the right banner
   comment. Delete anything that duplicates an existing class.
2. Markup becomes a component in `apps/desktop/src/components/`. Repeated blocks
   become `.map()`.
3. Inline SVGs move into `icons.tsx`, or reuse what is there.
4. State uses the existing patterns: `useState` in the screen, `api.*` from
   `lib/api.ts`, `useToast()` for outcomes.
5. Check both themes and the 940 x 620 minimum window size.
6. Add the component to section 6 of `design-system.md`.

---

## 7. Handing this to a human designer

Give them this file, plus:

- The hex values from section 3.1 of `design-system.md`, so they can build the
  palette in their own tool.
- A screenshot of an existing screen.

Tell them what is fixed: power-of-two spacing, three font weights, one accent,
monoline 1.5-stroke icons on a 24 grid, both themes. Tell them what is open:
illustration style, empty-state wording, and icons for concepts that do not have
one yet.
