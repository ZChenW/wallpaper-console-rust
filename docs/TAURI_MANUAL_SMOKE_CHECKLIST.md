# Tauri Manual Smoke Checklist

Run after `./install.sh` or after installing a `.deb` / `.rpm` bundle.

## 2026-06-12 SQLite-Only + WE One-Click Scan Note

Key behavioral changes:
- Library always reads from SQLite (`wallpapers.db`). TSV is legacy/CLI only.
- Scan Wallpaper Engine (Sources page) discovers projects AND indexes wallpapers in one action.
  No need to visit Settings → Rebuild after scanning.
- Settings: Storage backend and Library source selectors removed. Advanced DB maintenance folded.
- Startup auto-creates `wallpapers.db` schema if absent. Library shows "Library is empty.
  Add sources or scan Wallpaper Engine." until sources are added.

Manual verification items:
1. Fresh config dir: launch GUI, verify no errors, Library shows empty state prompt.
2. Scan WE: click Scan Wallpaper Engine, observe progress in toolbar.
3. After scan: Library shows wallpapers, no TUI intervention needed.
4. Settings: "Library Database" section shows wallpapers count. Advanced section is collapsed.
5. Restart: Library data persists across GUI restarts.

## 2026-06-11 Automated Session Note

Environment detected: Arch Linux, niri Wayland, `DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-1`.

Attempted:

```bash
target/release/wallpaper-console-tauri
dbus-run-session -- target/release/wallpaper-console-tauri
niri msg windows
grim /tmp/wcr-tauri-manual.png
```

Result: the binary could stay alive under a foreground `timeout`, but this automated terminal session could not produce a stable visible Tauri window in `niri msg windows`; screenshots captured the terminal workspace, not the app. Therefore real desktop GUI visual acceptance is **not marked complete** for this run.

Use the checklist below from an interactive desktop session.

- [ ] Launch `wallpaper-console-gui-rust` from a terminal.
- [ ] Confirm Library renders without a blank view.
- [ ] Switch Library / Favorites / History / Sources / Settings.
- [ ] Switch Library → History → Library. Confirm thumbnails persist and no "Loading library..." appears.
- [ ] Switch to Favorites. Confirm pagination "Load more" button appears if favorites exceed 120 items.
- [ ] Switch to History. Confirm pagination "Load more" button appears if history exceeds 120 items.
- [ ] Run Rescan and confirm the status bar reports completion.
- [ ] Open Sources and run Scan Wallpaper Engine.
- [ ] Confirm WE scene project `3558034522` appears as a single `WE Scene` card, not as `assets/*.png` fragments.
- [ ] Confirm WE web project `3650880224` appears as a single `WE Web` card, not as `index.html` or `assets/*.png` fragments.
- [ ] Confirm WE scene/web cards use `preview.gif` when present.
- [ ] Right-click a WE Scene card. Confirm `Apply`, `Apply preview GIF`, `Open folder`, and `Copy Workshop ID` are visible. `Apply with linux-wallpaperengine` must NOT appear.
- [ ] Right-click a WE Web card. Confirm only `Open folder` and `Copy Workshop ID` are visible. `Apply`, `Apply preview GIF`, `Apply with linux-wallpaperengine`, `Apply Web wallpaper`, and `Open experimental Chromium preview` must NOT appear.
- [ ] Double-click a WE Web card. Confirm it reports a clear unsupported error and does not change current wallpaper state.
- [ ] If `linux-wallpaperengine` is not installed, applying a WE scene card shows the install suggestion.
- [ ] If `linux-wallpaperengine` is installed, apply a WE scene card and confirm current state points at the project folder, not `preview.gif`.
- [ ] Confirm WE Web preview media is used only as the card thumbnail; it is not offered as a live wallpaper action.

### Apply execution

- [ ] Right-click a WE Scene and Apply: current state records the project path.
- [ ] Right-click the same WE Scene and Apply preview GIF: current state records the preview file path.
- [ ] WE Web does not show Apply and double-click shows a warning.
- [ ] Failed WE Scene shows Retry backend apply; after retry, the card refreshes.
- [ ] Rapidly click two different wallpapers; final status should match the last clicked item.

- [ ] Open Settings and run SQLite Verify.
- [ ] Open Settings and confirm Wallpaper Engine Backend shows Ready/Missing with the detected binary path or install hint.
- [ ] Open Settings and run Thumbnail Cache Status / Clear.
- [ ] Open Settings and click "Export diagnostics". Confirm the toast shows the file path.
- [ ] Right-click a wallpaper card and confirm context menu placement.
- [ ] Add a wallpaper to Favorites from Library context menu. Switch to Favorites tab. Confirm the new favorite appears.
- [ ] Apply a known safe image wallpaper.
- [ ] Run Stop and Restore.
- [ ] On niri, confirm app-id/window rules still match the Tauri app.
- [ ] Note WebKitGTK 4.1 rendering or animation issues.

### Backend switching

- Image -> image: should transition normally.
- Image -> video: should not briefly show an older unrelated image.
- Video -> image: should not flash the previous image before the requested image.
- Video -> scene: brief wait is acceptable; long black screen is not.
- Scene -> image/video: old scene should disappear only after target backend has started.
- Failed scene apply: current state should remain the previous wallpaper.
