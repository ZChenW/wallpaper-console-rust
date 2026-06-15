# Apply Execution Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make wallpaper apply execution use one explicit, testable pipeline so frontend actions, backend routing, process switching, current state, history, and user-facing errors cannot drift apart.

**Architecture:** Keep the completed `ApplyPlan` action model as the source of what the UI can show, then add an execution layer that consumes explicit apply intents. The new execution layer lives in `wc-app`, delegates backend process work to `wc-backend`, and returns structured `ApplyExecutionResult` data to Tauri so the GUI can distinguish applied, failed, unsupported, stale, and preview cases without string guessing.

**Tech Stack:** Rust workspace (`wc-app`, `wc-backend`, `wc-storage`, Tauri commands), React/TypeScript Tauri frontend, existing `CommandResultDto`, existing Playwright smoke tests, existing Node unit test runner.

---

## Current Code Facts

Read these files before editing:

- `crates/wc-app/src/apply_plan.rs`
  - Already defines `ApplyPlan`, `ApplyActionKind`, `ApplyAvailability`.
  - Used by DTO hydration, not by execution.
- `crates/wc-app/src/lib.rs`
  - `AppService::apply(path)` currently resolves and executes immediately.
  - `resolve_apply_target(path)` uses `wc_scan::make_entry()` and `backend_for_entry()`.
  - `AppError` already has structured fields.
- `crates/wc-backend/src/lib.rs`
  - `apply_wallpaper(s, path, backend)` does backend process work and writes `current`, `last_backend`, `history`.
  - `stop_backends_for_target()` still owns broad backend stop decisions.
- `crates/wc-backend/src/linux_wallpaperengine.rs`
  - Scene handoff already tries to keep old LWE alive until new one survives polling.
  - LWE apply writes state/history itself.
- `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`
  - Tauri `apply(path)` calls `AppService::apply(path)` and wraps errors.
- `apps/tauri-gui/frontend/src/App.tsx`
  - `handleApply(path)` implements latest-intent queue on the frontend.
  - It can only send a path, not an explicit action kind.
- `apps/tauri-gui/frontend/src/hooks/useLibraryEntryActions.ts`
  - Converts DTO actions into UI handlers.
  - `apply_preview` currently calls `onApply(e.previewPath)`, so preview apply is indistinguishable from normal apply in the backend.
- `apps/tauri-gui/frontend/src/api/feedback.ts`
  - Formats `CommandResult.error` when available.

## Problem Boundaries

This plan does not redesign scanning, settings, thumbnail cache, database schema, or UI layout.

This plan must preserve:

- WE Web remains unsupported as live wallpaper.
- WE Scene uses `linux-wallpaperengine`.
- Images/GIFs use `awww` by default.
- Videos use `mpvpaper` by default.
- `Apply preview GIF` remains available only when a preview path exists.
- Failed backend state for WE Scene can still be cleared and retried.
- Existing CLI behavior must not break.

## Target Behavior

Normal apply:

```text
UI action apply
  -> api.applyAction({ kind: "apply", path })
  -> Tauri apply_action
  -> AppService.execute_apply_request
  -> resolve using same rules as ApplyPlan
  -> preflight
  -> backend apply
  -> write state/history only on success
  -> structured result
```

Preview apply:

```text
UI action apply_preview
  -> api.applyAction({ kind: "apply_preview", path: project_path })
  -> backend resolves preview_path from project metadata
  -> applies preview file as a normal image/gif
  -> state/history records preview_path with backend awww/mpvpaper
```

Retry backend apply:

```text
UI action retry_backend_apply
  -> clear WE compatibility failure
  -> api.applyAction({ kind: "retry_backend_apply", path })
  -> backend applies real WE Scene project
  -> successful retry refreshes library/history/favorites state
```

Unsupported:

```text
UI should not expose apply for unsupported entries.
If invoked directly, backend returns structured unsupported error and does not stop current wallpaper.
```

Latest intent:

```text
If user clicks A then B quickly:
  - GUI should not run multiple overlapping Tauri applies.
  - Backend should still reject stale requests if the GUI sends overlap later.
  - Old apply result must not overwrite UI as if it were the latest user intent.
```

---

## File Structure

Create:

- `crates/wc-app/src/apply_execution.rs`
  - Owns `ApplyRequest`, `ApplyRequestKind`, `ApplyExecutionResult`, execution preflight, request-to-target conversion, structured execution errors.
