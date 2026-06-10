# Rust Migration Completion and Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the migration from the Bash/Python `wallpaper-console` project to a production-ready Rust GUI/CLI replacement, and eliminate the current Wails GUI CPU/memory/perceived-lag problems.

**Architecture:** Keep the existing Rust crates as the source of truth (`wc-core`, `wc-storage`, `wc-scan`, `wc-backend`, `wc-preview`, `wc-cli`). First close parity gaps in the current Rust/Wails app, then replace the Wails Go subprocess bridge with a Rust-native GUI shell so the GUI calls Rust code directly and does not spawn the CLI for normal UI operations.

**Tech Stack:** Rust workspace, SQLite via `rusqlite`, existing wallpaper backends (`awww`, `mpvpaper`), React/TypeScript for the current Wails UI, and a final Rust-native GUI shell using Tauri 2 + React/TypeScript unless a later explicit decision chooses native GTK4/libadwaita instead.

---

## Current Verdict

The Rust migration has been executed as a **side-by-side Rust CLI + Wails GUI replacement candidate**.

Current supported path:

- `wallpaper-console-rust` — Rust CLI
- `wallpaper-console-gui-rust` — Wails + React GUI backed by the Rust CLI

The Wails path has received the planned performance fixes: paginated SQLite-backed library loading,
frontend thumbnail queueing, Go-side thumbnail worker limiting, duplicate in-flight suppression, and a
smaller initial page size.

The Tauri path exists under `apps/tauri-gui/` as an experiment, but is not part of the default build
gate on this machine. Tauri v2 currently resolves to `webkit2gtk-4.1` on Linux; this Arch environment
has `webkitgtk-6.0` available and does not provide `webkit2gtk-4.1.pc`. Until that dependency is
installed or the GUI strategy changes to a WebKitGTK 6-compatible shell, Wails remains the supported
GUI target.

The migration should not replace the old Bash/Python command names by default until real-use validation
passes.

Historical pre-execution audit:

The Rust repository is a strong beta:

- Rust crates exist for config, formats, storage, scanning, backend application, preview, and CLI.
- Most CLI behavior is implemented.
- Wails/React GUI exists and builds.
- SQLite storage, migration, verify, backup, restore, and library indexing exist.
- The installed Rust GUI uses Wails v3 generated bindings instead of the broken `window.wails.Call` path.

But it is not ready to replace the Bash/Python project because:

- `tui` is still a stub in Rust.
- The old Bash command with no arguments opens the TUI; Rust with no arguments shows Clap help.
- Some old SQLite read/debug commands still exist in Bash but are absent in Rust help: `sqlite-config-get`, `sqlite-sources-list`, `sqlite-favorites-list`, `sqlite-history-list`, `sqlite-current-read`, `sqlite-last-backend-read`.
- The parity docs conflict: `MIGRATION_STATUS.md` says `__preview__` is complete, while `COMMAND_PARITY.md` still says browse has "no preview yet" and lists `__preview__` as a known gap.
- `steam-workshop` in Rust only scans native Steam paths and still omits Flatpak Steam paths.
- The Wails GUI is currently too heavy and can create CPU spikes because thumbnails are requested with uncontrolled concurrency.
- The Wails backend calls the Rust CLI as a subprocess for normal UI operations. That is acceptable as an interim bridge, but not a final high-performance architecture.

Do not deprecate the Bash/Python implementation until every acceptance gate in this plan passes.

---

## Evidence From Repository Audit

Reference old project:

- `/home/chakew/Projects/wallpaper-console/wallpaper-console`
- Bash modules under `/home/chakew/Projects/wallpaper-console/lib/wallpaper-console/`
- Python GTK GUI under `/home/chakew/Projects/wallpaper-console/gui/`

Rust project:

- `/home/chakew/Projects/wallpaper-console-rust`
- Rust crates under `/home/chakew/Projects/wallpaper-console-rust/crates/`
- Wails app under `/home/chakew/Projects/wallpaper-console-rust/apps/wails-gui/`

Observed Rust gaps:

