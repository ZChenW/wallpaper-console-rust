# Wallpaper Console

A Linux wallpaper manager for Wayland and Xorg. Browse local wallpapers and
supported Wallpaper Engine projects, manage favorites, and apply wallpapers per
display.

## Features

- Grid and Flow browsing
- Images, GIFs, videos, and compatible Wallpaper Engine scenes
- Multiple folders, favorites, and per-display selection
- Optional wallpaper restore after login
- Optional post-apply command for tools such as matugen

Wallpaper Engine Web projects can be browsed, but live Web wallpapers are not
supported. Scene rendering may differ from the original Wallpaper Engine output.

## Install

Download the AppImage, optional CLI bundle, and `SHA256SUMS` from the
[latest release](https://github.com/ZChenW/wallpaper-console-rust/releases/latest).

```bash
sha256sum -c SHA256SUMS
chmod +x wallpaper-console_0.1.0_x86_64.AppImage
./wallpaper-console_0.1.0_x86_64.AppImage
```

The AppImage contains the GUI. Install the CLI for login restoration and terminal
commands:

```bash
tar --zstd -xf wallpaper-console-cli_0.1.0_x86_64.tar.zst
install -Dm755 \
  wallpaper-console-cli_0.1.0_x86_64/wallpaper-console-rust \
  "$HOME/.local/bin/wallpaper-console-rust"
```

The release supports Linux x86_64. Wallpaper renderers are separate host tools;
install only those needed for your desktop and media:

- `awww` — Wayland images and GIFs
- `swaybg` — Wayland static images
- `feh` — Xorg static images
- `mpvpaper` — Wayland images, GIFs, and videos
- `linux-wallpaperengine` — compatible Wallpaper Engine scenes

## Build from source

Building requires Rust 1.88+, Node.js 22.6+, the
[Tauri 2 system dependencies](https://v2.tauri.app/start/prerequisites/), and a
folder picker such as `zenity`, `kdialog`, or `yad`.

```bash
git clone https://github.com/ZChenW/wallpaper-console-rust.git
cd wallpaper-console-rust
./install.sh
```

The installer uses `~/.local` by default. Launch it from the application menu or
run `wallpaper-console-gui-rust`.

## Optional automation

Restore the previous wallpaper after login:

```bash
wallpaper-console-rust config-set restore_on_login on
```

Then run `wallpaper-console-rust restore-at-login` from your compositor's
startup configuration.

Enable the post-apply hook:

```bash
wallpaper-console-rust config-set post_apply_enabled on
```

Its default command is `matugen image "$still"`. Configure
`post_apply_command` to integrate another theme tool.

## Troubleshooting

If the GUI opens as a blank window because of WebKitGTK rendering issues, try:

```bash
WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1 ./wallpaper-console_0.1.0_x86_64.AppImage
```

There is no automatic updater. Download and verify newer release assets before
replacing existing files.

For a source installation, update or uninstall with:

```bash
git pull --ff-only && ./install.sh
./install.sh --uninstall
```

Settings and the wallpaper library are preserved when uninstalling.

## License

[MIT](LICENSE)
