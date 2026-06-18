# wallpaper-console-rust

Rust/Tauri desktop wallpaper manager for Arch Linux + niri/Wayland.

## Status

**Beta — GUI-first Rust/Tauri app.**
The supported user entrypoint is `wallpaper-console-gui-rust`. The Rust CLI crate remains in the repository as a diagnostic and regression-test tool, but it is not installed by default.

## Quick Start

```bash
./install.sh
wallpaper-console-gui-rust

# Build only
./install.sh --build-only

# Raw Tauri binary after build
./target/release/wallpaper-console-tauri
```

## Features

### React GUI (`wallpaper-console-gui-rust`, Tauri 2 app)

- React 19 + TypeScript + Vite
- Virtualized wallpaper grid (@tanstack/react-virtual)
- Library: SQLite-backed SQL paging, filter by type, sort, filename/title/Workshop ID search
- Favorites & History views with apply, random, remove
- Sources management: grouped (Wallpaper Engine / Other), add/remove/validate/scan
- Wallpaper Engine project indexing. Scene wallpapers use optional `linux-wallpaperengine`; Web wallpapers are indexed for browsing/preview only and are not live-apply supported.
- Settings: all backends (awww/mpvpaper/linux-wallpaperengine), library config, SQLite management, thumbnail cache
- Smart video thumbnails (multi-point frame sampling, 400px scaled, atomic writes, short-lived failure cache)
- Async apply/stop/restore with status bar
- Scan progress and cancellation with single-scan guard
- Structured GUI command errors, optional debug logging, and developer performance overlay
- Installable through the binary-copy `install.sh` path.

> Developer note: `crates/wc-cli` is retained for diagnostics and parity tests, but the supported product interface is the Tauri GUI.

### Architecture

```
┌──────────────────────────────┐
│  React 19 + TypeScript       │
│  @tanstack/react-virtual      │
├──────────────────────────────┤
│  Tauri v2 Rust commands       │
│  Direct crate calls (no CLI) │
├──────────────────────────────┤
│  wc-app service layer         │
│  apply / inspect / errors     │
├──────────────────────────────┤
│  Rust crates                  │
│  wc-core  wc-storage  wc-scan│
│  wc-backend  wc-preview       │
├──────────────────────────────┤
│  Runtime files + SQLite       │
│  $XDG_CONFIG_HOME/wallpaper-console│
└──────────────────────────────┘
```

### Storage

SQLite is the only runtime storage backend. Legacy flat files in the config directory are imported into SQLite when needed, and flat-file export remains available as an explicit maintenance action.

### Performance

- **Incremental rescan**: unchanged files reuse cached metadata (no identify/ffprobe)
- **Source deduplication**: canonical paths eliminate symlink duplicates
- **Streaming rescan writes**: CLI rescan scans, probes metadata, writes TSV, and stages SQLite rows in bounded batches
- **Atomic SQLite library replacement**: scan writes either fully commit or preserve the old library
- **SQLite query indexes + FTS5 search**: indexed type/sort paths and FTS-backed path/title/workshop search used by GUI and CLI paged loading
- **Virtualized grid**: only visible rows rendered
- **Thumbnail queue**: bounded concurrency, visible-item priority, stale-request cancellation, batched UI updates
- **Tauri heavy commands**: scanning and thumbnail generation run on blocking worker threads

## Build & Verify

```bash
# Full local verification runner
cargo run -p xtask -- verify all

# Drift verification
# Checks for stale runtime/config references (removed APIs, migrated wording,
# retired schema options). Runs scripts/check_runtime_config_drift.sh via rg.
cargo run -p xtask -- verify drift

# Rust
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check

# Tauri frontend
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npm run build
npm run smoke

# Tauri build
cd ../src-tauri
cargo build --package wallpaper-console-tauri --release

# Install path verification
cd ../../..
./install.sh --build-only
./scripts/test_install_build_only.sh
```

See [docs/TAURI_MANUAL_SMOKE_CHECKLIST.md](docs/TAURI_MANUAL_SMOKE_CHECKLIST.md) for manual GUI verification.
See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for full development setup.
See [docs/PERFORMANCE_BASELINE.md](docs/PERFORMANCE_BASELINE.md) for repeatable library benchmark commands and current baseline numbers.

Package-manager installs such as AUR are future work; the supported local install path is the binary-copy installer.

## Prerequisites

- Rust 1.77+
- Node.js 22+
- `webkit2gtk-4.1` (Tauri 2)
- Optional: `ffmpeg`, `imagemagick`, `ffmpegthumbnailer` (thumbnails)
- Optional: `linux-wallpaperengine` for Wallpaper Engine scene wallpapers
  - Arch/AUR: `yay -S linux-wallpaperengine-git`

> **Historical note:** The project previously supported a Wails v3 + Go bridge GUI. That implementation has been retired in favor of Tauri 2. See git history for the archived docs.

## Rollback

The original Bash/Python commands are never modified:

```bash
# Remove files installed by this script
./install.sh --prefix "$HOME/.local" --uninstall
```

## License

MIT