- `apps/tauri-gui/frontend/src/domain/applyRequests.ts`
  - Converts `WallpaperDTO` + `ApplyActionKind` into a stable request DTO for Tauri.
- `apps/tauri-gui/frontend/src/domain/applyRequests.test.ts`
  - Unit tests for frontend request construction.

Modify:

- `crates/wc-app/src/lib.rs`
  - Export `apply_execution`.
  - Move or delegate `AppService::apply()` to `execute_apply_request()`.
  - Keep old `apply(path)` as compatibility wrapper.
- `crates/wc-app/src/apply_plan.rs`
  - No broad rewrite. Add helper functions only if needed to share preview/workshop checks.
- `crates/wc-backend/src/lib.rs`
  - Add `ApplyBackendRequest` or narrow helpers so state/history write is not duplicated incorrectly.
  - Keep existing `apply_wallpaper()` compatibility wrapper.
- `crates/wc-backend/src/linux_wallpaperengine.rs`
  - Keep current scene handoff behavior.
  - Expose only minimal helpers if tests need to verify state boundaries.
- `apps/tauri-gui/src-tauri/src/commands/common.rs`
  - Add DTOs for apply request/result if not using direct JSON structs from `wc-app`.
- `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`
  - Add `apply_action(request)` command.
  - Keep existing `apply(path)` for compatibility and CLI-style callers.
- `apps/tauri-gui/src-tauri/src/lib.rs`
  - Register `apply_action`.
- `apps/tauri-gui/frontend/src/api/bridge.ts`
  - Add `ApplyRequestDTO`, `ApplyResultDTO`, `api.applyAction`.
  - Keep `api.apply(path)` for temporary compatibility.
- `apps/tauri-gui/frontend/src/api/mockBridge.ts`
  - Add `applyAction`.
- `apps/tauri-gui/frontend/src/hooks/useLibraryEntryActions.ts`
  - Send explicit action kind and project path instead of applying preview path directly.
- `apps/tauri-gui/frontend/src/App.tsx`
  - Replace path-only queue with request queue.
  - Keep a compatibility `handleApply(path)` wrapper for components not yet converted.
- `apps/tauri-gui/frontend/e2e/smoke.spec.ts`
  - Add action execution smoke coverage with mock bridge.
- `docs/CURRENT_STATUS.md`
  - Record that apply display model and execution model are both unified.
- `docs/DEVELOPMENT.md`
  - Document apply action flow and testing commands.

---

## Task 1: Rust Execution Types

**Files:**
- Create: `crates/wc-app/src/apply_execution.rs`
- Modify: `crates/wc-app/src/lib.rs`

- [ ] **Step 1: Write failing tests for request shape and unsupported behavior**

Add to new file `crates/wc-app/src/apply_execution.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use wc_core::config::ConfigDir;

    fn temp_service() -> (tempfile::TempDir, crate::AppService) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("config"),
        };
        cd.init().unwrap();
        wc_core::config::write_config_value(&cd.path, "storage_backend", "sqlite").unwrap();
        (tmp, crate::AppService::from_config_dir(cd))
    }

    fn web_project(root: &Path) -> std::path::PathBuf {
        let project = root.join("steamapps/workshop/content/431960/3650880224");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), "<html></html>").unwrap();
        std::fs::write(project.join("project.json"), r#"{"type":"web","file":"index.html"}"#).unwrap();
        project
    }

    #[test]
    fn execute_apply_request_rejects_we_web_without_stopping() {
        let (tmp, service) = temp_service();
        let project = web_project(tmp.path());
        let request = ApplyRequest {
            kind: ApplyRequestKind::Apply,
            path: project.to_string_lossy().to_string(),
            request_id: Some("test-1".into()),
        };

        let err = service.execute_apply_request(request).unwrap_err();
        assert_eq!(err.code, "we_web_unsupported");
        assert!(err.recoverable);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p wc-app execute_apply_request_rejects_we_web_without_stopping
```

Expected: fail because `apply_execution` module and `execute_apply_request` do not exist.

- [ ] **Step 3: Implement execution types**

Add to `crates/wc-app/src/apply_execution.rs` above the test module:

```rust
use serde::{Deserialize, Serialize};
use wc_core::types::{Backend, FileType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyRequestKind {
    Apply,
    RetryBackendApply,
    ApplyPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    pub kind: ApplyRequestKind,
    pub path: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyExecutionResult {
    pub request_id: Option<String>,
    pub applied_path: String,
    pub state_path: String,
    pub backend: Backend,
    pub file_type: FileType,
    pub preview: bool,
}
```

