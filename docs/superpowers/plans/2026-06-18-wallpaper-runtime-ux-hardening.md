# Wallpaper Runtime UX Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix boot restore flashes, add a two-theme switcher, remove noisy single-click batch selection, make image/video/scene switching clean instead of preview-stale, and simplify installation to the binary-copy path.

**Architecture:** Keep runtime behavior in Rust (`wc-backend`, `wc-app`, Tauri commands), UI preference and interaction behavior in React, and install contract in `install.sh` plus docs. Use source truth over README prose, preserve successful-apply-only state writes, and prefer explicit clean backend lifecycle over visual fallback tricks.

**Tech Stack:** Rust workspace crates, Tauri 2 commands, React 19 + TypeScript + Vite, node:test frontend unit tests, Playwright smoke tests, Bash install script.

---

## Non-Negotiable Decisions

- Single click must not apply wallpaper.
- Single click must not show `1 selected Add to Favorites Clear`.
- Cross image/video/scene switching must prefer a short blank/clean transition over stale image or preview display.
- `current`, `last_backend`, and `history` must be written only after the target backend succeeds.
- Default user install path is binary copy through `./install.sh`; deb/rpm/AUR are not primary install paths in this pass.
- Do not reintroduce Wails or Python GUI code.

## File Map

- Modify `crates/wc-backend/src/lib.rs`: add clean restore entrypoint, adjust fallback/handoff execution, update tests.
- Modify `crates/wc-backend/src/lifecycle.rs`: make clean cross-backend stop policy explicit.
- Modify `crates/wc-backend/src/visual_handoff.rs`: disable preview fallback for video/scene targets and update tests.
- Modify `crates/wc-app/src/lib.rs` and `crates/wc-app/src/apply_execution.rs`: stop producing fallback paths for scene apply; keep preview action behavior separate.
- Modify `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`: use clean restore and keep structured errors.
- Modify `crates/wc-core/src/config.rs`: add `gui_theme` default.
- Modify `apps/tauri-gui/frontend/src/App.tsx`: load and apply theme.
- Modify `apps/tauri-gui/frontend/src/settings/configSchema.ts`, `types.ts`, `pages/GeneralPage.tsx`, `views/SettingsView.tsx`: expose `gui_theme`.
- Modify `apps/tauri-gui/frontend/src/styles/global.css`: convert current palette into theme variables and add Obsidian warm palette.
- Modify `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`, `views/LibraryView.tsx`: remove batch-selection behavior.
- Delete `apps/tauri-gui/frontend/src/hooks/useGridSelection.ts` and `useGridSelection.test.ts` if no callers remain.
- Modify `apps/tauri-gui/frontend/e2e/smoke.spec.ts`: update selection/theme expectations.
- Modify `install.sh`, `scripts/test_install_build_only.sh`, `README.md`, `docs/DEVELOPMENT.md`, `docs/TAURI_ARCHITECTURE.md`, `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`, `docs/RUNTIME_FORMATS.md`: sync install/theme/runtime docs.

## Task 1: Clean Boot Restore

**Files:**
- Modify: `crates/wc-backend/src/lib.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`
- Modify: `crates/wc-cli/src/main.rs`
- Test: `crates/wc-backend/src/lib.rs`
- Test: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`

- [ ] **Step 1: Add failing backend restore tests**

Add tests under `#[cfg(test)] mod tests` in `crates/wc-backend/src/lib.rs`.

First extend the existing `FakeRuntime` in the same test module. Do not create a second fake runtime type.

```rust
#[derive(Default)]
struct FakeRuntime {
    stop_awww_count: usize,
    stop_mpvpaper_count: usize,
    stop_lwe_count: usize,
    command_output_count: usize,
    command_status_count: usize,
    clear_awww_state_hint_count: usize,
    command_output_success: bool,
    command_status_success: bool,
    command_output_programs: Vec<String>,
    command_status_programs: Vec<String>,
}
```

Update `command_output`:

```rust
fn command_output(
    &mut self,
    command: &mut std::process::Command,
) -> Result<std::process::Output, WcError> {
    self.command_output_count += 1;
    self.command_output_programs
        .push(command.get_program().to_string_lossy().to_string());
    let program = if self.command_output_success { "true" } else { "false" };
    std::process::Command::new(program)
        .output()
        .map_err(|e| WcError::Other(format!("fake command failed: {}", e)))
}
```

Update `command_status`:

```rust
fn command_status(
    &mut self,
    command: &mut std::process::Command,
) -> Result<std::process::ExitStatus, WcError> {
    self.command_status_count += 1;
    self.command_status_programs
        .push(command.get_program().to_string_lossy().to_string());
    let program = if self.command_status_success { "true" } else { "false" };
    std::process::Command::new(program)
        .status()
        .map_err(|e| WcError::Other(format!("fake command failed: {}", e)))
}
```

Where existing tests expect fake commands to succeed, initialize with:

