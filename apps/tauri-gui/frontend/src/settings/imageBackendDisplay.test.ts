import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { resolveImageBackendDisplay } from './imageBackendDisplay.ts';

describe('resolveImageBackendDisplay', () => {
  it('returns empty display when configGet fails', () => {
    const r = resolveImageBackendDisplay(null, false);
    assert.equal(r.display, '');
    assert.equal(r.shouldMigrate, false);
  });

  it('returns empty display when configGet fails (null raw)', () => {
    const r = resolveImageBackendDisplay(null, true);
    assert.equal(r.display, '');
    assert.equal(r.shouldMigrate, false);
  });

  it('display=awww shouldMigrate=true for legacy swww', () => {
    const r = resolveImageBackendDisplay('swww', true);
    assert.equal(r.display, 'awww');
    assert.equal(r.shouldMigrate, true);
  });

  it('display=awww shouldMigrate=false for awww (already normal)', () => {
    const r = resolveImageBackendDisplay('awww', true);
    assert.equal(r.display, 'awww');
    assert.equal(r.shouldMigrate, false);
  });

  it('display=mpvpaper shouldMigrate=false for mpvpaper', () => {
    const r = resolveImageBackendDisplay('mpvpaper', true);
    assert.equal(r.display, 'mpvpaper');
    assert.equal(r.shouldMigrate, false);
  });

  it('display=awww shouldMigrate=false for unknown values', () => {
    assert.equal(resolveImageBackendDisplay('bad', true).display, 'awww');
    assert.equal(resolveImageBackendDisplay('bad', true).shouldMigrate, false);
    assert.equal(resolveImageBackendDisplay('', true).display, 'awww');
    assert.equal(resolveImageBackendDisplay('', true).shouldMigrate, false);
    assert.equal(resolveImageBackendDisplay('swaybg', true).display, 'awww');
    assert.equal(resolveImageBackendDisplay('swaybg', true).shouldMigrate, false);
  });
});
