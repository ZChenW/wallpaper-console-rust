# Migration Status

> Auto-generated as part of Phase 0 baseline. Updated: 2026-06-10.

## Phase Completion

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Freeze and Baseline | ✅ Complete |
| 1 | Rust CLI Preview Readiness (__preview__ + fzf) | ✅ Complete |
| 2 | Wails App Shell (Go backend + bridge) | ✅ Complete |
| 3 | React UI Foundation (Vite + TS + layout) | ✅ Complete |
| 4 | Library Grid (virtual scroll via @tanstack/react-virtual, filter, sort, search) | ✅ Complete |
| 5 | Thumbnail Strategy (useThumbnailQueue, Go pool, lazy loading, dedup) | ✅ Complete |
| 6 | Favorites & History (grid, random, clear confirm) | ✅ Complete |
| 7 | Sources View (grouped, add/remove/scan) | ✅ Complete |
| 8 | Settings View (backends, library, storage/SQLite) | ✅ Complete |
| 9 | Apply/Stop/Restore UX (status bar, async apply) | ✅ Complete |
| 10 | Packaging and Install | ✅ Complete (side-by-side Rust CLI + Wails GUI install) |
| 11 | niri Integration | ⬜ Pending (needs real-use validation) |
| 12 | Deprecate Bash/Python | ⬜ Deferred (needs real-use validation) |

## Current Migration Verdict

The Rust implementation is now a usable side-by-side replacement candidate for the Bash/Python
implementation, with the Rust CLI and Wails GUI as the supported path.

Do not replace the original `wallpaper-console` / `wallpaper-console-gui` commands by default yet.
Keep using the `-rust` command names until real-use validation passes for niri startup, source
management, image/video apply, favorites/history, and SQLite maintenance.

## Baseline Checks

| Check | Status |
|-------|--------|
| `cargo fmt --check` | ✅ passes |
| `cargo clippy --workspace -- -D warnings` | ✅ clean |
| `cargo test --workspace` | ✅ 54/54 (35 wc-cli integration + 7 wc-core + 12 wc-scan) |
| Smoke: `status` with temp XDG_CONFIG_HOME | ✅ works |
| Smoke: `add` + `rescan` + `library-count` | ✅ works |

## Frontend Build

| Check | Status |
|-------|--------|
| `npm run typecheck` | ✅ passes |
| `npm run build` | ✅ passes |

## Wails Build

| Check | Status |
|-------|--------|
| `go vet ./...` | ✅ passes |
| `go build ./...` | ✅ passes |
| `PATH="$HOME/go/bin:$PATH" wails3 build` | ✅ passes → `apps/wails-gui/bin/wallpaper-console-gui` |
| `wails3` CLI | ✅ installed at `~/go/bin/wails3` (v3.0.0-alpha.98) |

## Tauri Build (experimental)

| Check | Status |
|-------|--------|
| `cargo check` (src-tauri) | ✅ passes |
| `cargo tauri build --bundles deb,rpm` | ✅ passes → .deb + .rpm bundles |
| `webkit2gtk-4.1` | ✅ installed (v2.52.4) alongside `webkitgtk-6.0` |
| Asset protocol (local thumbnail loading) | ✅ enabled (`protocol-asset` feature, scope configured) |

Tauri v2 compiles and produces `.deb`/`.rpm` bundles. It calls Rust crates directly (no subprocess bridge).
The asset protocol has been configured to allow loading thumbnails from the local cache directory.
Tauri is not yet the default GUI; Wails remains the stable/supported target until Tauri passes full
real-use smoke testing.

## Installation

See [install.sh](../install.sh) in the repository root.

Quick install (side-by-side with existing Bash/Python versions):
```bash
./install.sh
```

This installs to:
- `~/.local/bin/wallpaper-console-rust` (Rust CLI)
- `~/.local/bin/wallpaper-console-gui-rust` (Wails GUI)

