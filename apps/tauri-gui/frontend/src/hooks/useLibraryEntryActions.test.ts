import assert from 'node:assert';
import test from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { api } from '../api/bridge.ts';
import type { ApplyRequestDTO, WallpaperDTO } from '../api/bridge.ts';
import type { ContextAction } from '../components/WallpaperGrid.tsx';
import { useLibraryEntryActions } from './useLibraryEntryActions.ts';

const FAILED_SCENE: WallpaperDTO = {
  path: '/wallpapers/failed-scene',
  type: 'we_scene',
  ext: 'scene',
  backend: 'linux-wallpaperengine',
  size: 1,
  mtime: 1,
  resolution: 'WE',
  applyAvailability: 'retryable_failure',
  applyActions: [
    { kind: 'retry_backend_apply', label: 'Retry backend apply', enabled: true },
  ],
};

test('retry action leaves failure cleanup to a confirmed backend success', async () => {
  const requests: ApplyRequestDTO[] = [];
  let actions: ContextAction[] = [];
  let clearCalls = 0;
  const originalClear = api.weClearBackendError;
  api.weClearBackendError = async () => {
    clearCalls += 1;
    return { success: true, stdout: '', stderr: '', exitCode: 0 };
  };

  function Probe() {
    const entryActions = useLibraryEntryActions({
      onApplyAction: (request) => requests.push(request),
      openFolder: () => {},
      findEntry: () => FAILED_SCENE,
    });
    actions = entryActions.buildContextActions(FAILED_SCENE);
    return null;
  }

  try {
    renderToStaticMarkup(createElement(Probe));
    assert.equal(actions.length, 1);
    await actions[0].action(FAILED_SCENE.path);
  } finally {
    api.weClearBackendError = originalClear;
  }

  assert.equal(clearCalls, 0);
  assert.equal(requests.length, 1);
  assert.equal(requests[0].kind, 'retry_backend_apply');
  assert.equal(requests[0].path, FAILED_SCENE.path);
});
