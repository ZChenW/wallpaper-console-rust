import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

import {
  DEFAULT_SHELL_PREFERENCES,
  type ShellPreferences,
} from './shellPreferences.ts';
import {
  DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
  type WallpaperBehaviorSettings,
} from './useWallpaperBehaviorSettings.ts';

async function importTsxModule(): Promise<typeof import('./CompactSettingsPanel.tsx')> {
  const selectorSourceUrl = new URL('./DisplayTargetSelector.tsx', import.meta.url);
  const panelSourceUrl = new URL('./CompactSettingsPanel.tsx', import.meta.url);
  const selectorOutputUrl = new URL(`./.DisplayTargetSelector.test-${randomUUID()}.mjs`, import.meta.url);
  const panelOutputUrl = new URL(`./.CompactSettingsPanel.test-${randomUUID()}.mjs`, import.meta.url);
  const compilerOptions = {
    jsx: ts.JsxEmit.ReactJSX,
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  } as const;
  const selectorOutput = ts.transpileModule(await readFile(selectorSourceUrl, 'utf8'), {
    compilerOptions,
    fileName: selectorSourceUrl.pathname,
  }).outputText;
  const panelOutput = ts.transpileModule(await readFile(panelSourceUrl, 'utf8'), {
    compilerOptions,
    fileName: panelSourceUrl.pathname,
  }).outputText.replace(
    "from './DisplayTargetSelector.tsx';",
    `from './${selectorOutputUrl.pathname.split('/').at(-1)}';`,
  );

  await Promise.all([
    writeFile(selectorOutputUrl, selectorOutput, 'utf8'),
    writeFile(panelOutputUrl, panelOutput, 'utf8'),
  ]);
  try {
    return await import(panelOutputUrl.href);
  } finally {
    await Promise.all([unlink(selectorOutputUrl), unlink(panelOutputUrl)]);
  }
}

function findElements(
  node: ReactNode,
  predicate: (element: ReactElement<Record<string, unknown>>) => boolean,
): ReactElement<Record<string, unknown>>[] {
  if (!isValidElement<Record<string, unknown>>(node)) return [];

  const matches = predicate(node) ? [node] : [];
  if (typeof node.type === 'function') {
    const rendered = node.type(node.props) as ReactNode;
    return [...matches, ...findElements(rendered, predicate)];
  }
  return [
    ...matches,
    ...Children.toArray(node.props.children).flatMap((child) => findElements(child, predicate)),
  ];
}

function viewProps() {
  return {
    open: true,
    preferences: { ...DEFAULT_SHELL_PREFERENCES } as ShellPreferences,
    updatePreferences: (_update: unknown) => undefined,
    behaviorSettings: { ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS } as WallpaperBehaviorSettings,
    updateBehaviorSettings: (_update: unknown) => undefined,
    behaviorReady: true,
    loadError: null,
    saveError: null,
    rendererStatuses: {
      awww: { available: true, message: 'awww is installed.' },
      mpvpaper: { available: true, message: 'mpvpaper is installed.' },
      linuxWallpaperEngine: {
        available: true,
        message: 'linux-wallpaperengine is installed.',
      },
    },
    sourceCount: 4,
    offlineSourceCount: 1,
    onOpenSources: () => undefined,
    onClose: () => undefined,
  };
}

test('closed settings panel renders nothing', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  assert.equal(CompactSettingsPanelView({ ...viewProps(), open: false }), null);
});

test('settings uses an icon close action and closes on Escape or backdrop only', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  let closed = 0;
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    onClose: () => { closed += 1; },
  });
  const [overlay] = findElements(tree, (element) => typeof element.props.onKeyDown === 'function');
  const [close] = findElements(tree, (element) => element.props['aria-label'] === 'Close settings');
  let prevented = 0;

  assert.equal(close.props.autoFocus, true);
  assert.equal(close.props['data-icon-button'], true);
  assert.equal(overlay.props.style.zIndex, 1100);
  assert.doesNotMatch(renderToStaticMarkup(close), />Close</);
  (overlay.props.onKeyDown as (event: unknown) => void)({
    key: 'Escape',
    preventDefault: () => { prevented += 1; },
  });
  assert.equal(prevented, 1);
  assert.equal(closed, 1);

  const backdrop = {};
  (overlay.props.onMouseDown as (event: unknown) => void)({
    currentTarget: backdrop,
    target: {},
  });
  assert.equal(closed, 1, 'clicking inside the dialog does not close settings');
  (overlay.props.onMouseDown as (event: unknown) => void)({
    currentTarget: backdrop,
    target: backdrop,
  });
  assert.equal(closed, 2, 'clicking the backdrop closes settings');
});

