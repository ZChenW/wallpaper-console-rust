# Command Parity Checklist

Reference implementation: `/home/chakew/Projects/wallpaper-console/wallpaper-console`
Run with `XDG_CONFIG_HOME=$(mktemp -d)` for clean testing.

## Wallpaper commands
- [x] `browse` / `browse-all` — fzf picker, apply on selection (no preview yet)
- [x] `browse-images` — image-only fzf
- [x] `browse-gifs` — GIF-only fzf
- [x] `browse-videos` — video-only fzf
- [x] `random` / `random-all` — random from all types
- [x] `random-image`
- [x] `random-gif`
- [x] `random-video`
- [x] `apply FILE`

## Favorites
- [x] `favorite-add FILE`
- [x] `favorite-add-current`
- [x] `favorites` — fzf browse + apply
- [x] `favorite-random`
- [x] `favorite-remove [FILE]`

## History
- [x] `history` — fzf browse + apply
- [x] `history-random`
- [x] `history-clear`

## Sources
- [x] `add DIR`
- [x] `remove` — fzf interactive select
- [x] `remove-source DIR`
- [x] `sources` — list
- [x] `steam-workshop`
- [x] `validate-sources`
- [x] `remove-missing`
- [x] `dedupe-sources`

## Search / Sort
- [x] `search [QUERY]` — filename search, prompts if no query, fzf select
- [x] `search-source [QUERY]` — source path search, fzf select
- [x] `search-type [QUERY]` — type filter (image/gif/video), fzf select
- [x] `sort-mtime` — fzf select after sort
- [x] `sort-size` — fzf select after sort
- [x] `sort-name` — fzf select after sort

## Config
- [x] `config-get KEY`
- [x] `config-set KEY VALUE...`

## Cache / Library
- [x] `rescan`
- [x] `library`
- [x] `library-count`
- [x] `browse-library` — fzf from library.tsv + apply
- [x] `random-library`
- [x] `library-json [--tsv|--sqlite]`
- [x] `favorites-json`
- [x] `history-json`

## SQLite
- [x] `migrate-to-sqlite`
- [x] `sqlite-verify`
- [x] `sqlite-resync`
- [x] `sqlite-export-flat`
- [x] `sqlite-backup`
- [x] `sqlite-restore BACKUP`

## System
- [x] `restore`
- [x] `stop`
- [x] `status`
- [ ] `tui` — not yet implemented in Rust
- [x] `help`

## Safety invariants
- [x] Stop-before-apply ordering
- [x] Image→image keeps awww daemon alive
- [x] Video→/→video kills both backends
- [x] State updates only after successful apply
- [x] mpvpaper: never kills other users' processes
- [x] setsid -f for awww-daemon
- [x] mpvpaper --fork for video detachment

## Known gaps (non-blocking for parity)
- fzf preview pane (`__preview__`) not yet wired — Bash uses a subprocess for thumbnail previews
- `tui` subcommand is a stub — full TUI requires GTK or ratatui port
- `steam-workshop` does not scan Flatpak Steam paths (Bash does)
- sort commands sort by scanned metadata, not library.tsv metadata
