# Apocrypha: context for a designer

Background for anyone designing the mark. This is about the product and the
feeling, not the specification. The hard constraints are in
[logo-brief.md](./logo-brief.md), and the wider visual system is in
[design-system.md](./design-system.md).

---

## What it does

Apocrypha installs and removes modifications for PC games on Linux.

Game mods are unofficial add-ons: new armour, retextured monsters, interface
tweaks, quality of life fixes. Installing one means copying files into the game's
own folder. Doing that by hand is easy to get wrong and very hard to undo, and
when a game update lands or a mod misbehaves, people often cannot remember what
they changed. The usual outcome is deleting the whole game and downloading a
hundred gigabytes again.

Apocrypha keeps a record of every file it touches. Before it changes anything it
shows you exactly what will change. If you want it gone, it puts the original
files back, one at a time, and refuses to delete anything it did not put there
itself. That is the entire product in one sentence: **it makes changing your game
safely reversible.**

## Why it exists

Almost every mod manager is a Windows program. Linux players either run them
through a compatibility layer and fight it, or they give up and copy files by
hand. The most respected manager, Mod Organizer 2, cannot be ported at all: it is
built on a Windows-only technique that has no Linux equivalent.

So this is not a port. It is built for Linux from the start, by someone who was
tired of the alternative.

## Who uses it

People comfortable with a computer, modding a game they have put a lot of hours
into, and nervous about breaking it. They are not beginners, but they are not
looking for a hobby in itself either. They want to change their game and then go
play it.

They run it alongside a terminal, a browser with a mod site open, and the game.
It should not look out of place in that company.

## What the name means

Apocrypha means hidden or set-aside writings: texts left outside the accepted
canon. Not forbidden exactly, just not official.

That is a quiet joke about what mods are. Every mod is unofficial content sitting
next to the real thing. The name is the most interesting asset the project has,
and the mark should draw on it rather than on gaming imagery.

The tone to take from it is a **library, an archive, a record**. Not the occult.
The name attracts pentagrams and mystical eyes, and those would be wrong: this is
a careful tool that keeps meticulous notes, not a horror aesthetic.

## How it feels to use

Quiet and deliberate. Dark by default, because people mod games at night.
Everything reversible, and the interface says so constantly: there is always a
preview before anything is applied, and an undo afterwards. Nothing celebrates,
nothing animates for personality, nothing shouts.

Three words: **precise, reversible, unshowy.**

## Where the mark will appear

This matters more than anything aesthetic, because it rules designs out:

| Place | Size |
| --- | --- |
| Application title bar, beside the word Apocrypha | 18px |
| Startup screen | 48px |
| Desktop icon, app menu, dock | 32 to 256px |
| Repository and website | any |
| Favicon | 16px |

**Most of the time it will be small.** An 18px title bar mark is the everyday
case. Anything that needs a large canvas to make sense has failed.

## What it sits next to

- **Type:** SF Pro Display, the typeface Apple ships. Set tight, medium weight.
- **Colour:** one accent, a deep desaturated green, roughly `#4CA987`. Not a
  bright or neon green. Everything else is near black or near white.
- **Icons:** monoline, single weight, round caps, no fills. The reference is
  Apple's SF Symbols.
- **Interface:** modelled on macOS System Settings. Quiet surfaces, generous
  spacing, one accent colour, hierarchy from typography rather than boxes.

The mark should look like it was drawn by the same hand as those icons, just with
a little more character.

## Neighbours, for tone

The right company is developer tooling: version control clients, terminal
emulators, code editors. Marks that are simple, confident, and do not try to be
exciting.

The wrong company is game storefronts and launchers: heavy 3D lettering, bevels,
neon, aggressive angles, anything that looks like it belongs on a gaming mouse.

## Things to avoid

Learned from looking at what everyone else in this space does:

- Controllers, dice, swords, shields, health bars.
- Gears, wrenches, screwdrivers, sliders. Every tool uses these.
- Puzzle pieces, the standard cliché for "add-on".
- Boxes with arrows going into them, the standard cliché for "install".
- Pentagrams, hooded figures, glowing eyes, anything ritual.
- Gradients, drop shadows, bevels, and multi-colour schemes.

## Practical

- **Format:** SVG. Strokes, not filled shapes, so it can inherit one colour.
- **Colour:** exactly one, inherited from context. It must read on near black and
  on white without modification.
- **Canvas:** square, with even optical margin.
- **Deliverables:** the SVG, plus a 1024px square PNG for packaging and for a
  registration request that requires a logo legible on a dark background.

## Links

- Repository: https://github.com/Ali-AbdulHadii/apocrypha
- Licence: MIT, free and open source
- Status: early development, first release not yet cut
