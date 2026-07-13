# Wallpaper Console Rust

Rust/Tauri wallpaper manager for Linux Wayland desktops, with niri in mind.

It provides a GUI for browsing wallpaper folders, scanning Wallpaper Engine
Workshop content, applying wallpapers, and managing sources, favorites,
thumbnails, and backend settings.

## Features

- Images and GIFs via `awww`
- Videos via `mpvpaper`
- Compatible Wallpaper Engine scenes via `linux-wallpaperengine`
- Wallpaper Engine Web projects are indexed for browsing only; live apply is not
  supported
- SQLite storage for the library, favorites, sources, and thumbnails

## Requirements

Install these before running the installer:

- Rust 1.77+
- Node.js 22+ and npm
- `cargo-tauri`
- Tauri 2 Linux system dependencies, including `webkit2gtk-4.1`

Optional runtime helpers:

- `awww` for images/GIFs
- `mpvpaper` for videos
- `linux-wallpaperengine` for Wallpaper Engine scenes
- `ffmpeg`, `imagemagick`, or `ffmpegthumbnailer` for better thumbnails

## Install

```bash
git clone https://github.com/ZChenW/wallpaper-console-rust.git
cd wallpaper-console-rust
./install.sh
wallpaper-console-gui-rust
```

## Niri Example

Startup restore:

```kdl
spawn-at-startup "/home/USER/.local/bin/wallpaper-console-rust" "restore"
```

Launch the GUI:

```kdl
Mod+Shift+0 hotkey-overlay-title="Open Wallpaper Console" {
    spawn "/home/USER/.local/bin/wallpaper-console-gui-rust"
}
```

Open the GUI as floating:

```kdl
window-rule {
    match app-id="wallpaper-console-gui-rust"
    open-floating true
}
```

## License

MIT
