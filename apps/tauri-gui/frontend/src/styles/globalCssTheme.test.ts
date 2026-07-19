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

test('search focus uses a hairline micro-glow without losing forced-colors visibility', async () => {
  const { css } = await sources();
  const wrapperFocus = ruleBody(css, '.single-page-search:focus-within');
  const inputFocus = ruleBody(
    css,
    ':root[data-theme="glass"] .single-page-search input[type="search"]:focus-visible',
  );
  const forced = atRuleBody(css, '@media (forced-colors: active)');
  const forcedInputFocus = ruleBody(
    forced,
    ':root[data-theme="glass"] .single-page-search input[type="search"]:focus-visible',
  );

  assert.match(
    wrapperFocus,
    /border-color:\s*color-mix\(in srgb, var\(--primary\) 50%, var\(--border\)\)/,
  );
  assert.match(
    wrapperFocus,
    /0 0 0\.75rem color-mix\(in srgb, var\(--primary\) 9%, transparent\)/,
  );
  assert.doesNotMatch(wrapperFocus, /0 0 0 0\.18rem/);
  assert.match(inputFocus, /outline:\s*none/);
  assert.match(forcedInputFocus, /outline:\s*2px solid ButtonText/);
  assert.match(forcedInputFocus, /outline-offset:\s*2px/);
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

test('shell stays hidden only while its first resolved theme is unavailable', async () => {
  const { css } = await sources();
  const pendingTheme = ruleBody(css, ':root:not([data-theme]) #root');

  assert.match(pendingTheme, /visibility:\s*hidden/);
  assert.doesNotMatch(css, /:root\[data-theme\] #root\s*\{[^}]*visibility:\s*hidden/s);
});

test('Flow keeps scrolling in the central stream and reserves independent side rails', async () => {
  const { css } = await sources();
  const viewport = ruleBody(css, '.library-viewport');
  const flow = ruleBody(css, '.wallpaper-flow');
  const stream = ruleBody(css, '.flow-preview-stream');
  const item = ruleBody(css, '.flow-preview-item');
  const media = ruleBody(css, '.flow-preview-item__media');

  assert.match(viewport, /container-name:\s*library-viewport/);
  assert.match(viewport, /container-type:\s*inline-size/);
  assert.match(viewport, /min-height:\s*0/);
  assert.match(flow, /display:\s*grid/);
  assert.match(flow, /grid-template-columns:\s*minmax\(/);
  assert.match(flow, /overflow:\s*hidden/);
  assert.match(stream, /overflow-y:\s*auto/);
  assert.match(stream, /overscroll-behavior(?:-y)?:\s*contain/);
  assert.match(stream, /scrollbar-gutter:\s*stable/);
  assert.match(item, /place-items:\s*center/);
  assert.match(item, /contain:\s*layout paint/);
  assert.match(media, /aspect-ratio:\s*var\(--flow-media-aspect\)/);
  assert.match(media, /max-height:/);
  assert.match(ruleBody(css, '.flow-metadata-rail'), /overflow:\s*hidden/);
});

test('Flow exposes additive visual states without making hover equivalent to selection', async () => {
  const { css } = await sources();
  const centered = ruleBody(css, '.flow-preview-item[data-centered]');
  const centeredMedia = ruleBody(css, '.flow-preview-item[data-centered] .flow-preview-item__media');
  const hovered = ruleBody(css, '.flow-preview-item[data-hovered]');
  const selected = ruleBody(css, '.flow-preview-item[data-selected] .flow-preview-item__media');
  const current = ruleBody(css, '.flow-preview-item[data-current] .flow-preview-item__media::before');
  const applying = ruleBody(css, '.flow-preview-item[data-applying] .flow-preview-item__media::after');
  const surroundingImage = ruleBody(
    css,
    '.flow-preview-item:not([data-centered]) .flow-preview-item__media > img',
  );
  const centeredImage = ruleBody(
    css,
    '.flow-preview-item[data-centered] .flow-preview-item__media > img',
  );
  const metadata = ruleBody(css, '.flow-metadata-rail');
  const hoveredMetadata = ruleBody(css, '.flow-metadata-rail[data-hovered]');

  assert.match(centered, /opacity:\s*1/);
  assert.match(centeredMedia, /transform:\s*scale\(/);
  assert.match(hovered, /opacity:/);
  assert.doesNotMatch(hovered, /outline|box-shadow/);
  assert.match(selected, /outline:/);
  assert.match(current, /content:\s*['"]['"]/);
  assert.match(applying, /animation:/);
  assert.match(surroundingImage, /filter:\s*grayscale\(/);
  assert.match(centeredImage, /filter:\s*none/);
  assert.match(metadata, /animation:\s*flow-metadata-enter 180ms/);
  assert.match(hoveredMetadata, /box-shadow:\s*inset/);
  assert.match(ruleBody(css, '.flow-index-rail__states'), /display:\s*flex/);
});

test('Flow suppresses decorative transitions while native scrolling is unsettled', async () => {
  const { css } = await sources();
  const item = ruleBody(css, '.flow-preview-item:not([data-settled])');
  const media = ruleBody(
    css,
    '.flow-preview-item:not([data-settled]) .flow-preview-item__media',
  );

  assert.match(item, /transition:\s*none/);
  assert.match(media, /transition:\s*none/);
});

test('Flow progressively collapses at container breakpoints and preserves in-layout actions', async () => {
  const { css } = await sources();
  const medium = atRuleBody(css, '@container library-viewport (max-width: 1023px)');
  const narrow = atRuleBody(css, '@container library-viewport (max-width: 759px)');
  const compact = atRuleBody(css, '@container library-viewport (max-width: 420px)');
  const compactViewport = atRuleBody(css, '@media (max-width: 760px)');
  const shortCompact = atRuleBody(
    css,
    '@media (max-width: 420px) and (max-height: 640px)',
  );

  assert.match(medium, /\.wallpaper-flow/);
  assert.match(medium, /grid-template-columns:/);
  assert.match(medium, /data-flow-metadata-priority="secondary"/);
  assert.match(medium, /display:\s*none/);
  assert.match(narrow, /grid-template-rows:\s*auto minmax\(0, 1fr\) auto/);
  assert.match(narrow, /\.flow-index-rail__list/);
  assert.match(narrow, /display:\s*none/);
  assert.match(narrow, /\.flow-metadata-rail/);
  assert.match(narrow, /position:\s*relative/);
  assert.match(narrow, /overflow-x:\s*visible/);
  assert.doesNotMatch(narrow, /overflow-y:\s*auto/);
  assert.match(compact, /\.flow-metadata-rail__actions/);
  assert.match(compact, /grid-template-columns:\s*repeat\(2, minmax\(0, 1fr\)\)/);
  assert.match(compact, /\.flow-metadata-rail__action--primary/);
  assert.match(compact, /grid-column:\s*1 \/ -1/);
  assert.match(compactViewport, /\.single-page-shell:has\(\.wallpaper-flow\).*\.feedback-overlay/);
  assert.match(compactViewport, /position:\s*relative\s*!important/);
  assert.match(compactViewport, /\.single-page-statusbar/);
  assert.match(compactViewport, /display:\s*none/);
  assert.match(shortCompact, /data-flow-metadata-field="compatibility"/);
  assert.match(shortCompact, /\.flow-metadata-rail__actions/);
  assert.match(shortCompact, /grid-template-columns:\s*repeat\(3, minmax\(0, 1fr\)\)/);
  assert.match(shortCompact, /\.flow-metadata-rail__return/);
  assert.match(shortCompact, /position:\s*absolute/);
});

test('Flow accessibility fallbacks cover reduced motion, forced colors, and coarse pointers', async () => {
  const { css } = await sources();
  const reduced = atRuleBody(css, '@media (prefers-reduced-motion: reduce)');
  const reducedMedia = ruleBody(reduced, '.library-viewport :is(.flow-preview-item__media)');
  const forced = atRuleBody(css, '@media (forced-colors: active)');
  const coarse = atRuleBody(css, '@media (pointer: coarse)');

  assert.match(reduced, /\.flow-preview-stream/);
  assert.match(reduced, /scroll-behavior:\s*auto\s*!important/);
  assert.match(reduced, /\.flow-preview-item/);
  assert.match(reduced, /transition:\s*none\s*!important/);
  assert.match(reducedMedia, /transform:\s*none\s*!important/);
  assert.match(forced, /\.flow-preview-item\[data-selected\]/);
  assert.match(forced, /Highlight/);
  assert.match(forced, /\.flow-preview-stream:focus-visible/);
  assert.match(forced, /outline:\s*2px solid Highlight/);
  assert.match(coarse, /\.flow-metadata-rail__action/);
  assert.match(coarse, /min-height:\s*44px/);
});

test('short compact Flow reserves the stream and bounds concurrent transient status', async () => {
  const { css } = await sources();
  const shortCompact = atRuleBody(
    css,
    '@media (max-width: 420px) and (max-height: 640px)',
  );
  const shortStatus = atRuleBody(
    css,
    '@media (max-width: 760px) and (max-height: 480px)',
  );
  const concurrentShell = ':is(.single-page-shell:has(.wallpaper-flow):has(> .scan-activity):has(> .feedback-overlay))';
  const flow = ruleBody(shortCompact, `${concurrentShell} .wallpaper-flow`);
  const metadata = ruleBody(shortCompact, `${concurrentShell} .flow-metadata-rail`);
  const secondary = ruleBody(
    shortCompact,
    `${concurrentShell} :is(.flow-metadata-rail__status-list, .flow-metadata-rail__metadata, .flow-metadata-rail__queue, .flow-metadata-rail__completion)`,
  );
  const actions = ruleBody(shortCompact, `${concurrentShell} .flow-metadata-rail__actions`);
  const action = ruleBody(shortCompact, `${concurrentShell} .flow-metadata-rail__action`);
  const returnToTop = ruleBody(shortCompact, `${concurrentShell} .flow-metadata-rail__return`);

  assert.match(flow, /grid-template-rows:\s*auto minmax\(6rem, 1fr\) auto/);
  assert.match(metadata, /grid-template-areas:\s*["']header actions return["']/);
  assert.match(secondary, /display:\s*none/);
  assert.match(actions, /grid-template-columns:\s*repeat\(3, minmax\(0, 1fr\)\)/);
  assert.match(action, /white-space:\s*nowrap/);
  assert.match(action, /text-overflow:\s*ellipsis/);
  assert.match(returnToTop, /position:\s*static/);
  assert.match(shortStatus, /grid-template-rows:\s*auto auto minmax\(0, 1fr\) 4\.75rem/);
  assert.match(shortStatus, /grid-template-columns:\s*minmax\(8rem, 2fr\) minmax\(0, 3fr\)/);
  assert.match(shortStatus, /> \.feedback-overlay\s*\{[^}]*max-height:\s*calc\(4\.75rem - 0\.5rem\)/s);
  assert.match(shortStatus, /grid-auto-rows:\s*max-content/);
  assert.match(shortStatus, /overflow-y:\s*auto/);
});
