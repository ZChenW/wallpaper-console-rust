# Wallpaper Console 施工日志

本文件为 append-only：新阶段只能追加新时间块，不改旧内容。

## 2026-06-19 11:52:04 CST

目标：
- 修复 Library 首屏空态闪烁和加载延迟感。
- 优化全屏 Library 滚动流畅度。
- 优化同类型与跨类型壁纸切换响应。

结果：
- 待 opencode 按性能硬化计划执行后追加阶段结果。

## 2026-06-19 Phase 1: Library 首屏与诊断

结果：
- usePagedWallpapers 状态模型改为 initialLoading/refreshing/hasLoadedOnce/lastRequestKind，保留 loading 派生字段以兼容 Favorites/History；刷新/筛选进行中保留旧 grid，不再卸载列表。
- 新增纯函数 resolveRequestKind、loadingStateForKind（hooks/usePagedWallpapers）与 resolveLibraryDisplay（views/libraryDisplay.ts），均有单元测试覆盖。
- LibraryView 仅在 hasLoadedOnce && total===0 && !scanProgress.running 时显示 empty；扫描运行中显示 indexing 状态；首屏指标 library.firstPage.ms / library.firstContent.ms / library.emptyFlash.count 记录到 metrics buffer。
- mockBridge 增加 setLibraryFirstPageEmpty 场景（首页空→后续填充）及测试，覆盖启动时不显示 empty。
- 后端 library_page_gui 增加阶段耗时 debug log（storage_init/query/dto_map），仅在 gui_debug_logs=on 时写入 library-page-last.log；SQLite 内部 open/index/count/page 细粒度拆分留待后续 wc-storage 插桩。
- 验证：npm run typecheck/test:unit（110 pass）/smoke（82 pass）；cargo test -p wallpaper-console-tauri library（5 pass）。

## 2026-06-19 Phase 2: 滚动与缩略图

结果：
- WallpaperGrid 改用 resetKey 触发回顶：仅 filter/sort/search 变化时 scrollToIndex(0)，loadMore append 与 thumbnail update 不再回顶；新增 shouldResetScroll 纯函数及测试。
- usePagedWallpapers 新增 replaceCount（非 append 加载自增），Favorites/History 用其作为 resetKey 保留刷新回顶、append 不回顶的行为。
- 拆出 memoized WallpaperCard（components/WallpaperCard.tsx）+ 纯渲染 helpers（wallpaperCardHelpers.ts，含 displayName/metaLine/formatSize/typeIcon/weBadge 等单元测试）；卡片只在 entry/thumbnail/applying 变化时重渲染。
- 缓存 convertFileSrc 结果（safeFileSrc 模块级 Map），img 增加 decoding="async" 避免 decode 阻塞滚动。
- ThumbnailRequestQueue 改为帧级批量 emit：同一 animation frame（node 测试回退 microtask）内完成的多个缩略图只触发一次 onUpdate；新增 batch emit 测试与跨帧 emit 测试。
- 验证：npm run typecheck/test:unit（123 pass）/build/smoke（82 pass）。

## 2026-06-19 Phase 3: 壁纸切换

结果：
- 前端 ApplyQueueController 抽出到独立可测文件 hooks/applyQueueController.ts（仅 type-only 导入），useApplyQueue 复用真实类；测试改为导入真实类，覆盖快速连续点击只执行当前 + 最新 pending（b 被丢弃），并新增 settling 阶段反馈与 apply.request.ms metric 测试。
- apply queue 增加阶段反馈：queued / starting backend / settling / applied；成功路径记录 apply.request.ms，失败不记录。
- 后端 execute_and_format_result 提取 apply_stale_guard(seq, current_seq)：拿到 APPLY_LOCK 后、执行 execute_apply_request 前用其判定过期，过期直接返回 stale_apply_result 不执行 stop/start；原 is_stale_apply 删除，测试改为 race-free 的 apply_stale_guard 单测。
- 新增 backend lifecycle 测试：same-backend awww->awww 不 stop awww daemon（stop_awww_count=0）、mpvpaper->mpvpaper 只 stop mpvpaper；cross-backend 既有 cleanup（mpvpaper->awww stop_mpvpaper=1）保留。
- apply path 增加阶段耗时 debug log（pre_stop/fallback/target/settle），仅在 gui_debug_logs=on 写入 backend-apply-timings-last.log；默认 settle 时长本轮未改。
- 验证：npm run typecheck/test:unit（126 pass）/build/smoke（82 pass）；cargo test -p wc-backend（101 pass）/wallpaper-console-tauri（43 pass）；cargo clippy -D warnings 干净。

