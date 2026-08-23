# Wallpaper Console

A wallpaper manager for Linux Wayland and Xorg desktops.

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

## Install the Linux release candidate

Download these three files from the
[latest GitHub release](https://github.com/ZChenW/wallpaper-console-rust/releases):

- `wallpaper-console_0.1.0-rc.2_x86_64.AppImage`
- `wallpaper-console-cli_0.1.0-rc.2_x86_64.tar.zst`
- `SHA256SUMS`

Verify both application assets before running them:

```bash
sha256sum -c SHA256SUMS
chmod +x wallpaper-console_0.1.0-rc.2_x86_64.AppImage
./wallpaper-console_0.1.0-rc.2_x86_64.AppImage
```

The AppImage contains the GUI only. Install the separate CLI bundle for login
restore, command-line configuration, and terminal library commands:

```bash
tar --zstd -xf wallpaper-console-cli_0.1.0-rc.2_x86_64.tar.zst
install -Dm755 \
  wallpaper-console-cli_0.1.0-rc.2_x86_64/wallpaper-console-rust \
  "$HOME/.local/bin/wallpaper-console-rust"
```

This release supports Linux x86_64. External wallpaper renderers are not bundled;
install the renderer needed for your desktop and wallpaper types as described
below.

## Build from source on Arch Linux

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

- `awww` for images and GIFs on Wayland
- `swaybg` for static images on Wayland
- `feh` for static images on Xorg (All Displays only)
- `mpvpaper` for images, GIFs, and videos on Wayland
- `linux-wallpaperengine` for compatible Wallpaper Engine scenes

You only need the renderers for the wallpaper types you use.
On Arch Linux, install the two optional static-image backends with:

```bash
sudo pacman -S --needed swaybg feh
```

The GUI enables only renderers compatible with the current session. swaybg
supports named Wayland outputs; feh updates the X root wallpaper and therefore
does not offer named-display targeting.

If the GUI opens to a blank window on a system with WebKitGTK rendering issues,
start it once with the matching command:

```bash
# AppImage release
WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1 ./wallpaper-console_0.1.0-rc.2_x86_64.AppImage

# Source installation
WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1 wallpaper-console-gui-rust
```

## Update

The release candidate has no automatic updater. For an AppImage/CLI installation,
download the newer assets and `SHA256SUMS`, verify them, then replace the old
files manually.

For a source installation:

```bash
cd wallpaper-console-rust
git pull --ff-only
./install.sh
```

## Uninstall

For a release asset installation, delete the AppImage and the optional CLI:

```bash
rm -f wallpaper-console_0.1.0-rc.2_x86_64.AppImage
rm -f "$HOME/.local/bin/wallpaper-console-rust"
```

For a source installation, run this from the cloned project directory:

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

## Post-apply theme hook

After a successful wallpaper apply, Wallpaper Console can run an external
command to sync Material You colors (similar to waypaper's
`post_command = matugen image $wallpaper`). Videos are handled by extracting a
still frame with `ffmpeg` first. Wallpaper Engine scenes use the safe project
preview image declared by `project.json`; Web and Application projects are
skipped.

Enable:

```bash
wallpaper-console-rust config-set post_apply_enabled on
```

Defaults:

| Key | Default |
|-----|---------|
| `post_apply_enabled` | `off` |
| `post_apply_command` | `matugen image "$still"` |
| `post_apply_timeout_secs` | `30` |

Placeholders in `post_apply_command`: `$wallpaper` / `$path`, `$still`,
`$backend`, `$outputs`. The same values are exported as `WCR_WALLPAPER`,
`WCR_STILL`, `WCR_BACKEND`, and `WCR_OUTPUTS`.

You need a matugen config (see [`examples/matugen/`](examples/matugen/)), or
point at an existing template set. Optional kitty or Waybar reloads can be added
directly to your configured post-apply command.

## License

[MIT](LICENSE)
