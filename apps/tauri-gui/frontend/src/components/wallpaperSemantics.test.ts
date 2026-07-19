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
  assert.match(card, /role="listitem"/);
  assert.match(card, /className="wallpaper-card__primary"/);
  assert.match(card, /aria-busy=\{applying \|\| undefined\}/);
  assert.doesNotMatch(card, /aria-selected=/);
  assert.match(card, /wallpaper-card__state/);
  assert.match(card, /Currently applied/);
  assert.match(card, /Pending apply/);
  assert.match(css, /\.wallpaper-card\.applying\s*\{/);
  assert.match(css, /\.wallpaper-card\.pending\s*\{/);
});
