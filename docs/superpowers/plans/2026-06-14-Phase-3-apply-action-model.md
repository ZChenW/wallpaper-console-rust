# Phase 3 — Frontend Apply Action Model Consolidation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make backend DTO's `applyActions` the single source of truth for frontend action model, eliminating scattered hardcoded type checks in LibraryView/WallpaperGrid.

**Architecture:** Create a pure-TS domain layer (`domain/applyActions.ts`) that normalizes `ApplyActionDTO[]` into `NormalizedApplyAction[]`, with a single centralized legacy fallback for old DTOs. LibraryView builds context menu actions per-entry from normalized actions via `buildContextActions`. WallpaperGrid uses `isApplyAvailable()` instead of inline type checks.

**Tech Stack:** TypeScript, React, Node test runner, Playwright smoke tests. No new dependencies.

---

## File Structure

```
apps/tauri-gui/frontend/src/
  domain/
    applyActions.ts          (CREATE)
    applyActions.test.ts     (CREATE)
  views/
    LibraryView.tsx          (MODIFY — delete hasAction/isFailedScene, add buildContextActions)
  components/
    WallpaperGrid.tsx        (MODIFY — accept buildContextActions prop, import isApplyAvailable)
  api/
    bridge.ts                (NO CHANGE)
    mockBridge.ts            (VERIFY only — already correct)
  e2e/
    smoke.spec.ts            (MODIFY — add Apply preview GIF assert for WE Web)
docs/
  CURRENT_STATUS.md          (MODIFY — add Phase 3 entry)
  DEVELOPMENT.md             (MODIFY — add domain layer note)
  TAURI_MANUAL_SMOKE_CHECKLIST.md (MODIFY — update action descriptions)
```

---

### Task 1: Create domain/applyActions.ts

**Files:**
- Create: `apps/tauri-gui/frontend/src/domain/applyActions.ts`

- [ ] **Step 1: Create domain directory and file**

```bash
mkdir -p apps/tauri-gui/frontend/src/domain
```

```typescript
import type { WallpaperDTO, ApplyActionDTO, ApplyActionKind } from '../api/bridge';

export type NormalizedApplyAction = {
  kind: ApplyActionKind;
  label: string;
  enabled: boolean;
  reason?: string;
};

const VALID_KINDS: ApplyActionKind[] = [
  'apply',
  'retry_backend_apply',
  'apply_preview',
  'open_folder',
  'copy_workshop_id',
];

function isValidAction(a: ApplyActionDTO): a is ApplyActionDTO & { kind: ApplyActionKind; label: string; enabled: boolean } {
  if (!a || !a.kind || !a.label) return false;
  if (typeof a.enabled !== 'boolean') return false;
  if (!VALID_KINDS.includes(a.kind)) return false;
  return true;
}

export function normalizeApplyActions(entry: WallpaperDTO): NormalizedApplyAction[] {
  if (entry.applyActions && entry.applyActions.length > 0) {
    return entry.applyActions.filter(isValidAction).map((a) => ({
      kind: a.kind,
      label: a.label,
      enabled: a.enabled,
      reason: a.reason,
    }));
  }

  // Legacy fallback — only used when backend DTO lacks applyActions (old DTO compat).
  // New backends MUST provide applyActions; this path exists for backward compatibility.
  const actions: NormalizedApplyAction[] = [];
  const canOpenFolder = Boolean(entry.path);
  const canCopyWorkshopId = Boolean(entry.workshopId);

  if (entry.type === 'image' || entry.type === 'gif' || entry.type === 'video') {
    actions.push({ kind: 'apply', label: 'Apply', enabled: true });
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
  } else if (entry.type === 'we_scene') {
    actions.push({ kind: 'apply', label: 'Apply', enabled: true });
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
    if (canCopyWorkshopId) actions.push({ kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true });
  } else if (entry.type === 'we_web') {
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
    if (canCopyWorkshopId) actions.push({ kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true });
  } else if (entry.type === 'unsupported') {
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
    if (canCopyWorkshopId) actions.push({ kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true });
  }

  return actions;
}

export function hasEnabledAction(entry: WallpaperDTO, kind: ApplyActionKind): boolean {
  return normalizeApplyActions(entry).some((a) => a.kind === kind && a.enabled);
}

export function isApplyAvailable(entry: WallpaperDTO): boolean {
  return hasEnabledAction(entry, 'apply');
}

export function getActionReason(entry: WallpaperDTO, kind: ApplyActionKind): string | undefined {
  const a = normalizeApplyActions(entry).find((a) => a.kind === kind);
  return a?.reason;
}
```

