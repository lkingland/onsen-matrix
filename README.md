# onsen-matrix

https://github.com/user-attachments/assets/ffd5f987-348c-4c74-a494-2e9fdc4314a7


The Matrix the way I remember it: a three-dimensional space you fly
through, columns of the Lotus Sutra falling like cherry blossoms, each character
glowing like living code. One scene, spanning every display simultaneously.

Built with no ceiling: Rust on the Bevy engine, one draw call for the whole
rain, 60 fps on three 4K displays at under half of a moderately specced GPU.

## Install

Three dependencies, one command.

    git clone https://github.com/lkingland/onsen-matrix
    cd onsen-matrix
    makepkg -D pkg -si

That gives you `onsen-matrix`, which takes every display, and
`onsen-matrix --outputs 'desc:LG Electronics'` for only the ones you name,
by make and model or connector. It needs Hyprland, a Vulkan driver and the
Noto CJK fonts, and touches nothing else on your system.

It pairs best with [Omarchy](https://omarchy.org): it is the screensaver of
Onsen, an Omarchy edition, where it sits in the Super+Esc menu under
System. On any Omarchy, a one-line menu extension puts it there too. The
quickest route of all: point your agent at this repository and ask for it.

## Tune

Copy [`config.toml`](config.toml) to `~/.config/onsen-matrix/` and change
what you like: how dense the rain is, how fast it falls, the frame rate,
how far it reaches, how long a run lasts. Every setting is in there at its
default, explained.

## More

[ARCHITECTURE.md](ARCHITECTURE.md) is how it works, what every knob does,
and the numbers from a burn-in. The look lives in one shader file that
reloads as you save, so tuning it is editing a constant and looking up.

MIT. The text is the Hōben and Juryō chapters of the Lotus Sutra, in
Kumārajīva's Chinese.
