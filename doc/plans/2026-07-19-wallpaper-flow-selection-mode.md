# Wallpaper Flow Selection Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persisted, production-quality Flow wallpaper browser beside Grid, preserving all existing Library state and actions while providing a virtualized Obys-inspired central stream, synchronized index/metadata rails, safe previews, keyboard/touch control, and responsive accessibility.

**Architecture:** `SinglePageShell` remains the single owner of query, selection, runtime current state, favorites, details, and Apply queue. A new `LibraryViewport` mounts exactly one adapter from a neutral `LibraryViewModel`; Grid keeps its behavior while `WallpaperFlow` owns only center, hover, snapping, index, and enhanced-preview state. Pure helper modules contain navigation, stable-anchor, media-eligibility, and presentation logic so behavior is test-first and independently verifiable.

**Tech Stack:** React 19, TypeScript 5.7, TanStack Virtual, Tauri 2 file URLs, Lucide icons, Node test runner, Playwright, CSS container queries.

---

### Task 1: Persist the Library view mode

**Files:**
- Modify: `apps/tauri-gui/frontend/src/shell/shellPreferences.ts`
- Modify: `apps/tauri-gui/frontend/src/shell/shellPreferences.test.ts`

- [ ] **Step 1: Add failing preference tests**

Assert the default is `grid`, round-tripping preserves `flow`, malformed values repair to `grid`, and serialization includes `libraryViewMode` but no transient anchor.

```ts
assert.equal(DEFAULT_SHELL_PREFERENCES.libraryViewMode, 'grid');
assert.equal(parseShellPreferences(serializeShellPreferences({
  ...DEFAULT_SHELL_PREFERENCES,
  libraryViewMode: 'flow',
})).libraryViewMode, 'flow');
assert.equal(parseShellPreferences('{"libraryViewMode":"carousel"}').libraryViewMode, 'grid');
```

- [ ] **Step 2: Verify RED**

Run `node --experimental-strip-types --test src/shell/shellPreferences.test.ts`; expect assertions/typecheck to fail because the field is absent.

- [ ] **Step 3: Add the normalized persisted field**

Define `LibraryViewMode = 'grid' | 'flow'`, add it to `ShellPreferences` and defaults, normalize it against an allow-list, and serialize it explicitly.

- [ ] **Step 4: Verify GREEN**

Run the focused test and `npm run typecheck`; both must pass.

### Task 2: Establish neutral Library contracts and presentation helpers

**Files:**
- Create: `apps/tauri-gui/frontend/src/components/libraryViewModel.ts`
- Create: `apps/tauri-gui/frontend/src/components/wallpaperPresentation.ts`
- Create: `apps/tauri-gui/frontend/src/components/wallpaperPresentation.test.ts`
- Modify: `apps/tauri-gui/frontend/src/components/ContextMenu.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperCard.tsx`
- Modify: `apps/tauri-gui/frontend/src/shell/SinglePageShell.tsx`

- [ ] **Step 1: Add failing presentation and contract tests**

Cover title fallback, source list, normalized type labels, resolution, byte size, author/date/Workshop/backend/compatibility omissions, and state-independent Apply availability.

```ts
assert.equal(presentWallpaper(entry).name, 'Northern Light');
assert.equal(presentWallpaper(entry).sources, 'Pictures, Workshop');
assert.equal(presentWallpaper(entry).resolution, '3840 × 2160');
```

- [ ] **Step 2: Verify RED**

Run the new focused test; expect a missing-module failure.

- [ ] **Step 3: Implement shared contracts**

Move `ContextAction` to `libraryViewModel.ts`; define `LibraryViewModel` with loaded entries, stable state paths, total/pagination flags, reset key, and semantic callbacks. Make selection callbacks accept `LibraryBrowserItemDTO`. Implement the presenter without inventing tags.

- [ ] **Step 4: Adapt Grid without behavior changes**

Update imports/types, remove Shell casts, and preserve existing gestures, virtualization, thumbnail queue, and context menu behavior.

- [ ] **Step 5: Verify GREEN**

Run the focused test, `npm run typecheck`, and all unit tests.

### Task 3: Implement pure Flow state, geometry, navigation, and media policy

**Files:**
- Create: `apps/tauri-gui/frontend/src/components/wallpaperFlowModel.ts`
- Create: `apps/tauri-gui/frontend/src/components/wallpaperFlowModel.test.ts`

- [ ] **Step 1: Add failing behavior tests**

Cover nearest-center calculation, local ±7 index window, stable-ID initial/switch/reset anchors, responsive class, Page step, finite Arrow/Home/End, key intent mapping, explicit select/apply separation, thumbnail range, tail paging, idle snap, reduced motion, enhanced-media eligibility, and additive state classes.