## 2026-06-19 代码评审修复

结果：
- P2-1：usePagedWallpapers 增加 emptyConfirmed——首个零页不确认 empty，需连续两次零页或延迟 400ms 确认重载后才显示 empty；resolveLibraryDisplay 新增 emptyConfirmed 入口，未确认时显示 loading；新增 Playwright 渲染测试验证 mock 场景下不闪烁 empty。
- P2-2：usePagedWallpapers 增加 loadError 追踪——初始加载失败不再设 hasLoadedOnce=true，resolveLibraryDisplay 新增 error 状态，LibraryView 渲染错误提示 + Retry 按钮；有旧 entries 时刷新失败仍保留 grid。
- P3-3：applyQueueController 移除 enqueue 中的独立 queued feedback emit（不再覆盖当前 apply 阶段）；改为在 starting/settling 阶段 detail 末尾追加 " · Next wallpaper queued." 后缀。
- P3-4：fileSrcCache 抽为 BoundedFileSrcCache（LRU，默认上限 2000），超出时淘汰最旧条目；新增纯单元测试覆盖缓存/淘汰/LRU 提升/clear。
- P3-5：doc/construct.md 末尾多余空行清理。

## 2026-06-19 14:24:07 CST: 手工验收后视觉 handoff 修复

目标：
- 修复其他类型壁纸切换到 image 时出现 image -> black -> image 的二次闪烁。

结果：
- 定位原因：cross-backend -> Awww 时 TargetImageInstant 已先通过 awww 显示目标 image，随后普通 Awww target 路径又调用 `awww clear` 并执行第二次 `awww img`，导致刚显示的 image layer 被清掉后再出现。
- 修复：当 Awww 的 instant fallback 已成功显示目标 image 时，跳过后续普通 Awww target command 和 `clear_awww_state_hint`，仍保留后置 stop 旧 backend 与状态写入。
- 测试：将 cross-backend image 测试改为断言只执行一次 awww command，且不调用 clear_awww_state_hint。

## 2026-06-19 14:43:00 CST: Stop Backends 后 image 闪回修复

目标：
- 修复点击 Stop Backends 后，再点击新 image 仍短暂出现旧 image，然后黑屏，再切到新 image 的问题。

结果：
- 定位原因：Stop Backends 已清空应用 runtime state，下一次 apply 的 previous 为 None；但 Awww target 路径在启动 awww-daemon 后仍执行 `awww clear`。awww-daemon 可能先恢复自身旧 layer，随后 `awww clear` 把画面清黑，再执行新 `awww img`，形成旧 image -> black -> 新 image。
- 修复：Awww target 路径不再对 previous=None 执行 `clear_awww_state_hint`；只有明确从其他 backend 或 Unknown 状态进入 Awww 时才做 state hint 清理。
- 测试：新增/更新 None -> Awww 用例，断言 Stop 后的新 image apply 不调用 clear_awww_state_hint 且只执行一次 awww command。

## 2026-06-19 14:53:34 CST: Stop Backends 等待 daemon 退出

目标：
- 修复 Stop Backends 发出停止命令后立即返回，用户快速点击新 image 时可能复用旧 awww-daemon 的竞态。

