export function normalizeConfigValue(key: string, value: string): string {
  if (key === 'storage_backend') {
    return 'sqlite';
  }
  if (key === 'gui_theme') {
    return value === 'obsidian_warm' ? 'obsidian_warm' : 'light';
  }
  if (key === 'awww_transition_type') {
    if (value === 'slide') return 'left';
    const valid = ['simple', 'fade', 'left', 'right', 'top', 'bottom', 'wipe', 'grow', 'center', 'outer', 'random', 'wave'];
    return valid.includes(value) ? value : 'fade';
  }
  if (key === 'image_backend') {
    if (value === 'mpvpaper') return 'mpvpaper';
    return 'awww';
  }
  if (key === 'gui_thumbnail_cleanup_days') {
    return clampIntString(value, 1, 3650, 30);
  }
  if (key === 'gui_thumbnail_failure_ttl_secs') {
    return clampIntString(value, 60, 86_400, 900);
  }
  if (key === 'linux_wallpaperengine_fps') {
    return clampIntString(value, 1, 240, 60);
  }
  if (key === 'linux_wallpaperengine_volume') {
    return clampIntString(value, 0, 100, 100);
  }
  if (key === 'wallpaper_transition_fps') {
    return clampIntString(value, 1, 240, 60);
  }
  if (key === 'post_apply_enabled') {
    return value === 'on' ? 'on' : 'off';
  }
  if (key === 'post_apply_timeout_secs') {
    return clampIntString(value, 1, 600, 30);
  }
  if (key === 'awww_transition_duration') {
    const trimmed = value.trim();
    const parsed = Number.parseFloat(trimmed);
    if (Number.isFinite(parsed) && parsed >= 0 && parsed <= 60 && NUM_RE.test(trimmed)) return trimmed;
    return '1';
  }
  if (key === 'gui_thumbnail_mode') {
    return value === 'icon' ? 'icon' : 'cache';
  }
  if (key === 'linux_wallpaperengine_path') {
    return value.trim() || 'auto';
  }
  if (key === 'linux_wallpaperengine_target') {
    return value.trim();
  }
  if (key === 'open_project_location_mode') {
    if (value === 'terminal') return 'terminal';
    if (value === 'files') return 'file_manager';
    return 'file_manager';
  }
  if (key === 'gui_file_manager') {
    const valid = ['auto', 'nautilus', 'dolphin', 'thunar', 'nemo', 'pcmanfm', 'custom'];
    return valid.includes(value) ? value : 'auto';
  }
  if (key === 'gui_terminal_file_manager') {
    return value === 'custom' ? 'custom' : 'yazi';
  }
  if (key === 'linux_wallpaperengine_target_mode') {
    const valid = ['auto', 'screen-root', 'screen-span'];
    return valid.includes(value) ? value : 'auto';
  }
  if (key === 'linux_wallpaperengine_scaling') {
    const valid = ['default', 'fill', 'fit', 'stretch'];
    return valid.includes(value) ? value : 'default';
  }
  return value;
}

function clampIntString(value: string, min: number, max: number, fallback: number): string {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) return String(fallback);
  return String(Math.min(max, Math.max(min, parsed)));
}

const NUM_RE = /^[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?$/;
