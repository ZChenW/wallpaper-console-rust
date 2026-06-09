# Migration Status

> Auto-generated as part of Phase 0 baseline. Updated: 2026-06-10.

## Baseline Checks

| Check | Status |
|-------|--------|
| `cargo fmt --check` | ✅ passes |
| `cargo clippy --workspace -- -D warnings` | ✅ clean |
| `cargo test --workspace` | ✅ 35/35 (23 integration + 7 wc-core + 5 wc-scan) |
| Smoke: `status` with temp XDG_CONFIG_HOME | ✅ works |
| Smoke: `add` + `rescan` + `library-count` | ✅ works |

## CLI Command Parity

See [COMMAND_PARITY.md](COMMAND_PARITY.md) for the full checklist.

- **Completed:** 52/53 commands
- **Remaining:** `tui` (stub — planned for Wails GUI replacement)

## Known CLI Gaps

| Gap | Severity | Plan |
|-----|----------|------|
| fzf `__preview__` subcommand | Medium | Phase 1 |
| kitty icat / chafa image preview in fzf | Medium | Phase 1 |
| ffmpegthumbnailer video preview in fzf | Medium | Phase 1 |
| Rust `tui` (ratatui) | Low | Deferred — Wails GUI replaces TUI |
| Flatpak Steam paths in `steam-workshop` | Low | Deferred |

## GUI Replacement Scope

| Python GTK View | Wails React Replacement | Phase |
|-----------------|------------------------|-------|
| Library grid | Library grid with virtualization | Phase 4 |
| Favorites | Favorites grid | Phase 6 |
| History | History list/grid | Phase 6 |
| Sources | Sources view with groups | Phase 7 |
| Settings | Settings view (backends, storage, thumbnails) | Phase 8 |
| Thumbnail cache | Rust-owned thumbnail generation | Phase 5 |

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
