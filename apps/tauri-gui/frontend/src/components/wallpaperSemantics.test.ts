import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('wallpaper collection uses actionable list items and distinct apply states', async () => {
  const [grid, card, css] = await Promise.all([
    readFile(new URL('./WallpaperGrid.tsx', import.meta.url), 'utf8'),
    readFile(new URL('./WallpaperCard.tsx', import.meta.url), 'utf8'),
    readFile(new URL('../styles/global.css', import.meta.url), 'utf8'),
  ]);

  assert.match(grid, /role="list"/);
  assert.doesNotMatch(grid, /aria-setsize=\{entries\.length\}/);
  assert.match(grid, /setSize=\{setSize\}/);
  assert.match(card, /role="listitem"/);
  assert.match(card, /aria-setsize=\{setSize\}/);
  assert.match(card, /className="wallpaper-card__primary"/);
  assert.match(card, /aria-busy=\{applying \|\| undefined\}/);
  assert.doesNotMatch(card, /aria-selected=/);
  assert.match(card, /wallpaper-card__state/);
  assert.match(card, /Currently applied/);
  assert.match(card, /Pending apply/);
  assert.match(css, /\.wallpaper-card\.applying\s*\{/);
  assert.match(css, /\.wallpaper-card\.pending\s*\{/);
});

test('known Library total, not loaded entry count, owns collection set size', async () => {
  const [viewport, flow] = await Promise.all([
    readFile(new URL('./LibraryViewport.tsx', import.meta.url), 'utf8'),
    readFile(new URL('./WallpaperFlow.tsx', import.meta.url), 'utf8'),
  ]);

  assert.match(
    viewport,
    /setSize=\{model\.totalKnown && model\.total !== null \? model\.total : undefined\}/,
  );
  assert.match(
    flow,
    /aria-setsize=\{model\.totalKnown && model\.total !== null \? model\.total : undefined\}/,
  );
});

test('criteria replacement is announced and marks the library viewport busy', async () => {
  const viewport = await readFile(new URL('./LibraryViewport.tsx', import.meta.url), 'utf8');

  assert.match(viewport, /aria-busy=\{model\.queryReplacementPending \|\| undefined\}/);
  assert.match(viewport, /role="status"/);
  assert.match(viewport, /Updating library results/);
});

test('display discovery failures are visible and retryable', async () => {
  const shell = await readFile(new URL('../shell/SinglePageShell.tsx', import.meta.url), 'utf8');

  assert.match(shell, /Display detection failed/);
  assert.match(shell, /Retry display detection/);
  assert.match(shell, /catalog\.reloadDisplays\(\)/);
});
