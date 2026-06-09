# Command Parity Checklist

Reference implementation: `/home/chakew/Projects/wallpaper-console/wallpaper-console`
Run with `XDG_CONFIG_HOME=$(mktemp -d)` for clean testing.

## Wallpaper commands
- [ ] `browse` / `browse-all` — fzf picker with preview
- [ ] `browse-images` — image-only fzf
- [ ] `browse-gifs` — GIF-only fzf
- [ ] `browse-videos` — video-only fzf
- [ ] `random` / `random-all` — random from all types
- [ ] `random-image`
- [ ] `random-gif`
- [ ] `random-video`
- [ ] `apply FILE`

## Favorites
- [ ] `favorite-add FILE`
- [ ] `favorite-add-current`
- [ ] `favorites` — browse favorites
- [ ] `favorite-random`
- [ ] `favorite-remove [FILE]`

## History
- [ ] `history` — browse history
- [ ] `history-random`
- [ ] `history-clear`

## Sources
- [ ] `add DIR`
- [ ] `remove` — interactive fzf
- [ ] `remove-source DIR`
- [ ] `sources` — list
- [ ] `steam-workshop`
- [ ] `validate-sources`
- [ ] `remove-missing`
- [ ] `dedupe-sources`

## Search / Sort
- [ ] `search`
- [ ] `search-source`
- [ ] `search-type`
- [ ] `sort-mtime`
- [ ] `sort-size`
- [ ] `sort-name`

## Config
- [ ] `config-get KEY`
- [ ] `config-set KEY VALUE...`

## Cache / Library
- [ ] `rescan`
- [ ] `library`
- [ ] `library-count`
- [ ] `browse-library`
- [ ] `random-library`
- [ ] `library-json [--tsv|--sqlite]`
- [ ] `favorites-json`
- [ ] `history-json`

## SQLite
- [ ] `migrate-to-sqlite`
- [ ] `sqlite-verify`
- [ ] `sqlite-resync`
- [ ] `sqlite-export-flat`
- [ ] `sqlite-backup`
- [ ] `sqlite-restore BACKUP`

## System
- [ ] `restore`
- [ ] `stop`
- [ ] `status`
- [ ] `tui`
- [ ] `help`

## Safety invariants
- [ ] Stop-before-apply ordering
- [ ] Image→image keeps awww daemon alive
- [ ] Video→/→video kills both backends
- [ ] State updates only after successful apply
- [ ] mpvpaper: never kills other users' processes
- [ ] setsid -f for awww-daemon
- [ ] mpvpaper --fork for video detachment
