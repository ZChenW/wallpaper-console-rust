# Codex completion summary

Date: 2026-07-15

Branch: `feat/glass-theme` (isolated worktree based on `codex/simple-wallpaper-console` at `853b183`)

Status: the agreed Simple Wallpaper Console MVP and the 2026-07-14 usability follow-up are complete and locally verified.

## What was completed

- Kept the existing Rust/Tauri project instead of forking Waypaper. Waypaper is used only as a reference for a direct, compact interaction model.
- Replaced the legacy multi-page GUI with one responsive wallpaper picker: search, display target, random selection, source/type/favorite/sort filters, virtualized grid, bottom status, compact settings, and source drawer.
- Unified ordinary directories, Downloads, overlapping directories, and Wallpaper Engine Workshop projects in one library. Sources support add, rename, recursion changes, refresh, offline state, and safe removal without deleting wallpaper files.
- Added first-run directory and Wallpaper Engine suggestions that require explicit confirmation before scanning.
- Routed wallpaper types automatically through compatible renderers: images/GIFs to awww by default, videos to mpvpaper, and supported Wallpaper Engine scenes to linux-wallpaperengine. Renderer installation/readiness and limitations are shown without requiring routine backend switching.
- Added explicit per-display targeting, persisted display state, safe apply planning, restore, and runtime reconciliation. Unsafe or unverified multi-display/backend combinations are rejected instead of silently expanding scope or disturbing another display.
- Added configurable single-click/double-click apply, theme, card size, fill/transition options, renderer-specific options, and opt-in login restoration.
- Added latest-request apply queuing, concise slow-operation feedback, non-modal delayed scan progress/cancellation, severity-based auto-close timing, hover pause, and countdown bars.
- Preserved responsive browsing through virtualization, visible-range thumbnail prioritization, automatic paging, stale-result protection, and retry fuses. Browser smoke coverage exercises 1,000+ and 5,000+ item fixtures; the intended 100–200 item library is comfortably within scope.
- Removed the history UI/API and stopped new history writes while retaining old database data only for migration/repair compatibility.
- Restricted database repair to confirmed integrity faults; unavailable storage or an empty filtered result no longer masquerades as corruption.
- Removed obsolete legacy views, navigation, tests, and CSS after replacement coverage was in place.
- Hardened final review findings: behavior settings now serialize concurrent writes, recover from partial/transient failures with a bounded retry, and reconcile reset generations; runtime awww observation has a two-second timeout, process cleanup, and no longer holds the apply lock during compositor display discovery.

## Usability follow-up completed

- Replaced the gray pill-like filter strip with a flat compact toolbar. Default labels are now `ALL SOURCES` and `ALL`, with visible subtle borders and a 6px radius.
- Fixed false mpvpaper stop failures by polling the exact user process list every 50ms for up to two seconds after TERM. A normal delayed exit succeeds; a genuine timeout still reports the remaining PIDs. No SIGKILL escalation was added.
- Reworked source rows around aliases: a persistent pencil opens an in-place editor, Enter saves, Escape cancels, failures preserve the draft, and the real directory path is never renamed. Refresh/remove icons stay visible and right-aligned. Availability backgrounds are green, red, and amber for available, offline, and unknown.
- Replaced the warm brown dark palette with neutral cool charcoal colors.
- Replaced renderer selects with compact `awww`/`mpvpaper` cards. Video is represented by a fixed mpvpaper card. Removed the duplicated Default display control, renderer suffixes, and renderer installation-status list.
- Modernized Settings as the highest modal side sheet with an icon close action, Escape/backdrop close, body scroll locking, and stacking above the library scrollbar and feedback layer.
- Fixed large-window scrolling pressure by generating static WebP thumbnails for Wallpaper Engine preview assets. Only one hovered GIF may animate, and scrolling immediately returns cards to static thumbnails; the details dialog keeps the original animated preview.

## Library and overlay performance follow-up completed

- Identified two independent release-path causes of maximized-window jank: the installed launcher disabled WebKitGTK's DMABUF renderer by default, and animated GIF fallbacks could preserve every frame inside cached WebP thumbnails.
- Made WebKitGTK's accelerated DMABUF path the default. Blank-window compatibility remains opt-in through `WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1`, while an explicitly supplied `WEBKIT_DISABLE_DMABUF_RENDERER` value is preserved.
- Introduced v3 GUI thumbnail cache keys and a strict single-frame contract for animated image sources. Rust now decodes GIFs directly; ImageMagick fallback explicitly selects frame zero.
- Removed grid-wide React state updates at scroll start/idle, stabilized card thumbnail subscriptions and shell callbacks, and memoized the grid boundary.
- Paused thumbnail reveal notifications while Settings or Sources overlays obscure the library, then releases completed results once the library becomes active again.
- Kept the Settings-to-Sources transition mounted and transform-only, while making Sources Close immediate, backdrop-dismissible, and keyboard/focus safe.
- Measured the new release binary in a 1797x1080 niri window. During repeated Page Up/Page Down input, the main and WebKit processes used about 1-7% combined CPU in the observed samples. Fifty-four newly generated v3 WebPs were all static; none contained an animated WebP marker. This replaces the earlier observed sustained WebKit load of roughly 43-88% CPU with animated cache entries.

## Glass theme completed

