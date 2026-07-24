# Wallpaper Console

A wallpaper manager for Linux Wayland desktops.

Browse local wallpapers and supported Wallpaper Engine projects, organize
favorites, and apply different wallpapers to connected displays.

## Highlights

- Grid and Flow browsing
- Images, GIFs, videos, and compatible Wallpaper Engine scenes
- Multiple wallpaper folders and favorites
- Per-display wallpaper selection
- Optional wallpaper restore after login

Wallpaper Engine Web projects can be browsed, but live Web wallpapers are not
currently supported. Scene rendering may differ from Wallpaper Engine.

## Install on Arch Linux

Install the build requirements:

```bash
sudo pacman -Syu
sudo pacman -S --needed \
  webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg xdotool zenity \
  rust nodejs npm git
```

Building requires Rust 1.88 or newer and Node.js 22.6 or newer. `zenity` provides
the folder picker; `kdialog` or `yad` can be used instead.

Download and install Wallpaper Console:

```bash
git clone https://github.com/ZChenW/wallpaper-console-rust.git
cd wallpaper-console-rust
./install.sh
```

Open **Wallpaper Console** from the application menu, or run:

```bash
wallpaper-console-gui-rust
```

The installer uses `~/.local` by default and does not require root access.
It publishes complete files atomically and records their hashes in an ownership
manifest. If files from an older untracked installation conflict, review or
move them first, or explicitly replace and adopt them with:

```bash
./install.sh --force
```

On another Linux distribution, install the
[Tauri 2 system requirements](https://v2.tauri.app/start/prerequisites/) for
your distribution and one of `zenity`, `kdialog`, or `yad`, then use the same
download and install commands.

## Wallpaper support

Wallpaper Console uses an external renderer when applying a wallpaper:

- `awww` for images and GIFs
- `mpvpaper` for videos
- `linux-wallpaperengine` for compatible Wallpaper Engine scenes

You only need the renderers for the wallpaper types you use.

If the GUI opens to a blank window on a system with WebKitGTK rendering issues,
start it once with:

```bash
WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1 wallpaper-console-gui-rust
```

## Update

```bash
cd wallpaper-console-rust
git pull --ff-only
./install.sh
```

## Uninstall

From the cloned project directory:

```bash
./install.sh --uninstall
```

Your wallpaper library and settings are kept in place.
Uninstall removes only files still matching the ownership manifest. Locally
modified installed files are preserved with a warning.

## Restore after login

Enable login restoration:

```bash
wallpaper-console-rust config-set restore_on_login on
```

Then add this command to your compositor's startup configuration:

```kdl
spawn-at-startup "/home/USER/.local/bin/wallpaper-console-rust" "restore-at-login"
```

Replace `USER` with your Linux username. Compositor startup commands do not
always expand `~`, so use an absolute path.

## License

[MIT](LICENSE)