Modify `crates/wc-app/src/lib.rs`:

```rust
pub mod apply_execution;
pub mod apply_plan;

pub use apply_execution::{ApplyExecutionResult, ApplyRequest, ApplyRequestKind};
```

- [ ] **Step 4: Add temporary stub method**

In `impl AppService` in `crates/wc-app/src/lib.rs`, add:

```rust
pub fn execute_apply_request(
    &self,
    request: ApplyRequest,
) -> Result<ApplyExecutionResult, AppError> {
    let target = self.resolve_apply_target(&request.path)?;
    wc_backend::apply_wallpaper(&self.storage, &target.resolved_path, target.backend)
        .map_err(AppError::from_wc_error)?;
    Ok(ApplyExecutionResult {
        request_id: request.request_id,
        applied_path: target.resolved_path.clone(),
        state_path: target.resolved_path,
        backend: target.backend,
        file_type: target.file_type,
        preview: false,
    })
}
```

This compiles but does not yet handle preview; later tasks replace it.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p wc-app execute_apply_request_rejects_we_web_without_stopping
```

Expected: pass.

---

## Task 2: Preview Apply Resolves From Project Metadata

**Files:**
- Modify: `crates/wc-app/src/apply_execution.rs`
- Modify: `crates/wc-app/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Append to `crates/wc-app/src/apply_execution.rs` test module:

```rust
fn scene_project_with_preview(root: &Path) -> std::path::PathBuf {
    let project = root.join("steamapps/workshop/content/431960/3558034522");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("preview.gif"), b"gif").unwrap();
    std::fs::write(project.join("scene.json"), "{}").unwrap();
    std::fs::write(
        project.join("project.json"),
        r#"{"type":"scene","file":"scene.json","preview":"preview.gif","title":"Scene"}"#,
    )
    .unwrap();
    project
}

#[test]
fn apply_preview_uses_preview_file_not_project_dir() {
    let (tmp, service) = temp_service();
    let project = scene_project_with_preview(tmp.path());
    let request = ApplyRequest {
        kind: ApplyRequestKind::ApplyPreview,
        path: project.to_string_lossy().to_string(),
        request_id: Some("preview-1".into()),
    };

    let target = service.resolve_apply_request_target(&request).unwrap();
    assert!(target.resolved_path.ends_with("preview.gif"));
    assert_eq!(target.file_type, wc_core::types::FileType::Gif);
    assert_eq!(target.backend, wc_core::types::Backend::Awww);
    assert!(target.preview);
}

#[test]
fn apply_preview_without_preview_is_structured_error() {
    let (tmp, service) = temp_service();
    let project = tmp.path().join("steamapps/workshop/content/431960/1");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("scene.json"), "{}").unwrap();
    std::fs::write(project.join("project.json"), r#"{"type":"scene","file":"scene.json"}"#).unwrap();

    let request = ApplyRequest {
        kind: ApplyRequestKind::ApplyPreview,
        path: project.to_string_lossy().to_string(),
        request_id: None,
    };

    let err = service.resolve_apply_request_target(&request).unwrap_err();
    assert_eq!(err.code, "preview_missing");
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p wc-app apply_preview_
```

Expected: fail because `resolve_apply_request_target` does not exist.

- [ ] **Step 3: Add execution target type**

