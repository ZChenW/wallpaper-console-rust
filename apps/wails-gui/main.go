package main

import (
	"embed"
	"net/http"
	"os"

	"github.com/wailsapp/wails/v3/pkg/application"
)

//go:embed frontend/dist
var assets embed.FS

func main() {
	bridge := NewBridge()

	app := application.New(application.Options{
		Name:        "wallpaper-console-gui",
		Description: "Terminal wallpaper manager GUI",
		Assets: application.AssetOptions{
			Handler: http.FileServer(http.FS(assets)),
		},
		Services: []application.Service{
			application.NewService(bridge),
		},
		Mac: application.MacOptions{
			ApplicationShouldTerminateAfterLastWindowClosed: true,
		},
		Linux: application.LinuxOptions{
			ProgramName: "wallpaper-console-gui",
		},
	})

	if bridge.Binary() == "" || bridge.Binary() == "wallpaper-console-rust" {
		app.Logger.Warn("Rust binary may not be on PATH — some features may be unavailable")
	}

	// Create main window
	window := app.Window.NewWithOptions(application.WebviewWindowOptions{
		Title:     "Wallpaper Console",
		Width:     1200,
		Height:    800,
		MinWidth:  800,
		MinHeight: 600,
	})

	window.Center()
	err := app.Run()
	if err != nil {
		println("Error:", err.Error())
		os.Exit(1)
	}
}
