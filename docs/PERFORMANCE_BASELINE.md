# Performance Baseline

Date: 2026-06-10

## Environment

- OS: Arch Linux
- Compositor: niri Wayland
- GUI binary: `~/.local/bin/wallpaper-console-gui-rust`
- Config dir: `$XDG_CONFIG_HOME/wallpaper-console` or `$HOME/.config/wallpaper-console`

## Current Config

```text
storage_backend=sqlite
gui_library_source=tsv
gui_thumbnail_mode=cache
library-count=33 total / 20 images / 0 gifs / 13 videos
```

## Baseline Observations

The installed GUI was not launched by Codex during this phase because opening a GUI app is an interactive compositor operation. The profiling script has been added so the next implementation agent or user can run the same measurement consistently:

```bash
./scripts/profile_gui.sh 45 | tee /tmp/wallpaper-console-gui-baseline.csv
```

Static code audit shows the primary expected hot paths:

- `apps/wails-gui/frontend/src/components/WallpaperGrid.tsx` requests thumbnails for up to `PAGE_SIZE=100` visible entries immediately.
- Each thumbnail response calls `setThumbCache`, causing many separate React renders.
- `apps/wails-gui/rust.go` generates thumbnails on demand and may spawn `magick`, `convert`, and `ffmpeg`.
- `apps/wails-gui/rust.go` shells out to `wallpaper-console-rust` for normal GUI operations.

## Root Cause Notes

- WebKitGTK baseline: expected non-trivial RSS for any Wails app.
- Rust CLI subprocess count: current Wails bridge uses subprocess calls for normal operations.
- Thumbnail generator process count: currently unbounded at frontend and backend layers.
- Peak RSS: measure with `scripts/profile_gui.sh`.
- Peak CPU: measure with `scripts/profile_gui.sh`.
