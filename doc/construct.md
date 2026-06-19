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

