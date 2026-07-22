import assert from 'node:assert/strict';
import test from 'node:test';

import type { LibraryBrowserItemDTO } from '../api/types.ts';
import * as previewMedia from './wallpaperPreviewMedia.ts';
import {
  attachVideoDecoder,
  enhancedMediaCandidates,
  previewFallbackState,
  releaseVideoDecoder,
  staticPreviewAssetPath,
  staticFallbackAssetPath,
} from './wallpaperPreviewMedia.ts';

function entry(overrides: Partial<LibraryBrowserItemDTO> = {}): LibraryBrowserItemDTO {
  return {
    wallpaperId: 7,
    path: '/walls/original.jpg',
    type: 'image',
    ext: 'jpg',
    backend: 'awww',
    size: 1024,
    mtime: 100,
    resolution: '3840x2160',
    favorite: false,
    author: null,
    addedAt: '2026-07-19T00:00:00Z',
    sources: [],
    ...overrides,
  };
}

const eligible = {
  active: true,
  centered: true,
  selected: true,
  settled: true,
  reducedMotion: false,
} as const;

test('static preview prefers a project preview without reading the original stream asset', () => {
  assert.equal(
    staticPreviewAssetPath(entry({ previewPath: '/cache/project-preview.jpg' })),
    '/cache/project-preview.jpg',
  );
  assert.equal(staticPreviewAssetPath(entry()), '/walls/original.jpg');
});

test('static fallback preloads only safe image-shaped assets when enabled', () => {
  assert.equal(staticFallbackAssetPath(entry(), false), null);
  assert.equal(
    staticFallbackAssetPath(entry({ previewPath: '/cache/project-preview.jpg' }), true),
    '/cache/project-preview.jpg',
  );
  assert.equal(staticFallbackAssetPath(entry(), true), '/walls/original.jpg');
  assert.equal(staticFallbackAssetPath(entry({ type: 'video' }), true), null);
});

test('enhanced media is limited to the one centered selected settled item', () => {
  assert.deepEqual(enhancedMediaCandidates(entry(), eligible), [
    { kind: 'image', path: '/walls/original.jpg' },
  ]);

  for (const disabled of [
    { ...eligible, active: false },
    { ...eligible, centered: false },
    { ...eligible, selected: false },
    { ...eligible, settled: false },
    { ...eligible, reducedMotion: true },
  ]) {
    assert.deepEqual(enhancedMediaCandidates(entry(), disabled), []);
  }
});

test('enhanced media activation waits for dwell and survives only transient unsettled motion', () => {
  type ActivationPlanner = (
    target: LibraryBrowserItemDTO,
    activated: boolean,
    eligibility: Parameters<typeof enhancedMediaCandidates>[1],
  ) => { readonly retain: boolean; readonly schedule: boolean };
  const planner = (previewMedia as typeof previewMedia & {
    enhancedMediaActivationPlan?: ActivationPlanner;
  }).enhancedMediaActivationPlan;

  assert.equal(typeof planner, 'function');
  if (!planner) return;
  assert.deepEqual(planner(entry(), false, { ...eligible, settled: false }), {
    retain: false,
    schedule: false,
  });
  assert.deepEqual(planner(entry(), false, eligible), {
    retain: false,
    schedule: true,
  });
  assert.deepEqual(planner(entry(), true, { ...eligible, settled: false }), {
    retain: true,
    schedule: false,
  });
  assert.deepEqual(planner(entry(), true, { ...eligible, centered: false }), {
    retain: false,
    schedule: false,
  });
  assert.deepEqual(planner(entry({ type: 'we_scene' }), false, eligible), {
    retain: false,
    schedule: false,
  });
});

test('GIF and video originals use one decoder candidate with a static preview fallback', () => {
  assert.deepEqual(
    enhancedMediaCandidates(entry({
      type: 'gif',
      ext: 'gif',
      path: '/walls/animated.gif',
      previewPath: '/cache/animated-preview.jpg',
    }), eligible),
    [
      { kind: 'image', path: '/walls/animated.gif' },
      { kind: 'image', path: '/cache/animated-preview.jpg' },
    ],
  );
  assert.deepEqual(
    enhancedMediaCandidates(entry({
      type: 'video',
      ext: 'mp4',
      path: '/walls/loop.mp4',
      previewPath: '/cache/loop-preview.jpg',
    }), eligible),
    [
      { kind: 'video', path: '/walls/loop.mp4' },
      { kind: 'image', path: '/cache/loop-preview.jpg' },
    ],
  );
});

test('Wallpaper Engine and unsupported projects remain honest static previews', () => {
  for (const type of ['we_scene', 'we_web', 'unsupported']) {
    assert.deepEqual(enhancedMediaCandidates(entry({ type }), eligible), []);
  }
});

test('duplicate preview paths are not decoded twice', () => {
  assert.deepEqual(
    enhancedMediaCandidates(entry({ previewPath: '/walls/original.jpg' }), eligible),
    [{ kind: 'image', path: '/walls/original.jpg' }],
  );
});

test('an enhanced-media failure remains an honest thumbnail fallback when available', () => {
  assert.equal(previewFallbackState(true, '/cache/original-thumb.jpg'), 'thumbnail');
  assert.equal(previewFallbackState(true, undefined), 'unavailable');
  assert.equal(previewFallbackState(false, undefined), 'pending');
});

test('a broken thumbnail cannot hide the terminal unavailable state', () => {
  assert.equal(
    previewFallbackState(true, '/cache/broken-thumb.jpg', true),
    'unavailable',
  );
});

test('video cleanup pauses and releases the decoder source', () => {
  const calls: string[] = [];
  const video = {
    pause: () => calls.push('pause'),
    removeAttribute: (name: string) => calls.push(`remove:${name}`),
    load: () => calls.push('load'),
  };

  releaseVideoDecoder(video);

  assert.deepEqual(calls, ['pause', 'remove:src', 'load']);
});

test('video ref replacement releases only the old decoder and restores a StrictMode reattach', () => {
  const createVideo = (initialSrc: string | null) => {
    let src = initialSrc;
    const calls: string[] = [];
    return {
      calls,
      video: {
        pause: () => calls.push('pause'),
        removeAttribute: (name: string) => {
          calls.push(`remove:${name}`);
          if (name === 'src') src = null;
        },
        load: () => calls.push('load'),
        getAttribute: (name: string) => (name === 'src' ? src : null),
        setAttribute: (name: string, value: string) => {
          calls.push(`set:${name}:${value}`);
          if (name === 'src') src = value;
        },
      },
    };
  };
  const first = createVideo('/first.mp4');
  const second = createVideo(null);

  let attached = attachVideoDecoder(null, first.video, '/first.mp4');
  attached = attachVideoDecoder(attached, null, null);
  attached = attachVideoDecoder(attached, first.video, '/first.mp4');
  assert.equal(attached, first.video);
  assert.deepEqual(first.calls, [
    'pause',
    'remove:src',
    'load',
    'set:src:/first.mp4',
  ]);

  attached = attachVideoDecoder(attached, second.video, '/second.mp4');
  assert.equal(attached, second.video);
  assert.deepEqual(first.calls.slice(-3), ['pause', 'remove:src', 'load']);
  assert.deepEqual(second.calls, ['set:src:/second.mp4']);
});
