# Source once from ~/.zshrc. Refresh exported theme variables at each prompt.
# Existing terminal ANSI colors are reloaded independently by kitty.
autoload -Uz add-zsh-hook
zmodload zsh/stat
_wcr_theme_precmd() {
  local cache_home="${XDG_CACHE_HOME:-$HOME/.cache}"
  [[ "$cache_home" == /* ]] || cache_home="$HOME/.cache"
  local theme_file="${WCR_ZSH_THEME_FILE:-$cache_home/wallpaper-console-rust/colors.zsh}"
  # Preserve existing Clavis setups; new configurations use WC's own path.
  if [[ -z "${WCR_ZSH_THEME_FILE:-}" && ! -r "$theme_file" && "${WCR_THEME_COMPAT:-auto}" != none ]]; then
    theme_file="$cache_home/quickshell/colors.zsh"
  fi
  local -A theme_stat
  [[ -r "$theme_file" ]] || return 0
  zstat -H theme_stat -- "$theme_file" 2>/dev/null || return 0
  local signature="$theme_file:$theme_stat[inode]:$theme_stat[mtime]:$theme_stat[size]"
  [[ "${_WCR_THEME_SIGNATURE:-}" == "$signature" ]] && return 0
  source "$theme_file" || return
  typeset -g _WCR_THEME_SIGNATURE="$signature"
}
add-zsh-hook -d precmd _wcr_theme_precmd
add-zsh-hook precmd _wcr_theme_precmd
_wcr_theme_precmd
