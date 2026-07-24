# Matugen examples for Wallpaper Console

Minimal Material You templates used with the post-apply theme hook.

## Setup

```bash
mkdir -p ~/.config/matugen
cp -a examples/matugen/. ~/.config/matugen/
```

Include the kitty theme from your `kitty.conf` if you use it:

```conf
include themes/matugen.conf
```

Enable the hook in Wallpaper Console:

```bash
wallpaper-console-rust config-set post_apply_enabled on
# default: post_apply_command=matugen image "$still"
```

Optional helper with kitty/waybar reload:

```bash
chmod +x scripts/post-apply-theme.sh
wallpaper-console-rust config-set post_apply_command "$PWD/scripts/post-apply-theme.sh"
```

If you already have iNiR (or another) matugen tree, point `~/.config/matugen`
at that instead of copying these examples.

## Targets

| Template | Output |
|----------|--------|
| waybar   | `~/.config/waybar/colors.css` |
| kitty    | `~/.config/kitty/themes/matugen.conf` |
| fuzzel   | `~/.config/fuzzel/fuzzel_theme.ini` |
| GTK3/4   | `~/.config/gtk-{3,4}.0/gtk.css` |