结果：
- 运行环境确认：niri 中的 `spawn-at-startup "awww-daemon"` 会让 daemon 独立于应用存在，可能保留旧 layer；用户已删除该配置，但当前会话中已存在的 daemon 仍需 kill 或重启 session 才消失。
- 后端修复：Stop Awww 改为通过 process_control 对 `awww-daemon` 发 TERM 后轮询确认退出；若仍未退出则发 KILL 并再次确认。`stop_all_backends` 与 `stop_non_lwe_backends` 统一走该等待路径。
- 测试：新增 stop_awww_waits_until_daemon_is_gone_before_returning 与 stop_awww_kills_daemon_when_term_does_not_exit，覆盖等待退出和 TERM 失败升级 KILL。

## 2026-06-19 15:01:58 CST: Stop 后首次 image 使用 instant apply

目标：
- 修复 Stop -> image A -> Stop -> image B 时，B 之前仍短暂出现 A 的问题。

结果：
- 定位原因：Stop 后应用 runtime state 为 None，但 Awww target 路径仍使用用户配置的普通 transition（默认 fade/1s）。如果 awww-daemon 冷启动时恢复上一张 layer，普通 fade 会把旧 image 当作过渡起点，表现为先出现 A 再切到 B。
- 修复：previous=None -> Awww 的首次 apply 改用 instant command（transition-type simple、transition-duration 0）；已有 Awww->Awww 正常切换仍保留用户配置的 transition。
- 测试：扩展 None -> Awww 用例，断言不调用 clear_awww_state_hint、只执行一次 awww command，且命令参数为 simple/0。

## 2026-06-19 15:11:01 CST: 禁用 Awww daemon cache 恢复

目标：
- 继续修复 Stop -> image A -> Stop -> image B 时，B 前仍出现 A 的问题。

结果：
- 定位原因：`awww-daemon --help` 明确说明默认会从 cache 搜索每个输出的上一张壁纸；因此 A 不是 transition 生成，而是 daemon 冷启动时先从 Awww cache 恢复出来。
- 修复：应用启动 awww-daemon 时改为 `awww-daemon --no-cache`；Stop Awww 等待 daemon 退出后执行 `awww clear-cache`，清理历史版本或外部 daemon 写入的 Awww cache。
- 测试：新增 ensure_awww_daemon_starts_with_no_cache_to_avoid_restoring_old_wallpaper，断言 daemon 启动命令包含 `--no-cache`；后端全量测试 104 pass。

## 2026-06-19 readiness / compatibility / stages — Phase 4: structured apply stage events

结果：
- wc-backend 新增 `apply_stage.rs`：`ApplyStage` / `ApplyStageEvent` / `ApplyStageReporter`（无 Tauri 依赖）；`apply_wallpaper_with_reporter` 在 Awww / LWE 成功与失败路径按序 emit `EnsureAwwwDaemon` / `AwwwSocketReady` / `StartLwe` / `WaitRendererAlive` / `CleanupPrevious` / `RefreshStatus`。
- wc-app 新增 `ApplyExecutionOptions` + `execute_apply_request_with_options`：在 `resolve_apply_request_target` 前 emit `ResolveTarget`；`apply_stage_labels` 在 wc-app 层根据 preview/backend 生成 label/detail（不塞进 wc-backend）。
- Tauri `wallpaper.rs`：`TauriStageReporter` 将 stage event 转为 `wc-apply-stage`（`{ requestId, stage, label, detail }`）；移除 apply 命令里静态 `wc-feedback`「Starting backend」占位。
- 前端：`APP_EVENTS.applyStage`；`ApplyQueueController` 在 `await applyAction` 期间订阅 `wc-apply-stage`，按 `requestId` 过滤并更新 running feedback，成功/失败均在 `finally` unsubscribe；`useFeedbackBridge` 仍只监听 `wc-feedback`。
- 测试：wc-backend capturing reporter（Awww 成功、LWE 成功、socket timeout 停在 EnsureAwwwDaemon、LWE crash 到 WaitRendererAlive）；wc-app ResolveTarget + LWE crash 顺序；Tauri stage payload 单测；前端 ApplyQueueController stage/unsubscribe/preview-vs-scene detail 单测。

## 2026-06-19 readiness / compatibility / stages — Final verification