```rust
let mut rt = FakeRuntime {
    command_output_success: true,
    command_status_success: true,
    ..Default::default()
};
```

Then add the restore tests:

```rust
#[test]
fn restore_clean_stops_all_before_reapplying_current_image() {
    let (tmp, s) = temp_storage();
    let img = tmp.path().join("current.png");
    std::fs::write(&img, b"png").unwrap();
    s.current_write(&img.to_string_lossy()).unwrap();
    s.last_backend_write("mpvpaper").unwrap();

    let mut rt = FakeRuntime {
        command_output_success: true,
        command_status_success: true,
        ..Default::default()
    };

    restore_clean_with_runtime(&s, &mut rt).unwrap();

    assert_eq!(rt.stop_awww_count, 1, "clean restore must stop awww first");
    assert_eq!(rt.stop_mpvpaper_count, 1, "clean restore must stop mpvpaper first");
    assert_eq!(rt.stop_lwe_count, 1, "clean restore must stop LWE first");
    assert!(
        rt.command_output_programs.iter().any(|p| p == "awww"),
        "restore should apply image through awww"
    );
    assert_eq!(s.current_read().unwrap().as_deref(), Some(img.to_string_lossy().as_ref()));
    assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
}

#[test]
fn restore_clean_failure_preserves_previous_state() {
    let (tmp, s) = temp_storage();
    let img = tmp.path().join("current.png");
    std::fs::write(&img, b"png").unwrap();
    s.current_write(&img.to_string_lossy()).unwrap();
    s.last_backend_write("mpvpaper").unwrap();
    s.history_add(&img.to_string_lossy(), "mpvpaper").unwrap();
    let history_before = s.history_list().unwrap();

    let mut rt = FakeRuntime {
        command_output_success: false,
        command_status_success: true,
        ..Default::default()
    };

    let err = restore_clean_with_runtime(&s, &mut rt).unwrap_err();
    assert!(err.to_string().contains("awww apply failed"));
    assert_eq!(s.current_read().unwrap().as_deref(), Some(img.to_string_lossy().as_ref()));
    assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
    assert_eq!(s.history_list().unwrap(), history_before);
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p wc-backend restore_clean -- --nocapture
```

Expected before implementation: compile failure because `restore_clean_with_runtime` does not exist.

- [ ] **Step 3: Implement clean restore helpers**

In `crates/wc-backend/src/lib.rs`, add:

```rust
pub fn restore_clean(s: &StorageApi) -> Result<(), WcError> {
    let mut runtime = runtime::SystemBackendRuntime;
    restore_clean_with_runtime(s, &mut runtime)
}

fn restore_clean_with_runtime(
    s: &StorageApi,
    runtime: &mut dyn runtime::BackendRuntime,
) -> Result<(), WcError> {
    let current = s
        .current_read()?
        .ok_or_else(|| WcError::Other("no previous wallpaper to restore".into()))?;
    let p = std::path::Path::new(&current);
    if !p.is_file() && !p.is_dir() {
        return Err(WcError::WallpaperMissing(p.to_path_buf()));
    }

    let entry = wc_scan::make_entry(&current)
        .ok_or_else(|| WcError::UnsupportedFileType(current.clone()))?;
    let backend = backend_for_restore_entry(s, &entry);
    let fallback_path = fallback_for_restore_entry(&entry, p);

    execute_stop_plan_with_runtime(s, lifecycle::StopPlan::All, runtime)?;
    apply_wallpaper_with_runtime(s, &current, backend, fallback_path.as_deref(), runtime)
}

fn backend_for_restore_entry(s: &StorageApi, entry: &wc_core::types::WallpaperEntry) -> Backend {
    match entry.file_type {
        wc_core::types::FileType::Image => {
            match wc_core::config::normalize_image_backend(&s.config_get("image_backend", "awww")) {
                "mpvpaper" => Backend::Mpvpaper,
                _ => Backend::Awww,
            }
        }
        wc_core::types::FileType::Gif => match s.config_get("gif_backend", "awww").as_str() {
            "mpvpaper" => Backend::Mpvpaper,
            _ => Backend::Awww,
        },
        wc_core::types::FileType::Video => match s.config_get("video_backend", "mpvpaper").as_str() {
            "awww" => Backend::Awww,
            _ => Backend::Mpvpaper,
        },
        wc_core::types::FileType::WeScene => Backend::LinuxWallpaperEngine,
        wc_core::types::FileType::WeWeb | wc_core::types::FileType::WeApplication => Backend::Unsupported,
    }
}

fn fallback_for_restore_entry(
    entry: &wc_core::types::WallpaperEntry,
    path: &std::path::Path,
) -> Option<String> {
    match entry.file_type {
        wc_core::types::FileType::Image | wc_core::types::FileType::Gif => {
            Some(entry.path.clone())
        }
        wc_core::types::FileType::Video
        | wc_core::types::FileType::WeScene
        | wc_core::types::FileType::WeWeb
        | wc_core::types::FileType::WeApplication => None,
    }
}
```