- [ ] **Step 2: Verify file typechecks**

```bash
cd apps/tauri-gui/frontend && npm run typecheck
```

Expected: no new type errors.

---

### Task 2: Write domain/applyActions.test.ts

**Files:**
- Create: `apps/tauri-gui/frontend/src/domain/applyActions.test.ts`

- [ ] **Step 1: Write tests — with-applyActions cases**

```typescript
import { describe, it } from 'node:test';
import assert from 'node:assert';
import { normalizeApplyActions, hasEnabledAction, isApplyAvailable, getActionReason } from './applyActions';
import type { WallpaperDTO } from '../api/bridge';

const IMAGE: WallpaperDTO = {
  path: '/test/image.jpg', type: 'image', ext: 'jpg', backend: 'awww',
  size: 1024, mtime: 1, resolution: '1920x1080',
  applyAvailability: 'available', applyBackend: 'awww',
  applyActions: [
    { kind: 'apply', label: 'Apply', enabled: true },
    { kind: 'open_folder', label: 'Open folder', enabled: true },
  ],
};

const WE_SCENE: WallpaperDTO = {
  path: '/test/scene', type: 'we_scene', ext: 'scene', backend: 'linux-wallpaperengine',
  size: 4096, mtime: 1, resolution: 'WE',
  projectType: 'we_scene', previewPath: '/test/scene/preview.gif',
  workshopId: '123', title: 'Test Scene',
  applyAvailability: 'available', applyBackend: 'linux-wallpaperengine',
  applyActions: [
    { kind: 'apply', label: 'Apply', enabled: true },
    { kind: 'apply_preview', label: 'Apply preview GIF', enabled: true },
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

const WE_SCENE_FAILED: WallpaperDTO = {
  path: '/test/scene-failed', type: 'we_scene', ext: 'scene', backend: 'linux-wallpaperengine',
  size: 4096, mtime: 1, resolution: 'WE',
  projectType: 'we_scene', previewPath: '/test/scene-failed/preview.gif',
  workshopId: '456', title: 'Failed Scene',
  backendStatus: 'failed',
  applyAvailability: 'retryable_failure', applyBackend: 'linux-wallpaperengine',
  applyActions: [
    { kind: 'retry_backend_apply', label: 'Retry backend apply', enabled: true },
    { kind: 'apply_preview', label: 'Apply preview GIF', enabled: true },
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

const WE_WEB: WallpaperDTO = {
  path: '/test/web', type: 'we_web', ext: 'web', backend: 'unsupported',
  size: 8192, mtime: 1, resolution: 'WE',
  projectType: 'we_web', workshopId: '789', title: 'Web Title',
  applyAvailability: 'unsupported', applyReason: 'browsing only',
  applyActions: [
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

const UNSUPPORTED: WallpaperDTO = {
  path: '/test/app', type: 'unsupported', ext: 'application', backend: 'unsupported',
  size: 1024, mtime: 1, resolution: 'WE',
  projectType: 'unsupported', workshopId: '999',
  applyAvailability: 'unsupported',
  applyActions: [
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

describe('normalizeApplyActions with applyActions present', () => {
  it('image has apply and open_folder', () => {
    const a = normalizeApplyActions(IMAGE);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('apply'));
    assert(kinds.includes('open_folder'));
  });

  it('image isApplyAvailable true', () => {
    assert(isApplyAvailable(IMAGE));
  });

  it('WE Scene normal has apply, apply_preview, open_folder, copy_workshop_id', () => {
    const a = normalizeApplyActions(WE_SCENE);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('apply'));
    assert(kinds.includes('apply_preview'));
    assert(kinds.includes('open_folder'));
    assert(kinds.includes('copy_workshop_id'));
  });

  it('failed WE Scene has retry_backend_apply and no apply', () => {
    const a = normalizeApplyActions(WE_SCENE_FAILED);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('retry_backend_apply'));
    assert(!kinds.includes('apply'));
  });

  it('failed WE Scene isApplyAvailable false', () => {
    assert(!isApplyAvailable(WE_SCENE_FAILED));
  });

  it('WE Web has open_folder and copy_workshop_id, no apply or apply_preview', () => {
    const a = normalizeApplyActions(WE_WEB);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('open_folder'));
    assert(kinds.includes('copy_workshop_id'));
    assert(!kinds.includes('apply'));
    assert(!kinds.includes('apply_preview'));
  });

  it('unknown action kind is ignored silently', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: 'apply' as any, label: 'Apply', enabled: true },
        { kind: 'bogus' as any, label: 'Bogus', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'apply');
  });

  it('malformed: missing label filtered', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: 'apply', label: '', enabled: true } as any,
        { kind: 'open_folder', label: 'Open folder', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'open_folder');
  });

  it('malformed: enabled=false filtered', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: 'apply', label: 'Apply', enabled: false },
        { kind: 'open_folder', label: 'Open folder', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'open_folder');
  });

  it('malformed: missing kind filtered', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: undefined as any, label: 'Bad', enabled: true },
        { kind: 'open_folder', label: 'Open folder', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'open_folder');
  });

  it('getActionReason returns reason', () => {
    const entry: WallpaperDTO = {
      ...IMAGE,
      applyActions: [
        { kind: 'apply', label: 'Apply', enabled: true, reason: 'test reason' },
      ],
    };
    assert.equal(getActionReason(entry, 'apply'), 'test reason');
  });

  it('preserves DTO order', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'we_scene', ext: 'scene', backend: 'linux-wallpaperengine',
      size: 1, mtime: 1, resolution: 'WE',
      applyActions: [
        { kind: 'apply_preview', label: 'Preview', enabled: true },
        { kind: 'apply', label: 'Apply', enabled: true },
        { kind: 'open_folder', label: 'Folder', enabled: true },
        { kind: 'copy_workshop_id', label: 'Copy', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert.equal(a[0].kind, 'apply_preview');
    assert.equal(a[1].kind, 'apply');
    assert.equal(a[2].kind, 'open_folder');
    assert.equal(a[3].kind, 'copy_workshop_id');
  });
});

describe('legacy fallback (applyActions missing)', () => {
  it('image fallback has apply', () => {
    const entry: WallpaperDTO = {
      path: '/test/img.jpg', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
    };
    assert(isApplyAvailable(entry));
  });

  it('we_web fallback has no apply', () => {
    const entry: WallpaperDTO = {
      path: '/test/web', type: 'we_web', ext: 'web', backend: 'unsupported',
      size: 1, mtime: 1, resolution: 'WE',
    };
    assert(!isApplyAvailable(entry));
    const a = normalizeApplyActions(entry);
    assert(a.some(x => x.kind === 'open_folder'));
    assert(!a.some(x => x.kind === 'apply'));
  });

  it('unsupported fallback has no apply', () => {
    const entry: WallpaperDTO = {
      path: '/test/exe', type: 'unsupported', ext: 'exe', backend: 'unsupported',
      size: 1, mtime: 1, resolution: 'WE',
    };
    assert(!isApplyAvailable(entry));
  });
});
```

