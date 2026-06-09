# Runtime File Formats

All paths under `$XDG_CONFIG_HOME/wallpaper-console` (default `~/.config/wallpaper-console`).

## Files

| File | Format | Description |
|------|--------|-------------|
| `config` | `key=value` per line | Configuration settings |
| `sources` | path per line | Wallpaper source directories |
| `favorites` | path per line | Saved favorite paths |
| `history` | path per line | Last 100 applied paths (newest first) |
| `current` | single path | Last successfully applied wallpaper |
| `last_backend` | single name | Backend used by last apply |
| `library.tsv` | tab-separated | Cached library index |
| `library.dirty` | empty file | Cache staleness flag |
| `wallpapers.db` | SQLite | SQLite storage (opt-in) |
| `wallpapers.db.bak.*` | SQLite | Timestamped DB backups |
| `cache/previews/` | JPEG images | Video thumbnail cache |
| `cache/gui-thumbnails/` | WebP images | GUI thumbnail cache |

## library.tsv schema

```
TYPE\tEXT\tBACKEND\tSIZE\tMTIME\tRESOLUTION\tPATH\n
```

7 tab-separated fields. PATH is always the last field.

## Config defaults

```
gif_backend=awww
image_backend=awww
video_backend=mpvpaper
mpvpaper_options=no-audio --loop-file=inf
mpvpaper_output=*
awww_transition_type=fade
awww_transition_duration=1
awww_resize=crop
min_wallpaper_width=1280
min_wallpaper_height=720
preview_metadata=compact
gui_thumbnail_mode=cache
storage_backend=file
gui_library_source=tsv
```

## SQLite schema

See `sqlite_schema()` in `lib/wallpaper-console/sqlite.sh` for the authoritative DDL.

Tables: `db_meta`, `config`, `sources`, `wallpapers`, `favorites`, `history`, `state`
