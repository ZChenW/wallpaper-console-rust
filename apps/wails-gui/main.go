package main

import (
    "embed"
    "os"

    "github.com/wailsapp/wails/v3/pkg/application"
)

//go:embed frontend/dist
var assets embed.FS

func main() {
    app := application.New(application.Options{
        Name:        "wallpaper-console-gui",
        Title:       "Wallpaper Console",
        Description: "Terminal wallpaper manager GUI",
        Assets: application.AssetOptions{
            FS: assets,
        },
        Mac: application.MacOptions{
            ApplicationShouldTerminateAfterLastWindowClosed: true,
        },
        Linux: application.LinuxOptions{
            ProgramName: "wallpaper-console-gui",
        },
    })

    bridge := NewBridge()

    // ── Status ────────────────────────────────────────────────────
    app.Bind("Status", bridge.Status)
    app.Bind("Apply", bridge.Apply)
    app.Bind("Stop", bridge.Stop)
    app.Bind("Restore", bridge.Restore)

    // ── Library ───────────────────────────────────────────────────
    app.Bind("LibraryList", bridge.LibraryList)
    app.Bind("LibraryCount", bridge.LibraryCount)
    app.Bind("Rescan", bridge.Rescan)
    app.Bind("BrowseDirectory", bridge.BrowseDirectory)

    // ── Favorites ─────────────────────────────────────────────────
    app.Bind("FavoritesList", bridge.FavoritesList)
    app.Bind("FavoriteAdd", bridge.FavoriteAdd)
    app.Bind("FavoriteRemove", bridge.FavoriteRemove)

    // ── History ───────────────────────────────────────────────────
    app.Bind("HistoryList", bridge.HistoryList)
    app.Bind("HistoryClear", bridge.HistoryClear)

    // ── Sources ───────────────────────────────────────────────────
    app.Bind("SourcesList", bridge.SourcesList)
    app.Bind("SourceAdd", bridge.SourceAdd)
    app.Bind("SourceRemove", bridge.SourceRemove)
    app.Bind("ValidateSources", bridge.ValidateSources)
    app.Bind("RemoveMissingSources", bridge.RemoveMissingSources)
    app.Bind("ScanSteamWorkshop", bridge.ScanSteamWorkshop)

    // ── Config ────────────────────────────────────────────────────
    app.Bind("ConfigGet", bridge.ConfigGet)
    app.Bind("ConfigSet", bridge.ConfigSet)

    // ── SQLite ────────────────────────────────────────────────────
    app.Bind("SqliteVerify", bridge.SqliteVerify)
    app.Bind("SqliteResync", bridge.SqliteResync)
    app.Bind("SqliteBackup", bridge.SqliteBackup)
    app.Bind("SqliteRestore", bridge.SqliteRestore)
    app.Bind("SqliteExportFlat", bridge.SqliteExportFlat)
    app.Bind("MigrateToSqlite", bridge.MigrateToSqlite)

    // ── Thumbnails ────────────────────────────────────────────────
    app.Bind("ThumbnailFor", bridge.ThumbnailFor)
    app.Bind("ThumbnailCacheStatus", bridge.ThumbnailCacheStatus)
    app.Bind("ThumbnailCacheClear", bridge.ThumbnailCacheClear)

    // ── Shell ─────────────────────────────────────────────────────
    app.Bind("OpenPath", bridge.OpenPath)
    app.Bind("RevealInFileManager", bridge.RevealInFileManager)

    // Create main window
    window := app.NewWebviewWindowWithOptions(application.WebviewWindowOptions{
        Title:  "Wallpaper Console",
        Width:  1200,
        Height: 800,
        MinWidth:  800,
        MinHeight: 600,
        BackgroundColour: application.RGBA{R: 18, G: 18, B: 18, A: 255},
    })

    if bridge.binary == "" {
        app.Logger.Warn("Rust binary not found — some features will be unavailable")
    }

    window.Center()
    err := app.Run()
    if err != nil {
        println("Error:", err.Error())
        os.Exit(1)
    }
}