```text
docs/COMMAND_PARITY.md:
- [ ] tui — not yet implemented in Rust
- Known gaps mention fzf preview, TUI stub, Flatpak Steam paths, and sort behavior differences.

crates/wc-cli/src/main.rs:
- Commands::Tui prints "TUI not yet implemented in Rust..."
- fzf_select does wire __preview__, so COMMAND_PARITY.md must be re-audited and corrected.

apps/wails-gui/frontend/src/components/WallpaperGrid.tsx:
- PAGE_SIZE = 100
- thumbnail effect loops through entries.slice(0, visible)
- each missing thumbnail starts api.thumbnailFor(e.path)
- every thumbnail completion calls setThumbCache with a new object

apps/wails-gui/rust.go:
- Runner.run spawns the Rust CLI for status, library, source, config, apply, etc.
- ThumbnailFor generates thumbnails on demand.
- generateThumbnail can start magick, convert, and ffmpeg without a global worker limit.
```

Observed runtime config state on the reviewed machine:

```text
gui_thumbnail_mode = cache
gui_library_source = tsv
storage_backend = sqlite
library-count = 33 total, 20 images, 0 gifs, 13 videos
```

This means the database backend is enabled, but the GUI library still defaults to TSV in `LibraryView.tsx`.

---

## Performance Root Cause

The current GUI can feel slow for four independent reasons:

1. **WebKitGTK baseline cost**
   - Wails uses WebKitGTK on Linux. A WebView app has a non-trivial memory baseline before wallpaper-console renders anything.

2. **Go bridge shells out to Rust CLI**
   - Normal UI calls go through `apps/wails-gui/rust.go`.
   - `Runner.run()` starts a new `wallpaper-console-rust` process for `status`, `library-json`, `sources`, `config-get`, and other operations.
   - This is simple and robust, but it adds process startup overhead and repeated JSON parsing.

3. **Thumbnail generation storm**
   - `WallpaperGrid.tsx` starts thumbnail requests for up to the first 100 items immediately.
   - There is no frontend concurrency limit, no cancellation, no viewport-based lazy loading, and no batching of React state updates.
   - `ThumbnailFor()` can start external processes (`magick`, `convert`, `ffmpeg`) for many files at once.
   - This explains CPU spikes and UI jank, especially with video wallpapers.

4. **Not true virtualization**
   - The grid renders a growing slice of entries, not a real virtual window.
   - The scroll listener is attached to `.wallpaper-grid`; the actual scroll area may be `.main-content`, depending on layout.
   - Large libraries will keep more DOM nodes and images alive than necessary.

The best long-term solution is to remove the Wails Go subprocess bridge and implement a Rust-native GUI backend. If keeping a web frontend is preferred, use Tauri 2 with the existing Rust crates. If lowest memory and most native Linux integration is the priority, use Rust GTK4/libadwaita instead.

Recommended path: **Tauri 2 + React/TypeScript**.

Reason:

- Reuses the current React UI concepts.
- Reuses the existing Rust crates directly.
- Removes Go and repeated CLI subprocess calls.
- Keeps a modern GUI development workflow.
- Still allows later replacement of React views if needed.

---

## Phase 0: Freeze and Measure the Current State

**Files:**

- Modify: `docs/MIGRATION_STATUS.md`
- Create: `scripts/profile_gui.sh`
- Create: `docs/PERFORMANCE_BASELINE.md`

- [ ] **Step 0.1: Add a GUI profiling script**

Create `scripts/profile_gui.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

duration="${1:-30}"
bin="${WALLPAPER_CONSOLE_GUI_RUST:-$HOME/.local/bin/wallpaper-console-gui-rust}"

if [[ ! -x "$bin" ]]; then
  printf 'ERROR: GUI binary not executable: %s\n' "$bin" >&2
  exit 1
fi

printf 'Launching: %s\n' "$bin"
"$bin" >/tmp/wallpaper-console-gui-rust.log 2>&1 &
gui_pid="$!"

cleanup() {
  kill "$gui_pid" >/dev/null 2>&1 || true
}
trap cleanup EXIT

printf 'pid=%s\n' "$gui_pid"
printf 'timestamp,pid,ppid,pcpu,pmem,rss_kb,comm,args\n'

end=$((SECONDS + duration))
while (( SECONDS < end )); do
  ps -eo pid,ppid,pcpu,pmem,rss,comm,args \
    | awk -v pid="$gui_pid" '
      NR == 1 { next }
      $1 == pid || $2 == pid || $7 ~ /wallpaper-console|ffmpeg|magick|convert|WebKit|webkit/ {
        printf "%s,%s,%s,%s,%s,%s,%s,", systime(), $1, $2, $3, $4, $5, $6
        for (i = 7; i <= NF; i++) printf "%s%s", $i, (i == NF ? ORS : " ")
      }
    '
  sleep 1
done

printf '\nGUI log:\n'
tail -n 80 /tmp/wallpaper-console-gui-rust.log || true
```

