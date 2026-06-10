# Wails Architecture

## Stack

```text
┌─────────────────────────────────────────┐
│ React 19 + TypeScript                    │
│ @tanstack/react-virtual (virtualized grid)│
│ Lucide icons                             │
├─────────────────────────────────────────┤
│ Wails v3 Go bridge                       │
│ ~30 bound methods, all thin wrappers     │
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
  → api.bridge.ts (typed wrappers)
  → window.wails.Call('MethodName', ...args)
  → Go Bridge.Method(args)
  → Runner.run('wallpaper-console-rust', 'command', ...args)
  → Rust CLI stdout/stderr JSON or text
  → Go parses → structured DTO
  → React receives typed response
```

## Key Design Decisions

1. **Rust owns all business logic.** Go never touches runtime files, config, wallpaper state, or SQLite directly. It only spawns CLI processes and parses output.

2. **Go bridge is process-based, not FFI.** Avoids cgo complexity, keeps Rust CLI testable independently, and lets the Bash/Python versions and Wails GUI use the same Rust command contract.

3. **React never edits runtime files.** All writes go through Wails methods → Rust CLI commands. This prevents the frontend from corrupting state.

4. **XDG_CONFIG_HOME compatibility.** The Wails GUI reads the same config directory as the Bash/Python versions. No migration needed.

## Directory Structure

```text
apps/wails-gui/
├── go.mod                    # Go module (Wails v3 dependency)
├── main.go                   # Wails app entry + method bindings
├── app.go                    # Bridge struct (thin wrappers → Runner)
├── rust.go                   # Runner, DTOs, all CLI command calls
└── frontend/
    ├── package.json           # React 19 + Vite + TypeScript
    ├── vite.config.ts
    ├── tsconfig.json
    ├── index.html
    └── src/
        ├── main.tsx           # React entry
        ├── App.tsx            # Layout, view routing, status polling
        ├── api/
        │   └── bridge.ts      # Typed Wails API + DTO interfaces
        ├── components/
        │   ├── Sidebar.tsx    # Navigation (Library/Favorites/History/Sources/Settings)
        │   ├── Toolbar.tsx    # Refresh, Rescan, Stop, Restore
        │   ├── StatusBar.tsx  # Current wallpaper, backend, source count
        │   ├── WallpaperGrid.tsx  # Virtualized grid with lazy thumbnails
        │   ├── ContextMenu.tsx    # Right-click apply/favorite/open
        │   └── ConfirmDialog.tsx  # Destructive action confirmation
        ├── views/
        │   ├── LibraryView.tsx   # Filter, sort, search, source select
        │   ├── FavoritesView.tsx  # Favorite grid + random + remove
        │   ├── HistoryView.tsx    # History grid + random + clear confirm
        │   ├── SourcesView.tsx    # Grouped sources + add/remove/scan
        │   └── SettingsView.tsx   # Backends, library, storage/SQLite
        └── styles/
            └── global.css      # Dark theme, utility-first, dense layout
```

## Wails Method Map

| Go Method | Rust CLI Command | Returns |
|-----------|-----------------|---------|
| Status() | `status` | StatusDTO |
| Apply(path) | `apply <path>` | CommandResult |
| Stop() | `stop` | CommandResult |
| Restore() | `restore` | CommandResult |
| LibraryList(source) | `library-json [--tsv\|--sqlite]` | []WallpaperDTO |
| LibraryCount() | `library-count` | LibraryCountDTO |
| Rescan() | `rescan` | CommandResult |
| FavoritesList() | `favorites-json` | []string |
| FavoriteAdd(path) | `favorite-add <path>` | CommandResult |
| FavoriteRemove(path) | `favorite-remove <path>` | CommandResult |
| HistoryList() | `history-json` | []HistoryDTO |
| HistoryClear() | `history-clear` | CommandResult |
| SourcesList() | `validate-sources` / `sources` | []SourceDTO |
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
| ThumbnailFor(path) | `thumbnail-path <path>` | ThumbnailDTO |
| ThumbnailCacheStatus() | (fs read cache dir) | ThumbnailCacheDTO |
| ThumbnailCacheClear() | (fs remove cache dir) | CommandResult |
| OpenPath(path) | `xdg-open <path>` | CommandResult |
| RevealInFileManager(path) | `xdg-open <dir>` | CommandResult |
