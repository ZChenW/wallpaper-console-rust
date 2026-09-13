# Wallpaper Console v0.1.2

Linux x86_64 follow-up to v0.1.0 focused on dual-display reliability and safer
cross-backend wallpaper apply.

The `v0.1.1` tag was pushed but never published (Cargo.lock sync); this release carries the same intended content.

## Changes since v0.1.0

- Dual-display apply planning now treats linux-wallpaperengine and mpvpaper as a
  verified cross-output pair, so applying a Wallpaper Engine scene on one output
  no longer fails with `ReliesOnUnknownCoexistence` while video keeps playing on
  another. LWE ↔ awww remains blocked until separately verified.
- Wallpaper Engine scenes use independent per-output processes. Changing one
  display no longer stops or restarts a healthy sibling LWE instance; shared or
  ambiguous multi-output LWE argv still refuses unsafe stop/apply.
- Runtime ownership, stop, restore, and observation paths better separate saved
  preferences from live renderer processes, including All Displays retirement and
  failed process inspection before mutation.
- mpvpaper Prepare checks bounded first-frame decode and IPC readiness so damaged
  media is rejected instead of reported as applied. Settings can reapply the
  current mpvpaper wallpapers after option changes without rerunning theme hooks.
- niri output recovery keeps previously playing assignments alive across software
  output re-enable, and theme-focus-follow helpers were refreshed for the
  per-output theme manifest flow.

## Supported platform

- Linux x86_64
- Wayland sessions, including named-output support when the selected renderer supports it
- Xorg sessions through the X root wallpaper path

The AppImage is built on Ubuntu 22.04 to retain compatibility with older glibc
baselines. Windows and macOS are not part of this release.

## Assets

- `wallpaper-console_0.1.2_x86_64.AppImage` — GUI application
- `wallpaper-console-cli_0.1.2_x86_64.tar.zst` — separate CLI bundle
- `SHA256SUMS` — SHA-256 checksums for both assets

The AppImage contains the GUI only. Install the separate CLI bundle when using
login restoration, command-line configuration, or terminal library commands.

## Verify downloads

Download all three assets into one directory, then run:

```bash
sha256sum -c SHA256SUMS
```

Both application assets must report `OK` before use.

## Install the AppImage

```bash
chmod +x wallpaper-console_0.1.2_x86_64.AppImage
./wallpaper-console_0.1.2_x86_64.AppImage
```

The AppImage can be moved anywhere in your home directory. Delete it to remove
the GUI. User settings and the wallpaper library remain under the normal XDG
configuration directory.

## Install the CLI

```bash
tar --zstd -xf wallpaper-console-cli_0.1.2_x86_64.tar.zst
install -Dm755 \
  wallpaper-console-cli_0.1.2_x86_64/wallpaper-console-rust \
  "$HOME/.local/bin/wallpaper-console-rust"
wallpaper-console-rust --help
```

## Wallpaper renderers

Wallpaper Console delegates wallpaper display to tools installed on the host.
Install only the renderers needed for your media and desktop:

- `awww` — Wayland images and GIFs
- `swaybg` — Wayland static images
- `feh` — Xorg static images; all displays only
- `mpvpaper` — Wayland images, GIFs, and videos
- `linux-wallpaperengine` — compatible Wallpaper Engine scenes

A directory picker such as `zenity`, `kdialog`, or `yad` is also recommended.

## Known limitations

- Wallpaper Engine Web projects can be browsed, but live Web wallpapers are not supported.
- Wallpaper Engine Scene rendering can differ from the original Wallpaper Engine output.
- Scene projects that render incorrectly can be moved to Unsupported from the Library.
- Named-display support depends on the selected renderer and compositor.
- LWE ↔ awww cross-output coexistence is still unverified and remains rejected.
- The AppImage does not bundle the external wallpaper renderers listed above.
- There is no automatic updater.

## Updating

Download the assets for a newer release, verify its `SHA256SUMS`, and replace
the old AppImage and/or CLI binary manually. Do not overwrite an existing asset
until its checksum has been verified.

Please report reproducible packaging or renderer problems through GitHub Issues,
including the distribution, desktop session, compositor, renderer, and terminal
output when available.