命令与结果：
- `cargo run -p xtask -- verify all`：Rust fmt/check/clippy/test（workspace 全绿）、frontend typecheck、frontend unit（146 pass）、frontend build 均通过；Playwright smoke 因沙箱缓存路径缺少 chromium 可执行文件失败（`browserType.launch: Executable doesn't exist`），属环境限制，非代码回归。
- `CARGO_TARGET_DIR=.../target cargo build --workspace`：通过。
- `git diff --check`：通过。

手工验收项（Phase 4，需在真实 Tauri 会话中确认）：
- 应用 WE Scene 时 UI 依次显示 ResolveTarget → StartLwe → WaitRendererAlive → CleanupPrevious → RefreshStatus 等阶段文案，而非静态「Starting renderer」。
- Awww socket 超时或 LWE 立即崩溃时，最后停留的阶段与失败 toast 衔接正常，不出现无限卡在某一阶段。
- 快速连续 apply 时，旧 requestId 的 stage event 不覆盖新请求的 feedback。
- Phase 1–3 手工项仍适用（awww readiness、renderer limitation 卡片、Library 滚动缩略图）；本轮未改其公共 command/config key。

## 2026-06-19 readiness / compatibility / stages — Phase 4 review fixes (db15c2d)

结果：
- P1：`subscribeApplyStage` 抽出为 `subscribeApplyStage.ts` + `subscribeApplyStageCore.ts`，`listen()` resolve 前 unsubscribe 时用 `disposed` 守卫立即调用 deferred unlisten，避免泄漏 listener 污染后续 apply feedback。
- P2：`ApplyQueueController` 收紧 `requestId` 过滤——当前请求有 ID 时拒绝 `null`/其他 ID 的 stage event；新增 null requestId 单测。
- P3：`apply_awww_instant_with_runtime` 在 cross-backend `TargetImageInstant` fallback 路径 emit `EnsureAwwwDaemon` / `AwwwSocketReady`（rollback 路径仍 silent）；新增 `cross_backend_image_fallback_emits_awww_readiness_stages` 测试。
- P4：`apply_stage_detail` 按 `ctx.backend` 生成 renderer 名称（`backend_renderer_name`），`StartLwe` / `WaitRendererAlive` 不再仅靠 preview 分支硬编码文案。

## 2026-06-19 readiness / compatibility / stages — Final verification (post db15c2d)

命令与结果：
- `CARGO_TARGET_DIR=.../target cargo run -p xtask -- verify all`：Rust fmt/check/clippy/test（workspace 全绿，wc-backend 113 pass、wc-app 25 pass、wallpaper-console-tauri 44 pass）、frontend typecheck、frontend unit（149 pass）、frontend build 均通过；Playwright smoke 因沙箱缓存路径缺少 chromium 可执行文件失败（`browserType.launch: Executable doesn't exist`），属环境限制，非代码回归。
- `git diff --check`：通过。

手工验收项（review fixes，需在真实 Tauri 会话中确认）：
- 快速 apply 后立即切换/完成时，旧 stage listener 不再更新后续请求的 running feedback。
- video/scene → image（Awww instant fallback）时，UI 在 fallback 期间显示 `EnsureAwwwDaemon` / `AwwwSocketReady`，而非长时间停在 `ResolveTarget`。
- 带 `requestId` 的 apply 忽略 `requestId: null` 的 stage event（防泄漏 listener 或 legacy 路径干扰）。

## 2026-06-20 Library 滚动 FPS 硬化

目标：
- 修复 Tauri GUI Library 大屏浏览时滚动 FPS 低、不跟手的问题。
- 减少滚动热路径上的缩略图调度与主线程分配。

根因（源码确认）：
- `WallpaperGrid` 虚拟范围变化时对可见 path 做 front-priority enqueue；宽屏 `calculateColumnCount(1920) === 8`，每次 range 变化覆盖更多卡片。
- `ThumbnailRequestQueue.enqueue` 原用 `queue.some()` 去重，backlog 大时为 O(incoming × pending)。
- 缩略图完成时 `ThumbnailStore` 逐 path 同步通知 listener，多 completion 可在一帧内触发多次卡片重渲染。
- fast scroll 时 `overscan` 随列数膨胀，虚拟化范围抖动加剧 enqueue。

