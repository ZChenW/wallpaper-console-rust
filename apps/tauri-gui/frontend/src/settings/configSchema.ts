import { ThumbnailCacheDTO } from '../api/bridge';

export interface ConfigGroup {
  key: string;
  label: string;
  type: 'select' | 'text' | 'number';
  options?: string[];
  placeholder?: string;
}

export const BACKEND_CONFIGS: ConfigGroup[] = [
  { key: 'image_backend', label: 'Image backend', type: 'select', options: ['awww', 'mpvpaper'] },
  { key: 'gif_backend', label: 'GIF backend', type: 'select', options: ['awww', 'mpvpaper'] },
  { key: 'video_backend', label: 'Video backend', type: 'select', options: ['mpvpaper', 'awww'] },
];

export const BACKEND_ADVANCED_CONFIGS: ConfigGroup[] = [
  { key: 'mpvpaper_options', label: 'mpvpaper options', type: 'text', placeholder: 'no-audio --loop-file=inf' },
  { key: 'mpvpaper_output', label: 'mpvpaper output', type: 'text', placeholder: '*' },
  { key: 'awww_transition_type', label: 'awww transition', type: 'select', options: ['fade', 'slide', 'wipe'] },
  { key: 'awww_transition_duration', label: 'awww duration (s)', type: 'text', placeholder: '1' },
  { key: 'awww_resize', label: 'awww resize', type: 'select', options: ['crop', 'fit', 'stretch'] },
];

export const WE_BACKEND_CONFIGS: ConfigGroup[] = [
  { key: 'linux_wallpaperengine_enabled', label: 'Enable scene backend', type: 'select', options: ['on', 'off'] },
  { key: 'linux_wallpaperengine_path', label: 'linux-wallpaperengine path', type: 'text', placeholder: 'auto' },
  { key: 'linux_wallpaperengine_target_mode', label: 'Target mode', type: 'select', options: ['auto', 'screen-root', 'screen-span', 'window'] },
  { key: 'linux_wallpaperengine_target', label: 'Output/window target', type: 'text', placeholder: 'eDP-1 or HDMI-A-1' },
];

export const WE_BACKEND_ADVANCED_CONFIGS: ConfigGroup[] = [
  { key: 'linux_wallpaperengine_scaling', label: 'Scaling', type: 'select', options: ['default', 'fill', 'fit', 'stretch'] },
  { key: 'linux_wallpaperengine_fps', label: 'FPS', type: 'select', options: ['30', '60'] },
  { key: 'linux_wallpaperengine_muted', label: 'Muted', type: 'select', options: ['off', 'on'] },
  { key: 'linux_wallpaperengine_volume', label: 'Volume', type: 'number', placeholder: '100' },
  { key: 'linux_wallpaperengine_assets_dir', label: 'Assets dir', type: 'text', placeholder: 'auto' },
];

export const LIBRARY_CONFIGS: ConfigGroup[] = [
  { key: 'min_wallpaper_width', label: 'Min width', type: 'number', placeholder: '1280' },
  { key: 'min_wallpaper_height', label: 'Min height', type: 'number', placeholder: '720' },
  { key: 'gui_thumbnail_mode', label: 'Thumbnail mode', type: 'select', options: ['cache', 'original', 'icon'] },
  { key: 'gui_thumbnail_cleanup_days', label: 'Clear thumbnail cache after days', type: 'number', placeholder: '30' },
];

export const LIBRARY_ADVANCED_CONFIGS: ConfigGroup[] = [
  { key: 'gui_thumbnail_failure_ttl_secs', label: 'Retry failed thumbnails after seconds', type: 'number', placeholder: '900' },
  { key: 'gui_debug_logs', label: 'Debug logs', type: 'select', options: ['off', 'on'] },
  { key: 'preview_metadata', label: 'fzf preview', type: 'select', options: ['compact', 'visual', 'full'] },
];

export function normalizeConfigValue(key: string, value: string): string {
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
  if (key === 'linux_wallpaperengine_path' || key === 'linux_wallpaperengine_assets_dir') {
    return value.trim() || 'auto';
  }
  if (key === 'linux_wallpaperengine_target') {
    return value.trim();
  }
  return value;
}

function clampIntString(value: string, min: number, max: number, fallback: number): string {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) return String(fallback);
  return String(Math.min(max, Math.max(min, parsed)));
}

export function cleanupDays(configs: Record<string, string>, cache: ThumbnailCacheDTO | null): number {
  const configured = Number.parseInt(configs['gui_thumbnail_cleanup_days'] ?? '', 10);
  if (Number.isFinite(configured)) return Math.min(3650, Math.max(1, configured));
  return cache?.cleanupDays ?? 30;
}