In `crates/wc-app/src/apply_execution.rs`, add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyExecutionTarget {
    pub input_path: String,
    pub resolved_path: String,
    pub state_path: String,
    pub file_type: FileType,
    pub backend: Backend,
    pub preview: bool,
}
```

- [ ] **Step 4: Implement request target resolution**

In `crates/wc-app/src/lib.rs`, add to `impl AppService`:

```rust
pub fn resolve_apply_request_target(
    &self,
    request: &ApplyRequest,
) -> Result<apply_execution::ApplyExecutionTarget, AppError> {
    match request.kind {
        ApplyRequestKind::Apply | ApplyRequestKind::RetryBackendApply => {
            let target = self.resolve_apply_target(&request.path)?;
            Ok(apply_execution::ApplyExecutionTarget {
                input_path: request.path.clone(),
                resolved_path: target.resolved_path.clone(),
                state_path: target.resolved_path,
                file_type: target.file_type,
                backend: target.backend,
                preview: false,
            })
        }
        ApplyRequestKind::ApplyPreview => {
            let project_path = resolve_wallpaper_path(&request.path).map_err(AppError::from_wc_error)?;
            let project = std::path::Path::new(&project_path);
            let info = wc_scan::read_we_project_info(project)
                .ok_or_else(|| AppError::preview_missing(&request.path))?;
            let preview = info
                .preview_path
                .ok_or_else(|| AppError::preview_missing(&request.path))?;
            let entry = wc_scan::make_entry(&preview)
                .ok_or_else(|| AppError::unsupported_path(preview.as_str()))?;
            let backend = self.backend_for_entry(&entry)?;
            if backend == Backend::Unsupported {
                return Err(AppError::unsupported_backend(entry.file_type, preview.as_str()));
            }
            Ok(apply_execution::ApplyExecutionTarget {
                input_path: request.path.clone(),
                resolved_path: preview.to_string(),
                state_path: preview.to_string(),
                file_type: entry.file_type,
                backend,
                preview: true,
            })
        }
    }
}
```

Add to `impl AppError`:

```rust
fn preview_missing(path: &str) -> Self {
    AppError {
        code: "preview_missing".into(),
        message: "This wallpaper has no preview file to apply.".into(),
        detail: Some(format!("project={}", path)),
        recoverable: true,
        suggestion: Some("Open the project folder or choose another wallpaper.".into()),
    }
}
```

- [ ] **Step 5: Update execute method to use target resolver**

Replace the body of `execute_apply_request`:

```rust
let target = self.resolve_apply_request_target(&request)?;
wc_backend::apply_wallpaper(&self.storage, &target.resolved_path, target.backend)
    .map_err(AppError::from_wc_error)?;
Ok(ApplyExecutionResult {
    request_id: request.request_id,
    applied_path: target.resolved_path,
    state_path: target.state_path,
    backend: target.backend,
    file_type: target.file_type,
    preview: target.preview,
})
```

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p wc-app apply_preview_
cargo test -p wc-app we_web_apply_returns_unsupported
```

Expected: pass.

---

## Task 3: Tauri `apply_action` Command

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/common.rs`
- Modify: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`
- Modify: `apps/tauri-gui/src-tauri/src/lib.rs`

- [ ] **Step 1: Add DTO types**

