# Wallpaper Console 性能硬化实施计划

## Summary

目标是解决三个用户可感知问题：启动后 Library 先显示 empty 再出现内容、全屏 Library 滚动不丝滑、壁纸切换不丝滑。执行顺序固定为：先记录构建日志，再做首屏与诊断，再做滚动渲染，最后做壁纸切换 pipeline。每个阶段独立提交、独立验证，避免把前端滚动问题和后端切换问题混在一起。

执行前先创建 doc/，并维护只增不改日志 doc/construct.md。本次追加内容见 doc/construct.md。

同时创建本文件 doc/performance-hardening-plan.md，写入本计划全文；后续只能追加 doc/construct.md 的新时间块，不修改历史记录。

## Key Changes

- Library 首屏
    - 修改分页状态模型：区分 initialLoading、refreshing、emptyConfirmed。
    - 刷新或筛选请求进行中时保留旧 grid，不再直接卸载列表。
    - 只有在首个有效 page 返回且 total === 0、扫描不在运行时，才显示 empty。
    - 扫描运行中显示 indexing/loading 状态，禁止显示 "Library is empty"。

- Library 查询与诊断
    - 保留现有 library.page.ms，增加首屏指标：library.firstPage.ms、library.firstContent.ms、library.emptyFlash.count。
    - 后端 library_page_gui 增加内部阶段耗时记录：storage init、SQLite open/index check、count query、page query、DTO mapping。
    - 不改变用户可见 API；诊断只进入现有 PerformanceOverlay 或 debug metric buffer。

- 滚动与缩略图
    - WallpaperGrid 只在 filter/sort/search 改变时回到顶部；append/loadMore/thumbnail update 不触发 scrollToIndex(0)。
    - 缩略图队列改成帧级批量更新：同一 animation frame 内完成的多个 thumbnail 只触发一次 React state update。
    - 缓存 convertFileSrc(path) 结果，避免每次 render 重算。
    - 将单个 wallpaper card 拆成 memoized component，只有对应 entry、thumbnail、applying 状态变化时重渲染。
    - 图片保留固定尺寸，增加 decoding="async"，避免 decode 阻塞滚动。

- 壁纸切换
    - 保持正确性优先：跨类型切换仍允许短黑屏，不保留错误残留层。
    - 前端 apply queue 保留"只执行当前 + 最新 pending"的语义，但增加阶段反馈：queued、starting backend、settling、applied。
    - 后端在进入重型副作用前再次检查 stale request；过期请求不执行 stop/start。
    - 同 backend fast path：image->image 的 awww 切换不执行不必要 stop；video->video 和 scene->scene 不做额外跨 backend cleanup。
    - 固定 settle 延迟先保留默认值，但打点记录实际耗时；只有证据证明过长后再降低默认值。

## Implementation Tasks

1. Documentation bootstrap
    - 创建 doc/。
    - 创建或追加 doc/construct.md，写入上面的时间、目标、结果块。
    - 创建 doc/performance-hardening-plan.md，写入本计划。
    - 提交：docs: add performance hardening plan

2. Phase 1: Library 首屏与诊断
    - 修改 usePagedWallpapers：返回 initialLoading、refreshing、hasLoadedOnce、lastRequestKind。
    - 修改 LibraryView：loading 时保留已有 entries；empty 只在 hasLoadedOnce && total === 0 && !scanProgress?.running 时显示。
    - 给 mockBridge 增加 delayed empty->filled 场景，覆盖启动时不显示 empty 的测试。
    - 提交：fix: keep library grid stable during initial refresh

3. Phase 2: 滚动与缩略图
    - 修改 WallpaperGrid：增加 resetKey，只有筛选/排序/搜索变化时回顶；entries append 不回顶。
    - 拆出 memoized WallpaperCard，缓存 file src。
    - 修改 ThumbnailRequestQueue：批量 emit，避免每张缩略图完成就触发全局重渲染。
    - 增加单元测试：loadMore 不触发 scroll reset；thumbnail batch 只 emit 一次。
    - 提交：perf: reduce library grid rerenders

4. Phase 3: 壁纸切换
    - 修改前端 apply queue 测试，覆盖快速连续点击时只执行当前和最新 pending。
    - 后端 apply_action 在拿到锁后、执行 execute_apply_request 前继续 stale 检查；过期直接返回 stale，不执行 backend stop/start。
    - 增加 backend lifecycle 测试：same-backend image->image 不触发 stop；cross-backend 保留既有 cleanup。
    - 给 apply path 增加阶段耗时 metric/log，先不改默认 settle。
    - 提交：perf: skip stale apply side effects

## Test Plan

- Rust:
    - cargo test -p wc-backend lifecycle visual_handoff apply
    - cargo test -p wallpaper-console-tauri
    - cargo test --workspace

- Frontend:
    - npm run typecheck
    - npm run test:unit -- src/hooks/usePagedWallpapers.test.ts src/hooks/thumbnailQueueCore.test.ts src/hooks/useApplyQueue.test.ts src/api/mockBridge.test.ts
    - npm run smoke

- Full gate:
    - cargo run -p xtask -- verify all
    - cargo build --workspace
    - git diff --check

- Manual acceptance:
    - 启动 wallpaper-console-gui-rust，Library 不应先显示 empty 再出现内容。
    - 全屏后快速上下滚动，缩略图逐步出现但滚动不应明显卡顿或跳回顶部。
    - 快速连续双击多个壁纸，最终只应应用最后一个请求；过期请求不应打断最终结果。
    - image->image、video->video、scene->scene、image->video、video->image 分别测试一次。

## Assumptions

- 使用 doc/，不是旧的 docs/；此前 docs 已被清理，本次只恢复轻量项目施工记录。
- doc/construct.md 是 append-only：新阶段只能追加新时间块，不改旧内容。
- 先不改变公开命令名称、配置 key、安装脚本和 niri 绑定。
- 先用指标证明瓶颈，再调低 backend settle 时间；本轮不盲目删除等待逻辑。
