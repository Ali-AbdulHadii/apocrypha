# Logo brief

A brief for designing the real Apocrypha mark, to hand to a designer or to an AI
design assistant. The current mark in `apps/desktop/src/components/icons.tsx` is
a placeholder drawn to keep the app coherent, not a finished identity.

For designing screens rather than the mark, use
[design-brief-paste.md](./design-brief-paste.md), which is a single file you can
paste or attach. The full reference is [design-system.md](./design-system.md).
This brief is only about the mark.

---

## 1. What to paste into the conversation

Everything in section 2 below. It is written to stand alone, so an assistant
with no access to the repository can still produce something usable.

Ask for **SVG output**, not a raster image. SVG is what the app needs, it stays
crisp at any size, and you can edit it afterwards. If the assistant offers an
image instead, ask it to write the SVG markup.

Then work in rounds. Ask for four or five distinct directions first, pick one,
and only then ask for refinements. Judging one option in isolation is much
harder than choosing between several.

---

## 2. The brief

> **Project**
>
> Apocrypha is a free, open source mod manager for Linux games. It installs and
> removes game modifications safely: every change it makes is recorded and can
> be undone file by file. It is a serious tool for people who tinker with their
> games, not a toy and not a launcher.
>
> The name means hidden or set-aside writings: texts kept outside the accepted
> canon. That is the association to work with. Mods are unofficial additions to
> a game, so the name is a quiet joke about non-canonical content.
>
> **What I need**
>
> A single application mark. Not a wordmark, not a lockup with text. Just the
> symbol, which will sit next to the word "Apocrypha" set in SF Pro Display.
>
> **Constraints, all of them firm**
>
> - **One colour.** The mark inherits a single colour from its surroundings and
>   must work in that colour alone. No gradients, no two-tone, no shading. It
>   has to read on a near black background and on white.
> - **Monoline.** Drawn as strokes of one consistent weight, not filled shapes.
>   Around 1.75 units on a 32 unit grid, or 1.5 on a 24 unit grid.
> - **Geometric.** Built from circles, straight lines, and simple arcs. Nothing
>   hand-drawn, nothing organic, no texture.
> - **Legible at 16 pixels.** It appears in a title bar at 18px and a sidebar at
>   32px. If detail disappears at 16px, the design has failed. Test by describing
>   what survives at that size.
> - **Square canvas**, centred, with even optical margin. Assume a 32x32 or
>   1024x1024 viewBox.
> - **Round caps and joins**, to match the icon set.
> - **No literal gaming clichés.** No controllers, no dice, no swords, no
>   puzzle pieces, no gears, no wrenches, no sliders, no boxes with arrows.
> - **Nothing occult-kitsch.** The name invites pentagrams and eyes. Avoid them.
>   The tone is a quiet library, not a horror film.
>
> **Tone**
>
> Restrained, precise, a little archival. It should look like it belongs beside
> developer tools, not beside a game launcher. Think of the marks used by
> version control tools and terminal emulators rather than by storefronts.
>
> The reference is Apple's iconography, specifically SF Symbols: one consistent
> stroke weight, geometric construction, round caps and joins, optically
> balanced rather than mathematically centred, and legible at very small sizes
> because that is where it usually appears. Simple enough that you could
> describe it over the phone.
>
> **Ideas worth exploring, not prescriptions**
>
> - A closed book or codex seen from the spine or the fore-edge.
> - A folded or sealed page.
> - Stacked layers, since mods layer over a base game and load order is central
>   to what the app does.
> - A bracket or containment shape holding something apart from a whole, which
>   is what "set aside from the canon" means literally.
> - A monogram A, only if it can avoid looking like a generic app icon.
>
> **What to give me**
>
> For each direction:
>
> 1. The SVG markup, with `stroke="currentColor"`, `fill="none"`, and a square
>    `viewBox`.
> 2. One sentence on the idea behind it.
> 3. One sentence on what it looks like at 16px.
>
> Give me four or five clearly different directions rather than four variations
> of the same idea.

---

## 3. Judging the results

Check each candidate against these before falling in love with one:

| Test | How |
| --- | --- |
| Small size | Render at 16px. Do strokes merge into a blob? |
| Single colour | Set every stroke to one colour. Does it still make sense? |
| Both themes | Put it on `#0B0F0E` and on `#FFFFFF`. |
| Silhouette | Squint. Is the shape distinct from a generic rounded square? |
| Company test | Does it look like fifty other developer tool icons? |
| Name test | Cover the word "Apocrypha". Does the mark still suggest something? |

A mark that fails the 16px test is not salvageable by making it fancier. Simplify
instead.

## 4. Where the files go

Once you have chosen one:

| File | Purpose |
| --- | --- |
| `apps/desktop/src/components/icons.tsx` | Replace the `Logo` component's paths. Keep the props and the `currentColor` stroke. |
| `apps/desktop/src-tauri/icons/icon.png` | 1024x1024 PNG for the packaged application. |
| `docs/images/logo.svg` | The source SVG, for the README and for reuse. |

For the Nexus Mods application request you also need a high resolution square
PNG that reads on a dark background. Export at 1024x1024 with the mark in the
light foreground colour on a transparent or near black field.

The app icon is the one place a flat single colour can look thin. It is
acceptable to set the packaged `icon.png` on a solid or subtly gradient
background using the accent colour, as long as the mark itself stays one colour
on top of it. Everywhere inside the application, the mark stays monoline and
inherits its colour.
