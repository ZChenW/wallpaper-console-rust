# Experimental cached theme focus-follow

WC manages wallpaper assignments. These adapters generate a complete theme per
output when wallpaper changes, then apply cached files when **keyboard focus**
moves between outputs. There is one global live palette, shared by all
screens. This is not simultaneous per-screen application theming.

## Requirements and setup

Python 3.11+, matugen, and a working matugen template configuration are
required. Focus following supports niri, Hyprland, Sway, or a custom focus query.
Templates are read from `$XDG_CONFIG_HOME/matugen/config.toml` (default
`~/.config/matugen/config.toml`); their actual
`output_path` values are used. Generation uses an isolated configuration and never
executes template hooks or wallpaper commands. Clavis mode and scheme are read
from its personalization file when present.

```sh
# Run from your checkout, wherever it is installed.
REPO="$(pwd)"
HOOK="$(python3 -c 'import shlex,sys; print(shlex.join(sys.argv[1:]))' python3 "$REPO/examples/theme-focus-follow/theme-palette.py" post-apply)"
wallpaper-console-rust config-set post_apply_enabled on
wallpaper-console-rust config-set post_apply_on_restore on
wallpaper-console-rust config-set post_apply_theme_source focused
wallpaper-console-rust config-set post_apply_command "$HOOK"
```

`post-apply.sh` generates changed palettes, activates the focused output (falling
back to the manifest source), then calls
`~/.local/bin/wcr-post-apply-waybar.sh` if present. This preserves the original
QuickShell `wallpaper externalApplied` notification. Override its path with
`WCR_ORIGINAL_POST_APPLY`.

The original bridge may skip **only its generation step** when
`WCR_THEME_PREGENERATED=1`; it must still send its IPC notification. Without this
small bridge integration, the original generator still runs and the follower
subsequently restores the focused palette. Generation failure falls back to the
original bridge. Clavis source does not need modification.

Apply/restore once to create `~/.config/wallpaper-console/theme-state.json` and
populate all available outputs. `WCR_THEME_MANIFEST` overrides this path; WC sets
it automatically for hooks. XDG config/cache/state directories are respected.

Generate the service from the actual checkout and Python interpreter paths.
The checked-in `.service` file is a template; do not copy it directly:

```sh
python3 "$REPO/examples/theme-focus-follow/theme-palette.py" install-service
systemctl --user import-environment PATH NIRI_SOCKET HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_CONFIG_HOME XDG_CACHE_HOME XDG_STATE_HOME XDG_DATA_HOME
systemctl --user daemon-reload
systemctl --user enable --now wcr-theme-focus.service
```

Re-run `install-service` and restart the unit after moving the checkout. For
session managers that do not start `graphical-session.target`, run `focus-follow.sh`
from the compositor's startup configuration instead of enabling this unit.

The service selects the active compositor from session environment variables.
niri uses its event stream and a 40 ms debounce; Hyprland/Sway/custom providers
query focus every 150 ms, plus the query's execution time (bounded to one second).
These intervals are scheduling settings, not measured end-to-end latency promises.
It retries busy theme locks and pauses when no supported renderer
(awww, mpvpaper, swaybg, linux-wallpaperengine) is running, so
switching back to QuickShell's internal wallpaper mode leaves its themes alone.
Set `WCR_FOCUS_REQUIRE_RENDERER=0` only for standalone testing. Presence is a
heuristic: other tools starting these same renderers share this desktop scope.

Optional settings can be exported by a post-apply wrapper. For the follower
service, use `systemctl --user edit wcr-theme-focus.service` with a `[Service]`
section and `Environment=` settings, then restart the service. For example,
`Environment=WCR_FOCUS_PROVIDER=sway` selects Sway explicitly.
Service settings do not automatically change WC's separate post-apply environment;
use the same settings in both when changing theme paths or compatibility mode.

