# wallpaper-console-rust

Rust rewrite of [wallpaper-console](https://github.com/ZChenW/wallpaper-console) — a terminal wallpaper manager for Arch Linux + niri/Wayland, with both CLI and GUI interfaces.

## Status

**Beta — side-by-side with the Bash/Python version.**  
All core CLI commands, SQLite storage, fzf browsing with preview, and React GUI (Tauri 2) are implemented. The original `wallpaper-console` (Bash) and `wallpaper-console-gui` (Python GTK) are not replaced; install uses `-rust` suffixed commands.

## Quick Start

```bash
# Build and install (CLI + GUI) side-by-side
./install.sh

# Custom prefix / uninstall
./install.sh --prefix "$HOME/.local"
./install.sh --prefix "$HOME/.local" --uninstall

# Installed commands:
#   wallpaper-console-rust          (Rust CLI)
#   wallpaper-console-gui-rust      (Tauri GUI)

# Try the CLI with a temp config
XDG_CONFIG_HOME=$(mktemp -d) wallpaper-console-rust status

# Raw Tauri binary (before install)
./target/release/wallpaper-console-tauri
```

## Features

### Rust CLI (`wallpaper-console-rust`)

| Category | Commands |
|----------|----------|
| Wallpaper | `apply`, `browse` / `browse-all` / `browse-images` / `browse-gifs` / `browse-videos`, `random` / `random-all` / `random-image` / `random-gif` / `random-video`, `stop`, `status`, `restore`, `inspect` |
| Sources | `add`, `remove` (fzf), `remove-source`, `sources`, `steam-workshop`, `validate-sources`, `remove-missing`, `dedupe-sources` |
| Favorites | `favorite-add`, `favorite-add-current`, `favorites` (fzf), `favorite-random`, `favorite-remove` |
| History | `history` (fzf), `history-random`, `history-clear` |
| Search / Sort | `search`, `search-source`, `search-type`, `sort-mtime`, `sort-size`, `sort-name` |
| Config | `config-get`, `config-set` |
| System | `tui` (stub), `help` |
| Library | `rescan` (incremental, profiled), `library`, `library-count`, `browse-library`, `random-library`, `library-json`, `library-page-json`, `favorites-json`, `history-json` |
| SQLite | `migrate-to-sqlite`, `sqlite-verify`, `sqlite-resync`, `sqlite-export-flat`, `sqlite-backup`, `sqlite-restore`, `sqlite-config-get`, `sqlite-sources-list`, `sqlite-favorites-list`, `sqlite-history-list`, `sqlite-current-read`, `sqlite-last-backend-read` |

All browse/search/sort/favorites/history commands use fzf with image/video preview via `__preview__`.

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
- Installable as `.deb`/`.rpm` bundle

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

Three modes: `file` (flat files only), `hybrid` (flat + SQLite mirror), `sqlite` (SQLite primary with flat compatibility copy). All modes share the same config directory — no migration needed when switching.

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

# Tauri bundle
cd ../src-tauri
cargo tauri build --bundles deb,rpm

# Install path verification
cd ../../..
./install.sh --build-only
./scripts/test_install_build_only.sh
```

See [docs/TAURI_MANUAL_SMOKE_CHECKLIST.md](docs/TAURI_MANUAL_SMOKE_CHECKLIST.md) for manual GUI verification.
See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for full development setup.
See [docs/PERFORMANCE_BASELINE.md](docs/PERFORMANCE_BASELINE.md) for repeatable library benchmark commands and current baseline numbers.

## Prerequisites

- Rust 1.77+
- Node.js 22+
- `webkit2gtk-4.1` (Tauri 2)
- Optional: `ffmpeg`, `imagemagick`, `ffmpegthumbnailer` (thumbnails)
- Optional: `fzf`, `kitty`/`chafa` (CLI browse preview)
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
