package main

import (
	"context"
	"crypto/md5"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

const defaultTimeout = 30 * time.Second

// ── Data transfer objects ───────────────────────────────────────────────────

type CommandResult struct {
	Success  bool   `json:"success"`
	Stdout   string `json:"stdout"`
	Stderr   string `json:"stderr"`
	ExitCode int    `json:"exitCode"`
}

type StatusDTO struct {
	ConfigDir   string `json:"configDir"`
	Current     string `json:"current"`
	LastBackend string `json:"lastBackend"`
	SourceCount int    `json:"sourceCount"`
}

type WallpaperDTO struct {
	Path       string `json:"path"`
	Type       string `json:"type"`
	Ext        string `json:"ext"`
	Backend    string `json:"backend"`
	Size       int64  `json:"size"`
	Mtime      int64  `json:"mtime"`
	Resolution string `json:"resolution"`
}

type LibraryCountDTO struct {
	Total  int `json:"total"`
	Images int `json:"images"`
	Gifs   int `json:"gifs"`
	Videos int `json:"videos"`
}

type LibraryPageDTO struct {
	Total int            `json:"total"`
	Items []WallpaperDTO `json:"items"`
}

type HistoryDTO struct {
	Path string `json:"path"`
}

type SourceDTO struct {
	Path   string `json:"path"`
	Exists bool   `json:"exists"`
	IsWE   bool   `json:"isWE"`
	Label  string `json:"label"`
}

type ThumbnailDTO struct {
	Path      string `json:"path"`
	Thumbnail string `json:"thumbnail,omitempty"`
	CacheHit  bool   `json:"cacheHit"`
}

type ThumbnailCacheDTO struct {
	Dir     string `json:"dir"`
	Size    string `json:"size"`
	Entries int    `json:"entries"`
}

// ── Runner ───────────────────────────────────────────────────────────────────

type Runner struct {
	Binary string
}

func NewRunner(binary string) *Runner {
	return &Runner{Binary: binary}
}

func (r *Runner) run(args ...string) CommandResult {
	ctx, cancel := context.WithTimeout(context.Background(), defaultTimeout)
	defer cancel()

	cmd := exec.CommandContext(ctx, r.Binary, args...)
	var stdout, stderr strings.Builder
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err := cmd.Run()
	result := CommandResult{
		Stdout: stdout.String(),
		Stderr: stderr.String(),
	}
	if err != nil {
		if ctx.Err() == context.DeadlineExceeded {
			result.Stderr = "command timed out after " + defaultTimeout.String()
			result.ExitCode = -1
		} else if exitErr, ok := err.(*exec.ExitError); ok {
			result.ExitCode = exitErr.ExitCode()
		} else {
			result.Stderr = err.Error()
			result.ExitCode = -1
		}
	}
	result.Success = result.ExitCode == 0
	return result
}

func (r *Runner) runJSON(dest interface{}, args ...string) error {
	result := r.run(args...)
	if !result.Success {
		return fmt.Errorf("%s", result.Stderr)
	}
	return json.Unmarshal([]byte(result.Stdout), dest)
}

// ── Commands ─────────────────────────────────────────────────────────────────

func (r *Runner) Status() (*StatusDTO, error) {
	out := r.run("status")
	if !out.Success {
		return nil, fmt.Errorf("status failed: %s", out.Stderr)
	}
	dto := &StatusDTO{}
	for _, line := range strings.Split(out.Stdout, "\n") {
		line = strings.TrimSpace(line)
		switch {
		case strings.HasPrefix(line, "config directory:"):
			dto.ConfigDir = strings.TrimSpace(strings.TrimPrefix(line, "config directory:"))
		case strings.HasPrefix(line, "current wallpaper:"):
			dto.Current = strings.TrimSpace(strings.TrimPrefix(line, "current wallpaper:"))
		case strings.HasPrefix(line, "last backend:"):
			dto.LastBackend = strings.TrimSpace(strings.TrimPrefix(line, "last backend:"))
		case strings.HasPrefix(line, "configured sources:"):
			fmt.Sscanf(line, "configured sources: %d", &dto.SourceCount)
		}
	}
	return dto, nil
}

func (r *Runner) LibraryList(source string) ([]WallpaperDTO, error) {
	args := []string{"library-json"}
	switch source {
	case "sqlite":
		args = append(args, "--sqlite")
	case "tsv":
		args = append(args, "--tsv")
	}
	var entries []WallpaperDTO
	if err := r.runJSON(&entries, args...); err != nil {
		return nil, err
	}
	return entries, nil
}

func (r *Runner) LibraryPage(source, filter, sort, search string, offset, limit int) (*LibraryPageDTO, error) {
	args := []string{
		"library-page-json",
		"--source", source,
		"--filter", filter,
		"--sort", sort,
		"--search", search,
		"--offset", fmt.Sprintf("%d", offset),
		"--limit", fmt.Sprintf("%d", limit),
	}
	var page LibraryPageDTO
	if err := r.runJSON(&page, args...); err != nil {
		return nil, err
	}
	return &page, nil
}

func (r *Runner) LibraryCount() (*LibraryCountDTO, error) {
	out := r.run("library-count")
	if !out.Success {
		return nil, fmt.Errorf("library-count failed")
	}
	dto := &LibraryCountDTO{}
	for _, line := range strings.Split(out.Stdout, "\n") {
		line = strings.TrimSpace(line)
		switch {
		case strings.HasPrefix(line, "total="):
			fmt.Sscanf(line, "total=%d", &dto.Total)
		case strings.HasPrefix(line, "images="):
			fmt.Sscanf(line, "images=%d", &dto.Images)
		case strings.HasPrefix(line, "gifs="):
			fmt.Sscanf(line, "gifs=%d", &dto.Gifs)
		case strings.HasPrefix(line, "videos="):
			fmt.Sscanf(line, "videos=%d", &dto.Videos)
		}
	}
	return dto, nil
}

func (r *Runner) Rescan() CommandResult               { return r.run("rescan") }
func (r *Runner) Apply(path string) CommandResult     { return r.run("apply", path) }
func (r *Runner) Stop() CommandResult                 { return r.run("stop") }
func (r *Runner) Restore() CommandResult              { return r.run("restore") }
func (r *Runner) ValidateSources() CommandResult      { return r.run("validate-sources") }
func (r *Runner) RemoveMissingSources() CommandResult { return r.run("remove-missing") }
func (r *Runner) ScanSteamWorkshop() CommandResult    { return r.run("steam-workshop") }
func (r *Runner) SqliteVerify() CommandResult         { return r.run("sqlite-verify") }
func (r *Runner) SqliteResync() CommandResult         { return r.run("sqlite-resync") }
func (r *Runner) SqliteBackup() CommandResult         { return r.run("sqlite-backup") }
func (r *Runner) SqliteExportFlat() CommandResult     { return r.run("sqlite-export-flat") }
func (r *Runner) MigrateToSqlite() CommandResult      { return r.run("migrate-to-sqlite") }

func (r *Runner) SqliteRestore(path string) CommandResult {
	return r.run("sqlite-restore", path)
}

func (r *Runner) FavoritesList() ([]string, error) {
	var favs []string
	if err := r.runJSON(&favs, "favorites-json"); err != nil {
		return nil, err
	}
	return favs, nil
}

func (r *Runner) FavoriteAdd(path string) CommandResult {
	return r.run("favorite-add", path)
}

func (r *Runner) FavoriteRemove(path string) CommandResult {
	return r.run("favorite-remove", path)
}

func (r *Runner) HistoryList() ([]HistoryDTO, error) {
	var hist []HistoryDTO
	if err := r.runJSON(&hist, "history-json"); err != nil {
		return nil, err
	}
	return hist, nil
}

func (r *Runner) HistoryClear() CommandResult {
	return r.run("history-clear")
}

func (r *Runner) SourcesList() ([]SourceDTO, error) {
	// Use `sources` command for reliable path list, os.Stat for exists check.
	out := r.run("sources")
	if !out.Success {
		return nil, fmt.Errorf("sources failed: %s", out.Stderr)
	}
	var sources []SourceDTO
	for _, path := range strings.Split(strings.TrimSpace(out.Stdout), "\n") {
		path = strings.TrimSpace(path)
		if path == "" || path == "(no source directories configured)" {
			continue
		}
		fi, err := os.Stat(path)
		exists := err == nil && fi.IsDir()
		sources = append(sources, SourceDTO{
			Path:   path,
			Exists: exists,
			IsWE:   strings.Contains(path, "/steamapps/workshop/content/431960"),
			Label:  fileLabel(path),
		})
	}
	return sources, nil
}

func fileLabel(path string) string {
	if i := strings.Index(path, "/431960/"); i != -1 {
		rest := path[i+len("/431960/"):]
		if j := strings.Index(rest, "/"); j != -1 {
			return "Steam Workshop: " + rest[:j]
		}
		return "Steam Workshop: " + rest
	}
	return filepath.Base(path)
}

func (r *Runner) SourceAdd(path string) CommandResult {
	return r.run("add", path)
}

func (r *Runner) SourceRemove(path string) CommandResult {
	return r.run("remove-source", path)
}

func (r *Runner) ConfigGet(key string) (string, error) {
	out := r.run("config-get", key)
	if !out.Success {
		return "", fmt.Errorf("%s", out.Stderr)
	}
	return strings.TrimSpace(out.Stdout), nil
}

func (r *Runner) ConfigSet(key, value string) CommandResult {
	return r.run("config-set", key, value)
}

func (r *Runner) ThumbnailFor(path string) (*ThumbnailDTO, error) {
	dto := &ThumbnailDTO{Path: path}

	// Determine config dir for GUI thumbnail cache
	configDir := os.Getenv("XDG_CONFIG_HOME")
	if configDir == "" {
		home, _ := os.UserHomeDir()
		configDir = filepath.Join(home, ".config", "wallpaper-console")
	} else {
		configDir = filepath.Join(configDir, "wallpaper-console")
	}
	cacheDir := filepath.Join(configDir, "cache", "gui-thumbnails")

	// Build cache key from canonical path + mtime + size
	fi, err := os.Stat(path)
	if err != nil {
		return dto, nil
	}
	mtime := fi.ModTime().Unix()
	size := fi.Size()

	realPath := path
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		realPath = resolved
	}

	key := fmt.Sprintf("%s:%d:%d", realPath, mtime, size)
	hash := fmt.Sprintf("%x", md5Sum([]byte(key)))
	thumbPath := filepath.Join(cacheDir, hash+".webp")

	if _, err := os.Stat(thumbPath); err == nil {
		dto.Thumbnail = thumbPath
		dto.CacheHit = true
		return dto, nil
	}

	thumbnailMu.Lock()
	if thumbnailFailed[thumbPath] {
		thumbnailMu.Unlock()
		return dto, nil
	}
	if waiter, ok := thumbnailInFlight[thumbPath]; ok {
		thumbnailMu.Unlock()
		<-waiter.done
		if waiter.err == nil && waiter.path != "" {
			dto.Thumbnail = waiter.path
			dto.CacheHit = false
		}
		return dto, nil
	}
	waiter := &thumbnailWaiter{done: make(chan struct{})}
	thumbnailInFlight[thumbPath] = waiter
	thumbnailMu.Unlock()

	defer func() {
		thumbnailMu.Lock()
		if waiter.err != nil {
			thumbnailFailed[thumbPath] = true
		}
		delete(thumbnailInFlight, thumbPath)
		thumbnailMu.Unlock()
		close(waiter.done)
	}()

	// Generate thumbnail on the fly with global backpressure.
	os.MkdirAll(cacheDir, 0755)
	thumbnailSem <- struct{}{}
	err = generateThumbnail(path, thumbPath)
	<-thumbnailSem
	if err == nil {
		dto.Thumbnail = thumbPath
		dto.CacheHit = false
		waiter.path = thumbPath
	} else {
		waiter.err = err
	}
	return dto, nil
}