Then simplify existing `restore` to call `restore_clean(s)` or keep `restore` as a thin wrapper:

```rust
pub fn restore(s: &StorageApi) -> Result<(), WcError> {
    restore_clean(s)
}
```

Do not keep old scene preview fallback in restore.

- [ ] **Step 4: Point Tauri and CLI restore at clean restore**

In `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`, change:

```rust
Ok(s) => match wc_backend::restore(&s) {
```

to:

```rust
Ok(s) => match wc_backend::restore_clean(&s) {
```

In `crates/wc-cli/src/main.rs`, change the `Commands::Restore` branch to:

```rust
Commands::Restore => {
    wc_backend::restore_clean(s)?;
    println!("Wallpaper restored.");
}
```

- [ ] **Step 5: Run restore tests**

Run:

```bash
cargo test -p wc-backend restore_clean -- --nocapture
cargo test -p wallpaper-console-tauri restore -- --nocapture
```

Expected: all targeted restore tests pass.

## Task 2: Clean Cross-Type Handoff

**Files:**
- Modify: `crates/wc-backend/src/lifecycle.rs`
- Modify: `crates/wc-backend/src/visual_handoff.rs`
- Modify: `crates/wc-backend/src/lib.rs`
- Modify: `crates/wc-app/src/lib.rs`
- Modify: `crates/wc-app/src/apply_execution.rs`
- Test: `crates/wc-backend/src/lifecycle.rs`
- Test: `crates/wc-backend/src/visual_handoff.rs`
- Test: `crates/wc-backend/src/lib.rs`
- Test: `crates/wc-app/src/apply_execution.rs`

- [ ] **Step 1: Change visual handoff tests first**

In `crates/wc-backend/src/visual_handoff.rs`, replace tests that expect `TargetPreviewInstant` for video/scene targets with clean expectations:

```rust
#[test]
fn video_after_image_uses_no_preview_fallback() {
    let plan = plan_visual_handoff(
        RunningBackend::Awww,
        Backend::Mpvpaper,
        Some("/tmp/preview.gif"),
    );
    assert_eq!(plan.fallback_stage, FallbackStage::None);
    assert_eq!(plan.target_startup_settle_ms, MPVPAPER_STARTUP_SETTLE_MS);
    assert!(!plan.stop_previous_after_fallback);
    assert!(!plan.stop_fallback_after_target_settle);
}

#[test]
fn scene_after_video_uses_no_preview_fallback() {
    let plan = plan_visual_handoff(
        RunningBackend::Mpvpaper,
        Backend::LinuxWallpaperEngine,
        Some("/tmp/preview.gif"),
    );
    assert_eq!(plan.fallback_stage, FallbackStage::None);
    assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
    assert!(!plan.stop_previous_after_fallback);
    assert!(!plan.stop_fallback_after_target_settle);
}

#[test]
fn scene_to_scene_uses_no_preview_fallback() {
    let plan = plan_visual_handoff(
        RunningBackend::LinuxWallpaperEngine,
        Backend::LinuxWallpaperEngine,
        Some("/tmp/preview.gif"),
    );
    assert_eq!(plan.fallback_stage, FallbackStage::None);
    assert_eq!(plan.target_startup_settle_ms, LWE_STARTUP_SETTLE_MS);
    assert!(!plan.stop_previous_after_fallback);
    assert!(!plan.stop_fallback_after_target_settle);
}
```

- [ ] **Step 2: Change app fallback tests first**

In `crates/wc-app/src/apply_execution.rs`, replace `scene_apply_target_exposes_preview_fallback_path` with:

```rust
#[test]
fn scene_apply_target_has_no_preview_fallback_for_clean_handoff() {
    let (tmp, service) = temp_service();
    let project = scene_project_with_preview(tmp.path());
    let request = ApplyRequest {
        kind: ApplyRequestKind::Apply,
        path: project.to_string_lossy().to_string(),
        request_id: Some("fb-scene".into()),
    };
    let target = service.resolve_apply_request_target(&request).unwrap();
    assert!(target.fallback_path.is_none());
    assert_eq!(target.backend, Backend::LinuxWallpaperEngine);
}
```

Keep `apply_preview_uses_preview_file_not_project_dir` unchanged; preview action is still allowed as an explicit user action.

- [ ] **Step 3: Run failing tests**

Run:

```bash
cargo test -p wc-backend visual_handoff -- --nocapture
cargo test -p wc-app fallback -- --nocapture
```

Expected before implementation: tests fail because scene/video still use preview fallback.

- [ ] **Step 4: Disable preview fallback for video and scene targets**

In `crates/wc-backend/src/visual_handoff.rs`, change `plan_mpvpaper_handoff` so every branch returns:

```rust
fallback_stage: FallbackStage::None,
stop_previous_after_fallback: false,
stop_fallback_after_target_settle: false,
```

Keep `target_startup_settle_ms: MPVPAPER_STARTUP_SETTLE_MS`.

