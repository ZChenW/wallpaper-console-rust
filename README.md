# wallpaper-console-rust

Rust/Tauri wallpaper manager for Arch Linux, Wayland, and niri.

Status: **beta, GUI-first**. The supported user command is
`wallpaper-console-gui-rust`. The Rust CLI crate stays in the workspace for
diagnostics and tests, but it is not installed by default.

## What It Does

- Browse wallpapers in a fast virtualized React grid.
- Scan local folders and Wallpaper Engine Workshop folders.
- Apply images/GIFs with `awww`, videos with `mpvpaper`, and compatible
  Wallpaper Engine scenes with `linux-wallpaperengine`.
- Index Wallpaper Engine Web projects for browsing and preview only; live apply
  is not supported for Web projects.
- Manage sources, favorites, history, thumbnails, backend settings, SQLite
  maintenance, and privacy-safe diagnostics from the GUI.

Runtime storage is SQLite-only. Legacy flat files can still be imported into
SQLite, and explicit flat export remains available as a maintenance action.

## Install

Prerequisites:

- Rust 1.77+
- Node.js 22+
- `webkit2gtk-4.1` for Tauri 2
- Optional thumbnail helpers: `ffmpeg`, `imagemagick`, `ffmpegthumbnailer`
- Optional scene backend: `linux-wallpaperengine`

Build and install:

```bash
./install.sh
wallpaper-console-gui-rust
```

Installed commands:

- `wallpaper-console-gui-rust` opens the Tauri GUI.
- `wallpaper-console-rust restore` restores the last wallpaper from startup
  hooks or scripts.

Build without installing:

```bash
./install.sh --build-only
./target/release/wallpaper-console-tauri
```

Install to another prefix:

```bash
./install.sh --prefix "$HOME/.local"
```

Uninstall files created by this installer:

```bash
./install.sh --prefix "$HOME/.local" --uninstall
```

The installer does not modify the older Bash/Python commands:
`wallpaper-console` and `wallpaper-console-gui`.

## Niri (Mine)

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

