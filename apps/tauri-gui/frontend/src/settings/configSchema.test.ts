import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { normalizeConfigValue } from './configNormalizer.ts';

describe('normalizeConfigValue', () => {
  describe('awww_transition_type', () => {
    it('normalizes legacy slide to left', () => {
      assert.equal(normalizeConfigValue('awww_transition_type', 'slide'), 'left');
    });

    it('returns known values unchanged', () => {
      for (const v of ['simple', 'fade', 'left', 'right', 'top', 'bottom', 'wipe', 'grow', 'center', 'outer', 'random', 'wave']) {
        assert.equal(normalizeConfigValue('awww_transition_type', v), v);
      }
    });

    it('falls back to fade for unknown values', () => {
      assert.equal(normalizeConfigValue('awww_transition_type', 'none'), 'fade');
      assert.equal(normalizeConfigValue('awww_transition_type', 'invalid'), 'fade');
    });
  });

  describe('storage_backend', () => {
    it('always returns sqlite', () => {
      assert.equal(normalizeConfigValue('storage_backend', 'file'), 'sqlite');
      assert.equal(normalizeConfigValue('storage_backend', 'hybrid'), 'sqlite');
      assert.equal(normalizeConfigValue('storage_backend', 'sqlite'), 'sqlite');
    });
  });

  describe('gui_theme', () => {
    it('normalizes current to light', () => {
      assert.equal(normalizeConfigValue('gui_theme', 'current'), 'light');
    });

    it('returns obsidian_warm unchanged', () => {
      assert.equal(normalizeConfigValue('gui_theme', 'obsidian_warm'), 'obsidian_warm');
    });

    it('falls back to light', () => {
      assert.equal(normalizeConfigValue('gui_theme', 'unknown'), 'light');
    });
  });

  describe('image_backend', () => {
    it('normalizes legacy swww to awww', () => {
      assert.equal(normalizeConfigValue('image_backend', 'swww'), 'awww');
    });

    it('returns mpvpaper unchanged', () => {
      assert.equal(normalizeConfigValue('image_backend', 'mpvpaper'), 'mpvpaper');
    });
  });

  describe('linux_wallpaperengine_target_mode', () => {
    it('returns valid values unchanged', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_target_mode', 'screen-root'), 'screen-root');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_target_mode', 'screen-span'), 'screen-span');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_target_mode', 'auto'), 'auto');
    });

    it('normalizes window and bad values to auto', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_target_mode', 'window'), 'auto');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_target_mode', 'bad'), 'auto');
    });
  });

  describe('linux_wallpaperengine_scaling', () => {
    it('returns valid values unchanged', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_scaling', 'fill'), 'fill');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_scaling', 'stretch'), 'stretch');
    });

    it('normalizes bad values to default', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_scaling', 'bad'), 'default');
    });
  });

  describe('awww_transition_duration', () => {
    it('returns finite values within 0..60 unchanged', () => {
      assert.equal(normalizeConfigValue('awww_transition_duration', '0'), '0');
      assert.equal(normalizeConfigValue('awww_transition_duration', '1.5'), '1.5');
      assert.equal(normalizeConfigValue('awww_transition_duration', '60'), '60');
    });

    it('falls back to 1 for out-of-range values', () => {
      assert.equal(normalizeConfigValue('awww_transition_duration', '-1'), '1');
      assert.equal(normalizeConfigValue('awww_transition_duration', '999'), '1');
    });

    it('falls back to 1 for non-numeric values', () => {
      assert.equal(normalizeConfigValue('awww_transition_duration', 'nan'), '1');
      assert.equal(normalizeConfigValue('awww_transition_duration', 'bad'), '1');
    });

    it('falls back to 1 for trailing garbage', () => {
      assert.equal(normalizeConfigValue('awww_transition_duration', '1.5abc'), '1');
      assert.equal(normalizeConfigValue('awww_transition_duration', '2.0foo'), '1');
    });
  });

  describe('wallpaper_transition_fps', () => {
    it('returns values within 1..240 unchanged', () => {
      assert.equal(normalizeConfigValue('wallpaper_transition_fps', '1'), '1');
      assert.equal(normalizeConfigValue('wallpaper_transition_fps', '60'), '60');
      assert.equal(normalizeConfigValue('wallpaper_transition_fps', '240'), '240');
    });

    it('clamps out-of-range values', () => {
      assert.equal(normalizeConfigValue('wallpaper_transition_fps', '0'), '1');
      assert.equal(normalizeConfigValue('wallpaper_transition_fps', '999'), '240');
    });

    it('falls back to 60 for non-numeric values', () => {
      assert.equal(normalizeConfigValue('wallpaper_transition_fps', 'bad'), '60');
    });
  });

  describe('linux_wallpaperengine_fps', () => {
    it('returns values within 1..240 unchanged', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_fps', '1'), '1');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_fps', '120'), '120');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_fps', '240'), '240');
    });

    it('clamps out-of-range values', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_fps', '0'), '1');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_fps', '999'), '240');
    });

    it('falls back to 60 for non-numeric values', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_fps', 'bad'), '60');
    });
  });

  describe('linux_wallpaperengine_volume', () => {
    it('returns values within 0..100 unchanged', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_volume', '0'), '0');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_volume', '50'), '50');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_volume', '100'), '100');
    });

    it('clamps out-of-range values', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_volume', '-5'), '0');
      assert.equal(normalizeConfigValue('linux_wallpaperengine_volume', '999'), '100');
    });

    it('falls back to 100 for non-numeric values', () => {
      assert.equal(normalizeConfigValue('linux_wallpaperengine_volume', 'bad'), '100');
    });
  });

  describe('gui_thumbnail_cleanup_days', () => {
    it('returns values within 1..3650 unchanged', () => {
      assert.equal(normalizeConfigValue('gui_thumbnail_cleanup_days', '1'), '1');
      assert.equal(normalizeConfigValue('gui_thumbnail_cleanup_days', '30'), '30');
      assert.equal(normalizeConfigValue('gui_thumbnail_cleanup_days', '3650'), '3650');
    });

    it('clamps out-of-range values', () => {
      assert.equal(normalizeConfigValue('gui_thumbnail_cleanup_days', '0'), '1');
      assert.equal(normalizeConfigValue('gui_thumbnail_cleanup_days', '3651'), '3650');
    });

    it('falls back to 30 for non-numeric values', () => {
      assert.equal(normalizeConfigValue('gui_thumbnail_cleanup_days', 'bad'), '30');
    });
  });

  describe('gui_thumbnail_failure_ttl_secs', () => {
    it('returns values within 60..86400 unchanged', () => {
      assert.equal(normalizeConfigValue('gui_thumbnail_failure_ttl_secs', '60'), '60');
      assert.equal(normalizeConfigValue('gui_thumbnail_failure_ttl_secs', '900'), '900');
      assert.equal(normalizeConfigValue('gui_thumbnail_failure_ttl_secs', '86400'), '86400');
    });

    it('clamps out-of-range values', () => {
      assert.equal(normalizeConfigValue('gui_thumbnail_failure_ttl_secs', '59'), '60');
      assert.equal(normalizeConfigValue('gui_thumbnail_failure_ttl_secs', '86401'), '86400');
    });

    it('falls back to 900 for non-numeric values', () => {
      assert.equal(normalizeConfigValue('gui_thumbnail_failure_ttl_secs', 'bad'), '900');
    });
  });

  describe('open_project_location_mode', () => {
    it('returns valid values unchanged', () => {
      assert.equal(normalizeConfigValue('open_project_location_mode', 'file_manager'), 'file_manager');
      assert.equal(normalizeConfigValue('open_project_location_mode', 'terminal'), 'terminal');
    });

    it('normalizes legacy files to file_manager', () => {
      assert.equal(normalizeConfigValue('open_project_location_mode', 'files'), 'file_manager');
    });

    it('falls back to file_manager for unknown values', () => {
      assert.equal(normalizeConfigValue('open_project_location_mode', 'bad'), 'file_manager');
    });
  });

  describe('gui_file_manager', () => {
    it('returns valid values unchanged', () => {
      assert.equal(normalizeConfigValue('gui_file_manager', 'auto'), 'auto');
      assert.equal(normalizeConfigValue('gui_file_manager', 'nautilus'), 'nautilus');
      assert.equal(normalizeConfigValue('gui_file_manager', 'custom'), 'custom');
    });

    it('falls back to auto for unknown values', () => {
      assert.equal(normalizeConfigValue('gui_file_manager', 'bad'), 'auto');
    });
  });

  describe('gui_terminal_file_manager', () => {
    it('returns custom unchanged', () => {
      assert.equal(normalizeConfigValue('gui_terminal_file_manager', 'custom'), 'custom');
    });

    it('falls back to yazi for any other value', () => {
      assert.equal(normalizeConfigValue('gui_terminal_file_manager', 'yazi'), 'yazi');
      assert.equal(normalizeConfigValue('gui_terminal_file_manager', 'bad'), 'yazi');
    });
  });
});