- Added an explicit `Glass` appearance option without changing System theme behavior. The choice is validated, persisted, restored across Settings sessions, and applied as `data-theme="glass"`.
- Added a low-saturation dark blue-grey Acrylic dashboard palette with fine borders, restrained cyan emphasis, two static ambient glows, and a dedicated dashboard monospace stack. Normal prose keeps the system sans-serif stack.
- Limited blur to the top bar and six floating surfaces: Settings, Sources, native select content, context menu, wallpaper details, and feedback. Wallpaper cards are excluded from all standard and WebKit backdrop-filter rules, so their computed filter remains `none` and grid scrolling stays protected.
- Replaced the remaining inline visual skins in wallpaper details and feedback with theme-aware CSS while preserving their layout and behavior.
- Added opaque fallback colors, reduced-transparency behavior, forced-colors behavior, primary-button contrast protection, paired `backdrop-filter`/`-webkit-backdrop-filter` declarations, and a consistent cyan keyboard focus ring with a 2px offset.
- Added regression tests for theme validation, persistence, native color-scheme mapping, Settings selection, CSS scope/accessibility invariants, and desktop/compact Playwright behavior.
- A read-only Cursor CLI audit and independent spec/quality reviews were incorporated. Their actionable findings covered card-blur guards, WebKit prefixes, accessibility modes, button contrast, and stable E2E grid readiness.
- Visually inspected the release build in a maximized 2560x1599 niri session using an isolated copy of the application configuration. Under the same sustained Page Up/Page Down script, both Dark and Glass settled around 3-5% main-process CPU; observed RSS was about 371-372 MiB. This is a small acceptance sample rather than a frame-time benchmark, but it found no Glass-specific scroll regression.

## Frameless window and Glass Sources follow-up completed

- Removed the GTK/Tauri native title bar by disabling decorations in the sole window configuration before creation. The existing application topbar is now a deep Tauri drag region, backed by the narrowly scoped `core:window:allow-start-dragging` capability; no custom minimize, maximize, or close controls were added.
- Fixed Wallpaper Engine and directory source cards appearing black under Glass. Their previous system `Canvas` formula is retained as the Light/Dark fallback, while Glass routes the card background through `--surface-muted` for a translucent blue-grey surface.
- Kept accessibility behavior explicit: reduced transparency resolves source cards to opaque `#13232f`, and forced-colours resolves them to system `Canvas`.
- Added two window-chrome configuration contracts, one SourcePanel component contract, expanded the Glass CSS contract, and extended the desktop/compact Glass Playwright scenario to verify two source-card computed backgrounds are non-black and blue-grey.
- Rebuilt and launched the release binary with an isolated Glass configuration under niri. Screenshots confirmed that the native title bar is absent and both real configured source cards use the Glass blue-grey surface. The undecorated floating window was resized from 800x600 to 850x650 through niri, confirming it remains compositor-resizable.

## Final verification

All commands below were run on the final code and exited successfully:

- `cargo run -p xtask -- verify all`
  - `cargo fmt --all -- --check`
  - `cargo check --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`
  - frontend type checking
  - frontend unit tests: 363 tests passed, 0 failed
  - production frontend build
  - Playwright smoke: 42 passed across desktop and compact layouts, 0 failed
  - runtime/config drift check
- `cargo build --workspace`
- `bash scripts/test_install_contract.sh`
- `./install.sh --build-only`
- `git diff --check`

Focused Glass checks also passed: 24 theme behavior tests, 6 static CSS contract tests, 21 SourcePanel tests, 2 window-chrome contract tests, 12 relevant visual/component tests, and the Glass Playwright scenario in both desktop and compact projects.

The final specification review found no unmet P0/P1/P2 product goals. The final standards review found no remaining P0/P1/P2 issue after the settings-persistence and runtime-observation fixes.

## Remaining limitations

- The maximized release measurement still showed about 1.17 GiB combined RSS for the Tauri and WebKit processes after thumbnail generation. CPU and interaction pressure improved substantially, but memory residency remains a separate profiling target.
- Wallpaper Engine Web and Application projects are indexed for browsing but cannot be applied. Scene rendering uses linux-wallpaperengine compatibility and does not promise full Wallpaper Engine parity.
- Multi-display combinations without verified renderer coexistence or output-scoped stopping are deliberately rejected. This favors correct visual state and protection of other displays over pretending unsupported parity.
- The `restore-at-login` command and opt-in setting are implemented, but the application does not create a desktop-environment/systemd autostart entry automatically.
- Downloads is intentionally an ordinary directory source, not a separate wallpaper type. The MVP does not include an online wallpaper downloader.
- Libraries above 5,000 entries, slideshows, always-playing animated grid previews, and wallpaper file deletion/move/rename are outside this release's scope. A single hovered GIF preview is supported.
- The frameless window intentionally has no in-app minimize, maximize, or close buttons. Window management remains the compositor's responsibility, as approved for the niri workflow.
- Pointer dragging was not successfully exercised by the available synthetic Wayland input tooling. The release contract, Tauri capability, deep drag-region markup, and Tauri 2.11.2 descendant-interaction semantics were verified, but a human should still drag the topbar once during installation acceptance.
- The static CSS contract helper remains formatting-sensitive (P3 test-maintenance risk only); it does not affect application behavior or the browser-level Glass coverage.

## Recommended follow-up

1. Perform a manual acceptance pass on the target Wayland session with the installed awww, mpvpaper, and linux-wallpaperengine binaries. Specifically switch repeatedly between image/video/scene wallpapers, confirm Settings backdrop closing, rename a source alias, and compare maximized-grid scrolling with a small window. Repeat with multiple physical displays because automated tests intentionally use fake runtimes and the development host capability probe had one connected display.
2. Add an optional packaging-time autostart installer if login restoration should become one-click setup.
3. Split `SinglePageShell.tsx` and the source drawer into smaller internal modules only when the next feature requires those seams; no broad refactor is needed for this release.

The release artifacts were rebuilt for verification but not installed over the user's current `~/.local` installation. The Glass changes remain uncommitted in the isolated `.worktrees/glass-theme` worktree because this task did not include commit, merge, install, or push authorization.

No remote push was performed.
