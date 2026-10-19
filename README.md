<p align="center">
  <img src="docs/logo.svg" alt="onsen-matrix: columns of Lotus Sutra characters falling in phosphor green into a hot spring" width="100%">
</p>

**The Matrix the way I remember it: a three-dimensional space you fly through, columns of the Lotus Sutra falling like cherry blossoms, each character glowing like living code.**

https://github.com/user-attachments/assets/ffd5f987-348c-4c74-a494-2e9fdc4314a7

One scene, spanning every display at once. Built with no ceiling: Rust on the Bevy engine, one draw call for the whole rain, 60 fps on three 4K displays at under half of a moderately specced GPU.

## What it is

- **A space, not a wallpaper.** You drift forward through rain that reaches far into the distance, near glyphs large and far ones small.
- **The Lotus Sutra as living code.** The Hōben and Juryō chapters in Kumārajīva's Chinese: 744 characters, each glyph reading on as it fades.
- **Phosphor, not pixels.** Every character glows like a tube: a soft aura, scattered light and a sharp core, under an aperture grille and rolling scanlines.
- **One picture across every display.** All your screens become one canvas with no seam. Choose them by make and model, not by which port they're in.
- **Gone the instant you touch anything.** Any key, click, scroll or pointer movement ends it, and so does the session lock. It also stops itself after half an hour.
- **Light on the machine.** It polls nothing, and the whole rain is a single draw.
- **Tuned in one file.** Density, speed, depth, frame rate and run length all live in one commented config.

## Install

Three dependencies, one command.

    git clone https://github.com/lkingland/onsen-matrix
    cd onsen-matrix
    makepkg -D pkg -si

That gives you `onsen-matrix`, which takes every display, and
`onsen-matrix --outputs 'desc:LG Electronics'` for only the ones you name,
by make and model or connector. It needs Hyprland, a Vulkan driver and the
Noto CJK fonts, and touches nothing else on your system.

## Pairs with Omarchy

It pairs best with [Omarchy](https://omarchy.org): it is the screensaver of
[Onsen](https://github.com/lkingland/onsen), an Omarchy edition, where it
sits in the Super+Esc menu under System. On any Omarchy, a one-line menu
extension puts it there too. The quickest route of all: point your agent at
this repository and ask for it.

## Tune

Copy [`config.toml`](config.toml) to `~/.config/onsen-matrix/` and change
what you like: how dense the rain is, how fast it falls, the frame rate,
how far it reaches, how long a run lasts. Every setting is in there at its
default, explained. On the command line, `--outputs`, `--exclude`, `--fps`
and `--limit` override the file for one run.

## How it works

[ARCHITECTURE.md](ARCHITECTURE.md) is how it works, what every knob does,
and the numbers from a burn-in. The look lives in one shader file that
reloads as you save, so tuning it is editing a constant and looking up.

## License

MIT. The text is the Hōben and Juryō chapters of the Lotus Sutra, in
Kumārajīva's Chinese.