test('settings body scroll lock restores the previous overflow value', async () => {
  const { lockBodyScroll } = await importTsxModule();
  const body = { style: { overflow: 'auto' } };
  const unlock = lockBodyScroll(body);

  assert.equal(body.style.overflow, 'hidden');
  unlock();
  assert.equal(body.style.overflow, 'auto');
});

test('renders exactly the three compact groups and excludes legacy diagnostics', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  const tree = CompactSettingsPanelView(viewProps());
  const markup = renderToStaticMarkup(tree);
  const groups = findElements(tree, (element) => typeof element.props['data-settings-group'] === 'string');

  assert.equal(groups.length, 3);
  assert.deepEqual(groups.map((group) => group.props['data-settings-group']), [
    'appearance-interaction',
    'wallpaper-behavior',
    'sources',
  ]);
  assert.match(markup, /Appearance &amp; interaction/);
  assert.match(markup, /Wallpaper behavior/);
  assert.match(markup, /Sources/);
  assert.match(markup, /role="dialog"/);
  assert.match(markup, /aria-label="Settings"/);
  assert.doesNotMatch(markup, /database|cache ttl|repair|raw config|mpv arguments|runtime stages/i);
});

test('appearance controls update only remembered shell preferences', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  let current = { ...DEFAULT_SHELL_PREFERENCES } as ShellPreferences;
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    preferences: current,
    updatePreferences: (update) => {
      current = typeof update === 'function' ? update(current) : update;
    },
  });
  const [theme] = findElements(tree, (element) => element.props['aria-label'] === 'Theme');
  const [gesture] = findElements(tree, (element) => element.props['aria-label'] === 'Apply gesture');
  const [cardSize] = findElements(tree, (element) => element.props['aria-label'] === 'Card size');
  const [interfaceCard] = findElements(
    tree,
    (element) => element.props['data-settings-card'] === 'interface',
  );

  assert.ok(interfaceCard);
  assert.deepEqual(
    findElements(interfaceCard, (element) => typeof element.props['aria-label'] === 'string')
      .map((control) => control.props['aria-label']),
    ['Theme', 'Apply gesture', 'Card size'],
  );

  (theme.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'dark' } });
  (gesture.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'double' } });
  (cardSize.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'large' } });

  assert.equal(current.theme, 'dark');
  assert.equal(current.applyGesture, 'double');
  assert.equal(current.cardSize, 'large');
  assert.deepEqual(
    findElements(tree, (element) => element.props['aria-label'] === 'Theme')
      .flatMap((select) => findElements(select, (element) => element.type === 'option'))
      .map((option) => option.props.value),
    ['system', 'light', 'dark'],
  );
});