- [ ] **Step 0.2: Make the script executable**

Run:

```bash
chmod +x scripts/profile_gui.sh
```

- [ ] **Step 0.3: Run the baseline measurement**

Run:

```bash
./scripts/profile_gui.sh 45 | tee /tmp/wallpaper-console-gui-baseline.csv
```

Expected:

- Captures GUI RSS and CPU.
- Captures child `ffmpeg`, `magick`, `convert`, and `wallpaper-console-rust` processes.
- Shows whether high CPU is thumbnail generation, WebKit, or repeated CLI calls.

- [ ] **Step 0.4: Document the baseline**

Create `docs/PERFORMANCE_BASELINE.md`:

```markdown
# Performance Baseline

Date: 2026-06-10

## Environment

- OS: Arch Linux
- Compositor: niri Wayland
- GUI binary: `~/.local/bin/wallpaper-console-gui-rust`
- Config dir: `$XDG_CONFIG_HOME/wallpaper-console` or `$HOME/.config/wallpaper-console`

## Current Config

```text
storage_backend=
gui_library_source=
gui_thumbnail_mode=
```

## Baseline Observations

Paste the key lines from `/tmp/wallpaper-console-gui-baseline.csv` here:

```text
```

## Root Cause Notes

- WebKitGTK baseline:
- Rust CLI subprocess count:
- Thumbnail generator process count:
- Peak RSS:
- Peak CPU:
```

- [ ] **Step 0.5: Commit Phase 0**

Run:

```bash
git add scripts/profile_gui.sh docs/PERFORMANCE_BASELINE.md docs/MIGRATION_STATUS.md
git commit -m "docs: add gui performance baseline"
```

---

## Phase 1: Close CLI Parity Gaps

**Files:**

- Modify: `crates/wc-cli/src/main.rs`
- Modify: `docs/COMMAND_PARITY.md`
- Modify: `docs/MIGRATION_STATUS.md`
- Test: `tests/cli_parity.rs` or existing integration test file under `tests/`

- [ ] **Step 1.1: Add tests for old SQLite read/debug commands**

Add integration coverage for:

```text
sqlite-config-get KEY
sqlite-sources-list
sqlite-favorites-list
sqlite-history-list
sqlite-current-read
sqlite-last-backend-read
```

Expected behavior:

- Commands read from `wallpapers.db`.
- Missing DB exits non-zero with a clear error.
- Empty current/last backend prints nothing and exits 0 only when DB exists and the key is empty.

- [ ] **Step 1.2: Implement the missing commands**

In `crates/wc-cli/src/main.rs`, add Clap enum variants and dispatch handlers.

Command semantics:

```text
sqlite-config-get KEY       read config.value from SQLite config table
sqlite-sources-list         print one source path per line from SQLite sources table
sqlite-favorites-list       print one favorite path per line from SQLite favorites table
sqlite-history-list         print one history path per line newest-first
sqlite-current-read         print state.current if present
sqlite-last-backend-read    print state.last_backend if present
```

- [ ] **Step 1.3: Fix default no-argument behavior**

Decide and implement one of these explicit behaviors:

Preferred for parity:

```text
wallpaper-console-rust
```

opens the Rust GUI if installed, otherwise opens Rust TUI if implemented, otherwise prints a clear message pointing to `wallpaper-console-gui-rust`.

Acceptable interim:

```text
wallpaper-console-rust
```

prints:

```text
Rust TUI is not implemented. Use wallpaper-console-gui-rust for the GUI or run wallpaper-console-rust help.
```