结果：
- **队列 O(1) 去重**：`thumbnailQueueCore` 增加 `queuedPaths: Set<string>`；enqueue/forget/reset/dispose 与 shift 路径同步维护；新增大 backlog 去重回归测试。
- **滚动热区暂停 enqueue**：`WallpaperGrid` 用 `thumbnailPaused` + 140ms idle timer（速度阈值 1.5 px/ms）；暂停期间跳过 visible enqueue，停止滚动后再补队列。
- **稳定 overscan**：`layout.overscanRowsFor` 改为固定行数（慢 2 / 快 1），不再按列数放大卡片数。
- **通知批处理**：`ThumbnailStore.scheduleNotify` 合并到单 `requestAnimationFrame`；新增 `thumbnailStore.test.ts`。
- **绘制隔离**：`.wallpaper-card` 增加 `contain: layout paint style`、`content-visibility: auto`；`.wallpaper-thumb img` 增加 `display: block`。
- **指标**：`ThumbnailStoreContext` 暴露 `snapshot()`；`WallpaperGrid` 记录 `library.visibleThumbnail.paths`、`thumbnail.queue.pending`、`thumbnail.queue.inFlight`。

## 2026-06-20 Library 滚动 FPS — review 修复

结果：
- P1-1：新增 `stats(): { pending, active, cached }`（`queue.length` 等 O(1) 计数）；`WallpaperGrid` / `useThumbnailQueue` 指标改用 `stats()`，避免 `snapshot()` 在滚动路径上 `queue.map` 分配整段 pending 数组。
- P1-2：`pump()` 仅在确认发起 load 后 `queuedPaths.delete()`；`inFlight.has(path)` 时 `unshift` 回队列且保持 Set 成员；新增 `enqueue → forget → enqueue ×2`（concurrency 2）回归测试，断言 pending 为 1。
- P2：`contain-intrinsic-size` 从 `188px 220px` 改为 `auto 188px`，与 `CARD_HEIGHT = 188` 及虚拟行 estimate 对齐，避免 `content-visibility: auto` 跳过卡片预留 220px 块高导致行测量不一致。

验证：
- `npm run test:unit`（152 pass，含 thumbnailQueueCore / thumbnailStore / layout 测试）、`npm run typecheck`、`git diff --check`：通过。
- `cargo build --workspace`：通过。
- `cargo run -p xtask -- verify all` / `npm run smoke`：agent 环境 Playwright Chromium Headless Shell 下载写入失败（`errno -122`），Rust 与前端单元测试段均绿；需本机 `npx playwright install chromium && npm run smoke` 补验。

手工验收项（需在真实 Tauri 全屏 Library 中确认）：
- 冷/半冷缩略图缓存下快速 touchpad/滚轮滚动：位移跟手，缩略图在停止滚动约 140ms 后出现；占位符稳定、无整格塌陷。
- Performance Overlay：`thumbnail.queue.pending` 持续滚动时不应无限增长；同可见 path 无重复请求风暴。
- 缓存预热后再次快滚：FPS 应明显改善且保持稳定。

## 2026-06-20 Settings Status 不实时更新诊断与方案

现象：
- 打开 Settings -> General -> Status 后，Database / Wallpaper Engine Scene / Thumbnail Cache 有时仍显示初始占位或空状态。
- 等待数秒后没有自动更新；关闭/重开或触发某些操作后才可能变化。

源码定位：
- `apps/tauri-gui/frontend/src/views/SettingsView.tsx` 只在 mount 时执行一次 status 加载：
  - `loadThumbCache()` -> `api.thumbnailCacheStatus()`
  - `loadWeStatus()` -> `api.linuxWallpaperEngineStatus()`
  - `api.librarySourceStatus().then(setLibraryStatus)`