test('wallpaper controls use renderer cards and omit the duplicated display selector', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  let preferences = { ...DEFAULT_SHELL_PREFERENCES } as ShellPreferences;
  let behavior = { ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS } as WallpaperBehaviorSettings;
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    preferences,
    behaviorSettings: behavior,
    updatePreferences: (update) => {
      preferences = typeof update === 'function' ? update(preferences) : update;
    },
    updateBehaviorSettings: (update) => {
      behavior = typeof update === 'function' ? update(behavior) : update;
    },
  });
  const [image] = findElements(tree, (element) => element.props['aria-label'] === 'Image');
  const [gif] = findElements(tree, (element) => element.props['aria-label'] === 'GIF');
  const [fill] = findElements(tree, (element) => element.props['aria-label'] === 'Fill behavior');
  const [transition] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'awww transition type',
  );
  const [duration] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'awww transition duration',
  );
  const [transitionFps] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'awww transition FPS',
  );
  const markup = renderToStaticMarkup(tree);

  assert.deepEqual(
    findElements(tree, (element) => typeof element.props['data-behavior-card'] === 'string')
      .map((card) => card.props['data-behavior-card']),
    ['renderer-selection', 'image-appearance', 'scene-playback', 'session-restore'],
  );
  const [rendererSelection] = findElements(
    tree,
    (element) => element.props['data-behavior-card'] === 'renderer-selection',
  );
  assert.deepEqual(
    findElements(rendererSelection, (element) => ['Image', 'GIF', 'Video'].includes(element.props['aria-label']))
      .map((field) => field.props['aria-label']),
    ['Image', 'GIF', 'Video'],
  );
  assert.deepEqual(
    findElements(tree, (element) => typeof element.props['data-control-unit'] === 'string')
      .map((unit) => unit.props['data-control-unit']),
    ['seconds', 'transition-fps', 'scene-fps', 'scene-volume'],
  );
  assert.ok(
    markup.indexOf('aria-label="Wallpaper Engine volume"')
      < markup.indexOf('aria-label="Mute Wallpaper Engine audio"'),
  );

  assert.equal(findElements(tree, (element) => element.props['aria-label'] === 'Default display target').length, 0);
  const imageMpvpaper = findElements(image, (element) => element.props['data-renderer'] === 'mpvpaper')[0];
  const gifMpvpaper = findElements(gif, (element) => element.props['data-renderer'] === 'mpvpaper')[0];
  (imageMpvpaper.props.onClick as () => void)();
  (gifMpvpaper.props.onClick as () => void)();
  (fill.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'stretch' } });
  (transition.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'wave' } });
  (duration.props.onChange as (event: unknown) => void)({ currentTarget: { value: '2.5' } });
  (transitionFps.props.onChange as (event: unknown) => void)({ currentTarget: { value: '144' } });

  assert.deepEqual(preferences.displayTarget, DEFAULT_SHELL_PREFERENCES.displayTarget);
  assert.deepEqual(behavior, {
    ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
    imageBackend: 'mpvpaper',
    gifBackend: 'mpvpaper',
    fillMode: 'stretch',
    awwwTransitionType: 'wave',
    awwwTransitionDuration: 2.5,
    awwwTransitionFps: 144,
  });
  for (const renderer of [image, gif]) {
    assert.deepEqual(
      findElements(renderer, (element) => typeof element.props['data-renderer'] === 'string')
        .map((option) => option.props['data-renderer']),
      ['awww', 'mpvpaper'],
    );
  }
  const [video] = findElements(tree, (element) => element.props['aria-label'] === 'Video');
  const videoCards = findElements(video, (element) => typeof element.props['data-renderer'] === 'string');
  assert.deepEqual(videoCards.map((card) => card.props['data-renderer']), ['mpvpaper']);
  assert.equal(videoCards[0].props['aria-pressed'], true);
  assert.equal(videoCards[0].props['aria-disabled'], true);
  assert.doesNotMatch(markup, /awww renderer|mpvpaper renderer|Image renderer|GIF renderer|Video renderer/i);
  assert.deepEqual(
    findElements(transition, (element) => element.type === 'option')
      .map((option) => option.props.value),
    ['simple', 'fade', 'left', 'right', 'top', 'bottom', 'wipe', 'grow', 'center', 'outer', 'random', 'wave'],
  );
});

test('renderer diagnostics are hidden while confirmed-missing backend cards stay disabled', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    behaviorSettings: {
      ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
      imageBackend: 'awww',
    },
    rendererStatuses: {
      awww: { available: false, message: 'awww is unavailable.', detail: 'not in PATH' },
      mpvpaper: { available: true, message: 'mpvpaper is installed.' },
      linuxWallpaperEngine: {
        available: false,
        message: 'linux-wallpaperengine is unavailable.',
      },
    },
  });
  const markup = renderToStaticMarkup(tree);
  const [image] = findElements(tree, (element) => element.props['aria-label'] === 'Image');
  const imageOptions = findElements(image, (element) => typeof element.props['data-renderer'] === 'string');
  const [lweScaling] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'Wallpaper Engine scaling',
  );

  assert.equal(imageOptions.find((option) => option.props['data-renderer'] === 'awww')?.props['aria-pressed'], true);
  assert.equal(imageOptions.find((option) => option.props['data-renderer'] === 'awww')?.props.disabled, true);
  assert.equal(imageOptions.find((option) => option.props['data-renderer'] === 'mpvpaper')?.props.disabled, false);
  assert.equal(lweScaling.props.disabled, true);
  assert.doesNotMatch(markup, /installation status|Installed|Unavailable/i);
});

test('unknown backend availability does not disable configuration choices', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    rendererStatuses: null,
  });
  const [image] = findElements(tree, (element) => element.props['aria-label'] === 'Image');
  const options = findElements(image, (element) => typeof element.props['data-renderer'] === 'string');
  const markup = renderToStaticMarkup(tree);

  assert.equal(options.every((option) => option.props.disabled !== true), true);
  assert.doesNotMatch(markup, /installation status|Unknown|probe unavailable/i);
});

