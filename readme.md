# crab-on-desk
A pixel pet for coding agents.

This project is a Linux-focused Rust clone of [rullerzhou-afk/clawd-on-desk](https://github.com/rullerzhou-afk/clawd-on-desk). Thanks to the original project for the ideas, UI layouts, animations, state mappings, etc.

## Installation
### Arch Linux and family
AUR packages are planned but not published yet:
- `crab-on-desk-git`: the widget, the Claude Code hook, the settings window and `crab-codex`.

### From source
Requirements:
- A Wayland compositor that supports `wlr-layer-shell` (for example Hyprland or KDE Plasma).
- OpenGL 4.2 or newer (the textures are BC7 compressed).
- To build: `git`, a Rust toolchain (`cargo`), `cmake`, `clang`, and network access on the first build (a build script downloads KTX-Software).
- Only for the themes of the original project: Electron (any `electron` or `electronNN` package).

Build and install the binaries into `~/.local/bin` (run as root to install into `/usr/local` instead):
```sh
git clone https://github.com/Supernovatux/crab-on-desk.git
cd crab-on-desk
make install
make themes-install # for source themes
```
Make sure `~/.local/bin` is on your `PATH`.

Install at least one theme (see [Themes](#themes)), then start the widget:
```sh
crab-widget
```
On the first start there is no configuration yet, so a setup window opens. Pick a theme, choose whether the pet may roam, and optionally install the Claude Code hooks. Your existing `~/.claude/settings.json` is backed up to `settings.json.crab-on-desk.bak` before it is changed. Press Finish and the pet appears.

Right-click the pet to open the settings.

To start the widget with your session on Hyprland, add this to your config:
```
exec-once = crab-widget
```

To remove everything again:
```sh
make uninstall themes-uninstall
```
Turn the Claude Code hooks off in the settings window first, otherwise Claude will keep calling a `crab-hook` that no longer exists.

## Goals
1. Use minimal RAM and CPU. (Currently uses about 84MB of ram and almost 0 CPU.)
2. Make the widget feature-complete with the original project.

## Features
1. GPU-accelerated rendering.
2. Wayland support.
3. Supports multiple displays.

### Rendering backend~~s~~
1. OpenGL

Vulkan will be added in the future.

### Tested desktop environments
1. Hyprland (note: background blur needs to be disabled manually.)
2. KDE Plasma (Wayland)

Other OSes and X11 could be supported fairly easily by implementing the needed interfaces. Feel free to open a PR.

### Supported agent~~s~~
- Claude Code

I don't use other agents, so please feel free to open a PR.

## Themes
This repository does not contain any themes.

You can use pets from [codex-pets.net](https://codex-pets.net). Install one with its page link or id:
```sh
crab-codex <pet_name>
```
The pets belong to their creators; `crab-codex` only downloads them for your own use.

The themes of the original project are not licensed under an open-source license. You can use the provided scripts to render them yourself from a checkout of rullerzhou-afk/clawd-on-desk:
```sh
make themes-install
```

## AI usage
I wanted to make this widget to get an feel of OpenGL. I wrote all the code up to the point where the first animation worked. After that, all code was written by Claude Opus 5.5. I did monitor the edits partly.

## License
This project is licensed under the GNU AGPL.
