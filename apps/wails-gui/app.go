package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// Bridge exposes Wails-bound methods to the React frontend.
type Bridge struct {
	runner *Runner
}

func NewBridge() *Bridge {
	binary := findRustBinary()
	return &Bridge{
		runner: NewRunner(binary),
	}
}

// Binary returns the path to the Rust CLI (for logging / diagnostics).
func (b *Bridge) Binary() string { return b.runner.Binary }

func findRustBinary() string {
	if env := os.Getenv("WALLPAPER_CONSOLE_RUST"); env != "" {
		if _, err := os.Stat(env); err == nil {
			return env
		}
	}
	exe, err := os.Executable()
	if err == nil {
		beside := filepath.Join(filepath.Dir(exe), "wallpaper-console-rust")
		if _, err := os.Stat(beside); err == nil {
			return beside
		}
	}
	return "wallpaper-console-rust"
}

// ── Status ──────────────────────────────────────────────────────────

func (b *Bridge) Status() (*StatusDTO, error)     { return b.runner.Status() }
func (b *Bridge) Apply(path string) CommandResult { return b.runner.Apply(path) }
func (b *Bridge) Stop() CommandResult             { return b.runner.Stop() }
func (b *Bridge) Restore() CommandResult          { return b.runner.Restore() }

// ── Library ─────────────────────────────────────────────────────────

func (b *Bridge) LibraryList(source string) ([]WallpaperDTO, error) {
	return b.runner.LibraryList(source)
}
func (b *Bridge) LibraryPage(source, filter, sort, search string, offset, limit int) (*LibraryPageDTO, error) {
	return b.runner.LibraryPage(source, filter, sort, search, offset, limit)
}
func (b *Bridge) LibraryCount() (*LibraryCountDTO, error) { return b.runner.LibraryCount() }
func (b *Bridge) Rescan() CommandResult                   { return b.runner.Rescan() }

// ── Favorites ───────────────────────────────────────────────────────

func (b *Bridge) FavoritesList() ([]string, error)         { return b.runner.FavoritesList() }
func (b *Bridge) FavoriteAdd(path string) CommandResult    { return b.runner.FavoriteAdd(path) }
func (b *Bridge) FavoriteRemove(path string) CommandResult { return b.runner.FavoriteRemove(path) }

// ── History ─────────────────────────────────────────────────────────

func (b *Bridge) HistoryList() ([]HistoryDTO, error) { return b.runner.HistoryList() }
func (b *Bridge) HistoryClear() CommandResult        { return b.runner.HistoryClear() }

// ── Sources ─────────────────────────────────────────────────────────

func (b *Bridge) SourcesList() ([]SourceDTO, error)      { return b.runner.SourcesList() }
func (b *Bridge) SourceAdd(path string) CommandResult    { return b.runner.SourceAdd(path) }
func (b *Bridge) SourceRemove(path string) CommandResult { return b.runner.SourceRemove(path) }
func (b *Bridge) ValidateSources() CommandResult         { return b.runner.ValidateSources() }
func (b *Bridge) RemoveMissingSources() CommandResult    { return b.runner.RemoveMissingSources() }
func (b *Bridge) ScanSteamWorkshop() CommandResult       { return b.runner.ScanSteamWorkshop() }

// ── Config ──────────────────────────────────────────────────────────

func (b *Bridge) ConfigGet(key string) (string, error)      { return b.runner.ConfigGet(key) }
func (b *Bridge) ConfigSet(key, value string) CommandResult { return b.runner.ConfigSet(key, value) }

// ── SQLite ──────────────────────────────────────────────────────────

func (b *Bridge) SqliteVerify() CommandResult             { return b.runner.SqliteVerify() }
func (b *Bridge) SqliteResync() CommandResult             { return b.runner.SqliteResync() }
func (b *Bridge) SqliteBackup() CommandResult             { return b.runner.SqliteBackup() }
func (b *Bridge) SqliteRestore(path string) CommandResult { return b.runner.SqliteRestore(path) }
func (b *Bridge) SqliteExportFlat() CommandResult         { return b.runner.SqliteExportFlat() }
func (b *Bridge) MigrateToSqlite() CommandResult          { return b.runner.MigrateToSqlite() }

// ── Thumbnails ──────────────────────────────────────────────────────

func (b *Bridge) ThumbnailFor(path string) (*ThumbnailDTO, error) {
	return b.runner.ThumbnailFor(path)
}
func (b *Bridge) ThumbnailCacheStatus() (*ThumbnailCacheDTO, error) {
	return b.runner.ThumbnailCacheStatus()
}
func (b *Bridge) ThumbnailCacheClear() CommandResult { return b.runner.ThumbnailCacheClear() }

// ── Shell ───────────────────────────────────────────────────────────

func (b *Bridge) OpenPath(path string) CommandResult { return b.runner.OpenPath(path) }
func (b *Bridge) RevealInFileManager(path string) CommandResult {
	return b.runner.RevealInFileManager(path)
}

// BrowseDirectory opens a native directory picker.
// Tries Wails v3 dialog API first, falls back to zenity/kdialog.
func (b *Bridge) BrowseDirectory() (string, error) {
	// Fallback 1: zenity (GTK, almost always available on Linux)
	if _, err := exec.LookPath("zenity"); err == nil {
		cmd := exec.Command("zenity", "--file-selection", "--directory",
			"--title=Select Wallpaper Directory")
		out, err := cmd.Output()
		if err == nil {
			return strings.TrimSpace(string(out)), nil
		}
	}
	// Fallback 2: kdialog (KDE)
	if _, err := exec.LookPath("kdialog"); err == nil {
		cmd := exec.Command("kdialog", "--getexistingdirectory",
			os.Getenv("HOME"))
		out, err := cmd.Output()
		if err == nil {
			return strings.TrimSpace(string(out)), nil
		}
	}
	// Fallback 3: yad (yet another dialog)
	if _, err := exec.LookPath("yad"); err == nil {
		cmd := exec.Command("yad", "--file-selection", "--directory",
			"--title=Select Wallpaper Directory")
		out, err := cmd.Output()
		if err == nil {
			return strings.TrimSpace(string(out)), nil
		}
	}
	return "", fmt.Errorf("no directory picker available (install zenity, kdialog, or yad)")
}