| Setting | Behavior |
| --- | --- |
| `WCR_FOCUS_PROVIDER` | `auto` (default), `niri`, `hyprland`, `sway`, or `custom` |
| `WCR_FOCUS_COMMAND` | JSON argument array, e.g. `["/path/to/focus-query", "--json"]`; no shell expansion; stdout must be `{"name":"OUTPUT"}` or `null` |
| `WCR_MATUGEN_CONFIG` | Use any matugen template configuration path |
| `WCR_THEME_MODE`, `WCR_THEME_SCHEME` | Override mode/scheme; defaults are `dark` / `scheme-tonal-spot` without Clavis |
| `WCR_THEME_COMPAT` | `auto` preserves detected Clavis installations; `clavis` forces compatibility; `none` disables automatic Clavis preferences, lock and bridge |
| `WCR_THEME_PERSONALIZATION` | Explicit optional preferences JSON path, with Clavis's `theme` schema |
| `WCR_THEME_LOCK` | Override WC's primary live-write lock; Clavis compatibility still acquires its shared lock |
| `WCR_ORIGINAL_POST_APPLY` | Explicit optional existing bridge executable; works with any compatibility mode |
| `WCR_ZSH_THEME_FILE` | Any generated zsh variables file; configure in `.zshrc` before sourcing the adapter |

An unsupported compositor leaves post-apply using the manifest source. The
follower reports the missing focus provider instead of guessing an output. A
custom provider allows other desktop environments to supply their own query.

## Cache and failure behavior

`~/.cache/wallpaper-console-rust/theme-palettes/` contains:

- `outputs.json`: output to immutable revision mapping, published after generation.
- `revisions/<hash>/`: rendered files and checksums. Hash includes still contents,
  matugen version, mode, scheme, templates and destinations.
- `active.json`: last activated output/revision; `pending-reloads.json`: retry work.
- Locks serialize generation and follower instances. Live writes use
  `$XDG_STATE_HOME/wallpaper-console-rust/theme.lock`. Detected/explicit Clavis
  compatibility additionally acquires `$XDG_STATE_HOME/quickshell/matugen.lock`.
  Clavis preferences use `$XDG_CACHE_HOME/quickshell/personalization.json`.
  Without Clavis, no QuickShell state directory or bridge is required.

Focus changes never run matugen. Identical generation inputs reuse cached files.
All cached files are checked before activation; destinations are staged before
replacement, with rollback if replacement fails. Consumers may briefly observe
individual file changes during a successful multi-file activation; files across
applications cannot be replaced as one filesystem transaction. The follower also
repairs external file changes and new revisions without requiring a focus change.

## Consumers

| Consumer | Update behavior |
| --- | --- |
| kitty | Reload configuration using caught SIGUSR1; actual included template path |
| niri | Included color config reload; compositor colors remain global |
| Waybar / QuickShell | Existing file watchers; Waybar root stylesheet touched |
| btop / cava | Caught SIGUSR2; cava reloads colors |
| fcitx5 | `fcitx5-remote --check -r` |
| Yazi | `ya emit-to 0 app:theme`; requires a version supporting this action |
| zsh | kitty ANSI colors; source `zsh-theme.zsh` from `.zshrc` for variables at prompts |
| GTK 3/4 | Optional generated CSS; existing applications are not universally hot-reloadable |

Set `WCR_THEME_GTK=1` in the post-apply wrapper to generate
`gtk-{3.0,4.0}/wcr-colors.css`, and import that file from each `gtk.css`. Remove
conflicting old color declarations while retaining application styling. GTK 4's
CSS variables require a sufficiently recent GTK/libadwaita. New applications read
the current CSS; WC does not restart running GTK applications or promise all of
them refresh on focus changes. This does not enable QuickShell per-screen colors.

The zsh adapter defaults to `$XDG_CACHE_HOME/wallpaper-console-rust/colors.zsh`;
set your template's output to that path, or specify `WCR_ZSH_THEME_FILE`. Existing
Clavis `quickshell/colors.zsh` is a fallback when compatibility is enabled.

## Mixed static image and video

The supported mixed pair is awww + mpvpaper on niri with awww 0.12 and an
alpha-capable default namespace daemon (argb/abgr). Other backend pairs remain
rejected. WC transparently releases an image output before video starts; clear
fills a surface and does not destroy it. New daemon startup also releases existing
video outputs. Only the target video process is replaced. Restore starts the
image daemon before the effective per-output videos and preserves named overrides.
Unsupported mixed environments are rejected before destructive stops. Ordinary
All Displays replacement does not require mixed-renderer compatibility.

## Disable

```sh
systemctl --user disable --now wcr-theme-focus.service
# Restore the hook value you saved before setup, or disable hooks:
wallpaper-console-rust config-set post_apply_enabled off
```

If restoring an earlier hook, apply once through that pipeline. Remove the `.zshrc` adapter
line and restore backed-up GTK CSS if reverting those optional integrations.
