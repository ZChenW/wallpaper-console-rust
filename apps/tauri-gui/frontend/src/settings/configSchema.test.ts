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
});