// generateThumbnail creates a 400px-wide webp thumbnail.
// Images/GIFs use ImageMagick; videos use ffmpeg directly.
func generateThumbnail(src, dst string) error {
	ext := strings.ToLower(strings.TrimPrefix(filepath.Ext(src), "."))
	if isVideoExt(ext) {
		if _, err := exec.LookPath("ffmpeg"); err == nil {
			cmd := exec.Command("ffmpeg", "-y", "-ss", "1", "-i", src,
				"-frames:v", "1", "-q:v", "3", dst)
			if err := cmd.Run(); err == nil {
				return nil
			}
		}
		return fmt.Errorf("no video thumbnail generator available")
	}

	// ImageMagick (works for images and GIFs)
	if _, err := exec.LookPath("magick"); err == nil {
		cmd := exec.Command("magick", src, "-resize", "400x", "-quality", "80",
			"-auto-orient", dst)
		if out, err := cmd.CombinedOutput(); err == nil {
			return nil
		} else if len(out) > 0 {
			// Try convert fallback
		}
	}
	if _, err := exec.LookPath("convert"); err == nil {
		cmd := exec.Command("convert", src, "-resize", "400x", "-quality", "80",
			"-auto-orient", dst)
		if err := cmd.Run(); err == nil {
			return nil
		}
	}
	return fmt.Errorf("no thumbnail generator available")
}