Do not leave a confusing generic Clap help as the default final behavior.

- [ ] **Step 1.4: Resolve preview documentation conflict**

Audit `__preview__` behavior:

```bash
wallpaper-console-rust __preview__ /path/to/image.png
wallpaper-console-rust browse
```

Then update `docs/COMMAND_PARITY.md`:

- If preview works, remove "no preview yet" and the known-gap line.
- If preview does not work, mark it incomplete and add a concrete task.

- [ ] **Step 1.5: Add Flatpak Steam paths**

Update Rust `steam-workshop` path search to include:

```text
$HOME/.var/app/com.valvesoftware.Steam/.local/share/Steam
$HOME/.var/app/com.valvesoftware.Steam/.steam/steam
```

Keep native priority:

```text
$HOME/.local/share/Steam
$HOME/.steam/steam
Flatpak paths
```

Canonicalize and deduplicate all detected workshop project directories.

- [ ] **Step 1.6: Run CLI parity tests**

Run:

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Expected:

- All tests pass.
- `docs/COMMAND_PARITY.md` no longer contradicts code.

- [ ] **Step 1.7: Commit Phase 1**

Run:

```bash
git add crates/wc-cli/src/main.rs docs/COMMAND_PARITY.md docs/MIGRATION_STATUS.md tests
git commit -m "fix: close remaining rust cli parity gaps"
```

---

## Phase 2: Stop the Current Wails GUI From Overloading the System

This phase keeps Wails, but removes the biggest CPU spikes.

**Files:**

- Modify: `apps/wails-gui/frontend/src/components/WallpaperGrid.tsx`
- Create: `apps/wails-gui/frontend/src/hooks/useThumbnailQueue.ts`
- Modify: `apps/wails-gui/rust.go`
- Create: `apps/wails-gui/thumbnail_pool.go`
- Test: frontend tests if available; otherwise add documented manual checks.

- [ ] **Step 2.1: Add a frontend thumbnail queue**

Create `apps/wails-gui/frontend/src/hooks/useThumbnailQueue.ts`:

```ts
import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api/bridge';

type ThumbState = Record<string, string>;

interface QueueItem {
  path: string;
  generation: number;
}

export function useThumbnailQueue(concurrency = 2) {
  const [thumbs, setThumbs] = useState<ThumbState>({});
  const queue = useRef<QueueItem[]>([]);
  const active = useRef(0);
  const generation = useRef(0);
  const failed = useRef(new Set<string>());
  const pending = useRef(new Set<string>());
  const buffered = useRef<ThumbState>({});
  const flushTimer = useRef<number | null>(null);

  const flush = useCallback(() => {
    const next = buffered.current;
    buffered.current = {};
    if (Object.keys(next).length > 0) {
      setThumbs((prev) => ({ ...prev, ...next }));
    }
    flushTimer.current = null;
  }, []);

  const scheduleFlush = useCallback(() => {
    if (flushTimer.current !== null) return;
    flushTimer.current = window.setTimeout(flush, 50);
  }, [flush]);

  const pump = useCallback(() => {
    while (active.current < concurrency && queue.current.length > 0) {
      const item = queue.current.shift()!;
      if (item.generation !== generation.current) continue;
      if (failed.current.has(item.path)) continue;
      active.current += 1;
      api.thumbnailFor(item.path)
        .then((result) => {
          if (item.generation !== generation.current) return;
          if (result.thumbnail) {
            buffered.current[item.path] = result.thumbnail;
            scheduleFlush();
          } else {
            failed.current.add(item.path);
          }
        })
        .catch(() => failed.current.add(item.path))
        .finally(() => {
          pending.current.delete(item.path);
          active.current -= 1;
          pump();
        });
    }
  }, [concurrency, scheduleFlush]);

  const reset = useCallback(() => {
    generation.current += 1;
    queue.current = [];
    active.current = 0;
    pending.current.clear();
    failed.current.clear();
    buffered.current = {};
    setThumbs({});
  }, []);

  const enqueue = useCallback((paths: string[]) => {
    const gen = generation.current;
    for (const path of paths) {
      if (thumbs[path] || pending.current.has(path) || failed.current.has(path)) continue;
      pending.current.add(path);
      queue.current.push({ path, generation: gen });
    }
    pump();
  }, [pump, thumbs]);

  useEffect(() => {
    return () => {
      if (flushTimer.current !== null) {
        window.clearTimeout(flushTimer.current);
      }
    };
  }, []);

  return { thumbs, enqueue, reset };
}
```

