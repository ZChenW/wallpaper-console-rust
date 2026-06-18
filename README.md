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

## Development

Full verification:

```bash
cargo run -p xtask -- verify all
```

Rust only:

```bash
cargo run -p xtask -- verify rust
```

Frontend only:

```bash
cargo run -p xtask -- verify frontend
```

Runtime/config drift checks:

```bash
cargo run -p xtask -- verify drift
```

Install-path verification:

```bash
./install.sh --build-only
./scripts/test_install_build_only.sh
```

Manual frontend commands:

```bash
cd apps/tauri-gui/frontend
npm install
npm run typecheck
npm run test:unit
npm run build
npm run smoke
```

## Architecture

```text
React 19 + TypeScript + Vite
        |
Tauri 2 Rust commands
        |
wc-app service layer
        |
wc-core / wc-storage / wc-scan / wc-backend / wc-preview
        |
$XDG_CONFIG_HOME/wallpaper-console + SQLite + thumbnail cache
```

Important boundaries:

- GUI commands call Rust crates directly; they do not shell out to the CLI.
- `wc-app` owns shared apply decisions and user-facing error mapping.
- `wc-storage` is SQLite-only at runtime.
- Heavy filesystem, scan, thumbnail, SQLite, and backend work runs off the
  WebView thread.
- `xtask` is the shared local and CI verification entrypoint.

## Runtime Notes

- Library paging, search, favorites, and history use SQLite helpers.
- Scans reuse cached metadata for unchanged files and replace the SQLite library
  atomically.
- SQLite runtime connections use WAL and a bounded busy timeout.
- Thumbnail generation uses bounded concurrency, visible-item priority, atomic
  writes, and a short-lived failure cache.
- Settings can export a privacy-safe diagnostics file. Full paths are redacted
  from diagnostic fields; only the exported file location itself is returned.

## Repository Layout

```text
apps/tauri-gui/        Tauri app and React frontend
crates/wc-app/         Shared application service layer
crates/wc-backend/     Wallpaper backend lifecycle and process control
crates/wc-cli/         Diagnostic CLI and parity tests
crates/wc-core/        Config, errors, shared types
crates/wc-preview/     Thumbnail generation and cache logic
crates/wc-scan/        Source scanning and Wallpaper Engine indexing
crates/wc-storage/     SQLite storage and legacy import/export
scripts/               Local verification and helper scripts
xtask/                 Unified verification runner
```

## License

MIT