Change `plan_lwe_handoff` so every branch returns:

```rust
fallback_stage: FallbackStage::None,
stop_previous_after_fallback: false,
stop_fallback_after_target_settle: false,
```

Keep `target_startup_settle_ms: LWE_STARTUP_SETTLE_MS`.

Do not change `plan_awww_handoff` for image targets yet; image after video/scene may still use `TargetImageInstant` if the target itself is the image.

- [ ] **Step 5: Stop old backend before video/scene target starts**

In `crates/wc-backend/src/lifecycle.rs`, change `pre_stop_plan` for clean targets:

```rust
pub fn pre_stop_plan(previous: RunningBackend, target: Backend) -> StopPlan {
    match target {
        Backend::Awww => StopPlan::None,
        Backend::Mpvpaper => match previous {
            RunningBackend::Awww => StopPlan::AwwwDaemonOnly,
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::LinuxWallpaperEngine => StopPlan::LweOnly,
            RunningBackend::None => StopPlan::All,
            RunningBackend::Unsupported | RunningBackend::Unknown => StopPlan::None,
        },
        Backend::LinuxWallpaperEngine => match previous {
            RunningBackend::Awww => StopPlan::AwwwDaemonOnly,
            RunningBackend::Mpvpaper => StopPlan::MpvpaperOnly,
            RunningBackend::LinuxWallpaperEngine => StopPlan::NonLwe,
            RunningBackend::None | RunningBackend::Unsupported | RunningBackend::Unknown => StopPlan::None,
        },
        Backend::Unsupported => StopPlan::All,
    }
}
```

Update affected lifecycle tests:

```rust
#[test]
fn video_after_image_stops_old_image_before_new_video() {
    let plan = plan_apply_lifecycle("awww", Backend::Mpvpaper);
    assert_eq!(plan.pre_stop, StopPlan::AwwwDaemonOnly);
    assert_eq!(plan.post_success_stop, StopPlan::None);
}

#[test]
fn scene_after_video_stops_old_video_before_scene() {
    let plan = plan_apply_lifecycle("mpvpaper", Backend::LinuxWallpaperEngine);
    assert_eq!(plan.pre_stop, StopPlan::MpvpaperOnly);
    assert_eq!(plan.post_success_stop, StopPlan::None);
}
```

For image targets, keep post-success cleanup because image can become visible quickly through awww.

- [ ] **Step 6: Stop producing scene fallback paths**

In `crates/wc-app/src/lib.rs`, change `resolve_fallback`:

```rust
fn resolve_fallback(target: &ApplyTarget) -> Option<String> {
    match target.file_type {
        FileType::Image | FileType::Gif => Some(target.resolved_path.clone()),
        FileType::Video
        | FileType::WeScene
        | FileType::WeWeb
        | FileType::WeApplication => None,
    }
}
```

Do not change `ApplyPreview`; it should continue to resolve the preview file and apply it as its own wallpaper.

- [ ] **Step 7: Add runtime execution regression tests**

In `crates/wc-backend/src/lib.rs`, use the extended `FakeRuntime` from Task 1 Step 1. Add:

```rust
#[test]
fn video_after_image_stops_awww_before_starting_mpvpaper_without_fallback() {
    let (tmp, s) = temp_storage();
    let video = tmp.path().join("next.mp4");
    std::fs::write(&video, b"mp4").unwrap();
    s.last_backend_write("awww").unwrap();

    let mut rt = FakeRuntime {
        command_output_success: true,
        command_status_success: true,
        ..Default::default()
    };

    apply_wallpaper_with_runtime(
        &s,
        &video.to_string_lossy(),
        Backend::Mpvpaper,
        Some("/tmp/should-not-be-used.gif"),
        &mut rt,
    )
    .unwrap();

    assert_eq!(rt.stop_awww_count, 1);
    assert_eq!(rt.stop_mpvpaper_count, 0);
    assert_eq!(
        rt.command_output_count, 0,
        "video target must not use awww fallback command_output"
    );
    assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("mpvpaper"));
}

#[test]
fn scene_after_video_stops_mpvpaper_before_lwe_without_preview_fallback() {
    let (tmp, s) = temp_storage();
    let scene = tmp.path().join("steamapps/workshop/content/431960/2651567796");
    std::fs::create_dir_all(&scene).unwrap();
    std::fs::write(scene.join("scene.pkg"), b"scene").unwrap();
    std::fs::write(
        scene.join("project.json"),
        r#"{"type":"scene","file":"scene.pkg","workshopid":"2651567796"}"#,
    ).unwrap();
    s.last_backend_write("mpvpaper").unwrap();

    let bin = tmp.path().join("fake-linux-wallpaperengine");
    std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
    }
    s.config_set("linux_wallpaperengine_path", &bin.to_string_lossy()).unwrap();

    let mut rt = FakeRuntime {
        command_output_success: true,
        command_status_success: true,
        ..Default::default()
    };
    apply_wallpaper_with_runtime(
        &s,
        &scene.to_string_lossy(),
        Backend::LinuxWallpaperEngine,
        Some("/tmp/should-not-be-used.gif"),
        &mut rt,
    ).unwrap();

    assert_eq!(rt.stop_mpvpaper_count, 1);
    assert_eq!(
        rt.command_output_count, 0,
        "scene target must not use preview fallback command_output"
    );
    assert_eq!(s.current_read().unwrap().as_deref(), Some(scene.to_string_lossy().as_ref()));
    assert_eq!(s.last_backend_read().unwrap().as_deref(), Some(crate::LWE_BACKEND_NAME));
}
```