- [ ] **Step 2.2: Use the queue in `WallpaperGrid.tsx`**

Replace the direct `entries.slice(0, visible).forEach(async ...)` thumbnail effect with:

```ts
const { thumbs: thumbCache, enqueue, reset } = useThumbnailQueue(2);

useEffect(() => {
  reset();
  setVisible(PAGE_SIZE);
}, [entries, reset]);

useEffect(() => {
  const paths = entries.slice(0, visible).map((entry) => entry.path);
  enqueue(paths);
}, [entries, visible, enqueue]);
```

Reduce initial page size:

```ts
const PAGE_SIZE = 36;
```

- [ ] **Step 2.3: Make scroll ownership explicit**

If `.main-content` is the actual scroll container, do not attach the infinite-scroll listener to `.wallpaper-grid`. Use one of these:

Preferred:

```tsx
<div className="wallpaper-grid-scroll" ref={gridRef} onScroll={handleScroll}>
  <div className="wallpaper-grid">
    ...
  </div>
</div>
```

CSS:

```css
.wallpaper-grid-scroll {
  overflow-y: auto;
  min-height: 0;
}
```

Alternative:

- Keep manual "Load more" only.
- Remove the broken scroll listener entirely.

- [ ] **Step 2.4: Add Go-side thumbnail concurrency control**

Create `apps/wails-gui/thumbnail_pool.go`:

```go
package main

import "sync"

var thumbnailSem = make(chan struct{}, 2)
var thumbnailMu sync.Mutex
var thumbnailInFlight = map[string]*thumbnailWaiter{}
var thumbnailFailed = map[string]bool{}

type thumbnailWaiter struct {
	done chan struct{}
	path string
	err  error
}
```

Then update `ThumbnailFor()` in `apps/wails-gui/rust.go`:

- Return cached thumbnails immediately.
- If a path failed this session, return no thumbnail immediately.
- If the same key is already in flight, wait for the first request instead of spawning another generator.
- Wrap generation with `thumbnailSem <- struct{}{}` and `<-thumbnailSem`.

- [ ] **Step 2.5: Route generators by extension**

Change `generateThumbnail(src, dst string)` so videos do not try ImageMagick first.

Required logic:

```text
image/gif extensions: magick -> convert fallback
video extensions: ffmpeg only
unknown: return error
```

This prevents video files from paying failed ImageMagick startup cost before ffmpeg.

- [ ] **Step 2.6: Verify current Wails performance improvement**

Run:

```bash
cd apps/wails-gui
npm run typecheck
npm run build
go test ./...
go vet ./...
go build ./...
wails3 build
cd ../..
./scripts/profile_gui.sh 45 | tee /tmp/wallpaper-console-gui-after-phase2.csv
```

Expected:

- No burst of dozens of `ffmpeg`, `magick`, or `convert` processes.
- At most 2 thumbnail generator processes at a time.
- Scrolling remains usable.
- Peak CPU and UI jank are lower than Phase 0.

- [ ] **Step 2.7: Commit Phase 2**

Run:

```bash
git add apps/wails-gui/frontend/src/components/WallpaperGrid.tsx apps/wails-gui/frontend/src/hooks/useThumbnailQueue.ts apps/wails-gui/rust.go apps/wails-gui/thumbnail_pool.go docs/PERFORMANCE_BASELINE.md
git commit -m "perf(gui): throttle thumbnail loading and generation"
```

---

## Phase 3: Make SQLite the Real Library Source for the GUI

This avoids loading and sorting the full library in React for normal browsing.

**Files:**

- Modify: `crates/wc-storage/src/sqlite.rs`
- Modify: `crates/wc-cli/src/main.rs`
- Modify: `apps/wails-gui/rust.go`
- Modify: `apps/wails-gui/app.go`
- Modify: `apps/wails-gui/frontend/src/api/bridge.ts`
- Modify: `apps/wails-gui/frontend/src/views/LibraryView.tsx`

