# Wallpaper Console v0.1.0

This is the first stable Linux release of Wallpaper Console. It promotes the
validated rc.3 release line and includes the final reliability fixes found during
release-candidate testing.

## Changes since rc.3

- Long XDG configuration paths no longer prevent the GUI from starting. When the
  ConfigDir-derived Unix socket would exceed Linux's path limit, the single-instance
  coordinator uses an owner-private, length-bounded runtime socket keyed to that
  ConfigDir while retaining the existing lock and focus handshake.
- Image and GIF resolution probing now passes ImageMagick's first-frame selector as
  part of the input path. Valid files no longer produce a spurious decode warning or
  fall back to an unknown resolution because a separate `[0]` operand failed.
- The release workflow accepts exact stable and rc tags, publishes stable tags without
  the prerelease flag, and uses reviewed immutable revisions of Node 24-based GitHub
  Actions. Annotated-tag identity, deterministic AppImage repacking, checksum, draft,
  and remote provenance gates remain mandatory.

## Release highlights

- Grid and Flow browsing for local wallpapers and supported Wallpaper Engine projects
- Images, GIFs, videos, and compatible Wallpaper Engine scenes
- Multiple wallpaper folders, favorites, and paged Library browsing
- Per-display wallpaper selection where the chosen renderer supports named outputs
- Optional wallpaper restoration after login through the separate CLI
- Host-first AppImage WebKitGTK startup with a verified bundled fallback
- Strict database migration, verification, backup, and release provenance gates

The published rc.1, rc.2, and rc.3 tags and assets remain immutable historical
pre-release artifacts and are not replaced by this release.

## Supported platform

- Linux x86_64
- Wayland sessions, including named-output support when the selected renderer supports it
- Xorg sessions through the X root wallpaper path

The AppImage is built on Ubuntu 22.04 to retain compatibility with older glibc
baselines. Windows and macOS are not part of this release.

## Assets

- `wallpaper-console_0.1.0_x86_64.AppImage` — GUI application
- `wallpaper-console-cli_0.1.0_x86_64.tar.zst` — separate CLI bundle
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
chmod +x wallpaper-console_0.1.0_x86_64.AppImage
./wallpaper-console_0.1.0_x86_64.AppImage
```

The AppImage can be moved anywhere in your home directory. Delete it to remove
the GUI. User settings and the wallpaper library remain under the normal XDG
configuration directory.

## Install the CLI

```bash
tar --zstd -xf wallpaper-console-cli_0.1.0_x86_64.tar.zst
install -Dm755 \
  wallpaper-console-cli_0.1.0_x86_64/wallpaper-console-rust \
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
- There is no automatic updater.

## Updating

Download the assets for a newer release, verify its `SHA256SUMS`, and replace
the old AppImage and/or CLI binary manually. Do not overwrite an existing asset
until its checksum has been verified.

Please report reproducible packaging or renderer problems through GitHub Issues,
including the distribution, desktop session, compositor, renderer, and terminal
output when available.