```ts
assert.deepEqual(resolveFlowKey({ key: 'ArrowDown', index: 3, count: 10 }), {
  kind: 'center', index: 4,
});
assert.deepEqual(resolveFlowKey({ key: 'Enter', ctrlKey: true, index: 3, count: 10 }), {
  kind: 'apply', index: 3,
});
assert.equal(resolveFlowKey({ key: 'Tab', index: 3, count: 10 }), null);
```

- [ ] **Step 2: Verify RED**

Run the focused model test; expect a missing-module failure.

- [ ] **Step 3: Implement pure helpers**

Use clamped finite indices, 250 ms idle and 300 ms snap constants, current-ID-first restart anchors, selected-ID-first mode-switch anchors, and original-media policy limited to selected + centered + settled + active + non-reduced-motion.

- [ ] **Step 4: Verify GREEN**

Run the focused test and full unit suite.

### Task 4: Extract reusable static/enhanced preview media

**Files:**
- Create: `apps/tauri-gui/frontend/src/components/WallpaperPreviewMedia.tsx`
- Create: `apps/tauri-gui/frontend/src/components/WallpaperPreviewMedia.test.ts`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperCard.tsx`

- [ ] **Step 1: Add failing media tests**

Test static failure rendering and exported source resolution: image/GIF original, muted looping video, preview fallback, WE scene static-only, reduced-motion static-only, one enhanced element, and decode-error fallback.

- [ ] **Step 2: Verify RED**

Run the focused test; expect missing exports.

- [ ] **Step 3: Implement the component**

Hide thumbnail-store subscription and bounded Tauri file conversion behind the component. Render cached static thumbnails for rows, create at most one enhanced element when eligible, clear media sources on cleanup, and expose a nonblocking preview-error label.

- [ ] **Step 4: Reuse it from Grid**

Preserve Grid hover GIF behavior and its failure placeholder while removing duplicated subscription code.

- [ ] **Step 5: Verify GREEN**

Run focused tests, typecheck, and unit tests.

### Task 5: Build the Flow adapter and synchronized rails

**Files:**
- Create: `apps/tauri-gui/frontend/src/components/WallpaperFlow.tsx`
- Create: `apps/tauri-gui/frontend/src/components/FlowIndexRail.tsx`
- Create: `apps/tauri-gui/frontend/src/components/FlowIndexDialog.tsx`
- Create: `apps/tauri-gui/frontend/src/components/FlowMetadataRail.tsx`
- Create: `apps/tauri-gui/frontend/src/components/WallpaperFlow.test.ts`

- [ ] **Step 1: Add failing structural tests**

Verify listbox/option semantics, `aria-activedescendant`, explicit `aria-selected`, additive state attributes, visible Apply/Favorite/Details, labelled return-to-top, loaded/total copy, local index native buttons, and expanded index dialog semantics.

- [ ] **Step 2: Verify RED**

Run the focused test; expect missing components.

- [ ] **Step 3: Implement the virtual center stream**

Use one `useVirtualizer`, aspect-aware estimates, center padding, bounded overscan, rAF-throttled nearest updates, visible thumbnail enqueue, and one tail `loadMore` request. The stream remains the only scroll container.

- [ ] **Step 4: Implement native input and snapping**

Scroll only changes center. Snap after 250 ms idle and cancel on wheel/pointer/touch/key input. Scope Arrow/Page/Home/End/Enter/Ctrl-or-Cmd+Enter/Shift+F10 to the focused listbox; never prevent Tab.

- [ ] **Step 5: Implement selection/actions**

Single click or index activation center/selects, double click and Apply select centered before Apply, hover only emphasizes, and controls stop propagation. Disabled Apply cannot be bypassed.

- [ ] **Step 6: Implement rails**

Render the ±7 index, complete loaded-only virtualized focus-trapped index dialog, right-side metadata/state/queue lines, exact loaded/total/end copy, and return-to-top after one viewport.

- [ ] **Step 7: Verify GREEN**

Run component/model tests, typecheck, and full unit suite.

### Task 6: Add the active-adapter boundary and mode switch

**Files:**
- Create: `apps/tauri-gui/frontend/src/components/LibraryViewSwitch.tsx`
- Create: `apps/tauri-gui/frontend/src/components/LibraryViewport.tsx`
- Create: `apps/tauri-gui/frontend/src/components/LibraryViewport.test.ts`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`
- Modify: `apps/tauri-gui/frontend/src/shell/SinglePageShell.tsx`

- [ ] **Step 1: Add failing adapter tests**

Assert the labelled switch has two native `aria-pressed` buttons, only one adapter mounts, switching chooses Selected then outgoing center then first, restart Flow chooses loaded Current then first, and query reset chooses first.

- [ ] **Step 2: Verify RED**

Run the focused test; expect missing components/functions.