- [ ] **Step 3.1: Add paginated library query in Rust**

Add a Rust command:

```text
library-page-json --source sqlite --filter all|image|gif|video --sort newest|largest|name --search QUERY --offset N --limit N
```

Output:

```json
{
  "total": 1234,
  "items": [
    {
      "path": "/path/file.mp4",
      "type": "video",
      "ext": "mp4",
      "backend": "mpvpaper",
      "size": 123,
      "mtime": 1710000000,
      "resolution": "1920x1080"
    }
  ]
}
```

SQL behavior:

```sql
SELECT path, type, ext, backend, size, mtime, resolution
FROM wallpapers
WHERE (:filter = 'all' OR type = :filter)
  AND (:search = '' OR lower(path) LIKE '%' || lower(:search) || '%')
ORDER BY
  CASE WHEN :sort = 'newest' THEN mtime END DESC,
  CASE WHEN :sort = 'largest' THEN size END DESC,
  CASE WHEN :sort = 'name' THEN path END ASC
LIMIT :limit OFFSET :offset;
```

Also query total count with the same filter/search.

- [ ] **Step 3.2: Add Wails binding for paginated library**

Add DTOs in `apps/wails-gui/rust.go`:

```go
type LibraryPageDTO struct {
	Total int            `json:"total"`
	Items []WallpaperDTO `json:"items"`
}
```

Add method:

```go
func (r *Runner) LibraryPage(source, filter, sort, search string, offset, limit int) (*LibraryPageDTO, error)
```

For the interim Wails bridge, call:

```text
wallpaper-console-rust library-page-json --source SOURCE --filter FILTER --sort SORT --search SEARCH --offset OFFSET --limit LIMIT
```

- [ ] **Step 3.3: Generate Wails bindings**

Run:

```bash
cd apps/wails-gui
wails3 generate bindings -ts -i -names ./...
```

Expected:

- `frontend/bindings/wallpaper-console-gui/bridge.ts` contains `LibraryPage`.

- [ ] **Step 3.4: Update React library view**

Change `LibraryView.tsx`:

- Read `gui_library_source` from config on mount.
- Default to `sqlite` when `storage_backend=sqlite`.
- Load one page at a time with `LibraryPage`.
- Do not fetch the entire library for filtering/sorting/searching.
- Debounce search by 200 ms.
- Reset offset to 0 when filter/sort/search changes.

- [ ] **Step 3.5: Verify large-library behavior**

Create a test fixture with at least 5,000 fake SQLite rows.

Expected:

- Initial GUI load fetches only the first page.
- Search/filter/sort triggers one paged request, not a full library load.
- Memory usage does not grow linearly with total library size.

- [ ] **Step 3.6: Commit Phase 3**

Run:

```bash
git add crates/wc-storage/src/sqlite.rs crates/wc-cli/src/main.rs apps/wails-gui
git commit -m "perf(gui): add paginated sqlite library API"
```

---

## Phase 4: Replace Wails With a Rust-Native GUI Shell

This is the best long-term fix. Wails can remain as a temporary app while this is built.

**Decision:** Use Tauri 2 + React/TypeScript unless the user explicitly requests native GTK4/libadwaita.

**Files:**

- Create: `apps/tauri-gui/`
- Reuse: `apps/wails-gui/frontend/src/`
- Modify: root `Cargo.toml`
- Modify: `install.sh`
- Modify: `docs/MIGRATION_STATUS.md`

- [ ] **Step 4.1: Scaffold a Tauri app**

Run:

```bash
cargo tauri init apps/tauri-gui
```

If `cargo-tauri` is missing, install it only after explicit user approval:

```bash
cargo install tauri-cli --version '^2'
```

- [ ] **Step 4.2: Move shared frontend code**

Create:

```text
apps/frontend/
```

Move reusable React files from:

```text
apps/wails-gui/frontend/src
```

to:

```text
apps/frontend/src
```

Both Wails and Tauri can temporarily import or copy from this shared frontend during migration.

- [ ] **Step 4.3: Implement Tauri Rust commands directly over Rust crates**

