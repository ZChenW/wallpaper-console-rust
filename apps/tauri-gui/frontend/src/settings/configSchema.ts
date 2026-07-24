import { ThumbnailCacheDTO } from '../api/bridge';
export { normalizeConfigValue } from './configNormalizer';

export interface SettingEntry {
  key: string;
  label: string;
  type: 'select' | 'text' | 'number';
  options?: string[];
  optionLabels?: Record<string, string>;
  placeholder?: string;
  category: 'general' | 'wallpaper' | 'we' | 'library' | 'advanced';
  advanced?: boolean;
  description?: string;
}

export const ALL_SETTINGS: SettingEntry[] = [
  // ── General ──
  {
    key: 'gui_theme', label: 'Theme', type: 'select',
    options: ['light', 'obsidian_warm'],
    optionLabels: {
      light: 'Light',
      obsidian_warm: 'Obsidian Warm',
    },
    category: 'general',
    description: 'Switch between the current UI palette and a warm Obsidian-style palette.',
  },

  // ── Wallpaper (image/GIF/video backends) ──
  {
    key: 'image_backend', label: 'Image backend', type: 'select',
    options: ['awww', 'mpvpaper'], category: 'wallpaper',
    description: 'Recommended: awww for smooth static image transitions',
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
    key: 'awww_resize', label: 'Image fit mode', type: 'select',
    options: ['crop', 'fit', 'stretch'], category: 'wallpaper',
    description: 'Controls how static images are scaled by awww.',
  },
  {
    key: 'awww_transition_type', label: 'Transition', type: 'select',
    options: ['simple', 'fade', 'left', 'right', 'top', 'bottom', 'wipe', 'grow', 'center', 'outer', 'random', 'wave'], category: 'wallpaper',
    description: 'Animation used when switching static images.',
  },
  {
    key: 'awww_transition_duration', label: 'Transition duration (s)', type: 'text',
    placeholder: '1', category: 'wallpaper',
    description: 'Duration of awww image transitions.',
  },
  {
    key: 'wallpaper_transition_fps', label: 'Transition frame rate', type: 'number',
    placeholder: '60', category: 'wallpaper', advanced: true,
    description: 'Higher values look smoother but use more GPU.',
  },
  {
    key: 'mpvpaper_options', label: 'mpvpaper arguments', type: 'text',
    placeholder: '--loop-file=inf --panscan=1.0', category: 'wallpaper', advanced: true,
    description: 'Advanced mpvpaper/mpv arguments. Default keeps audio, loops playback, and crops video to fill the screen.',
  },
  {
    key: 'mpvpaper_output', label: 'Video output target', type: 'text',
    placeholder: '*', category: 'wallpaper', advanced: true,
    description: 'mpvpaper output selector. Keep "*" unless you need a specific monitor.',
  },
  {
    key: 'post_apply_enabled', label: 'Post-apply theme hook', type: 'select',
    options: ['off', 'on'], category: 'wallpaper', advanced: true,
    description: 'After a successful apply, run an external command (for example matugen) to sync colors from the wallpaper.',
  },
  {
    key: 'post_apply_command', label: 'Post-apply command', type: 'text',
    placeholder: 'matugen image "$still"', category: 'wallpaper', advanced: true,
    description: 'Shell command run after apply. Placeholders: $wallpaper, $path, $still, $backend, $outputs.',
  },
  {
    key: 'post_apply_timeout_secs', label: 'Post-apply timeout (s)', type: 'number',
    placeholder: '30', category: 'wallpaper', advanced: true,
    description: 'Kill the post-apply command if it runs longer than this many seconds.',
  },

  // ── Wallpaper Engine Scene ──
  {
    key: 'linux_wallpaperengine_enabled', label: 'Enable scene backend', type: 'select',
    options: ['on', 'off'], category: 'we',
  },
  {
    key: 'linux_wallpaperengine_path', label: 'Wallpaper Engine executable', type: 'text',
    placeholder: 'auto', category: 'we',
    description: 'Use "auto" to find linux-wallpaperengine from PATH.',
  },
  {
    key: 'linux_wallpaperengine_target_mode', label: 'Target type', type: 'select',
    options: ['auto', 'screen-root', 'screen-span'], category: 'we',
    description: 'screen-root is recommended on Niri/Wayland.',
  },
  {
    key: 'linux_wallpaperengine_target', label: 'Display target', type: 'text',
    placeholder: 'e.g. eDP-1 or HDMI-A-1', category: 'we',
    description: 'Example: eDP-1 or HDMI-A-1.',
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

  // ── Library ──
  {
    key: 'gui_thumbnail_mode', label: 'Thumbnail mode', type: 'select',
    options: ['cache', 'icon'], category: 'library',
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
    key: 'preview_metadata',
    label: 'CLI preview metadata',
    type: 'select',
    options: ['compact', 'visual', 'full'],
    category: 'library',
    advanced: true,
    description: 'Controls metadata shown by the terminal/fzf preview, not the GUI Library cards.',
  },

  // ── Advanced ──
  {
    key: 'gui_debug_logs', label: 'Debug logs', type: 'select',
    options: ['off', 'on'], category: 'advanced',
  },
  {
    key: 'open_project_location_mode', label: 'Open project folders with', type: 'select',
    options: ['file_manager', 'terminal'], category: 'advanced',
    description: 'Choose your default after first use. The first time you will be asked to pick.',
  },
  {
    key: 'gui_file_manager', label: 'File Manager', type: 'select',
    options: ['auto', 'nautilus', 'dolphin', 'thunar', 'nemo', 'pcmanfm', 'custom'], category: 'advanced',
  },
  {
    key: 'gui_file_manager_custom', label: 'Custom file manager command', type: 'text',
    placeholder: 'thunar', category: 'advanced',
  },
  {
    key: 'gui_terminal_file_manager', label: 'Terminal File Manager', type: 'select',
    options: ['yazi', 'custom'], category: 'advanced',
  },
  {
    key: 'gui_terminal_file_manager_custom', label: 'Custom terminal file manager command', type: 'text',
    placeholder: 'yazi', category: 'advanced',
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

export function cleanupDays(configs: Record<string, string>, cache: ThumbnailCacheDTO | null): number {
  const configured = Number.parseInt(configs['gui_thumbnail_cleanup_days'] ?? '', 10);
  if (Number.isFinite(configured)) return Math.min(3650, Math.max(1, configured));
  return cache?.cleanupDays ?? 30;
}
