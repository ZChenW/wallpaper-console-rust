# Wallpaper Console v0.1.0-rc.2

This is the second Linux release candidate for Wallpaper Console. It fixes the
two release-blocking defects found during real-machine validation of rc.1. It is
intended for early adopters who can report desktop, renderer, and packaging
compatibility issues before v0.1.0.

## Changes since rc.1

- The AppImage now launches through a repository-owned host-first runtime. It uses
  the target system's coherent GTK/WebKit/EGL stack when available, retains the
  bundled Ubuntu libraries as a fallback, does not force X11, and maps the
  documented WebKit DMABUF fallback variable. This fixes the no-window
  `EGL_BAD_ALLOC` failure reproduced on Arch Linux, niri, and hybrid graphics.
- Databases upgraded from the original v1 schema now pass `sqlite-verify` and can
  be backed up. Validation accepts only the exact historical migration shape and
  continues to reject malformed current schemas.
- Linux packaging tools and the embedded AppImage runtime are checksum-verified
  before execution. AppDir timestamps and `SOURCE_DATE_EPOCH` are normalized for
  reproducible repacking.

The published rc.1 assets remain available as historical pre-release artifacts
and are not replaced by this release.

## Supported platform

- Linux x86_64
- Wayland sessions, including named-output support when the selected renderer supports it
- Xorg sessions through the X root wallpaper path

The AppImage is built on Ubuntu 22.04 to retain compatibility with older glibc
baselines. Windows and macOS are not part of this release candidate.

## Assets

- `wallpaper-console_0.1.0-rc.2_x86_64.AppImage` — GUI application
- `wallpaper-console-cli_0.1.0-rc.2_x86_64.tar.zst` — separate CLI bundle
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
chmod +x wallpaper-console_0.1.0-rc.2_x86_64.AppImage
./wallpaper-console_0.1.0-rc.2_x86_64.AppImage
```

The AppImage can be moved anywhere in your home directory. Delete it to remove
the GUI. User settings and the wallpaper library remain under the normal XDG
configuration directory.

## Install the CLI

```bash
tar --zstd -xf wallpaper-console-cli_0.1.0-rc.2_x86_64.tar.zst
install -Dm755 \
  wallpaper-console-cli_0.1.0-rc.2_x86_64/wallpaper-console-rust \
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
- The AppImage does not bundle the external wallpaper renderers listed above.
- There is no automatic updater in this release candidate.

## Updating

Download the assets for a newer release, verify its `SHA256SUMS`, and replace
the old AppImage and/or CLI binary manually. Do not overwrite an existing asset
until its checksum has been verified.

Please report reproducible packaging or renderer problems through GitHub Issues,
including the distribution, desktop session, compositor, renderer, and terminal
output when available.
