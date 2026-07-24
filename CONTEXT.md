# Wallpaper Console

Desktop wallpaper apply and library for Wayland/X11, with per-display targeting and multiple renderer backends.

## Language

**ApplyTransition**:
The previous→target visual transition for an apply or restore: stop/settle and optional instant fallback, scoped so full handoff runs only when the action covers all displays.
_Avoid_: Visual handoff (as the module name), lifecycle (as the product concept), ApplyWallpaper (for this transition policy)

**DisplayTarget**:
Where an apply or restore is aimed: all displays, or a named output.
_Avoid_: Screen, monitor (in domain talk — prefer Display / output as in code)

**Apply**:
Putting a wallpaper onto a DisplayTarget through the display apply path (plan → ApplyTransition → commit).
_Avoid_: Set wallpaper, change background

**Restore**:
Re-applying stored per-display (or all-displays) wallpaper mappings after login or an explicit restore.
_Avoid_: Resume, recover (unless meaning library integrity repair)

**ThumbnailSession**:
The Library thumbnail load + URL cache + reveal batching for visible cards; views report visible paths and scrolling/interaction, and read through one subscribe/get seam.
_Avoid_: Thumbnail store (as the deep module name), dual queue/store caches

**RuntimeWallpaper**:
Current wallpaper observation plus apply-queue feedback as one shell surface `{ current, apply }`; ApplyResult evidence is shared so queue parse and session confirm stay aligned.
_Avoid_: Separate Current/Apply coordinators as product concepts, duplicating ApplyResult parsers

**LibraryPaging**:
Paged library list loading that owns append policy (`canAppend` / `canAutoAppend` / `requestMoreIfNeeded` / `appendMore`); Grid/Flow only report geometry near the tail.
_Avoid_: Views remapping automaticAppendPaused, hasMore-as-policy
