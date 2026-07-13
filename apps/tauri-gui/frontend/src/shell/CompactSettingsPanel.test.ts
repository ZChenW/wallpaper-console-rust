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
    connectedOutputs: ['eDP-1', 'HDMI-A-1'],
    behaviorSettings: { ...DEFAULT_WALLPAPER_BEHAVIOR_SETTINGS } as WallpaperBehaviorSettings,
    updateBehaviorSettings: (_update: unknown) => undefined,
    behaviorReady: true,
    loadError: null,
    saveError: null,
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

test('wallpaper controls reuse the display selector and expose only compatible renderers', async () => {
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
  const [display] = findElements(tree, (element) => element.props['aria-label'] === 'Default display target');
  const [image] = findElements(tree, (element) => element.props['aria-label'] === 'Image renderer');
  const [gif] = findElements(tree, (element) => element.props['aria-label'] === 'GIF renderer');
  const [fill] = findElements(tree, (element) => element.props['aria-label'] === 'Fill behavior');
  const markup = renderToStaticMarkup(tree);

  assert.ok(display);
  (display.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'output:HDMI-A-1' } });
  (image.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'mpvpaper' } });
  (gif.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'mpvpaper' } });
  (fill.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'stretch' } });

  assert.deepEqual(preferences.displayTarget, { kind: 'output', output: 'HDMI-A-1' });
  assert.deepEqual(behavior, {
    imageBackend: 'mpvpaper',
    gifBackend: 'mpvpaper',
    videoBackend: 'mpvpaper',
    fillMode: 'stretch',
  });
  for (const renderer of [image, gif]) {
    assert.deepEqual(
      findElements(renderer, (element) => element.type === 'option').map((option) => option.props.value),
      ['awww', 'mpvpaper'],
    );
  }
  assert.match(markup, /Video renderer/);
  assert.match(markup, /mpvpaper/);
  assert.doesNotMatch(markup, /installed/i);
  assert.equal(findElements(tree, (element) => element.props['aria-label'] === 'Video renderer').length, 0);
});

test('awww fill control is shown only while awww handles images or GIFs', async () => {
  const { CompactSettingsPanelView } = await importTsxModule();
  const relevant = CompactSettingsPanelView(viewProps());
  const irrelevant = CompactSettingsPanelView({
    ...viewProps(),
    behaviorSettings: {
      imageBackend: 'mpvpaper',
      gifBackend: 'mpvpaper',
      videoBackend: 'mpvpaper',
      fillMode: 'crop',
    },
  });

  assert.equal(findElements(relevant, (element) => element.props['aria-label'] === 'Fill behavior').length, 1);
  assert.equal(findElements(irrelevant, (element) => element.props['aria-label'] === 'Fill behavior').length, 0);
  assert.match(renderToStaticMarkup(irrelevant), /available when awww handles images or GIFs/i);
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

  assert.match(markup, /Loading wallpaper behavior settings/);
  assert.match(markup, /could not read config/);
  assert.match(markup, /could not save config/);
  assert.equal(behaviorControls.length, 3);
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