- [ ] **Step 2: Run unit tests**

```bash
cd apps/tauri-gui/frontend && npm run test:unit
```

Expected: all tests pass.

---

### Task 3: Refactor WallpaperGrid.tsx

**Files:**
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`

- [ ] **Step 1: Add import for isApplyAvailable**

After line 4 (`import { WallpaperDTO } from '../api/bridge';`), add:

```typescript
import { isApplyAvailable } from '../domain/applyActions';
```

- [ ] **Step 2: Add buildContextActions to Props interface**

In the Props interface (lines 9-18), add after `contextActions`:

```typescript
  buildContextActions?: (entry: WallpaperDTO) => ContextAction[];
```

- [ ] **Step 3: Replace canApply function**

Replace the `canApply` function (lines 134-139):

```typescript
  const canApply = (entry: WallpaperDTO): boolean => isApplyAvailable(entry);
```

- [ ] **Step 4: Update context menu actions resolution**

In the ContextMenu render (around line 242), change:

```typescript
          actions={contextActions.filter((action) => !action.visible || action.visible(contextEntry))}
```

to:

```typescript
          actions={buildContextActions
            ? buildContextActions(contextEntry)
            : contextActions.filter((action) => !action.visible || action.visible(contextEntry))}
```

- [ ] **Step 5: Verify typecheck**

```bash
cd apps/tauri-gui/frontend && npm run typecheck
```

---

### Task 4: Refactor LibraryView.tsx

**Files:**
- Modify: `apps/tauri-gui/frontend/src/views/LibraryView.tsx`

- [ ] **Step 1: Update import**

On line 3, change:

```typescript
import { api, WallpaperDTO, ApplyActionKind } from '../api/bridge';
```

to:

```typescript
import { api, WallpaperDTO } from '../api/bridge';
import { normalizeApplyActions, isApplyAvailable } from '../domain/applyActions';
```

- [ ] **Step 2: Remove isFailedScene and hasAction helpers**

Delete lines 68-69 (the `isFailedScene` callback definition) and lines 120-122 (the `hasAction` callback definition).

The `isFailedScene` helper was only used in the contextActions `visible` for "Retry backend apply". Remove it entirely.

- [ ] **Step 3: Add buildContextActions callback**

After `handleBatchAddFavorites` (after line 118), insert:

```typescript
  const buildContextActions = useCallback((entry: WallpaperDTO): ContextAction[] => {
    const actions: ContextAction[] = [];

    for (const a of normalizeApplyActions(entry)) {
      if (!a.enabled) continue;
      if (a.kind === 'apply') continue; // handled by ContextMenu's canApply prop

      switch (a.kind) {
        case 'retry_backend_apply':
          actions.push({
            label: a.label,
            action: async (path: string) => {
              try { await api.weClearBackendError(path); } catch { /* */ }
              onApply(path);
              setTimeout(() => invalidateLibrary(), 500);
            },
          });
          break;
        case 'apply_preview':
          if (entry.previewPath) {
            actions.push({
              label: a.label,
              action: (path: string) => {
                const e = entryByPath.get(path);
                if (e?.previewPath) onApply(e.previewPath);
              },
            });
          }
          break;
        case 'open_folder':
          actions.push({
            label: a.label,
            action: handleOpenProjectFolder,
          });
          break;
        case 'copy_workshop_id':
          if (entry.workshopId) {
            actions.push({
              label: a.label,
              action: async (path: string) => {
                const e = entryByPath.get(path);
                if (e?.workshopId) {
                  try {
                    await navigator.clipboard?.writeText(e.workshopId);
                  } catch {
                    window.dispatchEvent(new CustomEvent('wc-feedback', {
                      detail: { state: 'error', label: 'Copy Workshop ID', detail: 'Clipboard write failed' },
                    }));
                  }
                }
              },
            });
          }
          break;
      }
    }

    actions.push({
      label: 'Add to Favorites',
      action: async (path: string) => {
        const r = await api.favoriteAdd(path);
        if (!r.success) throw new Error(r.stderr || 'Add to Favorites failed');
        invalidateFavoritesCache();
      },
    });

    return actions;
  }, [onApply, invalidateLibrary, entryByPath, handleOpenProjectFolder]);
