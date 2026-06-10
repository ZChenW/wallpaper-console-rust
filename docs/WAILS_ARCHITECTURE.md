# Wails Architecture

## Stack

```text
┌─────────────────────────────────────────┐
│ React 19 + TypeScript                    │
│ @tanstack/react-virtual (virtualized grid)│
│ Lucide icons                             │
├─────────────────────────────────────────┤
│ Wails v3 Go bridge (34 bound methods)    │
│ 33 user-facing + Binary helper           │
│ All thin wrappers → Rust CLI subprocess  │
├─────────────────────────────────────────┤
│ Rust CLI (wallpaper-console-rust)        │
│ wc-core, wc-storage, wc-scan, wc-backend│
│ wc-preview, wc-cli                       │
├─────────────────────────────────────────┤
│ Runtime files (XDG_CONFIG_HOME)          │
│ Flat files + SQLite wallpapers.db        │
└─────────────────────────────────────────┘
```

## Data Flow

```
React UI
  → api/bridge.ts (typed wrappers, camelCase)
  → Wails v3 generated bindings (PascalCase)
    (bindings/wallpaper-console-gui/bridge.ts)
  → Go Bridge.Method(args)
  → Runner.run('wallpaper-console-rust', 'command', ...args)
  → Rust CLI stdout/stderr JSON or text
  → Go parses → structured DTO
  → React receives typed response
```

**Important:** The frontend uses Wails v3 **generated bindings** (`wails3 generate bindings -ts -i -names -b`),
not the deprecated `window.wails.Call()` pattern. All 34 bridge methods (33 user-facing + Binary helper) are typed at the TypeScript level.

## Key Design Decisions

1. **Rust owns all business logic.** Go never touches runtime files, config, wallpaper state, or SQLite directly. It only spawns CLI processes and parses output.

2. **Go bridge is process-based, not FFI.** Avoids cgo complexity, keeps Rust CLI testable independently, and lets the Bash/Python versions and Wails GUI use the same Rust command contract.

3. **React never edits runtime files.** All writes go through Wails methods → Rust CLI commands. This prevents the frontend from corrupting state.

4. **XDG_CONFIG_HOME compatibility.** The Wails GUI reads the same config directory as the Bash/Python versions. No migration needed.

## Directory Structure

```text
apps/wails-gui/
├── go.mod                    # Go module (Wails v3 dependency)
├── main.go                   # Wails app entry point (Assets, Services, Window)
├── app.go                    # Bridge struct + BrowseDirectory (zenity/kdialog/yad)
├── rust.go                   # Runner, DTOs, all CLI command calls
├── Taskfile.yml              # build:bindings → build:frontend → build:backend
├── bindings/                 # Generated TypeScript bindings (wails3 generate)
│   └── wallpaper-console-gui/
│       └── bridge.ts         # PascalCase typed methods
└── frontend/
    ├── package.json           # React 19 + Vite + TypeScript
    ├── vite.config.ts
    ├── tsconfig.json
    ├── index.html
    └── src/
        ├── main.tsx           # React entry
        ├── App.tsx            # Layout, view routing, status polling
        ├── api/
        │   └── bridge.ts      # camelCase wrappers over generated bindings
        ├── components/
        │   ├── Sidebar.tsx    # Navigation (Library/Favorites/History/Sources/Settings)
        │   ├── Toolbar.tsx    # Refresh, Rescan, Stop, Restore
        │   ├── StatusBar.tsx  # Current wallpaper, backend, source count
        │   ├── WallpaperGrid.tsx  # Virtualized grid (useVirtualizer) with lazy thumbnails
        │   ├── ContextMenu.tsx    # Right-click apply/favorite/open
        │   └── ConfirmDialog.tsx  # Destructive action confirmation
        ├── views/
        │   ├── LibraryView.tsx   # Filter, sort, search, source select, paginated
        │   ├── FavoritesView.tsx  # Favorite grid + random + remove
        │   ├── HistoryView.tsx    # History grid + random + clear confirm
        │   ├── SourcesView.tsx    # Grouped sources + add/remove/validate/scan
        │   └── SettingsView.tsx   # Backends, library, storage/SQLite, thumbnail cache
        └── styles/
            └── global.css      # Light theme via CSS variables (:root)
```

