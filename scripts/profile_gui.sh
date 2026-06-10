#!/usr/bin/env bash
set -euo pipefail

duration="${1:-30}"
bin="${WALLPAPER_CONSOLE_GUI_RUST:-$HOME/.local/bin/wallpaper-console-gui-rust}"

if [[ ! -x "$bin" ]]; then
  printf 'ERROR: GUI binary not executable: %s\n' "$bin" >&2
  exit 1
fi

printf 'Launching: %s\n' "$bin"
"$bin" >/tmp/wallpaper-console-gui-rust.log 2>&1 &
gui_pid="$!"

cleanup() {
  kill "$gui_pid" >/dev/null 2>&1 || true
}
trap cleanup EXIT

printf 'pid=%s\n' "$gui_pid"
printf 'timestamp,pid,ppid,pcpu,pmem,rss_kb,comm,args\n'

end=$((SECONDS + duration))
while (( SECONDS < end )); do
  ps -eo pid,ppid,pcpu,pmem,rss,comm,args |
    awk -v pid="$gui_pid" '
      NR == 1 { next }
      $1 == pid || $2 == pid || $7 ~ /wallpaper-console|ffmpeg|magick|convert|WebKit|webkit/ {
        printf "%s,%s,%s,%s,%s,%s,%s,", systime(), $1, $2, $3, $4, $5, $6
        for (i = 7; i <= NF; i++) printf "%s%s", $i, (i == NF ? ORS : " ")
      }
    '
  sleep 1
done

printf '\nGUI log:\n'
tail -n 80 /tmp/wallpaper-console-gui-rust.log || true