Do not add a second fake runtime type; adapt the existing `FakeRuntime` once and reuse it for all backend lifecycle tests.

- [ ] **Step 8: Run handoff tests**

Run:

```bash
cargo test -p wc-backend lifecycle -- --nocapture
cargo test -p wc-backend visual_handoff -- --nocapture
cargo test -p wc-backend video_after_image -- --nocapture
cargo test -p wc-backend scene_after_video -- --nocapture
cargo test -p wc-app fallback -- --nocapture
```

Expected: all targeted tests pass.

## Task 3: Remove Library Batch Selection UI

**Files:**
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/LibraryView.tsx`
- Delete if unused: `apps/tauri-gui/frontend/src/hooks/useGridSelection.ts`
- Delete if unused: `apps/tauri-gui/frontend/src/hooks/useGridSelection.test.ts`
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`

- [ ] **Step 1: Update Playwright expectation first**

Replace the `batch add favorites does not apply to unsupported items` test in `apps/tauri-gui/frontend/e2e/smoke.spec.ts` with:

```ts
test('single click does not show batch selection toolbar', async ({ page }) => {
  await page.goto('/');
  const card = page.locator('.wallpaper-card').first();
  await card.click();
  await expect(page.getByText(/selected$/)).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Add to Favorites' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Clear' })).toHaveCount(0);
});
```

Keep context-menu tests that expect `Add to Favorites`.

- [ ] **Step 2: Remove selection props from grid**

In `WallpaperGrid.tsx`:

- Remove `useGridSelection` import.
- Remove `selectedPaths?: Set<string>` and `onSelectionChange?: (paths: Set<string>) => void` from `Props`.
- Remove `entryPaths`, `clearSelection`, `handleSelectionClick`, and Escape key effect.
- Change card class from:

```tsx
className={`wallpaper-card${applying ? ' disabled' : ''}${selected ? ' selected' : ''}`}
```

to:

```tsx
className={`wallpaper-card${applying ? ' disabled' : ''}`}
```

- Change card click handler to a no-op focus affordance:

```tsx
onClick={() => undefined}
```

or remove `onClick` entirely. Do not call `onApply` on single click.

- [ ] **Step 3: Remove Library selection state**

In `LibraryView.tsx`:

- Remove `X` import from lucide-react.
- Remove `selectedPaths` state.
- Remove `handleBatchAddFavorites`.
- Remove the selected toolbar fragment:

```tsx
{selectedPaths.size > 0 && (...)}
```

- Remove `selectedPaths={selectedPaths}` and `onSelectionChange={setSelectedPaths}` from `WallpaperGrid`.

- [ ] **Step 4: Delete unused hook files**

Run:

```bash
rg -n "useGridSelection|nextSelectionForClick|selectedPaths|onSelectionChange" apps/tauri-gui/frontend/src
```

If no production caller remains, delete:

```text
apps/tauri-gui/frontend/src/hooks/useGridSelection.ts
apps/tauri-gui/frontend/src/hooks/useGridSelection.test.ts
```

If `selectedPaths` appears only in unrelated types after edits, remove those references too.

- [ ] **Step 5: Run frontend tests**

Run:

```bash
cd apps/tauri-gui/frontend && npm run test:unit
cd apps/tauri-gui/frontend && npm run smoke -- --grep "single click does not show batch selection toolbar"
```

Expected: unit tests pass; smoke confirms no selection toolbar appears.

## Task 4: Add Current / Obsidian Warm Theme Switcher