**Rollback** (restore original Bash/Python):
```bash
rm ~/.local/bin/wallpaper-console-rust
rm ~/.local/bin/wallpaper-console-gui-rust
```

The original Bash/Python install is never removed.

## CLI Command Parity

See [COMMAND_PARITY.md](COMMAND_PARITY.md) for the full checklist.

- **Completed:** all non-TUI commands (50+), `__preview__`, all SQLite debug commands, Flatpak Steam paths
- **Remaining:** `tui` (stub — Wails/Tauri GUI replaces the old control TUI unless a Rust terminal TUI is explicitly requested)

## GUI Architecture Decision

The production GUI path is currently **Wails v3 + React + Rust CLI bridge**, with the following
performance optimizations implemented:

- Paginated library loading through `library-page-json`
- SQLite-backed library pages when `storage_backend=sqlite`
- Frontend thumbnail queue with concurrency 2
- Video thumbnail v2: multi-point smart frame sampling (25%/50%/10%/5s/75%), 400px WebP output, atomic `.tmp.webp` writes
- ThumbnailFor delegated to Rust CLI `thumbnail` subcommand (Go no longer spawns magick/ffmpeg directly)
- Virtualized grid via `@tanstack/react-virtual` with `useVirtualizer`
- Incremental rescan: unchanged files reuse cached metadata (no identify/ffprobe)
- Source deduplication and SQLite batch write transactions

An experimental Tauri v2 app exists under `apps/tauri-gui/`. It builds successfully and produces
`.deb`/`.rpm` bundles. Tauri commands call Rust crates directly (no subprocess bridge). The Tauri
build requires `webkit2gtk-4.1` which is installed alongside `webkitgtk-6.0`. The asset protocol
is configured (`protocol-asset` feature + scope) for local thumbnail loading via `convertFileSrc()`.

## Known CLI Gaps

| Gap | Severity | Plan |
|-----|----------|------|
| fzf `__preview__` subcommand | ✅ Done | Phase 1 |
| kitty icat / chafa image preview in fzf | ✅ Done | Phase 1 |
| ffmpegthumbnailer video preview in fzf | ✅ Done | Phase 1 |
| Rust `tui` (ratatui) | Low | Deferred — Wails GUI replaces TUI |
| Flatpak Steam paths in `steam-workshop` | ✅ Done | Native and Flatpak Steam paths are scanned |

## GUI Replacement Scope

| Python GTK View | Wails React Replacement | Phase |
|-----------------|------------------------|-------|
| Library grid | LibraryView (virtualized grid, filter, sort, search) | 4 ✅ |
| Favorites | FavoritesView (grid, random, remove) | 6 ✅ |
| History | HistoryView (grid, random, clear with confirm) | 6 ✅ |
| Sources | SourcesView (grouped, add/remove/scan WE) | 7 ✅ |
| Settings | SettingsView (backends, library, storage/SQLite, cache) | 8 ✅ |
| Thumbnail cache | Lazy loading, cache/icon/original modes, v2 smart sampling | 5 ✅ |

## Test Count

| Crate | Unit | Integration |
|-------|------|-------------|
| wc-core | 7 | — |
| wc-scan | 12 | — |
| wc-storage | 0 | — |
| wc-backend | 0 | — |
| wc-preview | 0 | — |
| wc-cli | 0 | 35 |
| **Total** | **19** | **35** |

Grand total: 54 tests.

## Storage Backend Compatibility

- [x] flat files read/write
- [x] SQLite migrate/verify/resync/export/backup/restore
- [x] Hybrid dual-write
- [x] `storage_backend=sqlite` reads from DB
- [x] Bootstrap-safe config reads
- [x] All SQLite debug commands: `sqlite-config-get`, `sqlite-sources-list`, `sqlite-favorites-list`, `sqlite-history-list`, `sqlite-current-read`, `sqlite-last-backend-read`
