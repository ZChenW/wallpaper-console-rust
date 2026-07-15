import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const globalCssUrl = new URL('./global.css', import.meta.url);
const detailsUrl = new URL('../shell/WallpaperDetailsDialog.tsx', import.meta.url);
const feedbackUrl = new URL('../shell/FeedbackOverlay.tsx', import.meta.url);

async function sources() {
  const [css, details, feedback] = await Promise.all([
    readFile(globalCssUrl, 'utf8'),
    readFile(detailsUrl, 'utf8'),
    readFile(feedbackUrl, 'utf8'),
  ]);
  return { css, details, feedback };
}

function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`);
  assert.notEqual(start, -1, `missing CSS rule: ${selector}`);
  const bodyStart = css.indexOf('{', start) + 1;
  const end = css.indexOf('}', bodyStart);
  return css.slice(bodyStart, end);
}

function atRuleBody(css: string, atRule: string): string {
  const start = css.indexOf(atRule);
  assert.notEqual(start, -1, `missing CSS at-rule: ${atRule}`);
  const bodyStart = css.indexOf('{', start) + 1;
  let depth = 1;
  for (let index = bodyStart; index < css.length; index += 1) {
    if (css[index] === '{') depth += 1;
    if (css[index] === '}') depth -= 1;
    if (depth === 0) return css.slice(bodyStart, index);
  }
  assert.fail(`unterminated CSS at-rule: ${atRule}`);
}

test('glass theme provides restrained dashboard tokens and two static glows', async () => {
  const { css } = await sources();
  const theme = ruleBody(css, ':root[data-theme="glass"]');
  const shell = ruleBody(css, ':root[data-theme="glass"] .single-page-shell');
  const body = ruleBody(css, 'body');
  const dashboardFontDeclaration = 'font-family: var(--font-dashboard);';
  const declarationStart = css.indexOf(dashboardFontDeclaration);

  assert.notEqual(declarationStart, -1, 'missing scoped dashboard font declaration');
  const selectorStart = css.lastIndexOf('}', declarationStart) + 1;
  const selectorEnd = css.lastIndexOf('{', declarationStart);
  const dashboardSelector = css.slice(selectorStart, selectorEnd);

  assert.match(theme, /color-scheme:\s*dark/);
  assert.match(theme, /--bg:\s*#[0-9a-f]{6}/i);
  assert.match(theme, /--primary:\s*#[0-9a-f]{6}/i);
  const foreground = theme.match(/--primary-foreground:\s*#([0-9a-f]{6})/i)?.[1];
  assert.ok(foreground, 'Glass theme needs a primary foreground token');
  const foregroundChannels = foreground.match(/../g)?.map((channel) => Number.parseInt(channel, 16)) ?? [];
  assert.ok(foregroundChannels.every((channel) => channel < 64), 'Glass primary foreground must be dark');
  assert.ok((css.match(/--primary-foreground:/g) ?? []).length >= 3, 'all theme groups need a primary foreground');
  assert.match(css, /\.btn\.primary\s*\{[^}]*color:\s*var\(--primary-foreground,\s*#fff\)/);
  assert.match(theme, /--font-dashboard:\s*'JetBrains Mono',\s*'IBM Plex Mono',\s*ui-monospace/);
  assert.match(body, /font-family:\s*-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif/);
  assert.doesNotMatch(body, /font-dashboard|dashboard-font|monospace/);
  for (const selector of [
    '.single-page-brand',
    '.single-page-topbar',
    '.single-page-filters',
    '.single-page-statusbar',
    '.wallpaper-name',
    '.wallpaper-meta',
    '.wallpaper-badge',
    '.btn',
    '.select-field-trigger',
    '.select-field-item',
    '.context-menu button',
    '.single-page-favorite-filter',
    '.settings-behavior-category',
    '.settings-behavior-row',
    '.settings-renderer-card',
    '.settings-number-control :is(input, [data-control-unit])',
    '.settings-navigation-card__copy strong',
    '.wallpaper-details :is(dt, dd)',
  ]) {
    assert.ok(dashboardSelector.includes(selector), `dashboard font selector missing ${selector}`);
  }
  assert.doesNotMatch(
    dashboardSelector,
    /(?:^|[,\s(])(?:body|p|details)(?=[,\s)>])|settings-error|feedback-overlay__card/,
  );
  assert.equal((shell.match(/radial-gradient\(/g) ?? []).length, 2);
  assert.doesNotMatch(shell, /animation|transition/);
});

test('glass blur is limited to the topbar and approved floating surfaces', async () => {
  const { css } = await sources();
  const topbar = ruleBody(css, ':root[data-theme="glass"] .single-page-topbar');
  const floating = ruleBody(css, ':root[data-theme="glass"] :is(.settings-panel, .source-panel, .select-field-content, .context-menu, .wallpaper-details, .feedback-overlay__card)');
  const feedbackCard = ruleBody(css, '.feedback-overlay__card');

  assert.match(topbar, /backdrop-filter:\s*blur\(16px\)/);
  assert.match(topbar, /-webkit-backdrop-filter:\s*blur\(16px\)/);
  assert.match(floating, /backdrop-filter:\s*blur\(18px\)/);
  assert.match(floating, /-webkit-backdrop-filter:\s*blur\(18px\)/);
  assert.match(feedbackCard, /backdrop-filter:\s*var\(--feedback-card-backdrop-filter\)/);
  assert.match(feedbackCard, /-webkit-backdrop-filter:\s*var\(--feedback-card-backdrop-filter\)/);

  const rules = css.matchAll(/([^{}]+)\{([^{}]*)\}/g);
  for (const [, selector, body] of rules) {
    if (!selector.includes('.wallpaper-card')) continue;
    assert.doesNotMatch(body, /(?:^|[;\s])(?:-webkit-)?backdrop-filter\s*:/, `card blur in ${selector.trim()}`);
  }
  assert.match(ruleBody(css, ':root[data-theme="glass"] .wallpaper-card'), /background:\s*rgba?\(/);
});

test('Glass accessibility modes remove transparency and decorative effects', async () => {
  const { css } = await sources();
  const theme = ruleBody(css, ':root[data-theme="glass"]');
  const reduced = atRuleBody(css, '@media (prefers-reduced-transparency: reduce)');
  const reducedTheme = ruleBody(reduced, ':root[data-theme="glass"]');
  const reducedShell = ruleBody(reduced, ':root[data-theme="glass"] .single-page-shell');
  const reducedSurfaces = ruleBody(reduced, ':root[data-theme="glass"] :is(.single-page-topbar, .settings-panel, .source-panel, .select-field-content, .context-menu, .wallpaper-details, .feedback-overlay__card)');
  const forced = atRuleBody(css, '@media (forced-colors: active)');
  const forcedTheme = ruleBody(forced, ':root[data-theme="glass"]');
  const forcedPrimaryButton = ruleBody(forced, ':root[data-theme="glass"] .btn.primary');

  assert.match(theme, /--source-card-background:\s*var\(--surface-muted\)/);
  assert.match(reducedTheme, /--surface-muted:\s*#13232f/);
  assert.match(reducedShell, /background:\s*var\(--bg\)/);
  assert.doesNotMatch(reducedShell, /gradient/);
  assert.match(reducedSurfaces, /background:\s*#13232f/);
  assert.match(reducedSurfaces, /backdrop-filter:\s*none/);
  assert.match(reducedSurfaces, /-webkit-backdrop-filter:\s*none/);
  assert.match(forcedTheme, /--surface-muted:\s*Canvas/);

  for (const systemColour of ['Canvas', 'CanvasText', 'ButtonText']) {
    assert.match(forced, new RegExp(`\\b${systemColour}\\b`));
  }
  assert.doesNotMatch(forced, /gradient/);
  assert.match(forced, /box-shadow:\s*none/);
  assert.match(forced, /backdrop-filter:\s*none/);
  assert.match(forced, /-webkit-backdrop-filter:\s*none/);
  assert.match(forced, /\.wallpaper-card/);
  assert.match(forcedPrimaryButton, /background:\s*ButtonText/);
  assert.match(forcedPrimaryButton, /color:\s*Canvas/);
});

test('Glass keyboard focus uses a visible cyan ring with separation', async () => {
  const { css } = await sources();
  const focus = ruleBody(
    css,
    ':root[data-theme="glass"] :is(button, input, select, [role="button"], [role="menuitem"], [tabindex]:not([tabindex="-1"])):focus-visible',
  );

  assert.match(focus, /outline:\s*2px solid var\(--primary\)/);
  assert.match(focus, /outline-offset:\s*2px/);
});

test('glass has an opaque fallback when backdrop filtering is unavailable', async () => {
  const { css } = await sources();

  assert.match(css, /@supports\s+not\s+\(\(backdrop-filter:/);
  assert.match(css, /background:\s*#13232f/);
});

test('dialog and feedback skin lives in CSS while inline layout remains', async () => {
  const { css, details, feedback } = await sources();
  const dialog = ruleBody(css, '.wallpaper-details');
  const card = ruleBody(css, '.feedback-overlay__card');
  const dialogInline = details.match(/const dialogStyle:[\s\S]*?\n};/)?.[0] ?? '';
  const cardInline = feedback.match(/const cardStyle:[\s\S]*?\n};/)?.[0] ?? '';

  for (const property of ['border:', 'background:', 'color:', 'box-shadow:']) {
    assert.match(dialog, new RegExp(property));
    assert.match(card, new RegExp(property));
  }
  assert.match(card, /backdrop-filter:/);
  assert.doesNotMatch(dialogInline, /\b(?:border|background|color|boxShadow):/);
  assert.doesNotMatch(cardInline, /\b(?:border|background|color|boxShadow|backdropFilter):/);
  assert.match(dialogInline, /display:\s*'grid'/);
  assert.match(cardInline, /position:\s*'relative'/);
});