## Wails Method Map

All methods use Wails v3 generated bindings (NOT `window.wails.Call`).

| Go Method | Rust CLI Command | Returns |
|-----------|-----------------|---------|
| Status() | `status` | StatusDTO |
| Apply(path) | `apply <path>` | CommandResult |
| Stop() | `stop` | CommandResult |
| Restore() | `restore` | CommandResult |
| LibraryList(source) | `library-json [--tsv\|--sqlite]` | []WallpaperDTO |
| LibraryCount() | `library-count` | LibraryCountDTO |
| LibraryPage(source,filter,sort,search,offset,limit) | `library-page-json` | LibraryPageDTO |
| Rescan() | `rescan` | CommandResult |
| FavoritesList() | `favorites-json` | []string |
| FavoriteAdd(path) | `favorite-add <path>` | CommandResult |
| FavoriteRemove(path) | `favorite-remove <path>` | CommandResult |
| HistoryList() | `history-json` | []HistoryDTO |
| HistoryClear() | `history-clear` | CommandResult |
| SourcesList() | `sources` (with os.Stat for metadata) | []SourceDTO |
| SourceAdd(path) | `add <path>` | CommandResult |
| SourceRemove(path) | `remove-source <path>` | CommandResult |
| ValidateSources() | `validate-sources` | CommandResult |
| RemoveMissingSources() | `remove-missing` | CommandResult |
| ScanSteamWorkshop() | `steam-workshop` | CommandResult |
| ConfigGet(key) | `config-get <key>` | string |
| ConfigSet(key,value) | `config-set <key> <value>` | CommandResult |
| SqliteVerify() | `sqlite-verify` | CommandResult |
| SqliteResync() | `sqlite-resync` | CommandResult |
| SqliteBackup() | `sqlite-backup` | CommandResult |
| SqliteRestore(path) | `sqlite-restore <path>` | CommandResult |
| SqliteExportFlat() | `sqlite-export-flat` | CommandResult |
| MigrateToSqlite() | `migrate-to-sqlite` | CommandResult |
| ThumbnailFor(path) | Rust CLI `thumbnail` subcommand (v2 smart sampling) | ThumbnailDTO |
| ThumbnailCacheStatus() | (fs read cache dir) | ThumbnailCacheDTO |
| ThumbnailCacheClear() | (fs remove cache dir) | CommandResult |
| OpenPath(path) | `xdg-open <path>` | CommandResult |
| RevealInFileManager(path) | `xdg-open <dir>` | CommandResult |
| BrowseDirectory() | zenity → kdialog → yad (Go-native, no Rust CLI) | string |
| Binary() | returns configured Rust CLI binary path | string |

### Thumbnail Generation (v2)

Thumbnails are generated by the Rust CLI `thumbnail` subcommand, not by Go directly:

- **Go `ThumbnailFor(path)`** calls `wallpaper-console-rust thumbnail <path>`
- **Rust `wc-preview`** handles:
  - Cache key: `v2-` prefix with `realpath:mtime:size`
  - Images/GIFs: ImageMagick `identify` for resolution, `magick` for resize
  - Videos: `ffprobe` for duration, multi-point frame sampling (25%/50%/10%/5s/75%), `frame_has_content()` via `identify -format "%[fx:mean] %[fx:standard_deviation]"` to skip black/title frames
  - Output: 400px scaled WebP, atomic `.tmp.webp` → rename to `.webp`
  - Cache: `$XDG_CONFIG_HOME/wallpaper-console/cache/gui-thumbnails/`
- **Frontend**: `useThumbnailQueue(concurrency=2)` scoped to virtualizer visible range + overscan