func isVideoExt(ext string) bool {
	switch ext {
	case "mp4", "webm", "mkv", "mov":
		return true
	default:
		return false
	}
}

func md5Sum(data []byte) [16]byte {
	return md5.Sum(data)
}

func (r *Runner) ThumbnailCacheStatus() (*ThumbnailCacheDTO, error) {
	dto := &ThumbnailCacheDTO{}
	configDir := os.Getenv("XDG_CONFIG_HOME")
	if configDir == "" {
		home, _ := os.UserHomeDir()
		configDir = filepath.Join(home, ".config", "wallpaper-console")
	} else {
		configDir = filepath.Join(configDir, "wallpaper-console")
	}
	cacheDir := filepath.Join(configDir, "cache", "gui-thumbnails")
	dto.Dir = cacheDir
	entries, err := os.ReadDir(cacheDir)
	if err != nil {
		return dto, nil
	}
	dto.Entries = len(entries)
	var totalSize int64
	for _, e := range entries {
		if fi, err := e.Info(); err == nil {
			totalSize += fi.Size()
		}
	}
	dto.Size = formatBytes(totalSize)
	return dto, nil
}

func formatBytes(b int64) string {
	switch {
	case b >= 1<<30:
		return fmt.Sprintf("%.1f GB", float64(b)/float64(1<<30))
	case b >= 1<<20:
		return fmt.Sprintf("%.1f MB", float64(b)/float64(1<<20))
	case b >= 1<<10:
		return fmt.Sprintf("%.1f KB", float64(b)/float64(1<<10))
	default:
		return fmt.Sprintf("%d B", b)
	}
}

