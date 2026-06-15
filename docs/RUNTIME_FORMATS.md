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
| `library.tsv` | tab-separated | Legacy compatibility library export |
| `library.dirty` | empty file | Legacy cache staleness flag |
| `wallpapers.db` | SQLite | Primary GUI storage |
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
mpvpaper_options=--loop-file=inf --panscan=1.0
mpvpaper_output=*
awww_transition_type=fade
awww_transition_duration=1
awww_resize=crop
min_wallpaper_width=1280
min_wallpaper_height=720
preview_metadata=compact
gui_thumbnail_mode=cache
gui_thumbnail_cleanup_days=30
gui_thumbnail_failure_ttl_secs=900
gui_debug_logs=off
storage_backend=sqlite
```

## SQLite schema

See `create_schema()` in `crates/wc-storage/src/sqlite.rs` for the authoritative DDL.

Tables: `db_meta`, `config`, `sources`, `wallpapers`, `favorites`, `history`, `state`

The `wallpapers` table includes Wallpaper Engine metadata columns used by the GUI:
`project_type`, `preview_path`, `workshop_id`, `title`, `we_file`, and `unsupported_reason`.