Create `apps/tauri-gui/src-tauri/src/commands.rs` with commands equivalent to current Wails bridge:

```rust
#[tauri::command]
async fn status() -> Result<StatusDto, String> {
    // Use wc_storage and wc_core directly.
}

#[tauri::command]
async fn library_page(
    source: String,
    filter: String,
    sort: String,
    search: String,
    offset: u32,
    limit: u32,
) -> Result<LibraryPageDto, String> {
    // Use wc_storage SQLite query directly.
}

#[tauri::command]
async fn apply(path: String) -> Result<CommandResult, String> {
    // Use wc_backend directly.
}
```

Do not call `wallpaper-console-rust` as a subprocess from Tauri commands except for temporary compatibility tasks explicitly documented.

- [ ] **Step 4.4: Implement thumbnail service in Rust**

Create:

```text
crates/wc-thumbnail/
```

Responsibilities:

- Compute cache key from `realpath:mtime:size`.
- Store thumbnails under `cache/gui-thumbnails`.
- Use a worker queue with concurrency 2.
- Skip duplicate in-flight requests.
- Keep a session failure cache.
- Route by extension: images through ImageMagick or Rust image pipeline, videos through `ffmpeg`.

Expose API:

```rust
pub struct ThumbnailRequest {
    pub path: PathBuf,
}

pub struct ThumbnailResult {
    pub path: PathBuf,
    pub thumbnail: Option<PathBuf>,
    pub cache_hit: bool,
}

pub async fn thumbnail_for(request: ThumbnailRequest) -> anyhow::Result<ThumbnailResult>;
```

- [ ] **Step 4.5: Replace Wails bridge in frontend**

Create `apps/frontend/src/api/tauriBridge.ts`:

```ts
import { invoke } from '@tauri-apps/api/core';

export const api = {
  status: () => invoke('status'),
  libraryPage: (args) => invoke('library_page', args),
  apply: (path: string) => invoke('apply', { path }),
  stop: () => invoke('stop'),
  restore: () => invoke('restore'),
};
```

Keep `apps/wails-gui/frontend/src/api/bridge.ts` untouched until Tauri reaches parity.

- [ ] **Step 4.6: Tauri verification**

Run:

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cd apps/tauri-gui
npm run typecheck
npm run build
cargo tauri build
```

Expected:

- Tauri binary builds.
- GUI opens.
- Library page loads without Wails.
- Normal UI operations do not spawn `wallpaper-console-rust` subprocesses.

- [ ] **Step 4.7: Commit Phase 4**

Run:

```bash
git add Cargo.toml crates/wc-thumbnail apps/tauri-gui apps/frontend docs/MIGRATION_STATUS.md
git commit -m "feat(gui): add tauri rust-native gui shell"
```

---

## Phase 5: Native Virtualized Library UX

**Files:**

- Modify: `apps/frontend/src/views/LibraryView.tsx`
- Modify: `apps/frontend/src/components/WallpaperGrid.tsx`
- Add dependency: `@tanstack/react-virtual`

- [ ] **Step 5.1: Add real virtualization**

Install:

```bash
cd apps/frontend
npm install @tanstack/react-virtual
```

Use `useVirtualizer` with a fixed card height and responsive column count.

Required behavior:

- Only render rows currently visible plus overscan.
- Keep total scroll height correct.
- Thumbnail requests only occur for visible/overscan items.
- Changing filter/sort/search cancels old pending thumbnail work.

- [ ] **Step 5.2: Add search debounce**

Implement a `useDebouncedValue(search, 200)` hook and trigger page reload only after the debounce.

- [ ] **Step 5.3: Add performance tests**

Add a large fake dataset test:

```text
10,000 library rows
```

Assertions:

- Initial render creates fewer than 200 card DOM nodes.
- Changing filter does not freeze the UI.
- Thumbnail queue active count never exceeds 2.

- [ ] **Step 5.4: Commit Phase 5**

Run:

```bash
git add apps/frontend package.json package-lock.json
git commit -m "perf(gui): virtualize library grid"
```

---

## Phase 6: Final Migration Cutover

**Files:**

- Modify: `install.sh`
- Modify: `docs/MIGRATION_STATUS.md`
- Modify: `README.md`
- Create: `docs/DEPRECATE_BASH_PYTHON.md`

- [ ] **Step 6.1: Keep old implementation available**

Install Rust binaries as:

```text
~/.local/bin/wallpaper-console-rust
~/.local/bin/wallpaper-console-gui-rust
```

Do not overwrite:

```text
~/.local/bin/wallpaper-console
~/.local/bin/wallpaper-console-gui
```

until the user explicitly requests cutover.

- [ ] **Step 6.2: Add explicit cutover command**

Add install option:

```bash
./install.sh --replace-original
```

Behavior:

- Backup old symlinks to `~/.local/bin/wallpaper-console.bak.TIMESTAMP`.
- Point `wallpaper-console` to Rust CLI.
- Point `wallpaper-console-gui` to Tauri GUI.
- Print rollback commands.

- [ ] **Step 6.3: Validate niri integration**

Document:

```kdl
spawn-at-startup "wallpaper-console-rust" "restore"
```

And GUI launch:

```kdl
Mod+Shift+0 { spawn "wallpaper-console-gui-rust"; }
```

If a distinct niri `app-id` is needed for floating rules, ensure the Tauri window sets:

```text
app-id = wallpaper-console-gui-rust
title = Wallpaper Console
```

- [ ] **Step 6.4: Cutover acceptance tests**

Run:

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cd apps/tauri-gui
npm run typecheck
npm run build
cargo tauri build
cd ../..
./install.sh --build-only
```

