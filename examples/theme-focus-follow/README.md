# Theme focus-follow (fast path)

Wallpaper Console owns the **per-output theme manifest** (`theme-state.json`).
Your compositor / theme tools own **matugen generation** and **palette swap**.
Clavis / QuickShell are not required.

## Fast path

1. On wallpaper apply (or restore), WC writes `~/.config/wallpaper-console-rust/theme-state.json`
   with a still image path per output, then runs `post_apply_command`.
2. `pregenerate-from-manifest.sh` (as the post-apply command) runs **matugen once per
   output** that has a still, caches consumer theme files under
   `$XDG_CACHE_HOME/wallpaper-console-rust/theme-palettes/<output>/`, and activates the
   theme-source output.
3. `focus-follow.sh` polls the focused output (niri by default). On change it only
   calls `activate-palette.sh` — **no matugen** on the focus hot path.

## Enable

```bash
REPO=/path/to/wallpaper-console-rust
chmod +x "$REPO/examples/theme-focus-follow/"*.sh

wallpaper-console-rust config-set post_apply_enabled on
wallpaper-console-rust config-set post_apply_on_restore on
wallpaper-console-rust config-set post_apply_theme_source last_applied
# or: focused | output:DP-8

wallpaper-console-rust config-set post_apply_command \
  "\"$REPO/examples/theme-focus-follow/pregenerate-from-manifest.sh\""
```

Install matugen templates first (see `examples/matugen/README.md`).

Start the focus follower from niri (or your compositor) startup:

```kdl
spawn-at-startup "bash" "-lc" "/path/to/wallpaper-console-rust/examples/theme-focus-follow/focus-follow.sh"
```

## Files

| Script | Role |
|--------|------|
| `pregenerate-from-manifest.sh` | Read `WCR_THEME_MANIFEST`, matugen per still, activate theme-source |
| `activate-palette.sh` | Copy cached palette files to live config paths; reload kitty if available |
| `focus-follow.sh` | Poll focused output; activate cached palette only |

## Palette cache layout

```
${XDG_CACHE_HOME:-$HOME/.cache}/wallpaper-console-rust/theme-palettes/
  DP-8/
    still.jpg          # symlink/copy of manifest still
    waybar-colors.css
    kitty-matugen.conf
    gtk-3.0-gtk.css
    gtk-4.0-gtk.css
    fuzzel_theme.ini
  eDP-1/
    ...
  active -> DP-8       # symlink to last activated output dir
```

Live targets (adjust in `activate-palette.sh` if your paths differ):

- `~/.config/waybar/colors.css`
- `~/.config/kitty/themes/matugen.conf`
- `~/.config/gtk-3.0/gtk.css` / `gtk-4.0/gtk.css`
- `~/.config/fuzzel/fuzzel_theme.ini`

## Notes

- Outputs without a still (e.g. WE web/application) are skipped during pregenerate.
- If matugen is missing, pregenerate exits non-zero so WC logs the hook failure;
  wallpaper apply itself still succeeds.
- `post_apply_theme_source=focused` makes WC probe the focused output when publishing
  the manifest so the initial `active` palette matches focus when possible.