func (r *Runner) ThumbnailCacheClear() CommandResult {
	configDir := os.Getenv("XDG_CONFIG_HOME")
	if configDir == "" {
		home, _ := os.UserHomeDir()
		configDir = filepath.Join(home, ".config", "wallpaper-console")
	} else {
		configDir = filepath.Join(configDir, "wallpaper-console")
	}
	cacheDir := filepath.Join(configDir, "cache", "gui-thumbnails")
	if err := os.RemoveAll(cacheDir); err != nil {
		return CommandResult{Success: false, Stderr: err.Error()}
	}
	os.MkdirAll(cacheDir, 0755)
	return CommandResult{Success: true}
}

func (r *Runner) OpenPath(path string) CommandResult {
	cmd := exec.Command("xdg-open", path)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return CommandResult{Success: false, Stderr: string(out) + ": " + err.Error()}
	}
	return CommandResult{Success: true, Stdout: string(out)}
}

func (r *Runner) RevealInFileManager(path string) CommandResult {
	dir := path
	if fi, err := os.Stat(path); err == nil && !fi.IsDir() {
		dir = filepath.Dir(path)
	}
	cmd := exec.Command("xdg-open", dir)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return CommandResult{Success: false, Stderr: string(out) + ": " + err.Error()}
	}
	return CommandResult{Success: true, Stdout: string(out)}
}