Manual checks:

```text
1. Open GUI.
2. Add source.
3. Rescan.
4. Browse/filter/search/sort library.
5. Apply image.
6. Apply video.
7. Add/remove favorite.
8. Browse history.
9. Verify SQLite.
10. Stop backends.
11. Quit GUI and confirm wallpaper remains.
12. Re-login or run restore and confirm wallpaper restores.
```

- [ ] **Step 6.5: Commit Phase 6**

Run:

```bash
git add install.sh README.md docs/MIGRATION_STATUS.md docs/DEPRECATE_BASH_PYTHON.md
git commit -m "docs: prepare rust cutover and rollback"
```

---

## Acceptance Gates

Migration is complete only when all of these are true:

- [ ] Rust CLI command parity is complete or each intentionally removed command is documented with a replacement.
- [ ] `COMMAND_PARITY.md` and `MIGRATION_STATUS.md` agree.
- [ ] Rust GUI can add/remove/list sources reliably in flat, hybrid, and sqlite modes.
- [ ] Rust GUI uses paginated SQLite for library browsing by default when SQLite is available.
- [ ] GUI opening does not launch an uncontrolled burst of thumbnail generator processes.
- [ ] Thumbnail worker concurrency is capped at 2 by test or instrumentation.
- [ ] Large library test with 10,000 rows remains responsive.
- [ ] Normal GUI operations do not spawn the Rust CLI once the Tauri GUI is the primary GUI.
- [ ] Images route to `awww`, videos route to `mpvpaper`, GIFs follow config.
- [ ] Apply updates `current`, `last_backend`, and history only after success.
- [ ] Stop-before-apply invariants are preserved.
- [ ] Video wallpapers remain after the GUI exits.
- [ ] niri autostart restore works.
- [ ] Installation has a rollback path.

---

## DeepSeek Implementation Instructions

DeepSeek should execute this plan in order and stop only at explicit phase gates:

1. Do not skip Phase 0 profiling. Performance fixes must be evidence-based.
2. Do not mark migration complete while `tui` is a stub unless the docs explicitly state Wails/Tauri replaces TUI and the user accepts it.
3. Do not keep Wails as the final architecture if the goal is best CPU/memory behavior. Wails is acceptable only as an interim stabilization target.
4. Do not solve thumbnail jank by disabling thumbnails globally. Implement queueing, caching, and virtualization.
5. Do not call the Rust CLI from the final Rust-native GUI for normal operations.
6. Do not overwrite the old Bash/Python commands without explicit `--replace-original`.
7. Commit after every phase.
8. Run the listed verification commands after every phase.
9. If a phase fails, document the exact failing command and do not proceed to the next phase.
