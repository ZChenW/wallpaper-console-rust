import { ThumbnailCacheDTO } from '../api/bridge';

export interface SettingEntry {
  key: string;
  label: string;
  type: 'select' | 'text' | 'number';
  options?: string[];
  placeholder?: string;
  category: 'wallpaper' | 'we' | 'library' | 'advanced';
  advanced?: boolean;
  description?: string;
}

export const ALL_SETTINGS: SettingEntry[] = [
  // ── Wallpaper (image/GIF/video backends) ──
  {
    key: 'image_backend', label: 'Image backend', type: 'select',
    options: ['awww', 'mpvpaper'], category: 'wallpaper',
    description: 'Recommended: awww for static images',
  },
  {
    key: 'gif_backend', label: 'GIF backend', type: 'select',
    options: ['awww', 'mpvpaper'], category: 'wallpaper',
    description: 'Recommended: awww for animated GIFs',
  },
  {
    key: 'video_backend', label: 'Video backend', type: 'select',
    options: ['mpvpaper', 'awww'], category: 'wallpaper',
    description: 'Recommended: mpvpaper for videos',
  },
  {
    key: 'awww_resize', label: 'awww resize', type: 'select',
    options: ['crop', 'fit', 'stretch'], category: 'wallpaper',
  },
  {
    key: 'awww_transition_type', label: 'awww transition', type: 'select',
    options: ['fade', 'slide', 'wipe'], category: 'wallpaper',
  },
  {
    key: 'awww_transition_duration', label: 'awww duration (s)', type: 'text',
    placeholder: '1', category: 'wallpaper',
  },
  {
    key: 'mpvpaper_options', label: 'mpvpaper options', type: 'text',
    placeholder: 'no-audio --loop-file=inf', category: 'wallpaper', advanced: true,
  },
  {
    key: 'mpvpaper_output', label: 'mpvpaper output', type: 'text',
    placeholder: '*', category: 'wallpaper', advanced: true,
  },

  // ── Wallpaper Engine Scene ──
  {
    key: 'linux_wallpaperengine_enabled', label: 'Enable scene backend', type: 'select',
    options: ['on', 'off'], category: 'we',
  },
  {
    key: 'linux_wallpaperengine_path', label: 'linux-wallpaperengine path', type: 'text',
    placeholder: 'auto', category: 'we',
  },
  {
    key: 'linux_wallpaperengine_target_mode', label: 'Target mode', type: 'select',
    options: ['auto', 'screen-root', 'screen-span', 'window'], category: 'we',
  },
  {
    key: 'linux_wallpaperengine_target', label: 'Output/window target', type: 'text',
    placeholder: 'eDP-1 or HDMI-A-1', category: 'we',
  },
  {
    key: 'linux_wallpaperengine_scaling', label: 'Scaling', type: 'select',
    options: ['default', 'fill', 'fit', 'stretch'], category: 'we', advanced: true,
  },
  {
    key: 'linux_wallpaperengine_fps', label: 'FPS', type: 'select',
    options: ['30', '60'], category: 'we', advanced: true,
  },
  {
    key: 'linux_wallpaperengine_muted', label: 'Muted', type: 'select',
    options: ['off', 'on'], category: 'we', advanced: true,
  },
  {
    key: 'linux_wallpaperengine_volume', label: 'Volume', type: 'number',
    placeholder: '100', category: 'we', advanced: true,
  },
  {
    key: 'linux_wallpaperengine_assets_dir', label: 'Assets dir', type: 'text',
    placeholder: 'auto', category: 'we', advanced: true,
  },

  // ── Library ──
  {
    key: 'min_wallpaper_width', label: 'Min width', type: 'number',
    placeholder: '1280', category: 'library',
  },
  {
    key: 'min_wallpaper_height', label: 'Min height', type: 'number',
    placeholder: '720', category: 'library',
  },
  {
    key: 'gui_thumbnail_mode', label: 'Thumbnail mode', type: 'select',
    options: ['cache', 'original', 'icon'], category: 'library',
  },
  {
    key: 'gui_thumbnail_cleanup_days', label: 'Clear thumbnail cache after days', type: 'number',
    placeholder: '30', category: 'library',
  },
  {
    key: 'gui_thumbnail_failure_ttl_secs', label: 'Retry failed thumbnails after seconds', type: 'number',
    placeholder: '900', category: 'library', advanced: true,
  },
  {
    key: 'preview_metadata', label: 'fzf preview', type: 'select',
    options: ['compact', 'visual', 'full'], category: 'library', advanced: true,
  },

  // ── Advanced ──
  {
    key: 'gui_debug_logs', label: 'Debug logs', type: 'select',
    options: ['off', 'on'], category: 'advanced',
  },
  {
    key: 'open_project_location_mode', label: 'Open project folders with', type: 'select',
    options: ['ask', 'files', 'terminal'], category: 'advanced',
    description: 'Choose whether to open project locations in file manager or terminal',
  },
];

export type SettingsCategory = 'general' | 'wallpaper' | 'we' | 'library' | 'database' | 'advanced';

export const CATEGORY_LABELS: Record<SettingsCategory, string> = {
  general: 'General',
  wallpaper: 'Wallpaper',
  we: 'Wallpaper Engine',
  library: 'Library',
  database: 'Database',
  advanced: 'Advanced',
};

export const CATEGORY_ORDER: SettingsCategory[] = [
  'general', 'wallpaper', 'we', 'library', 'database', 'advanced',
];

export function getSettingsByCategoryAndLevel(
  cat: SettingsCategory,
  advanced: boolean,
): SettingEntry[] {
  return ALL_SETTINGS.filter((s) => s.category === cat && (s.advanced ?? false) === advanced);
}

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