test('awww-specific controls are shown only while awww handles images or GIFs', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  const relevant = CompactSettingsPanelView(viewProps());
  const irrelevant = CompactSettingsPanelView({
    ...viewProps(),
    behaviorSettings: {
      ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
      imageBackend: 'mpvpaper',
      gifBackend: 'mpvpaper',
    },
  });

  for (const label of [
    'Fill behavior',
    'awww transition type',
    'awww transition duration',
    'awww transition FPS',
  ]) {
    assert.equal(findElements(relevant, (element) => element.props['aria-label'] === label).length, 1);
    assert.equal(findElements(irrelevant, (element) => element.props['aria-label'] === label).length, 0);
  }
  assert.match(renderToStaticMarkup(irrelevant), /available when awww handles images or GIFs/i);
});

test('renderer-specific options update Wallpaper Engine audio and login restore behavior', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  let behavior = { ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS } as WallpaperBehaviorSettings;
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    behaviorSettings: behavior,
    updateBehaviorSettings: (update) => {
      behavior = typeof update === 'function' ? update(behavior) : update;
    },
  });
  const [scaling] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'Wallpaper Engine scaling',
  );
  const [fps] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'Wallpaper Engine FPS',
  );
  const [mute] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'Mute Wallpaper Engine audio',
  );
  const [volume] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'Wallpaper Engine volume',
  );
  const [restore] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'Restore on login',
  );

  assert.ok(scaling);
  assert.ok(fps);
  assert.ok(mute);
  assert.ok(volume);
  assert.ok(restore);
  assert.equal(volume.props.disabled, false);
  (scaling.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'fit' } });
  (fps.props.onChange as (event: unknown) => void)({ currentTarget: { value: '120' } });
  (mute.props.onChange as (event: unknown) => void)({ currentTarget: { checked: true } });
  (volume.props.onChange as (event: unknown) => void)({ currentTarget: { value: '25' } });
  (restore.props.onChange as (event: unknown) => void)({ currentTarget: { checked: true } });
  assert.deepEqual(behavior, {
    ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
    lweScaling: 'fit',
    lweFps: 120,
    lweMuted: true,
    lweVolume: 25,
    restoreOnLogin: true,
  });

  const mutedTree = CompactSettingsPanelView({
    ...viewProps(),
    behaviorSettings: {
      ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS,
      lweMuted: true,
    },
  });
  const [mutedVolume] = findElements(
    mutedTree,
    (element) => element.props['aria-label'] === 'Wallpaper Engine volume',
  );
  assert.equal(mutedVolume.props.disabled, true);
  const markup = renderToStaticMarkup(tree);
  assert.doesNotMatch(markup, /restore-at-login.*session startup/i);
  assert.doesNotMatch(markup, /Availability is checked when applying/i);
});

test('behavior readiness and errors are explicit without disabling unrelated settings', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    behaviorReady: false,
    loadError: new Error('could not read config'),
    saveError: new Error('could not save config'),
  });
  const markup = renderToStaticMarkup(tree);
  const behaviorControls = findElements(tree, (element) => element.props['data-behavior-control'] === true);
  const [theme] = findElements(tree, (element) => element.props['aria-label'] === 'Theme');

  assert.doesNotMatch(markup, /Loading wallpaper behavior settings/);
  assert.match(markup, /controls are disabled until configuration can be read/);
  assert.match(markup, /could not read config/);
  assert.match(markup, /could not save config/);
  assert.equal(behaviorControls.length, 13);
  assert.equal(behaviorControls.every((control) => control.props.disabled === true), true);
  assert.equal(theme.props.disabled, undefined);
  assert.ok(findElements(tree, (element) => element.props.role === 'alert').length >= 2);
});

test('sources group is a status and one entry point, not duplicated source management', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  let opened = 0;
  let closed = 0;
  const tree = CompactSettingsPanelView({
    ...viewProps(),
    onOpenSources: () => { opened += 1; },
    onClose: () => { closed += 1; },
  });
  const markup = renderToStaticMarkup(tree);
  const [sources] = findElements(tree, (element) => element.props['aria-label'] === 'Manage wallpaper sources');
  const [close] = findElements(tree, (element) => element.props['aria-label'] === 'Close settings');

  assert.match(markup, /4 sources/);
  assert.match(markup, /1 offline/);
  assert.equal(findElements(tree, (element) => element.props['data-source-management-action'] === true).length, 1);
  (sources.props.onClick as () => void)();
  (close.props.onClick as () => void)();
  assert.equal(opened, 1);
  assert.equal(closed, 1);
});