**Files:**
- Modify: `crates/wc-core/src/config.rs`
- Modify: `docs/RUNTIME_FORMATS.md`
- Modify: `apps/tauri-gui/frontend/src/settings/configSchema.ts`
- Modify: `apps/tauri-gui/frontend/src/settings/types.ts`
- Modify: `apps/tauri-gui/frontend/src/settings/pages/GeneralPage.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/SettingsView.tsx`
- Modify: `apps/tauri-gui/frontend/src/App.tsx`
- Modify: `apps/tauri-gui/frontend/src/styles/global.css`
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`

- [ ] **Step 1: Add config default test**

In `crates/wc-core/src/config.rs`, add to tests:

```rust
#[test]
fn default_config_includes_gui_theme() {
    let defaults = default_config_map();
    assert_eq!(defaults.get("gui_theme").copied(), Some("current"));
}
```

If there is no `default_config_map()` helper, use the existing test helper that reads `DEFAULT_CONFIG_PAIRS`.

- [ ] **Step 2: Add config default**

In `DEFAULT_CONFIG_PAIRS`, add:

```rust
("gui_theme", "current"),
```

Place it near other GUI settings such as `gui_debug_logs`.

- [ ] **Step 3: Add frontend schema entry**

In `apps/tauri-gui/frontend/src/settings/configSchema.ts`, add category support:

```ts
export interface SettingEntry {
  key: string;
  label: string;
  type: 'select' | 'text' | 'number';
  options?: string[];
  optionLabels?: Record<string, string>;
  placeholder?: string;
  category: 'general' | 'wallpaper' | 'we' | 'library' | 'advanced';
  advanced?: boolean;
  description?: string;
}
```

Add to `ALL_SETTINGS`:

```ts
{
  key: 'gui_theme',
  label: 'Theme',
  type: 'select',
  options: ['current', 'obsidian_warm'],
  optionLabels: {
    current: 'Current',
    obsidian_warm: 'Obsidian Warm',
  },
  category: 'general',
  description: 'Switch between the current UI palette and a warm Obsidian-style palette.',
},
```

Update `normalizeConfigValue`:

```ts
if (key === 'gui_theme') {
  return value === 'obsidian_warm' ? 'obsidian_warm' : 'current';
}
```

Update `ConfigRow` if needed so `optionLabels` controls visible text:

```tsx
{setting.options?.map((option) => (
  <option key={option} value={option}>
    {setting.optionLabels?.[option] ?? option}
  </option>
))}
```

- [ ] **Step 4: Wire General page setting**

Update `GeneralPageProps` in `apps/tauri-gui/frontend/src/settings/types.ts`:

```ts
export interface GeneralPageProps {
  libraryStatus: LibrarySourceStatusDTO | null;
  weStatus: LinuxWallpaperEngineStatusDTO | null;
  thumbCache: ThumbnailCacheDTO | null;
  configs: Record<string, string>;
  saving: string | null;
  onSet: (key: string, value: string) => Promise<boolean>;
}
```

In `GeneralPage.tsx`, import `getSettingsByCategoryAndLevel` and `ConfigRow`, then add a Preferences section before Status:

```tsx
const preferenceSettings = getSettingsByCategoryAndLevel('general', false);

<PageSection title="Preferences">
  {preferenceSettings.map((setting) => (
    <ConfigRow
      key={setting.key}
      setting={setting}
      value={configs[setting.key] ?? ''}
      saving={saving === setting.key}
      onSet={(value) => onSet(setting.key, value)}
    />
  ))}
</PageSection>
```

Update `SettingsView.tsx` `GeneralPage` call:

```tsx
<GeneralPage
  libraryStatus={libraryStatus}
  weStatus={weStatus}
  thumbCache={thumbCache}
  configs={configs}
  saving={saving}
  onSet={handleSet}
/>
```

- [ ] **Step 5: Apply theme in App**

In `App.tsx`, import `useEffect` and `api`:

```tsx
import { lazy, startTransition, Suspense, useCallback, useEffect, useState } from 'react';
import { api, CommandResult, ApplyRequestDTO } from './api/bridge';
import { APP_EVENTS } from './events/appEvents';
```

Add:

```tsx
function normalizeTheme(value: string | null | undefined): 'current' | 'obsidian_warm' {
  return value === 'obsidian_warm' ? 'obsidian_warm' : 'current';
}