```

- [ ] **Step 4: Remove old contextActions useMemo**

Delete the entire `contextActions` useMemo block (lines 124-165):

```typescript
  const contextActions: ContextAction[] = useMemo(() => [
    ...
  ], [onApply, invalidateLibrary, entryByPath, isFailedScene, hasAction, handleOpenProjectFolder]);
```

- [ ] **Step 5: Update WallpaperGrid prop**

In the JSX (around line 214), change:

```typescript
        <WallpaperGrid
          entries={entries}
          onApply={onApply}
          applying={applying}
          emptyText="Library is empty. Add sources or scan Wallpaper Engine."
          contextActions={contextActions}
          active={active}
          selectedPaths={selectedPaths}
          onSelectionChange={setSelectedPaths}
        />
```

to:

```typescript
        <WallpaperGrid
          entries={entries}
          onApply={onApply}
          applying={applying}
          emptyText="Library is empty. Add sources or scan Wallpaper Engine."
          buildContextActions={buildContextActions}
          active={active}
          selectedPaths={selectedPaths}
          onSelectionChange={setSelectedPaths}
        />
```

- [ ] **Step 6: Verify typecheck**

```bash
cd apps/tauri-gui/frontend && npm run typecheck
```

---

### Task 5: Update WallpaperGrid double-click handler

**Files:**
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`

- [ ] **Step 1: Verify the double-click handler uses the new canApply**