- [ ] **Step 3: Implement `LibraryViewport`**

Pass one model to either Grid or Flow, expose stable anchor handles, and transfer focus after the incoming adapter restores the stable ID. Never render both adapters.

- [ ] **Step 4: Integrate Shell**

Place `Grid / Flow` in the Library toolbar, persist with `updatePreferences`, hide Card size in Flow, pass shared state/actions, ensure Flow Apply selects first, keep load-more retry, and preserve details/context focus return.

- [ ] **Step 5: Verify GREEN**

Run focused tests, typecheck, and all units.

### Task 7: Add themes, responsive behavior, and accessibility fallbacks

**Files:**
- Modify: `apps/tauri-gui/frontend/src/styles/global.css`
- Modify: `apps/tauri-gui/frontend/src/styles/editorialTheme.css`
- Modify: `apps/tauri-gui/frontend/src/styles/globalCssTheme.test.ts`
- Modify: `apps/tauri-gui/frontend/src/styles/editorialTheme.test.ts`

- [ ] **Step 1: Add failing CSS contract tests**

Require three-region wide/medium layout, `<760px` single column with in-flow sticky metadata, `<=420px` wrapped actions, container queries, every additive state selector, focus visibility, reduced motion, forced colors, coarse pointer, and Editorial square high-contrast hierarchy.

- [ ] **Step 2: Verify RED**

Run the two CSS tests; expect selector/token assertions to fail.

- [ ] **Step 3: Implement shared Flow CSS**

Use semantic theme variables, strict rails, aspect-aware central widths, 55vh media cap, 240/180 ms transitions, bounded sticky regions, no horizontal overflow, and no overlay over scan/feedback surfaces.

- [ ] **Step 4: Implement Editorial overrides**

Use black/white hierarchy, small uppercase metadata, grayscale surrounding items, square geometry, strict alignment, and at most 4px metadata movement.

- [ ] **Step 5: Add accessibility modes**

Reduced motion removes smooth snap/transforms/autoplay, forced colors restores boundaries/focus, and coarse pointer keeps actions visible and touch-sized.

- [ ] **Step 6: Verify GREEN**

Run CSS tests, typecheck, full units, and production build.

### Task 8: Cover real interaction paths with Playwright

**Files:**
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`
- Modify: `apps/tauri-gui/frontend/e2e/library-perf.spec.ts`
- Modify only if required: `apps/tauri-gui/frontend/src/api/mockBridge.ts`
- Modify only if required: `apps/tauri-gui/frontend/src/api/mockBridge.test.ts`

- [ ] **Step 1: Add failing Flow smoke cases**

Cover first-use Grid, Flow persistence, repeated switching with stable anchor/filters, wheel metadata without selection/Apply, click/Enter select, double click/button/Ctrl+Enter Apply, local/expanded index, pagination/retry/end/top, details/context focus, reduced motion, and touch discoverability.

- [ ] **Step 2: Verify RED**

Run Flow-grep smoke cases and confirm failures are caused by missing behavior.

- [ ] **Step 3: Add only required deterministic mock seams**

Reuse the current fixture. Add counters/scenarios only where behavior cannot otherwise be observed, and reset them in `resetAll`.

- [ ] **Step 4: Add responsive/theme/performance cases**

Check 320, 390, 760, 1024, 1440; all themes; no overflow; reachable actions; scan/feedback coexistence; 5,000 items with bounded Flow/index DOM; and at most one enhanced media element.

- [ ] **Step 5: Verify GREEN**

Run Flow-grep smoke, complete `npm run smoke`, and `npm run perf:library` where supported.

### Task 9: Full verification, review, visual iteration, and final jj commit

**Files:**
- Review all paths in `jj diff`
- Update implementation/tests only for discovered issues

- [ ] **Step 1: Run frontend gates**

Run `npm run typecheck`, `npm run test:unit`, `npm run build`, and `npm run smoke` in `apps/tauri-gui/frontend`.

- [ ] **Step 2: Run repository gates**

Run `cargo run -p xtask -- verify all`, `cargo build --workspace`, and `git diff --check`.

- [ ] **Step 3: Perform visual review**

Capture settled/in-motion Flow at 320, 390, 760, 1024, 1440 under Editorial/Light/Dark/Glass; inspect long/missing metadata and combined states; fix and repeat.

- [ ] **Step 4: Run two-stage review**

Dispatch one reviewer against the approved spec, then a separate senior code-quality/a11y/performance reviewer. Fix every material finding and rerun affected gates.

- [ ] **Step 5: Inspect and commit**

Run `jj status`, `jj diff --summary`, `jj diff --git`, and `git diff --check`, then `jj commit -m "feat(frontend): add Flow wallpaper selection mode"`. Confirm the new working copy is clean.
