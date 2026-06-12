# Migration Status

> Auto-generated as part of Phase 0 baseline. Updated: 2026-06-11.

## Phase Completion

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Freeze and Baseline | ✅ Complete |
| 1 | Rust CLI Preview Readiness (__preview__ + fzf) | ✅ Complete |
| 2 | Wails App Shell (Go backend + bridge) | ✅ Complete (retired) |
| 3 | React UI Foundation (Vite + TS + layout) | ✅ Complete |
| 4 | Library Grid (virtual scroll, filter, sort, search) | ✅ Complete |
| 5 | Thumbnail Strategy (useThumbnailQueue, lazy loading, dedup) | ✅ Complete |
| 6 | Favorites & History (grid, random, clear confirm) | ✅ Complete |
| 7 | Sources View (grouped, add/remove/scan) | ✅ Complete |
| 8 | Settings View (backends, library, storage/SQLite) | ✅ Complete |
| 9 | Apply/Stop/Restore UX (status bar, async apply) | ✅ Complete |
| 10 | Packaging and Install | ✅ Complete (side-by-side Rust CLI + Tauri GUI install) |
| 11 | niri Integration | ⬜ Pending (needs real-use validation) |
| 12 | Deprecate Bash/Python | ⬜ Deferred (needs real-use validation) |

## Current Migration Verdict

The Rust implementation is a usable side-by-side replacement candidate for the Bash/Python
implementation. The supported GUI path is **Tauri 2 + React/TypeScript**, which calls Rust crates
directly — no subprocess bridge.

Do not replace the original `wallpaper-console` / `wallpaper-console-gui` commands by default yet.
Keep using the `-rust` command names until real-use validation passes for niri startup, source
management, image/video apply, favorites/history, and SQLite maintenance.

## Baseline Checks

| Check | Status |
|-------|--------|
| `cargo fmt --check` | ✅ passes |
| `cargo clippy --workspace -- -D warnings` | ✅ clean |
| `cargo test --workspace` | ✅ 68 tests (35 wc-cli + 7 wc-core + 12 wc-scan + 14 wc-storage) |
| `cargo tauri build --bundles deb,rpm` | ✅ produces .deb + .rpm |
| Tauri frontend build | ✅ passes |
| Tauri smoke tests (Playwright) | ✅ 26 passed |

## GUI Architecture

The production GUI path is **Tauri 2 + React + direct Rust crate calls**. The Tauri app
(`apps/tauri-gui/`) builds and produces `.deb`/`.rpm` bundles. All GUI operations call Rust
crates directly via `#[tauri::command]` functions — no subprocess bridge. The asset protocol
is configured (`protocol-asset` feature + scope) for local thumbnail loading via `convertFileSrc()`.

> **Historical:** Wails v3 + Go bridge was the previous GUI path. It has been retired.
> Start with [HISTORICAL_WAILS_ARCHIVE.md](HISTORICAL_WAILS_ARCHIVE.md) before reading archived Wails architecture details.

## Installation

See [install.sh](../install.sh) in the repository root.

Quick install (side-by-side with existing Bash/Python versions):
```bash
./install.sh
```

This installs to:
- `~/.local/bin/wallpaper-console-rust` (Rust CLI)
- `~/.local/bin/wallpaper-console-gui-rust` (Tauri GUI)

**Rollback** (restore original Bash/Python):
```bash
./install.sh --prefix "$HOME/.local" --uninstall
```

The original Bash/Python install is never removed.

## CLI Command Parity

See [COMMAND_PARITY.md](COMMAND_PARITY.md) for the full checklist.

- **Completed:** all non-TUI commands (50+), `__preview__`, all SQLite debug commands, Flatpak Steam paths
- **Remaining:** `tui` (stub — Tauri GUI replaces the old control TUI unless a Rust terminal TUI is explicitly requested)

## GUI Replacement Scope

| Python GTK View | React Replacement | Phase |
|-----------------|-------------------|-------|
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
| wc-storage | 14 | — |
| wc-backend | 0 | — |
| wc-preview | 0 | — |
| wc-cli | 0 | 35 |
| app_lib (Tauri) | 1 | — |
| **Total** | **34** | **35** |

Grand total: 69 tests (was 68; Tauri unit test addition may change exact count).

## Storage Backend Compatibility

- [x] flat files read/write
- [x] SQLite migrate/verify/resync/export/backup/restore
- [x] Hybrid dual-write
- [x] `storage_backend=sqlite` reads from DB
- [x] Bootstrap-safe config reads
- [x] All SQLite debug commands