function applyTheme(value: string | null | undefined) {
  document.documentElement.dataset.theme = normalizeTheme(value);
}
```

Inside `AppShell`:

```tsx
useEffect(() => {
  let cancelled = false;
  api.configGet('gui_theme')
    .then((value) => {
      if (!cancelled) applyTheme(value);
    })
    .catch(() => {
      if (!cancelled) applyTheme('current');
    });

  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ key: string; value: string }>).detail;
    if (detail?.key === 'gui_theme') applyTheme(detail.value);
  };
  window.addEventListener(APP_EVENTS.configChanged, handler);
  return () => {
    cancelled = true;
    window.removeEventListener(APP_EVENTS.configChanged, handler);
  };
}, []);
```

If `APP_EVENTS.configChanged` is not exported, add it in `apps/tauri-gui/frontend/src/events/appEvents.ts` using the same pattern as existing app events.

- [ ] **Step 6: Refactor CSS variables**

In `global.css`:

- Replace `:root { ... }` light theme block with:

```css
:root,
:root[data-theme="current"] {
  color-scheme: light;
  --bg: #f6f7fb;
  --panel: #ffffff;
  --panel-alt: #f1f4f8;
  --panel-hover: #e8edf4;
  --surface: #ffffff;
  --surface-muted: #f1f4f8;
  --text: #20242c;
  --text-muted: #667085;
  --text-soft: #8a94a6;
  --border: #d9dee8;
  --border-strong: #c4ccda;
  --border-subtle: #e8ecf2;
  --primary-bg: #e9f1ff;
  --primary-bg-hover: #dceaff;
  --primary: #2563eb;
  --primary-strong: #1d4ed8;
  --danger-bg: #fff1f1;
  --danger-bg-hover: #ffe1e1;
  --danger: #dc2626;
  --success-bg: #ecfdf3;
  --success-border: #bbf7d0;
  --success: #15803d;
  --warning-bg: #fff7dc;
  --warning: #a16207;
  --shadow: rgba(15, 23, 42, 0.14);
}
```

- Add:

```css
:root[data-theme="obsidian_warm"] {
  color-scheme: dark;
  --bg: #1e1b18;
  --panel: #29231f;
  --panel-alt: #322a25;
  --panel-hover: #3a3029;
  --surface: #26211d;
  --surface-muted: #342c26;
  --text: #e7dccf;
  --text-muted: #b8a99a;
  --text-soft: #9f9082;
  --border: #4a3d34;
  --border-strong: #6b5748;
  --border-subtle: #3a3029;
  --primary-bg: #3a2f21;
  --primary-bg-hover: #4a3a25;
  --primary: #d19a66;
  --primary-strong: #e3b17c;
  --danger-bg: #3b2422;
  --danger-bg-hover: #4a2b28;
  --danger: #ef9a91;
  --success-bg: #203126;
  --success-border: #38543e;
  --success: #9ac49f;
  --warning-bg: #3d321f;
  --warning: #d6b36a;
  --shadow: rgba(0, 0, 0, 0.34);
}
```

- Convert remaining hardcoded light/dark colors in shared components to variables when they affect themed surfaces. Leave icon bitmap content and syntax-highlight-like raw key/value colors if they already use variables after the block.

- [ ] **Step 7: Add smoke test**

In `smoke.spec.ts`, add:

```ts
test('settings theme switch applies obsidian warm theme', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('combobox').filter({ hasText: /Current|Obsidian/ }).selectOption('obsidian_warm');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'obsidian_warm');
});
```

If Playwright cannot select by that locator, target the `Theme` config row:

```ts
const themeSelect = page.locator('.config-row').filter({ hasText: 'Theme' }).locator('select');
await themeSelect.selectOption('obsidian_warm');
await expect(page.locator('html')).toHaveAttribute('data-theme', 'obsidian_warm');
```

- [ ] **Step 8: Run theme tests**

Run:

```bash
cargo test -p wc-core gui_theme -- --nocapture
cd apps/tauri-gui/frontend && npm run typecheck
cd apps/tauri-gui/frontend && npm run smoke -- --grep "theme"
```

Expected: config default exists, TypeScript passes, smoke sees `data-theme="obsidian_warm"`.

## Task 5: Binary-Only Install Path

**Files:**
- Modify: `install.sh`
- Modify: `scripts/test_install_build_only.sh`
- Modify: `README.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/TAURI_ARCHITECTURE.md`
- Modify: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`

- [ ] **Step 1: Change install build command**

In `install.sh`, replace:

```bash
info "Building Tauri GUI..."
cd "$SCRIPT_DIR/apps/tauri-gui/src-tauri"
cargo tauri build --bundles deb,rpm
TAURI_BIN="$(realpath "$SCRIPT_DIR/target/release/wallpaper-console-tauri")"
```

with:

```bash
info "Building Tauri GUI binary..."
cd "$SCRIPT_DIR/apps/tauri-gui/src-tauri"
cargo tauri build --no-bundle
TAURI_BIN="$(realpath "$SCRIPT_DIR/target/release/wallpaper-console-tauri")"
```

If the installed Tauri CLI does not support `--no-bundle`, use:

```bash
cargo build --package wallpaper-console-tauri --release
```

and keep `TAURI_BIN` unchanged. Use exactly one of these; do not keep deb/rpm in `install.sh`.

- [ ] **Step 2: Update install script text**

Change comments and output text from bundle language to binary language:

```bash
# install.sh — build release binaries and install wallpaper-console-rust (CLI + Tauri GUI)
```

Post-install text must not mention deb/rpm. Keep desktop launcher and icon install.

- [ ] **Step 3: Keep build-only test binary-focused**

In `scripts/test_install_build_only.sh`, keep:

```bash
test -x target/release/wallpaper-console-rust
test -x target/release/wallpaper-console-tauri
```

Add an assertion that no bundle is required:

```bash
if find target/release/bundle -type f 2>/dev/null | grep -q .; then
  echo "NOTE: bundle artifacts exist from a previous build, but install verification only requires release binaries"
fi
```

Do not fail if stale bundle files exist.

- [ ] **Step 4: Update README**

In `README.md`:

- Replace “Installable as `.deb`/`.rpm` bundle” with “Installable through the binary-copy `install.sh` path.”
- Replace build instructions:

```bash
cd ../src-tauri
cargo tauri build --bundles deb,rpm
```

with:

```bash
./install.sh --build-only
```

- Add one sentence:

