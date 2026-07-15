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

/** Collect every rule whose selector is under the glass theme scope. */
function glassScopedRules(css: string): Array<{ selector: string; body: string }> {
  const rules: Array<{ selector: string; body: string }> = [];
  for (const match of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = match[1].trim();
    if (selector.includes('[data-theme="glass"]') || selector.includes("[data-theme='glass']")) {
      rules.push({ selector, body: match[2] });
    }
  }
  return rules;
}

test('liquid glass theme provides iOS blue tokens and two static aurora glows', async () => {
  const { css } = await sources();
  const theme = ruleBody(css, ':root[data-theme="glass"]');
  const shell = ruleBody(css, ':root[data-theme="glass"] .single-page-shell');
  const body = ruleBody(css, 'body');

  assert.match(theme, /color-scheme:\s*dark/);
  assert.match(theme, /--bg:\s*#0d0d12/i);
  assert.match(theme, /--primary:\s*#0071e3/i);
  assert.match(theme, /--primary-strong:\s*#0a84ff/i);
  assert.match(theme, /--primary-foreground:\s*#ffffff/i);
  assert.match(theme, /--panel:\s*rgb\(255 255 255 \/ 9%\)/);
  assert.match(theme, /--text:\s*#f5f5f7/i);
  assert.match(theme, /--wallpaper-details-background:\s*rgb\(28 28 34 \/ 62%\)/);
  assert.match(theme, /--feedback-card-background:\s*rgb\(28 28 34 \/ 62%\)/);
  assert.match(theme, /--feedback-card-backdrop-filter:\s*blur\(24px\) saturate\(180%\)/);
  assert.ok((css.match(/--primary-foreground:/g) ?? []).length >= 3, 'all theme groups need a primary foreground');
  assert.match(css, /\.btn\.primary\s*\{[^}]*color:\s*var\(--primary-foreground,\s*#fff\)/);
  assert.doesNotMatch(theme, /--font-dashboard/);
  assert.match(body, /font-family:\s*-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif/);
  assert.doesNotMatch(body, /font-dashboard|dashboard-font|monospace/);

  for (const { selector, body: rule } of glassScopedRules(css)) {
    assert.doesNotMatch(
      rule,
      /(?:^|[;\s])font-family\s*:/,
      `glass scope must not override font-family in ${selector}`,
    );
  }

  assert.equal((shell.match(/radial-gradient\(/g) ?? []).length, 2);
  assert.match(shell, /rgb\(10 132 255 \/ 14%\)/);
  assert.match(shell, /rgb\(191 90 242 \/ 10%\)/);
  assert.doesNotMatch(shell, /animation|transition/);
});

test('liquid glass blur is limited to the topbar and approved floating surfaces', async () => {
  const { css } = await sources();
  const topbar = ruleBody(css, ':root[data-theme="glass"] .single-page-topbar');
  const floating = ruleBody(
    css,
    ':root[data-theme="glass"] :is(.settings-panel, .source-panel, .select-field-content, .context-menu, .wallpaper-details, .feedback-overlay__card)',
  );
  const feedbackCard = ruleBody(css, '.feedback-overlay__card');
  const card = ruleBody(css, ':root[data-theme="glass"] .wallpaper-card');

  assert.match(topbar, /backdrop-filter:\s*blur\(20px\) saturate\(180%\)/);
  assert.match(topbar, /-webkit-backdrop-filter:\s*blur\(20px\) saturate\(180%\)/);
  assert.match(topbar, /inset 0 1px 0 rgb\(255 255 255 \/ 22%\)/);
  assert.match(floating, /backdrop-filter:\s*blur\(24px\) saturate\(180%\)/);
  assert.match(floating, /-webkit-backdrop-filter:\s*blur\(24px\) saturate\(180%\)/);
  assert.match(floating, /inset 0 1px 0 rgb\(255 255 255 \/ 22%\)/);
  assert.match(floating, /saturate\(180%\)/);
  assert.match(feedbackCard, /backdrop-filter:\s*var\(--feedback-card-backdrop-filter\)/);
  assert.match(feedbackCard, /-webkit-backdrop-filter:\s*var\(--feedback-card-backdrop-filter\)/);

  const rules = css.matchAll(/([^{}]+)\{([^{}]*)\}/g);
  for (const [, selector, body] of rules) {
    if (!selector.includes('.wallpaper-card')) continue;
    assert.doesNotMatch(body, /(?:^|[;\s])(?:-webkit-)?backdrop-filter\s*:/, `card blur in ${selector.trim()}`);
  }
  assert.match(card, /background:\s*rgb\(255 255 255 \/ 6%\)/);
  assert.match(card, /border-radius:\s*1rem/);
});

test('Liquid Glass accessibility modes remove transparency and decorative effects', async () => {
  const { css } = await sources();
  const theme = ruleBody(css, ':root[data-theme="glass"]');
  const reduced = atRuleBody(css, '@media (prefers-reduced-transparency: reduce)');
  const reducedTheme = ruleBody(reduced, ':root[data-theme="glass"]');
  const reducedShell = ruleBody(reduced, ':root[data-theme="glass"] .single-page-shell');
  const reducedSurfaces = ruleBody(
    reduced,
    ':root[data-theme="glass"] :is(.single-page-topbar, .settings-panel, .source-panel, .select-field-content, .context-menu, .wallpaper-details, .feedback-overlay__card)',
  );
  const forced = atRuleBody(css, '@media (forced-colors: active)');
  const forcedTheme = ruleBody(forced, ':root[data-theme="glass"]');
  const forcedPrimaryButton = ruleBody(forced, ':root[data-theme="glass"] .btn.primary');

  assert.match(theme, /--source-card-background:\s*var\(--surface-muted\)/);
  assert.match(reducedTheme, /--surface-muted:\s*#1c1c22/);
  assert.match(reducedShell, /background:\s*var\(--bg\)/);
  assert.doesNotMatch(reducedShell, /gradient/);
  assert.match(reducedSurfaces, /background:\s*#1c1c22/);
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

test('Liquid Glass keyboard focus uses a visible iOS-blue ring with separation', async () => {
  const { css } = await sources();
  const focus = ruleBody(
    css,
    ':root[data-theme="glass"] :is(button, input, select, [role="button"], [role="menuitem"], [tabindex]:not([tabindex="-1"])):focus-visible',
  );

  assert.match(focus, /outline:\s*2px solid var\(--primary\)/);
  assert.match(focus, /outline-offset:\s*2px/);
});

test('liquid glass has an opaque fallback when backdrop filtering is unavailable', async () => {
  const { css } = await sources();

  assert.match(css, /@supports\s+not\s+\(\(backdrop-filter:/);
  assert.match(css, /background:\s*#1c1c22/);
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
