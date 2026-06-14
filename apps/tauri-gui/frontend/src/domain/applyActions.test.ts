import { describe, it } from 'node:test';
import assert from 'node:assert';
import { normalizeApplyActions, hasEnabledAction, isApplyAvailable, getActionReason } from './applyActions.ts';
import type { WallpaperDTO } from '../api/bridge.ts';

const IMAGE: WallpaperDTO = {
  path: '/test/image.jpg', type: 'image', ext: 'jpg', backend: 'awww',
  size: 1024, mtime: 1, resolution: '1920x1080',
  applyAvailability: 'available', applyBackend: 'awww',
  applyActions: [
    { kind: 'apply', label: 'Apply', enabled: true },
    { kind: 'open_folder', label: 'Open folder', enabled: true },
  ],
};

const WE_SCENE: WallpaperDTO = {
  path: '/test/scene', type: 'we_scene', ext: 'scene', backend: 'linux-wallpaperengine',
  size: 4096, mtime: 1, resolution: 'WE',
  projectType: 'we_scene', previewPath: '/test/scene/preview.gif',
  workshopId: '123', title: 'Test Scene',
  applyAvailability: 'available', applyBackend: 'linux-wallpaperengine',
  applyActions: [
    { kind: 'apply', label: 'Apply', enabled: true },
    { kind: 'apply_preview', label: 'Apply preview GIF', enabled: true },
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

const WE_SCENE_FAILED: WallpaperDTO = {
  path: '/test/scene-failed', type: 'we_scene', ext: 'scene', backend: 'linux-wallpaperengine',
  size: 4096, mtime: 1, resolution: 'WE',
  projectType: 'we_scene', previewPath: '/test/scene-failed/preview.gif',
  workshopId: '456', title: 'Failed Scene',
  backendStatus: 'failed',
  applyAvailability: 'retryable_failure', applyBackend: 'linux-wallpaperengine',
  applyActions: [
    { kind: 'retry_backend_apply', label: 'Retry backend apply', enabled: true },
    { kind: 'apply_preview', label: 'Apply preview GIF', enabled: true },
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

const WE_WEB: WallpaperDTO = {
  path: '/test/web', type: 'we_web', ext: 'web', backend: 'unsupported',
  size: 8192, mtime: 1, resolution: 'WE',
  projectType: 'we_web', workshopId: '789', title: 'Web Title',
  applyAvailability: 'unsupported', applyReason: 'browsing only',
  applyActions: [
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

const UNSUPPORTED: WallpaperDTO = {
  path: '/test/app', type: 'unsupported', ext: 'application', backend: 'unsupported',
  size: 1024, mtime: 1, resolution: 'WE',
  projectType: 'unsupported', workshopId: '999',
  applyAvailability: 'unsupported',
  applyActions: [
    { kind: 'open_folder', label: 'Open folder', enabled: true },
    { kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true },
  ],
};

describe('normalizeApplyActions with applyActions present', () => {
  it('image has apply and open_folder', () => {
    const a = normalizeApplyActions(IMAGE);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('apply'));
    assert(kinds.includes('open_folder'));
  });

  it('image isApplyAvailable true', () => {
    assert(isApplyAvailable(IMAGE));
  });

  it('WE Scene normal has apply, apply_preview, open_folder, copy_workshop_id', () => {
    const a = normalizeApplyActions(WE_SCENE);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('apply'));
    assert(kinds.includes('apply_preview'));
    assert(kinds.includes('open_folder'));
    assert(kinds.includes('copy_workshop_id'));
  });

  it('failed WE Scene has retry_backend_apply and no apply', () => {
    const a = normalizeApplyActions(WE_SCENE_FAILED);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('retry_backend_apply'));
    assert(!kinds.includes('apply'));
  });

  it('failed WE Scene isApplyAvailable false', () => {
    assert(!isApplyAvailable(WE_SCENE_FAILED));
  });

  it('WE Web has open_folder and copy_workshop_id, no apply or apply_preview', () => {
    const a = normalizeApplyActions(WE_WEB);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('open_folder'));
    assert(kinds.includes('copy_workshop_id'));
    assert(!kinds.includes('apply'));
    assert(!kinds.includes('apply_preview'));
  });

  it('unsupported has open_folder and copy_workshop_id, no apply', () => {
    const a = normalizeApplyActions(UNSUPPORTED);
    const kinds = a.map(x => x.kind);
    assert(kinds.includes('open_folder'));
    assert(kinds.includes('copy_workshop_id'));
    assert(!kinds.includes('apply'));
  });

  it('unknown action kind is ignored silently', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: 'apply' as any, label: 'Apply', enabled: true },
        { kind: 'bogus' as any, label: 'Bogus', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'apply');
  });

  it('malformed: missing label filtered', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: 'apply', label: '', enabled: true } as any,
        { kind: 'open_folder', label: 'Open folder', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'open_folder');
  });

  it('malformed: enabled=false filtered', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: 'apply', label: 'Apply', enabled: false },
        { kind: 'open_folder', label: 'Open folder', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'open_folder');
  });

  it('malformed: missing kind filtered', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
      applyActions: [
        { kind: undefined as any, label: 'Bad', enabled: true },
        { kind: 'open_folder', label: 'Open folder', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert(a.length === 1);
    assert(a[0].kind === 'open_folder');
  });

  it('getActionReason returns reason', () => {
    const entry: WallpaperDTO = {
      ...IMAGE,
      applyActions: [
        { kind: 'apply', label: 'Apply', enabled: true, reason: 'test reason' },
      ],
    };
    assert.equal(getActionReason(entry, 'apply'), 'test reason');
  });

  it('preserves DTO order', () => {
    const entry: WallpaperDTO = {
      path: '/test/x', type: 'we_scene', ext: 'scene', backend: 'linux-wallpaperengine',
      size: 1, mtime: 1, resolution: 'WE',
      applyActions: [
        { kind: 'apply_preview', label: 'Preview', enabled: true },
        { kind: 'apply', label: 'Apply', enabled: true },
        { kind: 'open_folder', label: 'Folder', enabled: true },
        { kind: 'copy_workshop_id', label: 'Copy', enabled: true },
      ],
    };
    const a = normalizeApplyActions(entry);
    assert.equal(a[0].kind, 'apply_preview');
    assert.equal(a[1].kind, 'apply');
    assert.equal(a[2].kind, 'open_folder');
    assert.equal(a[3].kind, 'copy_workshop_id');
  });
});