- `loadThumbCache` / `loadWeStatus` 的 `catch` 是空块；失败时状态保持 `null`，StatusCard 只能继续显示初始占位。
- `librarySourceStatus` 没有 `catch`；失败时 promise rejection 不会写入任何 UI 状态，也不会触发 retry。
- SettingsView 接收了 `onRefresh`，但当前参数名为 `_onRefresh` 且没有使用；打开 Settings 不会刷新全局 status，也不会驱动 Settings 内部 status 重新拉取。
- `LibraryPage` 清理/清空 thumbnail cache 后只刷新 `thumbCache`；Database rebuild / verify / restore 等动作没有统一刷新 `libraryStatus` / `thumbCache` / `weStatus`。

根因：
- Settings Status 目前是“一次性 best-effort fetch”，不是实时状态模型。
- 前端没有显式区分 `loading / ready / error / stale`，也没有重试/轮询；失败被吞掉后用户只能看到永久占位。
- 各维护动作各自局部刷新，缺少统一的 `refreshSettingsStatus()`，所以状态容易漂移。

实施方案（交给 Cursor）：
1. 在 `SettingsView.tsx` 增加统一状态刷新函数：
   - 新增 `settingsStatusLoading` / `settingsStatusError`，或更细粒度的 `libraryStatusError`、`weStatusError`、`thumbCacheError`。
   - 实现 `refreshSettingsStatus(reason?: string)`，内部用 `Promise.allSettled` 并行请求：
     - `api.librarySourceStatus()`
     - `api.linuxWallpaperEngineStatus()`
     - `api.thumbnailCacheStatus()`
     - 可选：`api.weDebugInfo()`
   - 每个 fulfilled 单独 set 对应状态；每个 rejected 写入对应 error，不要让一个失败阻断其他卡片更新。
   - 调用传入的 `onRefresh()`，让底部 StatusBar 与 Settings Status 同步刷新。

2. 打开 Settings 后立即刷新，并在 Settings 打开期间轻量轮询：
   - `useEffect(() => { void refreshSettingsStatus('open'); const id = window.setInterval(..., 3000); return clearInterval; }, [refreshSettingsStatus])`
   - 轮询只刷新 status 类数据，不重新拉全部 configs，避免覆盖用户正在编辑的设置。
   - 如果担心成本，可只在 `activeCategory === 'general' || activeCategory === 'database' || activeCategory === 'library' || activeCategory === 'we'` 时轮询。

3. 所有会改变状态的操作完成后统一刷新：
   - `handleCleanupThumbnails` 成功/失败后调用 `refreshSettingsStatus('thumbnail-cleanup')`，替代单独 `loadThumbCache()`。
   - Clear Thumbnail Cache 成功/失败后调用 `refreshSettingsStatus('thumbnail-clear')`。
   - `runDbAction` finally 中调用 `refreshSettingsStatus('db-action')`；rebuild / restore / export / verify 后都能更新 Database 卡片。
   - `handleSet` 保存 `linux_wallpaperengine_*` 后调用 `refreshSettingsStatus('we-config')`，替代只调用 `loadWeStatus()`。

4. UI 表达要从永久占位改成可诊断状态：
   - `StatusCard` 可增加 `loading?: boolean` / `error?: string`，或者在页面层把 value/detail/tone 显式传入。
   - Database：
     - loading: `Checking...`
     - ready: `${sqliteRows} wallpapers indexed`
     - error: `Unavailable` + detail 为错误文本，tone=`warning`
   - Wallpaper Engine Scene：
     - unknown/loading 时显示 `Checking...`，不要把 `null` 直接当作 `Missing`
     - ready/missing/error 区分；只有明确返回 unavailable 才显示 `Missing`
   - Thumbnail Cache：
     - loading: `Checking...`
     - ready: `${entries} thumbnails, ${size}`
     - error: `Unavailable`

