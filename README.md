# wallpaper-console-rust

Rust rewrite of [wallpaper-console](https://github.com/ZChenW/wallpaper-console) — a terminal wallpaper manager for Arch Linux + niri/Wayland, with both CLI and GUI interfaces.

## Status

**Beta — side-by-side with the Bash/Python version.**  
All core CLI commands, SQLite storage, fzf browsing with preview, and a Wails React GUI are implemented. The original `wallpaper-console` (Bash) and `wallpaper-console-gui` (Python GTK) are not replaced; install uses `-rust` suffixed commands.

## Quick Start

```bash
# Build and install (CLI + GUI) side-by-side
./install.sh

# Try the CLI with a temp config
XDG_CONFIG_HOME=$(mktemp -d) wallpaper-console-rust status

# Launch the GUI
WALLPAPER_CONSOLE_RUST="$HOME/.local/bin/wallpaper-console-rust" wallpaper-console-gui-rust
```

## Features

### Rust CLI (`wallpaper-console-rust`)

| Category | Commands |
|----------|----------|
| Wallpaper | `apply`, `browse` / `browse-images` / `browse-gifs` / `browse-videos`, `random` / `random-image` / `random-gif` / `random-video`, `stop`, `status`, `restore` |
| Sources | `add`, `remove` (fzf), `remove-source`, `sources`, `steam-workshop`, `validate-sources`, `remove-missing`, `dedupe-sources` |
| Favorites | `favorite-add`, `favorite-add-current`, `favorites` (fzf), `favorite-random`, `favorite-remove` |
| History | `history` (fzf), `history-random`, `history-clear` |
| Search / Sort | `search`, `search-source`, `search-type`, `sort-mtime`, `sort-size`, `sort-name` |
| Config | `config-get`, `config-set` |
| Library | `rescan` (incremental, profiled), `library`, `library-count`, `browse-library`, `random-library`, `library-json`, `library-page-json`, `favorites-json`, `history-json` |
| SQLite | `migrate-to-sqlite`, `sqlite-verify`, `sqlite-resync`, `sqlite-export-flat`, `sqlite-backup`, `sqlite-restore`, `sqlite-config-get`, `sqlite-sources-list`, `sqlite-favorites-list`, `sqlite-history-list`, `sqlite-current-read`, `sqlite-last-backend-read` |

All browse/search/sort/favorites/history commands use fzf with image/video preview via `__preview__`.

### Wails GUI (`wallpaper-console-gui-rust`)

- React 19 + TypeScript + Vite
- Virtualized wallpaper grid (@tanstack/react-virtual)
- Library: filter by type, sort, filename search, TSV/SQLite source toggle
- Favorites & History views with apply, random, remove
- Sources management: grouped (Wallpaper Engine / Other), add/remove/validate/scan
- Settings: all backends (awww/mpvpaper), library config, SQLite management, thumbnail cache
- Smart video thumbnails (multi-point frame sampling, 400px scaled, atomic writes)
- Async apply/stop/restore with status bar
- Light theme

### Architecture

```
┌────────────────────────────────┐
│  React 19 + TypeScript         │  Wails GUI or Tauri GUI
│  @tanstack/react-virtual       │
├────────────────────────────────┤
│  Wails v3 Go bridge (thin)     │  OR  Tauri v2 Rust commands
├────────────────────────────────┤
│  Rust CLI / crates             │
│  wc-core  wc-storage  wc-scan  │
│  wc-backend  wc-preview  wc-cli│
├────────────────────────────────┤
│  Runtime files                 │
│  Flat files + SQLite           │
│  $XDG_CONFIG_HOME/wallpaper-console│
└────────────────────────────────┘
```

### Storage

Three modes: `file` (flat files only), `hybrid` (flat + SQLite mirror), `sqlite` (SQLite primary with flat compatibility copy). All modes share the same config directory — no migration needed when switching.

### Performance

- **Incremental rescan**: unchanged files reuse cached metadata (no identify/ffprobe)
- **Source deduplication**: canonical paths eliminate symlink duplicates
- **SQLite batch writes**: single transaction per rescan
- **Virtualized grid**: only visible rows rendered
- **Thumbnail queue**: bounded concurrency, in-flight deduplication

## Build & Verify

```bash
# Rust
cargo build --workspace
cargo test --workspace          # 54/54
cargo clippy --workspace -- -D warnings
cargo fmt --check

# Wails GUI
cd apps/wails-gui
go vet ./...
go build ./...
cd frontend && npm run typecheck && npm run build
cd .. && wails3 build

# Tauri GUI (experimental)
cd apps/tauri-gui/src-tauri
cargo tauri build --bundles deb,rpm
```

## Prerequisites

- Rust 1.77+
- Go 1.26+
- Node.js 22+
- `webkitgtk-6.0` (Wails) / `webkit2gtk-4.1` (Tauri)
- `wails3` CLI (`go install github.com/wailsapp/wails/v3/cmd/wails3@latest`)
- Optional: `ffmpeg`, `imagemagick`, `ffmpegthumbnailer` (thumbnails)
- Optional: `fzf`, `kitty`/`chafa` (CLI browse preview)

## Rollback

The original Bash/Python commands are never modified:

```bash
# Remove the Rust variants
rm ~/.local/bin/wallpaper-console-rust
rm ~/.local/bin/wallpaper-console-gui-rust
```

## License

MIT