describe('legacy fallback (applyActions missing)', () => {
  it('image fallback has apply', () => {
    const entry: WallpaperDTO = {
      path: '/test/img.jpg', type: 'image', ext: 'jpg', backend: 'awww',
      size: 1, mtime: 1, resolution: '1x1',
    };
    assert(isApplyAvailable(entry));
    const a = normalizeApplyActions(entry);
    assert(a.some(x => x.kind === 'apply'));
  });

  it('we_web fallback has no apply', () => {
    const entry: WallpaperDTO = {
      path: '/test/web', type: 'we_web', ext: 'web', backend: 'unsupported',
      size: 1, mtime: 1, resolution: 'WE',
    };
    assert(!isApplyAvailable(entry));
    const a = normalizeApplyActions(entry);
    assert(a.some(x => x.kind === 'open_folder'));
    assert(!a.some(x => x.kind === 'apply'));
  });

  it('unsupported fallback has no apply', () => {
    const entry: WallpaperDTO = {
      path: '/test/exe', type: 'unsupported', ext: 'exe', backend: 'unsupported',
      size: 1, mtime: 1, resolution: 'WE',
    };
    assert(!isApplyAvailable(entry));
  });

  it('we_scene fallback has apply and copy_workshop_id if workshopId', () => {
    const entry: WallpaperDTO = {
      path: '/test/scene', type: 'we_scene', ext: 'scene', backend: 'linux-wallpaperengine',
      size: 1, mtime: 1, resolution: 'WE',
      workshopId: 'abcd',
    };
    assert(isApplyAvailable(entry));
    const a = normalizeApplyActions(entry);
    assert(a.some(x => x.kind === 'apply'));
    assert(a.some(x => x.kind === 'open_folder'));
    assert(a.some(x => x.kind === 'copy_workshop_id'));
  });
});