5. 增加最小测试：
   - 优先抽一个纯函数，例如 `settings/statusCards.ts`：
     - `resolveDatabaseStatusCard(libraryStatus, error, loading)`
     - `resolveWeStatusCard(weStatus, error, loading)`
     - `resolveThumbnailStatusCard(thumbCache, error, loading)`
   - 单测覆盖：
     - null + loading -> `Checking...`
     - rejected/error -> `Unavailable` 且 tone warning
     - fulfilled -> 展示真实行数/路径/缓存数量
     - weStatus null 不应显示 `Missing`
   - 如果实现刷新 orchestration，可加 `refreshSettingsStatusCore(loaders)` 单测，验证一个 loader reject 时其他 fulfilled 仍写入。

验收标准：
- 打开 Settings 后 0-1 秒内 Status 卡片从 `Checking...` 更新到真实值或明确错误。
- 单个 status command 失败不会阻止其他卡片更新。
- 等待数秒会再次刷新 status；不会永久停在 `...` 或空状态。
- thumbnail clear / cleanup、database rebuild / verify / restore、WE 设置保存后，对应 Status 卡片自动更新。
- 运行：
  - `cd apps/tauri-gui/frontend && npm run test:unit`
  - `cd apps/tauri-gui/frontend && npm run typecheck`
  - `git diff --check`
  - 可选本机补验：打开 Tauri GUI，进入 Settings -> General，观察 Status 卡片自动更新与轮询。

## 2026-06-20 Settings Status 实时刷新实现

结果：
- 新增 `settings/statusCards.ts`（`resolveDatabaseStatusCard` / `resolveWeStatusCard` / `resolveThumbnailStatusCard`）及 `statusCards.test.ts`，三态 UI：loading=`Checking...`、error=`Unavailable`+warning、ready 展示真实数据；`weStatus null` 不显示 `Missing`。
- 新增 `settings/refreshSettingsStatusCore.ts`：`Promise.allSettled` 并行拉取 library / WE / thumbnail / weDebugInfo；单 loader reject 不阻断其他卡片；`runLoader()` 用 `Promise.resolve().then()` 包装，同步 throw 也记入 error。
- `SettingsView` 实现 `refreshSettingsStatus()`：打开即刷、3s 轮询（仅 status，不碰 configs）、`onRefresh()` 同步 StatusBar；thumbnail cleanup/clear、db verify/backup/rebuild/restore/export、WE 配置保存后统一刷新。
- `GeneralPage` / `DatabasePage` / `LibraryPage` / `WallpaperEnginePage` 改用 statusCards 解析结果渲染 StatusCard。

验证：
- `npm run test:unit`（166 pass）、`npm run typecheck`、`git diff --check`：通过。

## 2026-06-20 Settings Status — review 修复

结果：
- P1：新增 `createSettingsStatusRequestSeq()`；`refreshSettingsStatus` 每次 `begin()` 递增 request id，`await` 后仅 `isLatest(requestId)` 时 apply snapshot 并清 loading，避免慢 poll 覆盖新操作结果；单测覆盖 stale request 场景。
- P2：`onRefresh` 改为 `void Promise.resolve(onRefresh()).catch(() => {})`，全局 `api.status()` 失败不拖垮 Settings status 刷新链。
- P3：`refreshSettingsStatusCore` 全部 loader 经 `runLoader()` 调用；新增同步 throw 单测。

验证：
- `npm run test:unit`（169 pass）、`npm run typecheck`：通过。

## 2026-06-20 Library 窗口最大化/缩放卡顿修复

目标：
- 修复窗口最大化、全屏、拖拽缩放后 Library 滚动卡顿（“大屏”指窗口变宽，非 DPI）。

根因：
- `calculateColumnCount()` 仅按 `width / 220px` 增列，无上限；行虚拟化下宽窗口同 overscan 行数渲染更多卡片（如 16 列 × 10 行 ≈ 160 卡）。
- `ResizeObserver` 每个 event 直接 `setColCount`，触发 anchor scroll + `virtualizer.measure()`，resize 期间主线程抖动。
- 滚动虽暂停 enqueue，但已完成 thumbnail 仍批量换 `<img src>`，与 scroll/resize 抢帧。

