import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildApplyRequest } from './applyRequests.ts';
import type { WallpaperDTO } from '../api/bridge.ts';

const scene: WallpaperDTO = {
  path: '/we/scene',
  type: 'we_scene',
  ext: 'scene',
  backend: 'linux-wallpaperengine',
  size: 1,
  mtime: 1,
  resolution: 'WE',
};

describe('buildApplyRequest', () => {
  it('builds normal apply request', () => {
    const r = buildApplyRequest(scene, 'apply');
    assert.equal(r.kind, 'apply');
    assert.equal(r.path, '/we/scene');
    assert.ok(r.requestId);
  });

  it('builds preview request using project path, not preview path', () => {
    const r = buildApplyRequest(
      { ...scene, previewPath: '/we/scene/preview.gif' },
      'apply_preview',
    );
    assert.equal(r.kind, 'apply_preview');
    assert.equal(r.path, '/we/scene');
  });

  it('rejects non-execution actions', () => {
    assert.throws(
      () => buildApplyRequest(scene, 'open_folder' as any),
      /not executable/,
    );
  });
});