The `handleDoubleClick` function (line 124) already calls `canApply(entry)` which now uses `isApplyAvailable(entry)`. No further change needed — verify the handler logic is correct:

```typescript
  const handleDoubleClick = (entry: WallpaperDTO) => {
    if (!canApply(entry)) {
      window.dispatchEvent(new CustomEvent('wc-feedback', {
        detail: { state: 'warning', label: 'Cannot apply', detail: 'This item cannot be applied as a live wallpaper.' },
      }));
      return;
    }
    onApply(entry.path);
  };
```

This is correct as-is after the `canApply` change in Task 3. WE Web scenes return `isApplyAvailable=false`, so double-click will show the warning. Failed WE Scene with no `apply` action returns `isApplyAvailable=false`, so double-click will also show the warning.

---

### Task 6: Verify mockBridge.ts consistency

**Files:**
- Verify: `apps/tauri-gui/frontend/src/api/mockBridge.ts`

- [ ] **Step 1: Check all mock entries have correct applyActions**

Running verification checklist (no code changes needed — data is already correct):

| Mock item | Expected actions | Actual | OK? |
|-----------|-----------------|--------|-----|
| WE Scene normal | apply, apply_preview, open_folder, copy_workshop_id | ✓ | Yes |
| WE Scene failed | retry_backend_apply, apply_preview, open_folder, copy_workshop_id (no apply) | ✓ | Yes |
| WE Web | open_folder, copy_workshop_id (no apply, no apply_preview) | ✓ | Yes |
| Unsupported application | open_folder, copy_workshop_id (no apply) | ✓ | Yes |
| Image/video entries | apply, open_folder | ✓ | Yes |

- [ ] **Step 2: Verify favorites/history fallback entries have applyActions**

The fallback entries in `favoritesList` and `historyList` already have `applyActions` with `apply` and `open_folder`. Confirmed.

No changes needed to mockBridge.ts.

---

### Task 7: Update smoke tests

**Files:**
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`

- [ ] **Step 1: Add WE Web Apply preview GIF assertion**

In the `'WE Web is indexed but unsupported for live apply'` test (line 9), after the existing `Apply` count assertion (line 16), add:

```typescript
  await expect(page.getByText('Apply preview GIF')).toHaveCount(0);
```

Full updated test block (lines 9-21 become):

```typescript
test('WE Web is indexed but unsupported for live apply', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_web');
  const card = page.locator('.wallpaper-card').first();
  await expect(card.getByText('WE Web · Unsupported')).toBeVisible();
  await expect(card.getByText(/Web wallpaper — unsupported/)).toBeVisible();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Apply preview GIF')).toHaveCount(0);
  await expect(page.getByText('Open experimental Chromium preview')).toHaveCount(0);
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
  await expect(page.getByText('Open folder')).toBeVisible();
  await expect(page.getByText('Copy Workshop ID')).toBeVisible();
});
```

- [ ] **Step 2: Run smoke tests**

```bash
cd apps/tauri-gui/frontend && npm run smoke
```

Expected: all tests pass (38-39 tests).

---

### Task 8: Update documentation

**Files:**
- Modify: `docs/CURRENT_STATUS.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`

- [ ] **Step 1: Update CURRENT_STATUS.md**

Add this row to the status table (after line 36, before the separator):

```markdown
| Frontend apply action model consolidation | In progress | Unit tests, typecheck, smoke tests, domain layer |
```

- [ ] **Step 2: Update DEVELOPMENT.md**

After the "Application Service Layer" section (after line 31), add:

```markdown
## Frontend Apply Action Domain

Frontend action availability is driven by the backend `ApplyPlan::plan_for_entry()` through `WallpaperDTO.applyActions`. All UI action decisions (Apply, Retry, Preview, Open folder, Copy Workshop ID) use `domain/applyActions.ts` to normalize DTO actions into enabled/disabled render decisions:

```ts
import { normalizeApplyActions, isApplyAvailable, hasEnabledAction } from './domain/applyActions';

const actions = normalizeApplyActions(entry);
if (isApplyAvailable(entry)) { /* show Apply button */ }
```

