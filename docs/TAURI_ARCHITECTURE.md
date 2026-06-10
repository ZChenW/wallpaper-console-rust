# Tauri Architecture (Experimental)

> Tauri v2 builds and produces `.deb`/`.rpm` bundles. Wails remains the stable GUI target until Tauri
> passes full real-use smoke testing.

## Stack

```text
┌─────────────────────────────────────────┐
│ React 19 + TypeScript                    │
│ @tanstack/react-virtual (virtualized grid)│
│ Lucide icons                             │
├─────────────────────────────────────────┤
│ Tauri v2 Rust commands (direct crate calls)│
│ No subprocess bridge — Rust crates linked │
├─────────────────────────────────────────┤
│ wc-core, wc-storage, wc-scan, wc-backend│
│ wc-preview (thumbnails), wc-cli          │
├─────────────────────────────────────────┤
│ Runtime files (XDG_CONFIG_HOME)          │
│ Flat files + SQLite wallpapers.db        │
└─────────────────────────────────────────┘
```

## Data Flow

```
React UI
  → api/bridge.ts (typed wrappers, camelCase)
  → invoke<T>('command_name', { args }) from @tauri-apps/api/core
  → Rust #[tauri::command] fn command_name(args) -> Result<T, String>
  → Direct Rust crate call (no subprocess)
  → JSON-serialized response
  → React receives typed response

Thumbnail loading:
  → thumbnail_for() returns cache path
  → convertFileSrc(path) from @tauri-apps/api/core
  → Tauri asset protocol serves file:// → asset:// URL
  → <img src={assetUrl}> loads from local cache
```

## Key Differences from Wails

