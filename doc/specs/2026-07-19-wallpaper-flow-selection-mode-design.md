# Wallpaper Flow Selection Mode Design

Date: 2026-07-19

Status: approved design, pending written-spec review and implementation plan

## Goal

Add an independent `Flow` wallpaper selection mode alongside the existing `Grid` mode. Flow adapts the vertical portfolio-browsing rhythm of [Obys Agency](https://obys.agency/?ref=landing.love) to a real, mutable wallpaper library without copying Obys branding, fixed-project assumptions, global input interception, or inaccessible keyboard behavior.

The mode must preserve the current Library query, filtering, sorting, favorites, current-wallpaper observation, selection, Apply queue, details, thumbnail scheduling, and pagination behavior. Grid remains available and retains its current interaction contract.

## Confirmed product decisions

- The Library toolbar exposes a `Grid / Flow` view-mode switch.
- A new installation starts in Grid.
- The last chosen view mode is persisted and restored after an application restart.
- Grid and Flow are available under every theme. Editorial gives Flow its closest Obys-inspired visual treatment; other themes use the same layout and behavior with their existing theme variables.
- Switching modes preserves context by stable wallpaper ID, not by pixel scroll position.
- Mode-specific scroll pixels and anchors are session-only and are not persisted across application restarts.
- Flow is finite and forward-paginated. It does not wrap from the final wallpaper to the first wallpaper.
- At the true end, Flow reports that all matching wallpapers have been browsed and exposes a small upward arrow in the right rail to return to the first loaded item.
- The left index covers loaded results only and grows as pages are appended. It reports loaded count and exact total when the total is available.
- Flow does not add a backend full-index or arbitrary-seek API in this feature.
- Flow does not add a tags data model. The current Library has no real tags field, so type, source, backend, and compatibility badges must not be presented as user-authored tags.

## Reference analysis and deliberate adaptation

Obys desktop Vertical mode uses four synchronized layers around the viewport center:

- a narrow, vertically continuous thumbnail stream;
- a left project-name index whose active item becomes fully opaque;
- a fixed metadata line split across the remaining grid columns;
- grayscale, opacity, parallax, and snapping transitions driven by the nearest central project.

Its desktop implementation works with a fixed set of 19 projects duplicated into a visual loop. Wheel deltas are intercepted globally, Arrow keys and Space feed a custom scroller, and Tab is prevented. Its mobile implementation removes the desktop rails and mode switch, uses a centered native-scrolling image stream, marks the nearest image active, and performs a delayed snap after touch scrolling.

Flow keeps the useful spatial model but deliberately changes the implementation:

- native scrolling replaces global wheel interception;
- keyboard handling is scoped to the Flow composite and never blocks Tab;
- a revision-bound paged Library remains finite rather than visually duplicated;
- central previews are larger than Obys thumbnails so wallpapers remain useful to inspect;
- state names distinguish browsing, selection, runtime current state, and Apply queue state;
- responsive behavior is based on available window width, not a server-selected mobile user agent.

## Terminology and state model

The implementation must use these concepts consistently:

- **View mode**: persisted `grid` or `flow` choice.
- **Centered wallpaper**: the item nearest the Flow viewport center. It is the current browsing focus and drives the synchronized index and metadata.
- **Hovered wallpaper**: a temporary pointer relationship. Hover never scrolls, selects, or applies.
- **Selected wallpaper**: the last item explicitly chosen by click, index activation, or Enter. Selection enables enhanced preview but is not proof that the wallpaper is running.
- **Current wallpaper**: the wallpaper confirmed by runtime display observation. It must never be inferred from selection or an optimistic Apply request.
- **Applying wallpaper**: the active Apply queue item.
- **Pending wallpaper**: the queued replacement Apply item.

Centered and hovered state belong to Flow. Selected, Current, favorite state, and Apply queue state remain owned by the shell.

The state transition contract is:

```text
native scroll / touch / Arrow navigation
  -> update centered wallpaper
  -> synchronize central treatment, local index, and right metadata
  -> do not select
  -> do not Apply

single click on preview / activate index / Enter
  -> if the target is not centered, scroll it to the center
  -> update Selected
  -> start the one permitted enhanced preview
  -> do not Apply

double click / Apply button / Ctrl-or-Cmd+Enter
  -> update Selected to the centered target
  -> submit that target to the existing Apply queue

runtime Apply observation succeeds
  -> existing runtime reconciliation updates Current
```

If Selected and centered differ because the user resumed scrolling, every visible action in the right rail targets the centered wallpaper. Apply first synchronizes Selected to that target. The UI must never apply an old Selected wallpaper that has scrolled out of context.

## Selected architecture

Keep one Library business model and add a presentation adapter boundary:

```text
SinglePageShell
  |- useLibraryBrowser (only query and paging owner)
  |- persisted filters, sort, view mode, and theme
  |- Selected, Current, favorite, details, and Apply queue state
  `- LibraryViewport
       |- LibraryViewSwitch
       |- WallpaperGrid (existing adapter)
       `- WallpaperFlow (new adapter)
            |- FlowIndexRail
            |- FlowPreviewStream
            `- FlowMetadataRail
```

`LibraryViewport` receives one shared view model and a small set of semantic intents rather than duplicating the current Grid prop surface. It mounts only the active adapter. Keeping the inactive adapter mounted but visually hidden is prohibited because both views would otherwise compete for the global thumbnail pending queue and reveal-pause flag.

The view switch is a labeled two-button group using `aria-pressed`. After either pointer or keyboard activation, the incoming view restores the stable wallpaper anchor and then receives focus: Flow focuses its composite listbox, while Grid focuses the anchored card's primary button.

The shared model includes:

- loaded `LibraryBrowserItemDTO` entries;
- selected, current, applying, pending, and favorite-pending identities;
- refresh and pagination state;
- stable reset/query key;
- display/apply availability;
- semantic callbacks for select, Apply, favorite, details, context menu, and load more.

The adapters own layout-specific state only. Grid keeps its current rows, virtualizer, gesture preference, and scroll behavior. Flow owns its center calculation, idle snap, local index window, expanded-index state, hover state, and enhanced-preview eligibility.

## Focused shared refactors

The second view exposes existing coupling that should be corrected without broad unrelated refactoring:

- Move `ContextAction` out of `WallpaperGrid.tsx` into a neutral Library action module.
- Type the shared select intent with `LibraryBrowserItemDTO`; remove the Shell cast from `WallpaperDTO` to the richer browser item.
- Extract thumbnail subscription, static preview path selection, animated-media eligibility, and media cleanup into a `WallpaperPreviewMedia` component with its media lifecycle hidden behind that component boundary.
- Extract name, source, type, resolution, size, date, author, Workshop, backend, and compatibility formatting into `wallpaperPresentation`.
- Keep querying and paging in the existing `useLibraryBrowser` and `usePagedWallpapers`; Flow must not create a second query hook or cursor.

These refactors are in scope only where needed to keep Grid and Flow consistent.

## View-mode preference and initialization

Add `libraryViewMode: 'grid' | 'flow'` to `ShellPreferences`.

- The default is `grid`.
- Normalization repairs missing or unknown values to `grid`.
- Explicit serialization includes the field.
- Existing preference payloads migrate through normalization without a separate storage migration.
- Mode switching updates the persisted preference through the existing preference controller.

When switching during a session:

- use Selected as the anchor when it exists;
- otherwise use the item nearest the outgoing viewport center;
- locate the stable wallpaper ID in the incoming adapter;
- if Selected is not present in the loaded result, use the outgoing viewport anchor;
- if neither stable ID is present, use the first loaded item.

When the app restarts directly into Flow, no session anchor exists. If the runtime-confirmed Current wallpaper is in the first loaded filtered page, center it. Otherwise begin at the first result and do not scan all pages merely to find Current.

Changing search, source, type, favorites, or sort centers the first loaded item in the new query, matching the existing query reset contract. Selected reconciliation continues to use stable wallpaper ID and the existing existence probe.

## Desktop layout

Flow lives inside the existing fixed-height Library region. It must not move scrolling to `body` or bypass the shell's overlay scroll locks.

The desktop layout uses the current theme's spacing variables and a strict three-region grid:

- **Left rail**: synchronized local index, loaded/total count, and complete-index trigger.
- **Center**: the only vertical scroll container and the only thumbnail consumer.
- **Right rail**: centered-item metadata, state, actions, end feedback, and return-to-top control.

The central preview is intentionally larger than the Obys reference:

- landscape previews target roughly 34-38% of available Library width;
- square and portrait previews use a smaller width that produces comparable visual area;
- an item is capped near 55% of the available viewport height;
- original aspect ratio is preserved;
- spacing remains large enough to establish a distinct center item without making each wheel gesture feel like a page transition.

The exact width is clamped by the real Library container rather than the browser viewport so Settings, Sources, and window resizing remain correct.

## Left index

The default left rail shows a synchronized window containing the centered item and up to seven loaded items before and after it. Near either boundary, the window contains the available items without duplicating names.

- The centered name stays visually aligned with the central anchor.
- The rail does not own wheel scrolling and never steals trackpad momentum from the preview stream.
- The active browsing name has full emphasis; surrounding names are muted.
- Selected, Current, and Favorite indicators are additive and remain visible when applicable.
- Hover creates a temporary relationship highlight across the name, thumbnail, and metadata only.
- Activating a visible name centers it, updates Selected, and starts enhanced preview without applying it.

An `Index` control opens a complete virtualized list of loaded names. The complete index supports keyboard navigation and direct selection but does not imply access to unloaded pages. It displays loaded count and exact total when known. Additional names appear as the central stream appends pages.

Activating an expanded-index item closes the index dialog, centers and selects that item, starts its eligible preview after settling, and restores focus to the Flow listbox. Escape closes the dialog without changing center or selection.

## Center preview stream

Use a single-column TanStack Virtual flow with bounded overscan.

- Estimated item sizes are derived from preview aspect ratio and responsive width.
- Real measurements correct estimates without scanning every item during scroll.
- Start and end padding allow the first and final item to occupy the same viewport-center anchor as every other item.
- The centered item is computed from the virtual range and measured center distances in a requestAnimationFrame-throttled path.
- The calculation updates only when the nearest stable ID changes.
- Approaching the loaded tail calls the existing `loadMore` contract and respects automatic-append pause and retry behavior.

Scrolling remains native for trackpads, wheels, and touch. Flow observes activity and schedules a snap after 250ms without scroll input. The snap uses a 300ms restrained ease to the nearest item. It can be cancelled immediately by renewed wheel, pointer-down, touch, or keyboard input.

Keyboard behavior while the Flow composite has focus:

- ArrowUp and ArrowDown move one loaded item.
- PageUp and PageDown move by a viewport-relative step and settle on an item.
- Home moves to the first loaded item.
- End moves to the final currently loaded item and triggers at most one normal next-page request when more results exist; it does not chase successive pages automatically.
- Enter selects and starts preview.
- Ctrl+Enter or Cmd+Enter selects and applies.
- Shift+F10 opens the existing contextual actions.
- Tab and Shift+Tab leave the composite normally.

No handler activates when focus is in a search field, select, button, dialog, Settings, or Sources.

## Selection, pointer, and touch behavior

- Single-clicking a central preview selects it and enables enhanced preview.
- Double-clicking a preview selects it and submits Apply.
- Clicking a non-centered visible item first centers and selects that item; it does not Apply.
- Hover never changes center, Selected, or Apply state. It creates a light synchronized emphasis and makes already-available secondary actions more prominent; it does not hide those actions from touch or keyboard users.
- Touch uses native vertical scrolling. A single tap selects and previews.
- Touch does not require double-tap to Apply. The right-side or bottom Apply button remains visible and is the canonical touch action.
- Favorite and Details controls do not trigger selection or Apply through event bubbling.

Grid retains the existing single/double Apply gesture preference. Flow deliberately uses the safer interaction contract above and does not reinterpret the Grid card gesture setting.

## Metadata and actions

The rails provide a glanceable layer; the existing details surface remains the complete technical layer.

Left-side glanceable information includes:

- ordinal within loaded results;
- display name;
- Selected, Current, and Favorite state.

Right-side glanceable information includes, when present:

- source display names;
- wallpaper type;
- resolution;
- file size;
- added date;
- author;
- Workshop identity;
- renderer/apply compatibility warning.

The right rail also owns Favorite, Details, Apply, Apply queue status, loaded/total count, final-end feedback, and return-to-top. Raw paths, backend error details, renderer diagnostics, and alternate Apply actions remain in the existing full details surface.

Favorite and Details target the centered wallpaper without implicitly changing Selected. Apply targets the centered wallpaper and does change Selected before enqueueing. If another wallpaper is already Applying or Pending, the rail shows that queue item's name in a separate global queue line rather than attributing the state to the centered wallpaper.

When runtime reconciliation reports a mixed multi-display state, no individual Flow item is marked Current; the existing mixed-target summary remains authoritative.

Metadata follows the centered wallpaper, not old Selected state. Shared behavior uses a 180ms crossfade without layout movement; Editorial may add at most 4px of clipped vertical translation over the same duration.

## State visuals

State treatment is additive and cannot rely on color alone:

- **Centered**: full color and full opacity; surrounding previews remain restrained grayscale and reduced opacity.
- **Hovered**: a fine temporary boundary or underline with no persistent state implication.
- **Selected**: a persistent compact `Selected` marker and fine framing treatment.
- **Current**: a separate `Current` label with a solid state dot.
- **Applying**: a short progress treatment tied to the existing active Apply state.
- **Pending**: a distinct queued label or line treatment.
- **Favorite**: the existing favorite icon/state.

The same wallpaper may be Centered, Selected, Current, Favorite, and Applying simultaneously. The DOM and CSS must represent that combination without replacing one state class with another.

## Enhanced preview media

Scrolling always uses the existing static thumbnail path. Enhanced preview is allowed only when an item is both centered and explicitly Selected, and the scroll has settled.

- Image and GIF entries use `entry.path` through the existing bounded Tauri asset-URL conversion. GIF uses its original animation when the WebView can decode it.
- Video entries use `entry.path` in one muted, looping, `playsInline` video element with `preload="metadata"`.
- If the original asset cannot be decoded, Flow falls back first to `previewPath` and then to the cached static thumbnail.
- Enhanced media pauses and releases immediately when scrolling resumes, center changes, selection changes, the view unmounts, a dialog takes over, or the application becomes inactive.
- No more than one animated decoder may be active.
- Wallpaper Engine scene items remain on a static preview. Flow must not imply full Wallpaper Engine scene parity.
- `prefers-reduced-motion` forces static media and disables autoplay.
- A media load or decode error falls back to the existing static thumbnail and exposes a concise nonblocking status.

The implementation must not eagerly read original files for overscan items or use original images as the base virtualized stream.

## Pagination, end state, and return to top

Flow preserves the backend's revision-bound keyset order.

- It automatically appends pages near the loaded tail.
- A recoverable append failure retains loaded items and presents the existing retry path.
- A revision replacement restores a stable loaded anchor where possible.
- No cloned rows or circular logical indices are created.
- Once `hasMore` is false, the right rail reports `All N wallpapers viewed` using the exact total when available.
- After the user has moved more than one Flow viewport from the first item, a small upward arrow appears in the right rail. At the true end it becomes part of the completion treatment.
- The arrow is a labeled button, is keyboard reachable, and returns to the first item using reduced-motion-aware scrolling.

This finite model avoids duplicate focus targets, ambiguous ordinals, changing loop length during paging, and hidden completion state.

## Responsive behavior

Responsive behavior follows the `.library-viewport` container width and preserves every core operation.

### Wide desktop (at least 1024px)

- Full three-region layout.
- Local index and right metadata remain sticky.
- Central preview uses the target aspect-aware dimensions.

### Medium window (760-1023px)

- Side rails compress and use denser typography.
- Low-priority metadata moves behind Details before core actions are removed.
- Central preview keeps a useful readable size.

### Narrow window (below 760px)

- The central flow becomes the primary single column.
- The left rail collapses to a current-ordinal / loaded-count `Index` button that opens the virtualized index panel.
- Right metadata moves into a sticky bottom information region inside the Library layout. It occupies layout space and does not overlay the preview, scan progress, or feedback.
- Apply, Favorite, Details, state labels, and return-to-top remain visible and touch reachable.
- No horizontal carousel or global body scrolling is introduced.

At 420px and below, bottom actions may wrap to two rows, but Apply, Favorite, Details, current ordinal, and return-to-top remain visible without horizontal scrolling.

The design must be verified at 320, 390, 760, 1024, and 1440 CSS pixels, including concurrent scan and feedback overlays.

## Motion and performance boundaries

Default motion is restrained:

- idle snap: 300ms;
- grayscale/opacity/scale response: 240ms;
- metadata transition: 180ms;
- center parallax: very small and compositor-only.

Fast scrolling suppresses nonessential transition work. Avoid blur, canvas rendering, per-item observers for every loaded wallpaper, and state updates on raw wheel events.

Performance requirements:

- mount only the active view adapter;
- keep preview and complete-index DOM counts bounded under a 5,000-item fixture;
- enqueue thumbnails only for the active virtual range and overscan;
- preserve the global thumbnail concurrency and reveal batching behavior;
- keep one enhanced media decoder at most;
- avoid per-frame React updates when the centered stable ID has not changed;
- preserve the existing Grid performance baseline.

With `prefers-reduced-motion`, snapping becomes immediate, parallax and scale are removed, metadata changes without spatial animation, and all media remains static.

## Themes

Flow structure and semantics are theme-independent.

- Shared Flow CSS consumes existing semantic color, surface, border, text, focus, spacing, and motion variables.
- Editorial adds the strongest black/white hierarchy, strict alignment, small metadata typography, grayscale contrast, and square geometry.
- Light, Dark, and Glass retain their established palettes and material treatment without changing Flow behavior.
- Forced-colors rules preserve state boundaries, labels, and focus.
- No theme receives a separate Flow component implementation.

## Accessibility

- The view switch has an explicit accessible group label and selected state.
- Its two native buttons expose `aria-pressed`, and activation follows the focus-transfer contract defined under Selected architecture.
- `FlowPreviewStream` is a focusable `listbox` with a stable accessible name. Rendered wallpapers are `option` elements.
- The listbox retains DOM focus and points `aria-activedescendant` at the centered rendered option. `aria-selected` represents explicit Selected state and may therefore differ from the active descendant.
- The local index is a labeled navigation list of native buttons. The expanded index is a focus-trapped dialog containing a virtualized list of native buttons, avoiding a second competing listbox.
- Selected semantics represent explicit Selected state, not merely Centered or Hovered state.
- Runtime Current state is announced independently.
- Scroll-driven center changes are not placed in an assertive live region.
- Apply results continue through the existing feedback announcements.
- All icon-only actions, including return-to-top, have accessible names and visible focus.
- Pointer hover is never required to discover Apply, Favorite, or Details on coarse-pointer devices.
- Dialog and context-menu focus restoration continues to target the originating wallpaper or stable Flow composite.
- Global Tab, wheel, touch, and Arrow behavior is never blocked outside the active Flow composite.

## Failure behavior

- Static thumbnail failure uses the existing failure placeholder.
- Enhanced image/GIF/video failure falls back to the static thumbnail and does not affect selection or Apply availability.
- Pagination failure retains the loaded flow and exposes retry.
- A removed selected wallpaper follows the existing stable-ID existence reconciliation and notification.
- If a centered item disappears during a revision replacement, choose the nearest surviving loaded item; if no items remain, render the existing empty state.
- Apply, favorite, details, source, and scan failures continue through their existing feedback systems.
- An unavailable Apply action remains disabled with the existing reason rather than allowing double click to bypass validation.

## Testing and acceptance

### Unit coverage

- preference default, normalization, serialization, persistence, and invalid-value repair for `libraryViewMode`;
- shared Library view model and semantic intent mapping;
- centered-item calculation from virtual ranges and geometry;
- idle-snap scheduling, cancellation, and reduced-motion behavior;
- Arrow, Page, Home, End, Enter, Ctrl/Cmd+Enter, and Shift+F10 navigation;
- scroll and hover never produce select or Apply intents;
- index activation selects without Apply;
- explicit Apply always targets and selects the centered item;
- Centered, Hovered, Selected, Current, Applying, Pending, and Favorite combinations;
- metadata formatting and missing-value fallbacks;
- enhanced-media eligibility and cleanup;
- stable-ID mode switching and query reset behavior;
- responsive layout helpers where behavior is computed in TypeScript.

### Playwright coverage

- Grid is the first-use default; Flow persists after reload.
- Grid and Flow switch repeatedly without losing filters, favorites, current state, or the stable wallpaper anchor.
- Trackpad/wheel-equivalent scrolling changes centered metadata without selecting or applying.
- Single click and Enter select and preview.
- Double click, Apply button, and Ctrl/Cmd+Enter apply the centered wallpaper.
- Touch interaction never requires hover or double tap.
- Left local index and expanded index synchronize with the central stream.
- Right metadata and all state indicators follow their defined state owners.
- Pagination appends, append failure retries, revision replacement, final-end feedback, and return-to-top work.
- GIF/video cleanup leaves at most one active enhanced preview.
- Wallpaper Engine scene items remain honest static previews.
- Shift+F10, Details, and mode-switch focus restoration work after virtualization.
- 5,000 entries keep DOM size bounded and scrolling responsive.
- 320, 390, 760, 1024, and 1440 layouts have no horizontal overflow or unreachable actions.
- Editorial, Light, Dark, and Glass preserve function and readable state contrast.
- reduced-motion, forced-colors, and coarse-pointer behavior pass.
- scan and Apply feedback overlays do not cover the bottom metadata/action region.

### Verification gates

```bash
cargo run -p xtask -- verify all
cargo build --workspace
git diff --check
```

Focused frontend checks remain:

```bash
cd apps/tauri-gui/frontend
npm run typecheck
npm run test:unit
npm run smoke
```

Manual visual review must cover settled and in-motion states, switching between Grid and Flow, long names and missing metadata, all combined status states, mouse and touch-equivalent interaction, responsive breakpoints, and every theme.

## Out of scope

- Replacing or behaviorally changing Grid.
- Adding a backend full-Library index or arbitrary seek endpoint.
- Loading all Library pages merely to populate the index.
- Circular or cloned infinite looping.
- Adding user-authored tags or collections.
- Full Wallpaper Engine scene rendering inside the selector.
- Playing audio in previews.
- Persisting selection, centered wallpaper, or scroll offsets across restarts.
- Copying Obys branding, custom logo behavior, custom cursor, global wheel interception, or Tab suppression.
- Refactoring unrelated shell, backend, or storage modules.

## Acceptance summary

The design is accepted when a user can switch between Grid and Flow at any time, browse the same real filtered Library through a smooth central stream, understand Centered/Selected/Current/Apply states without ambiguity, preview one selected item safely, apply only through an explicit action, use the complete feature by mouse, keyboard, or touch, and retain responsive performance with thousands of wallpapers under every existing theme.
