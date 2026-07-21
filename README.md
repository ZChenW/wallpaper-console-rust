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
- Node.js 22.6+ and npm (see `apps/tauri-gui/frontend/package.json` `engines`)
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

Login restore is opt-in. Enable it once, then point the compositor startup hook
at the guarded command:

```bash
wallpaper-console-rust config-set restore_on_login on
```

```kdl
spawn-at-startup "/home/USER/.local/bin/wallpaper-console-rust" "restore-at-login"
```

Set `restore_on_login` back to `off` to disable login restoration without
editing the compositor configuration. Manual `restore` and `restore-displays`
commands remain unconditional.

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

## Verification

Correctness gate (Rust fmt/check/clippy/test, frontend typecheck including tests,
unit tests, production build, mock-browser smoke, and drift checks):

```bash
export TMPDIR="${TMPDIR:-$HOME/tmp/rust-tmp}"
mkdir -p "$TMPDIR"
cargo run -p xtask -- verify all
```

Library wall-clock performance budgets are intentionally separate:

```bash
cargo run -p xtask -- verify perf
```

### Real Tauri WebView E2E (not automated)

Automated smoke currently uses mock Vite + Playwright Chromium
(`npm run smoke`). A minimal real Tauri WebView smoke is **not** wired yet
because:

1. Headless runners lack a reliable Wayland/X11 + `webkit2gtk-4.1` WebView path for
   Tauri 2 without a dedicated display server and GPU/software GL setup.
2. Apply/backend lifecycle (`awww` / `mpvpaper` / `linux-wallpaperengine`) needs
   compositor access that typical CI hosts do not provide.
3. There is no checked-in WebDriver/sidecar harness for the packaged GUI binary.

Until those blockers are removed, treat mock-browser smoke + Rust unit/integration
tests (`cargo run -p xtask -- verify all`) as the automated gate, and use a local
interactive GUI run for native apply/runtime acceptance.

## License

MIT