结果：
- **列数上限**：`layout.ts` 增加 `MAX_GRID_COLUMNS = 8`；超宽窗口卡片变宽而非无限加列；补 cap 单测。
- **resize 节流**：`WallpaperGrid` ResizeObserver 经 `requestAnimationFrame` 合并 + 100ms debounce 最终校正；cleanup 时 `cancelAnimationFrame` / `clearTimeout`；记录 `library.grid.resize`。
- **interaction pause**：`pauseThumbnailInteraction()` 统一暂停 enqueue（200ms idle）与 `ThumbnailStore.setRevealPaused()`，scroll 与 resize 共用；unpause 后批量 flush 积压通知。
- **稳定 Context**：`ThumbnailStoreProvider` 用 `useMemo` 固定 context value，避免父级 render 牵连全部 `useThumbnailStore()` 消费者。
- **指标**：`library.grid.containerWidth`、`colCount`、`renderedRows`、`renderedCards` 写入 metrics buffer（PerformanceOverlay `Ctrl+Shift+P`）。

验证：
- `npm run test:unit`（169 pass）、`npm run typecheck`、`npm run build`、`git diff --check`：通过。

手工验收项：
- 最大化/全屏后 `library.grid.colCount ≤ 8`，`renderedCards` 不随窗口宽度无限增长。
- 拖拽缩放窗口时无明显连续抖动；快滚跟手，缩略图可延迟约 200ms 出现。

## 2026-06-20 Library reveal pause — rAF 已排队通知修复

现象：
- `setRevealPaused(true)` 只能拦住后续 `scheduleNotify()` 新入队路径；若 thumbnail 已完成并把通知排进 `requestAnimationFrame`，滚动/resize 开始后该 rAF 仍会 flush 并触发卡片 `<img src>` 更新，削弱 interaction pause 效果。

结果：
- `ThumbnailStore.scheduleNotifyFlush()` 的 rAF callback 开头重新检查 `revealPaused`；若已暂停，将 `pendingNotifyPaths` 转入 `pausedNotifyPaths` 后 return，不通知 listener。
- 新增回归测试：thumbnail 完成并排入 rAF → `setRevealPaused(true)` → 执行 rAF（listener 仍为 0）→ `setRevealPaused(false)` → 下一帧 listener 被调用。

验证：
- `npm run test:unit`（170 pass）、`npm run typecheck`：通过。

## 2026-06-20 Library 滚动修复 — review follow-up

结果：
- 保留自然列数（无 `MAX_GRID_COLUMNS`），`overscanRowsFor` 恢复按 `MAX_SLOW_OVERSCAN_CARDS` / `MAX_FAST_OVERSCAN_CARDS` 总卡片上限缩放，超宽屏不再固定 2 行 × N 列。
- `ThumbnailStore` 暂停期间 completion 不再逐条写 `thumbnail.reveal.pending`；rAF flush 在 batch 通知前二次检查 `revealPaused`。
- `WallpaperGrid` 程序化 `scrollToIndex` / 列变更 anchor scroll 用 `suppressScrollPauseRef` 跳过 interaction pause；grid 指标改为 500ms `setInterval` 采样。
- Settings status 轮询仅在 `general|database|library|we` tab 激活；`poll` 不触发 `onRefresh()`；`weDebugError` 接入 Advanced 页。

## 2026-06-20 Library 滚动修复 — review follow-up

结果：
- 保留自然列数（无 `MAX_GRID_COLUMNS`），`overscanRowsFor` 恢复按 `MAX_SLOW_OVERSCAN_CARDS` / `MAX_FAST_OVERSCAN_CARDS` 总卡片上限缩放，超宽屏不再固定 2 行 × N 列。
- `ThumbnailStore` 暂停期间 completion 不再逐条写 `thumbnail.reveal.pending`；rAF flush 在 batch 通知前二次检查 `revealPaused`。
- `WallpaperGrid` 程序化 `scrollToIndex` / 列变更 anchor scroll 用 `suppressScrollPauseRef` 跳过 interaction pause；grid 指标改为 500ms `setInterval` 采样。
- Settings status 轮询仅在 `general|database|library|we` tab 激活；`poll` 不触发 `onRefresh()`；`weDebugError` 接入 Advanced 页。
