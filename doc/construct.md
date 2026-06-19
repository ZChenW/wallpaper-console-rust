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
