# Wallpaper Console v0.1.6

Linux x86_64, Wayland. Faster switching, lower idle cost, and independent
per-display switching.

## Changes since v0.1.5

- Image switches return in about half the time; video and scene switches are
  also faster.
- The display watcher uses roughly a third of its previous idle CPU. A
  reconnected display may take up to about 2 seconds to be noticed.
- Each display switches independently. Login restore skips displays that
  already show the right wallpaper.
- Video and scene media is re-checked before the old wallpaper is stopped.
- Arrow keys return to the wallpapers after choosing a filter, sort, or display.
- New top bar button to scan Wallpaper Engine.
- `feh` and X11/Xorg are no longer supported. Saved `feh` settings are kept but
  cannot be applied; pick a Wayland image renderer in Settings.

## Assets

- `wallpaper-console_0.1.6_x86_64.AppImage` — GUI
- `wallpaper-console-cli_0.1.6_x86_64.tar.zst` — CLI, needed for login restore
- `SHA256SUMS`

Verify with `sha256sum -c SHA256SUMS`. Install steps are in the README.

## Renderers

Install the ones you need: `awww` (images, GIFs), `mpvpaper` (video),
`swaybg` (static images), `linux-wallpaperengine` (Wallpaper Engine scenes).

## Known limitations

- Wallpaper Engine Web wallpapers can be browsed but not applied.
- Scene rendering can differ from Wallpaper Engine.
- Mixed renderers across displays are verified on niri only.
- No automatic updater.