| Aspect | Wails v3 | Tauri v2 |
|--------|----------|----------|
| Bridge language | Go (thin subprocess wrapper) | Rust (direct crate calls) |
| Backend call cost | Process spawn + JSON parse | Function call |
| Thumbnail generation | Go calls Rust CLI `thumbnail` subprocess | Tauri command calls `wc_preview::thumbnail_for` directly |
| Window toolkit | WebKitGTK 6.0 (`webkitgtk-6.0`) | WebKitGTK 4.1 (`webkit2gtk-4.1`) |
| Frontend API | Generated bindings (PascalCase) | `invoke<T>()` (snake_case commands) |
| Asset loading | N/A (local file:// paths) | `convertFileSrc()` with asset protocol |

## Directory Structure

```text
apps/tauri-gui/
├── src-tauri/
│   ├── Cargo.toml              # tauri = "2.11.2", features = ["protocol-asset"]
│   ├── tauri.conf.json          # Window config, assetProtocol scope, bundle targets
│   ├── build.rs                 # Tauri build script
│   ├── capabilities/
│   │   └── default.json         # Permissions: core:default
│   ├── icons/                   # App icons (32x32, 128x128, icns, ico)
│   └── src/
│       ├── main.rs              # Entry point → app_lib::run()
│       ├── lib.rs               # Tauri Builder, setup, invoke_handler (all commands)
│       └── commands.rs           # All #[tauri::command] functions
└── frontend/
    ├── package.json              # React 19 + Vite + TypeScript (shared codebase)
    ├── vite.config.ts
    ├── tsconfig.json
    ├── index.html
    └── src/
        ├── main.tsx              # React entry
        ├── App.tsx               # Layout, view routing, status polling
        ├── api/
        │   └── bridge.ts         # invoke<T>() wrappers with explicit generics
        ├── components/
        │   ├── Sidebar.tsx
        │   ├── Toolbar.tsx
        │   ├── StatusBar.tsx
        │   ├── WallpaperGrid.tsx # Virtualized grid + convertFileSrc for thumbs
        │   ├── ContextMenu.tsx
        │   └── ConfirmDialog.tsx
        ├── views/
        │   ├── LibraryView.tsx
        │   ├── FavoritesView.tsx
        │   ├── HistoryView.tsx
        │   ├── SourcesView.tsx
        │   └── SettingsView.tsx
        └── styles/
            └── global.css        # Light theme via CSS variables
```

## Tauri Command Map

All commands defined in `src-tauri/src/commands.rs` and registered in `lib.rs`:

| Rust Command | Implementation | Returns |
|-------------|---------------|---------|
| `status` | `StorageApi::current_read` + `last_backend_read` + `sources_list` | StatusDTO |
| `apply(path)` | `formats::classify_extension` → local `backend_for_type()` → `wc_backend::apply_wallpaper` | CommandResult |
| `stop()` | `wc_backend::stop_all_backends` | CommandResult |
| `restore()` | `wc_backend::restore` | CommandResult |
| `config_get(key)` | `StorageApi::config_get` | String |
| `config_set(key, value)` | `StorageApi::config_set` | CommandResult |
| `sources_list()` | `StorageApi::sources_list` + local DTO builder (exists/is_we/label) | Vec<SourceDTO> |
| `source_add(path)` | `std::fs::canonicalize` → `StorageApi::sources_add` | CommandResult |
| `source_remove(path)` | `StorageApi::sources_remove` | CommandResult |
| `validate_sources()` | calls local `sources_list()`, formats OK/MISSING per-line | CommandResult |
| `remove_missing_sources()` | `StorageApi::sources_list` + `sources_remove` loop over missing dirs | CommandResult |
| `scan_steam_workshop()` | local Steam path enumeration (native + Flatpak) → `StorageApi::sources_add` | CommandResult |
| `favorites_list()` | `StorageApi::favorites_list` | Vec<String> |
| `favorite_add(path)` | `StorageApi::favorites_add` | CommandResult |
| `favorite_remove(path)` | `StorageApi::favorites_remove` | CommandResult |
| `history_list()` | `StorageApi::history_list` + HistoryDto mapping | Vec<HistoryDTO> |
| `history_clear()` | `StorageApi::history_clear` | CommandResult |
| `library_count()` | local `library_entries_from_tsv()` → count by type | LibraryCountDTO |
| `library_list(source)` | delegates to local `library_page()` with limit=MAX | Vec<WallpaperDTO> |
| `library_page(...)` | `library_page_sqlite()` (raw SQL via rusqlite) or `library_page_tsv()` (TSV parse + in-memory filter/sort/paginate) | LibraryPageDTO |
| `rescan()` | `wc_scan::scan_wallpapers` + `wc_storage::sqlite::library_clear/insert` + local TSV write | CommandResult |
| `migrate_to_sqlite()` | `wc_storage::sqlite::migrate_to_sqlite` | CommandResult |
| `sqlite_verify()` | `wc_storage::sqlite::verify` | CommandResult |
| `sqlite_resync()` | `wc_storage::sqlite::resync` | CommandResult |
| `sqlite_backup()` | `wc_storage::sqlite::backup` | CommandResult |
| `sqlite_restore(path)` | `wc_storage::sqlite::restore` | CommandResult |
| `sqlite_export_flat()` | `wc_storage::sqlite::export_flat` | CommandResult |
| `thumbnail_for(path)` | `wc_preview::thumbnail_for` (v2 smart sampling) | ThumbnailDTO |
| `thumbnail_cache_status()` | local `fs::read_dir` + metadata (entries count, total bytes) | ThumbnailCacheDTO |
| `thumbnail_cache_clear()` | local `fs::remove_dir_all` + `create_dir_all` | CommandResult |
| `open_path(path)` | `run_external("xdg-open", [path])` | CommandResult |
| `reveal_in_file_manager(path)` | `run_external("xdg-open", [parent])` | CommandResult |
| `browse_directory()` | local zenity → kdialog → yad fallback chain | String |

## Asset Protocol (Local Thumbnail Loading)

**Problem:** Tauri WebView cannot load `file://` URLs by default. Thumbnails stored at
`~/.config/wallpaper-console/cache/gui-thumbnails/*.webp` must be served through Tauri's
asset protocol.

**Fix applied (2026-06-10):**

1. **`Cargo.toml`**: `tauri = { version = "2.11.2", features = ["protocol-asset"] }`
2. **`tauri.conf.json`**:
   ```json
   "security": {
     "csp": null,
     "assetProtocol": {
       "enable": true,
       "scope": {
         "allow": ["$HOME/.config/wallpaper-console/cache/gui-thumbnails/**"]
       }
     }
   }
   ```
3. **Frontend**: `<img src={convertFileSrc(thumbPath)} />` — `convertFileSrc` from `@tauri-apps/api/core`

If `$HOME` variable is not resolved by Tauri's static scope, the fallback is dynamic registration
in `lib.rs` setup:
```rust
app.asset_protocol_scope()
    .allow_directory(&config_dir.gui_thumbnail_cache_dir())?;
```

## Build

```bash
cd apps/tauri-gui/src-tauri
cargo tauri build --bundles deb,rpm
```

Outputs:
- `target/release/bundle/deb/wallpaper-console-gui-rust_0.1.0_amd64.deb`
- `target/release/bundle/rpm/wallpaper-console-gui-rust-0.1.0-1.x86_64.rpm`
- Binary: `target/release/wallpaper-console-tauri`

## Prerequisites

- `webkit2gtk-4.1` (Tauri v2 requires this, NOT `webkitgtk-6.0` which Wails uses)
- All other Rust crate dependencies are workspace-managed