A legacy fallback exists for old DTOs without `applyActions` (centralized in one file). New backends MUST provide `applyActions`. Frontend components should NOT infer action availability from `entry.type` or `entry.backendStatus`.
```

- [ ] **Step 3: Update TAURI_MANUAL_SMOKE_CHECKLIST.md**

On line 50, update the WE Scene context menu description from:

```markdown
- [ ] Right-click a WE Scene card. Confirm the generic `Apply`, `Apply preview GIF`, and `Copy Workshop ID` are visible. `Apply with linux-wallpaperengine` must NOT appear.
```

to:

```markdown
- [ ] Right-click a WE Scene card. Confirm `Apply`, `Apply preview GIF`, `Open folder`, and `Copy Workshop ID` are visible. `Apply with linux-wallpaperengine` must NOT appear.
```

On line 51, update the WE Web description from:

```markdown
- [ ] Right-click a WE Web card. Confirm only browsing actions such as `Open folder` and `Copy Workshop ID` are visible. `Apply`, `Apply preview GIF`, `Apply with linux-wallpaperengine`, `Apply Web wallpaper`, and `Open experimental Chromium preview` must NOT appear.
```

to:

```markdown
- [ ] Right-click a WE Web card. Confirm only `Open folder` and `Copy Workshop ID` are visible. `Apply`, `Apply preview GIF`, `Apply with linux-wallpaperengine`, `Apply Web wallpaper`, and `Open experimental Chromium preview` must NOT appear.
```

---

### Task 9: Full verification

**Files:** (all modified files)

- [ ] **Step 1: Run Rust verification**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Expected: 176 tests pass, no clippy warnings, formatting clean.

- [ ] **Step 2: Run frontend verification**

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npm run build
npm run smoke
```

Expected: unit tests pass, typecheck clean, build succeeds, 38-39 smoke tests pass.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: Phase 3 frontend apply action model consolidation"
```

---

### Task 10: Code review round 1 — Behavioral consistency

**Checklist:**
- [ ] Backend ApplyPlan rules match frontend normalize behavior
  - Image/Gif/Video → `apply` + `open_folder`
  - WE Scene normal → `apply` + `apply_preview` + `open_folder` + `copy_workshop_id`
  - WE Scene failed → `retry_backend_apply` + `apply_preview` + `open_folder` + `copy_workshop_id` (NO `apply`)
  - WE Web → `open_folder` + `copy_workshop_id` (NO `apply`, NO `apply_preview`)
  - Unsupported → `open_folder` + `copy_workshop_id` (NO `apply`)
- [ ] WE Web has zero apply-class actions in context menu
- [ ] Failed WE Scene shows Retry, not Apply
- [ ] Normal image/video still has Apply + Open folder
- [ ] MockBridge data matches backend ApplyPlan

If any discrepancy found, fix and re-run verification.

---

### Task 11: Code review round 2 — Architecture and maintainability

**Checklist:**
- [ ] LibraryView has no remaining `entry.type === 'we_web'` checks determining actions
- [ ] LibraryView has no remaining `isFailedScene` helper
- [ ] LibraryView has no remaining `hasAction` helper
- [ ] WallpaperGrid has no remaining `entry.type !== 'we_web'` check for `canApply`
- [ ] All action kind → handler mapping is in LibraryView's `buildContextActions`
- [ ] Legacy fallback (no applyActions) exists ONLY in `applyActions.ts`
- [ ] Unknown action kinds are silently ignored (no throw)
- [ ] `ContextMenu` component unchanged (except WallpaperGrid's prop passing)

If any issue found, fix and re-run verification.

---

### Task 12: Code review round 3 — Tests and docs

**Checklist:**
- [ ] Unit tests cover all action types: image, we_scene, we_scene_failed, we_web, unsupported
- [ ] Unit tests cover malformed actions: missing label, missing kind, enabled=false, unknown kind
- [ ] Unit tests cover legacy fallback for image, we_web, unsupported
- [ ] Unit tests cover `getActionReason`
- [ ] Unit tests cover order preservation
- [ ] Smoke tests verify WE Web has no Apply and no Apply preview GIF
- [ ] Smoke tests verify WE Scene menus
- [ ] Smoke tests verify failed WE Scene menus
- [ ] MockBridge data consistent with real DTO
- [ ] Docs reference "Apply with linux-wallpaperengine" only in negation context
- [ ] No stale "Apply with linux-wallpaperengine" as a visible action in any docs

If any issue found, fix and re-run full verification.

---

## Verification Matrix (run after all tasks)

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings

cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
npm run build
npm run smoke
```

All MUST pass before this phase is merge-ready.