In `apps/tauri-gui/src-tauri/src/commands/common.rs`, add near command DTOs:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequestDto {
    pub kind: String,
    pub path: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResultDto {
    pub request_id: Option<String>,
    pub applied_path: String,
    pub state_path: String,
    pub backend: String,
    pub file_type: String,
    pub preview: bool,
}
```

- [ ] **Step 2: Add conversion helper**

In `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`, add:

```rust
fn apply_request_from_dto(dto: super::common::ApplyRequestDto) -> Result<wc_app::ApplyRequest, wc_app::AppError> {
    let kind = match dto.kind.as_str() {
        "apply" => wc_app::ApplyRequestKind::Apply,
        "retry_backend_apply" => wc_app::ApplyRequestKind::RetryBackendApply,
        "apply_preview" => wc_app::ApplyRequestKind::ApplyPreview,
        other => {
            return Err(wc_app::AppError {
                code: "invalid_apply_action".into(),
                message: format!("Unsupported apply action: {}", other),
                detail: None,
                recoverable: true,
                suggestion: None,
            });
        }
    };
    Ok(wc_app::ApplyRequest {
        kind,
        path: dto.path,
        request_id: dto.request_id,
    })
}
```

- [ ] **Step 3: Add command**

In `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`, add:

```rust
#[tauri::command]
pub async fn apply_action(request: super::common::ApplyRequestDto) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => {
            let service = wc_app::AppService::from_config_dir(wc_core::ConfigDir {
                path: s.cd.path.clone(),
            });
            let request = match apply_request_from_dto(request) {
                Ok(r) => r,
                Err(err) => return command_error_from_app_error(err),
            };
            match service.execute_apply_request(request) {
                Ok(result) => {
                    if result.file_type == FileType::WeScene {
                        wc_storage::we_compat::clear_failure(&result.state_path).ok();
                    }
                    let dto = super::common::ApplyResultDto {
                        request_id: result.request_id,
                        applied_path: result.applied_path.clone(),
                        state_path: result.state_path,
                        backend: result.backend.as_str().to_string(),
                        file_type: result.file_type.as_str().to_string(),
                        preview: result.preview,
                    };
                    match serde_json::to_string(&dto) {
                        Ok(json) => ok(json),
                        Err(e) => fail(e.to_string()),
                    }
                }
                Err(err) => command_error_from_app_error(err),
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

fn command_error_from_app_error(err: wc_app::AppError) -> CommandResult {
    CommandResult {
        success: false,
        stdout: String::new(),
        stderr: err.message.clone(),
        exit_code: 1,
        error: Some(super::common::CommandErrorDto {
            kind: err.code,
            message: err.message,
            detail: err.detail,
            recoverable: err.recoverable,
            suggestion: err.suggestion,
        }),
    }
}
```

If `command_error_from_app_error` duplicates existing code in `apply`, refactor `apply` to use it.

- [ ] **Step 4: Register command**

In `apps/tauri-gui/src-tauri/src/lib.rs`, add `commands::apply_action` to `generate_handler!`.

- [ ] **Step 5: Keep old command as wrapper**

Modify existing `apply(path)` to call `execute_apply_request` with `ApplyRequestKind::Apply`, not `service.apply(path)`.

- [ ] **Step 6: Verify**

Run:

```bash
cargo check -p wallpaper-console-tauri
cargo test -p wc-app
```

Expected: pass.

---

## Task 4: Frontend Request DTO and Explicit Action Dispatch

**Files:**
- Create: `apps/tauri-gui/frontend/src/domain/applyRequests.ts`
- Create: `apps/tauri-gui/frontend/src/domain/applyRequests.test.ts`
- Modify: `apps/tauri-gui/frontend/src/api/bridge.ts`
- Modify: `apps/tauri-gui/frontend/src/api/mockBridge.ts`
- Modify: `apps/tauri-gui/frontend/src/hooks/useLibraryEntryActions.ts`

- [ ] **Step 1: Add bridge types**

In `apps/tauri-gui/frontend/src/api/bridge.ts`, add:

```ts
export type ApplyRequestKind = 'apply' | 'retry_backend_apply' | 'apply_preview';

export interface ApplyRequestDTO {
  kind: ApplyRequestKind;
  path: string;
  requestId?: string;
}

export interface ApplyResultDTO {
  requestId?: string;
  appliedPath: string;
  statePath: string;
  backend: string;
  fileType: string;
  preview: boolean;
}
```

Add API method:

```ts
applyAction: (request: ApplyRequestDTO): Promise<CommandResult> =>
  invoke<CommandResult>('apply_action', { request }),
```

- [ ] **Step 2: Add request builder tests**

Create `apps/tauri-gui/frontend/src/domain/applyRequests.test.ts`:

```ts
import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildApplyRequest } from './applyRequests.ts';
import type { WallpaperDTO } from '../api/bridge.ts';

const scene: WallpaperDTO = {
  path: '/we/scene',
  type: 'we_scene',
  ext: 'scene',
  backend: 'linux-wallpaperengine',
  size: 1,
  mtime: 1,
  resolution: 'WE',
};

describe('buildApplyRequest', () => {
  it('builds normal apply request', () => {
    const r = buildApplyRequest(scene, 'apply');
    assert.equal(r.kind, 'apply');
    assert.equal(r.path, '/we/scene');
    assert.ok(r.requestId);
  });

  it('builds preview request using project path, not preview path', () => {
    const r = buildApplyRequest({ ...scene, previewPath: '/we/scene/preview.gif' }, 'apply_preview');
    assert.equal(r.kind, 'apply_preview');
    assert.equal(r.path, '/we/scene');
  });

  it('rejects non-execution actions', () => {
    assert.throws(() => buildApplyRequest(scene, 'open_folder' as any), /not executable/);
  });
});
```

- [ ] **Step 3: Implement request builder**

Create `apps/tauri-gui/frontend/src/domain/applyRequests.ts`:

```ts
import type { ApplyActionKind, ApplyRequestDTO, WallpaperDTO } from '../api/bridge';

const EXECUTABLE_ACTIONS = new Set<ApplyActionKind>([
  'apply',
  'retry_backend_apply',
  'apply_preview',
]);

export function buildApplyRequest(entry: WallpaperDTO, kind: ApplyActionKind): ApplyRequestDTO {
  if (!EXECUTABLE_ACTIONS.has(kind)) {
    throw new Error(`Action is not executable as apply: ${kind}`);
  }
  return {
    kind: kind as ApplyRequestDTO['kind'],
    path: entry.path,
    requestId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
  };
}
```

- [ ] **Step 4: Add mock method**

In `apps/tauri-gui/frontend/src/api/mockBridge.ts`, add:

```ts
applyAction: async (): Promise<CommandResult> => ok,
```

- [ ] **Step 5: Modify hook to dispatch explicit requests**

Change `UseLibraryEntryActionsCallbacks` in `useLibraryEntryActions.ts`:

```ts
onApplyAction: (request: ApplyRequestDTO) => void;
```

Keep `onApply?: (path: string) => void` only if needed temporarily. Prefer changing callers in Task 5.

In action cases:

```ts
const request = buildApplyRequest(entry, a.kind);
onApplyAction(request);
```

For `apply_preview`, do not pass `entry.previewPath` to the frontend apply handler. The backend should resolve preview from the project path.

- [ ] **Step 6: Verify**

Run:

```bash
cd apps/tauri-gui/frontend
npm run test:unit
npm run typecheck
```

Expected: pass.

---

## Task 5: Frontend Latest-Intent Queue Uses ApplyRequest

**Files:**
- Modify: `apps/tauri-gui/frontend/src/App.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/LibraryView.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/FavoritesView.tsx`
- Modify: `apps/tauri-gui/frontend/src/views/HistoryView.tsx`
- Modify: `apps/tauri-gui/frontend/src/components/WallpaperGrid.tsx`

- [ ] **Step 1: Update component props**

Add a new prop where needed:

```ts
onApplyAction: (request: ApplyRequestDTO) => void;
```

Keep `onApply(path)` only in `WallpaperGrid` for double-click compatibility until Step 4.

- [ ] **Step 2: Add App handler**

In `App.tsx`, add:

```ts
const pendingApplyRef = useRef<ApplyRequestDTO | null>(null);

const handleApplyAction = useCallback(async (request: ApplyRequestDTO) => {
  if (applyingRef.current) {
    pendingApplyRef.current = request;
    return;
  }
  applyingRef.current = true;
  setApplying(true);

  let currentRequest: ApplyRequestDTO | null = request;
  while (currentRequest !== null) {
    const req = currentRequest;
    currentRequest = null;
    setFeedbackWithAutoDismiss({ state: 'running', label: 'Applying wallpaper' });
    try {
      const r = await api.applyAction(req);
      if (r.success) {
        invalidateHistoryCache();
        const detail = r.stdout ? JSON.parse(r.stdout) : undefined;
        setFeedbackWithAutoDismiss({
          state: 'success',
          label: 'Applied',
          detail: detail?.preview ? 'Preview wallpaper applied.' : detail?.appliedPath?.split('/').pop(),
        });
      } else {
        setFeedbackWithAutoDismiss(commandErrorFeedback('Apply', r));
      }
      await refreshStatus();
    } catch (e) {
      setFeedbackWithAutoDismiss(commandErrorFeedback('Apply', e));
    }
    const next = pendingApplyRef.current;
    pendingApplyRef.current = null;
    if (next && next.requestId !== req.requestId) {
      currentRequest = next;
    }
  }

  setApplying(false);
  applyingRef.current = false;
}, [refreshStatus, setFeedbackWithAutoDismiss]);
```

- [ ] **Step 3: Keep compatibility wrapper**

In `App.tsx`:

```ts
const handleApply = useCallback((path: string) => {
  handleApplyAction({
    kind: 'apply',
    path,
    requestId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
  });
}, [handleApplyAction]);
```

- [ ] **Step 4: Use explicit request for double click**

In `WallpaperGrid.tsx`, when double-clicking:

```ts
if (onApplyAction) {
  onApplyAction(buildApplyRequest(entry, 'apply'));
} else {
  onApply(entry.path);
}
```

If adding `onApplyAction` to `WallpaperGrid` is too much for this step, leave double-click on compatibility wrapper and document it. Context menu must use explicit requests.

- [ ] **Step 5: Pass handlers to views**

In `App.tsx`:

```tsx
<LibraryView onApply={handleApply} onApplyAction={handleApplyAction} ... />
<FavoritesView onApply={handleApply} onApplyAction={handleApplyAction} ... />
<HistoryView onApply={handleApply} onApplyAction={handleApplyAction} ... />
```

In each view, pass `onApplyAction` into `useLibraryEntryActions`.

- [ ] **Step 6: Verify**

Run:

```bash
cd apps/tauri-gui/frontend
npm run typecheck
npm run test:unit
npm run smoke
```

Expected: pass.

---

## Task 6: Backend Stale Request Guard

**Files:**
- Modify: `apps/tauri-gui/src-tauri/src/commands/wallpaper.rs`

- [ ] **Step 1: Add global request generation**

At module level in `wallpaper.rs`:

```rust
static APPLY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
```

- [ ] **Step 2: Use sequence in apply_action**

At command start:

```rust
let seq = APPLY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
```

Before returning success:

```rust
let latest = APPLY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst);
if seq != latest {
    return CommandResult {
        success: false,
        stdout: String::new(),
        stderr: "Apply request was superseded by a newer request.".into(),
        exit_code: 1,
        error: Some(super::common::CommandErrorDto {
            kind: "stale_apply_request".into(),
            message: "This apply request was superseded by a newer request.".into(),
            detail: None,
            recoverable: true,
            suggestion: None,
        }),
    };
}
```

Important: this guard is not a cancellation system. It prevents stale success from being reported as current. Do not stop already-running backend work with this guard.

- [ ] **Step 3: Add tests if practical**

If Tauri command unit tests are hard, add a pure helper:

```rust
fn stale_apply_result(seq: u64, latest: u64) -> Option<CommandResult> { ... }
```

Test:

```rust
#[test]
fn stale_apply_result_returns_structured_error() {
    let r = stale_apply_result(1, 2).unwrap();
    assert!(!r.success);
    assert_eq!(r.error.unwrap().kind, "stale_apply_request");
}
```

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p wallpaper-console-tauri stale_apply_result
cargo check -p wallpaper-console-tauri
```

Expected: pass.

---

## Task 7: Backend State Write Boundary Tests

**Files:**
- Modify: `crates/wc-app/src/apply_execution.rs`
- Modify: `crates/wc-backend/src/lib.rs`
- Modify: `crates/wc-backend/src/linux_wallpaperengine.rs`

- [ ] **Step 1: Add tests for failed apply preserving state**

In `crates/wc-backend/src/lib.rs` tests, add:

```rust
#[test]
fn failed_regular_apply_preserves_current_state() {
    let (tmp, s) = temp_storage();
    let current = tmp.path().join("old.jpg");
    std::fs::write(&current, b"old").unwrap();
    s.current_write(&current.to_string_lossy()).unwrap();
    s.last_backend_write("awww").unwrap();

    let missing = tmp.path().join("missing.jpg");
    let err = apply_wallpaper(&s, &missing.to_string_lossy(), Backend::Awww).unwrap_err();
    assert!(err.to_string().contains("missing") || err.to_string().contains("not"));
    assert_eq!(s.current_read().unwrap().as_deref(), Some(current.to_string_lossy().as_ref()));
    assert_eq!(s.last_backend_read().unwrap().as_deref(), Some("awww"));
}
```

- [ ] **Step 2: Add tests for unsupported apply preserving history**

In `crates/wc-app/src/apply_execution.rs` tests:

```rust
#[test]
fn unsupported_request_does_not_add_history() {
    let (tmp, service) = temp_service();
    let project = web_project(tmp.path());
    let before = service.storage().history_list().unwrap().len();
    let request = ApplyRequest {
        kind: ApplyRequestKind::Apply,
        path: project.to_string_lossy().to_string(),
        request_id: None,
    };
    assert!(service.execute_apply_request(request).is_err());
    let after = service.storage().history_list().unwrap().len();
    assert_eq!(before, after);
}
```

If `storage()` accessor does not exist, add this test in `crates/wc-app/src/lib.rs` test module where private fields are accessible, or add a crate-private accessor:

```rust
#[cfg(test)]
pub(crate) fn storage_for_tests(&self) -> &StorageApi {
    &self.storage
}
```

- [ ] **Step 3: Verify**

Run:

```bash
cargo test -p wc-app unsupported_request_does_not_add_history
cargo test -p wc-backend failed_regular_apply_preserves_current_state
```

Expected: pass.

---

## Task 8: Smoke Tests for Explicit Execution Semantics

**Files:**
- Modify: `apps/tauri-gui/frontend/src/api/mockBridge.ts`
- Modify: `apps/tauri-gui/frontend/e2e/smoke.spec.ts`

- [ ] **Step 1: Make mock applyAction observable**

In `mockBridge.ts`, add module-level tracking:

```ts
let lastApplyActionRequest: ApplyRequestDTO | null = null;
```

Implement:

```ts
applyAction: async (request: ApplyRequestDTO): Promise<CommandResult> => {
  lastApplyActionRequest = request;
  return {
    ...ok,
    stdout: JSON.stringify({
      requestId: request.requestId,
      appliedPath: request.path,
      statePath: request.path,
      backend: request.kind === 'apply_preview' ? 'awww' : 'mock',
      fileType: request.kind === 'apply_preview' ? 'gif' : 'image',
      preview: request.kind === 'apply_preview',
    }),
  };
},
```

Expose for smoke only by adding a debug bridge method if the existing mock pattern allows it. If not, assert via UI feedback text.

- [ ] **Step 2: Add smoke for preview action**

In `smoke.spec.ts`:

```ts
test('Apply preview GIF sends preview action without applying project path directly', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_scene');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'Scene title' }).first();
  await card.click({ button: 'right' });
  await page.getByText('Apply preview GIF').click();
  await expect(page.locator('.toast')).toContainText(/Applied|Preview/);
});
```

- [ ] **Step 3: Add smoke for unsupported direct apply guard**

Existing WE Web context menu test already checks no Apply. Add double-click behavior:

```ts
test('WE Web double click shows cannot apply warning', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_web');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'Web title' }).first();
  await card.dblclick();
  await expect(page.locator('.toast')).toContainText('Cannot apply');
});
```

- [ ] **Step 4: Verify**

Run:

```bash
cd apps/tauri-gui/frontend
npm run smoke
```

Expected: all smoke tests pass.

---

## Task 9: Documentation and Final Verification

**Files:**
- Modify: `docs/CURRENT_STATUS.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`

- [ ] **Step 1: Document current architecture**

Add to `docs/DEVELOPMENT.md`:

```markdown
### Apply execution pipeline

The GUI receives `applyActions` from Rust DTOs and sends explicit `ApplyRequestDTO`
objects to Tauri through `apply_action`.

- `apply`: apply the real wallpaper path/project.
- `retry_backend_apply`: clear compatibility failure first, then apply the real project.
- `apply_preview`: apply the project's preview media as a normal image/GIF; the frontend sends
  the project path and the backend resolves the preview path.

The older `apply(path)` command remains for compatibility but should not be used by new GUI actions.
```

- [ ] **Step 2: Add manual checks**

Add to `docs/TAURI_MANUAL_SMOKE_CHECKLIST.md`:

```markdown
### Apply execution

- Right-click a WE Scene and Apply: current state records the project path.
- Right-click the same WE Scene and Apply preview GIF: current state records the preview file path.
- WE Web does not show Apply and double-click shows a warning.
- Failed WE Scene shows Retry backend apply; after retry, the card refreshes.
- Rapidly click two different wallpapers; final status should match the last clicked item.
```

- [ ] **Step 3: Full verification**

Run from repo root:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Run from frontend:

```bash
cd apps/tauri-gui/frontend
npm run typecheck
npm run test:unit
npm run build
npm run smoke
```

Expected:

- Rust tests pass.
- Clippy clean.
- Typecheck clean.
- Unit tests pass.
- Production build succeeds.
- Smoke tests pass.

---

## Review Checklist for Implementer

Run at least 3 review loops after implementation.

### Review 1: Correctness

- `apply_preview` sends project path from frontend.
- Backend resolves preview path.
- Unsupported WE Web still cannot apply.
- Failed WE Scene retry still clears compatibility error.
- Existing `api.apply(path)` remains compatible.

### Review 2: State and Race Safety

- Failed preflight does not stop current wallpaper.
- Failed backend apply does not write current/history.
- Stale request does not show false success.
- Rapid clicks keep only latest frontend intent.
- Scene-to-scene handoff behavior is not weakened.

### Review 3: UX and Tests

- Error toast uses `CommandResult.error.message/suggestion/detail`.
- Smoke covers Library/Favorites/History menus.
- Smoke covers WE Web no-apply behavior.
- Unit tests cover request builder.
- Rust tests cover preview resolution and state preservation.

## Self-Review

Spec coverage:

- Unified execution request: Tasks 1, 3, 4, 5.
- Preview apply boundary: Tasks 2, 4, 8.
- Unsupported behavior: Tasks 1, 8.
- Latest intent and stale guard: Tasks 5, 6.
- State/history safety: Tasks 2, 7.
- Docs and verification: Task 9.

Placeholder scan:

- No `TBD`, `TODO`, or unspecified tests remain.
- All changed files and commands are named explicitly.

Type consistency:

- Frontend request kind names match Rust `serde(rename_all = "snake_case")`.
- DTO field names use camelCase across Tauri and frontend.
- Existing `ApplyActionKind` remains the source for UI actions.