```md
Package-manager installs such as AUR are future work; the supported local install path is the binary-copy installer.
```

- [ ] **Step 5: Update development docs**

In `docs/DEVELOPMENT.md`, keep bundle build only under a developer-only heading:

```md
## Developer Bundle Experiments

The user-facing install path is `./install.sh`, which copies release binaries into the selected prefix. deb/rpm/AUR packaging is not the supported install path for this pass.
```

Remove wording that implies deb/rpm is a validated user install method.

In `docs/TAURI_ARCHITECTURE.md`, change the Build section to list:

```md
- Binary: `target/release/wallpaper-console-tauri`
- Installed GUI command: `wallpaper-console-gui-rust`
```

Do not list deb/rpm as primary outputs.

In `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`, change:

```md
Run after `./install.sh` or after installing a `.deb` / `.rpm` bundle.
```

to:

```md
Run after `./install.sh` or `./install.sh --build-only`.
```

- [ ] **Step 6: Run install verification**

Run:

```bash
./install.sh --build-only
./scripts/test_install_build_only.sh
```

Expected: both commands pass and `target/release/wallpaper-console-tauri` exists.

## Task 6: Docs Sync for Runtime Formats and Manual Verification

**Files:**
- Modify: `docs/RUNTIME_FORMATS.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`

- [ ] **Step 1: Runtime formats**

In `docs/RUNTIME_FORMATS.md`, add:

```text
gui_theme=current
```

near other GUI settings.

Add a small note:

```md
`gui_theme` accepts `current` and `obsidian_warm`.
```

- [ ] **Step 2: Manual smoke checklist**

Add manual checks:

```md
- [ ] Reboot/login restore: with `current` set to a scene project such as `/home/chakew/.local/share/Steam/steamapps/workshop/content/431960/2651567796`, confirm no unrelated project such as `3558034522` appears before the target.
- [ ] Theme: switch Settings → General → Theme between Current and Obsidian Warm; confirm the whole shell changes without text overlap.
- [ ] Library click: single-click a card; confirm no `selected/Add to Favorites/Clear` toolbar appears.
- [ ] Cross-type switch: image → video → scene → video; confirm no old image or preview remains after the target starts.
```

- [ ] **Step 3: Development note**

In `docs/DEVELOPMENT.md`, add:

```md
Clean handoff policy: cross image/video/scene switching intentionally favors stopping conflicting renderers over using preview fallback layers. A short blank transition is acceptable; stale image or preview persistence is not.
```

## Task 7: Final Verification and Drift Check

**Files:**
- Verify all modified files.

- [ ] **Step 1: Search for forbidden leftovers**

Run:

```bash
rg -n "1 selected|selectedPaths|onSelectionChange|useGridSelection|TargetPreviewInstant|--bundles deb,rpm|\\.deb|\\.rpm|AUR" README.md docs install.sh apps/tauri-gui/frontend/src apps/tauri-gui/frontend/e2e crates -S
```

Expected:

- No `1 selected`, `selectedPaths`, `onSelectionChange`, or `useGridSelection` in frontend source.
- `TargetPreviewInstant` may remain as an enum variant only if used for future explicit preview logic, but it must not be selected for `Mpvpaper` or `LinuxWallpaperEngine` target plans.
- deb/rpm/AUR may appear only as future/developer-only wording, not as the main install path.

- [ ] **Step 2: Targeted test matrix**

Run:

```bash
cargo test -p wc-core gui_theme -- --nocapture
cargo test -p wc-app -- --nocapture
cargo test -p wc-backend -- --nocapture
cargo test -p wallpaper-console-tauri -- --nocapture
cd apps/tauri-gui/frontend && npm run test:unit
cd apps/tauri-gui/frontend && npm run typecheck
cd apps/tauri-gui/frontend && npm run smoke
```

Expected: all pass.

- [ ] **Step 3: Workspace verification**

Run from repo root:

```bash
cargo run -p xtask -- verify rust
cargo run -p xtask -- verify frontend
cargo build --workspace
git diff --check
```

Expected: all pass.

- [ ] **Step 4: Manual runtime check**

Run the built GUI:

```bash
./target/release/wallpaper-console-tauri
```

Manually verify:

- Settings → General has Theme selector.
- Obsidian Warm applies immediately.
- Library single click shows no batch toolbar.
- Right-click still shows context actions.
- Apply image → video → scene → video does not leave the old image/preview visible after the new target starts.
- If possible, perform one real logout/reboot restore check; do not claim reboot behavior is fully verified without this.

## Opencode Guardrails

- Implement tasks in order. Do not skip tests that are expected to fail first.
- Keep changes scoped to listed files unless a compiler error points to a directly related type import/export.
- Do not redesign the UI navigation, database schema, scan pipeline, thumbnail queue, or source management.
- Do not add a new installer, package format, service manager, or autostart manager.
- If `linux-wallpaperengine` behavior cannot be manually verified on this machine, leave a final note naming that runtime verification gap.
