# Migration Status

> Auto-generated as part of Phase 0 baseline. Updated: 2026-06-10.

## Phase Completion

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Freeze and Baseline | ✅ Complete |
| 1 | Rust CLI Preview Readiness (__preview__ + fzf) | ✅ Complete |
| 2 | Wails App Shell (Go backend + bridge) | ✅ Complete |
| 3 | React UI Foundation (Vite + TS + layout) | ✅ Complete |
| 4 | Library Grid (virtualized, filter, sort, search) | ✅ Complete |
| 5 | Thumbnail Strategy (cache modes, lazy loading) | ✅ Complete |
| 6 | Favorites & History (grid, random, clear confirm) | ✅ Complete |
| 7 | Sources View (grouped, add/remove/scan) | ✅ Complete |
| 8 | Settings View (backends, library, storage/SQLite) | ✅ Complete |
| 9 | Apply/Stop/Restore UX (status bar, async apply) | ✅ Complete |
| 10 | Packaging and Install | 🟡 Partial (build ok, install script: see install.sh) |
| 11 | niri Integration | ⬜ Pending (needs real-use validation) |
| 12 | Deprecate Bash/Python | ⬜ Deferred (needs real-use validation) |

## Baseline Checks

| Check | Status |
|-------|--------|
| `cargo fmt --check` | ✅ passes |
| `cargo clippy --workspace -- -D warnings` | ✅ clean |
| `cargo test --workspace` | ✅ 35/35 (23 integration + 7 wc-core + 5 wc-scan) |
| Smoke: `status` with temp XDG_CONFIG_HOME | ✅ works |
| Smoke: `add` + `rescan` + `library-count` | ✅ works |

## Frontend Build

| Check | Status |
|-------|--------|
| `npx tsc --noEmit` | ✅ passes |
| `npx vite build` | ✅ 225KB JS + 7.5KB CSS (1591 modules) |

## Wails Build

| Check | Status |
|-------|--------|
| `go build ./...` | ✅ passes |
| `wails3 build` | ✅ passes → `apps/wails-gui/bin/wallpaper-console-gui` (19MB ELF) |
| `wails3` CLI | ✅ installed at `~/go/bin/wails3` (v3.0.0-alpha.98) |

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
ln -sf /path/to/bash/wallpaper-console ~/.local/bin/wallpaper-console
ln -sf /path/to/python/wallpaper-console-gui ~/.local/bin/wallpaper-console-gui
```

The original Bash/Python install is never removed.

## CLI Command Parity

See [COMMAND_PARITY.md](COMMAND_PARITY.md) for the full checklist.

- **Completed:** 53/54 commands (+ `__preview__`)
- **Remaining:** `tui` (stub — Wails GUI replaces TUI)

## Known CLI Gaps

| Gap | Severity | Plan |
|-----|----------|------|
| fzf `__preview__` subcommand | ✅ Done | Phase 1 |
| kitty icat / chafa image preview in fzf | ✅ Done | Phase 1 |
| ffmpegthumbnailer video preview in fzf | ✅ Done | Phase 1 |
| Rust `tui` (ratatui) | Low | Deferred — Wails GUI replaces TUI |
| Flatpak Steam paths in `steam-workshop` | Low | Deferred |

## GUI Replacement Scope

| Python GTK View | Wails React Replacement | Phase |
|-----------------|------------------------|-------|
| Library grid | LibraryView (virtualized grid, filter, sort, search) | 4 ✅ |
| Favorites | FavoritesView (grid, random, remove) | 6 ✅ |
| History | HistoryView (grid, random, clear with confirm) | 6 ✅ |
| Sources | SourcesView (grouped, add/remove/scan WE) | 7 ✅ |
| Settings | SettingsView (backends, library, storage/SQLite, cache) | 8 ✅ |
| Thumbnail cache | Lazy loading, cache/icon/original modes | 5 ✅ |

## Test Count

| Crate | Unit | Integration |
|-------|------|-------------|
| wc-core | 7 | — |
| wc-scan | 5 | — |
| wc-storage | 0 | — |
| wc-backend | 0 | — |
| wc-preview | 0 | — |
| wc-cli | 0 | 23 |
| **Total** | **12** | **23** |

## Storage Backend Compatibility

- [x] flat files read/write
- [x] SQLite migrate/verify/resync/export/backup/restore
- [x] Hybrid dual-write
- [x] `storage_backend=sqlite` reads from DB
- [x] Bootstrap-safe config reads
