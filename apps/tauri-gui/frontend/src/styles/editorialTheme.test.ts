import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const cssUrl = new URL('./editorialTheme.css', import.meta.url);
const mainUrl = new URL('../main.tsx', import.meta.url);

async function readCss(): Promise<string> {
  try {
    return await readFile(cssUrl, 'utf8');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return '';
    throw error;
  }
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

test('Editorial CSS is independently scoped and loaded after the shared theme', async () => {
  const [css, main] = await Promise.all([readCss(), readFile(mainUrl, 'utf8')]);

  assert.match(css, /:root\[data-theme=["']editorial["']\]\s*\{/);
  assert.doesNotMatch(
    css,
    /(?:^|\n)\s*\.(?:single-page|wallpaper|settings|source|select-field|context-menu|feedback|scan-activity|library)/,
    'all structural selectors must remain under the Editorial theme root',
  );

  const globalImport = main.indexOf("import './styles/global.css';");
  const editorialImport = main.indexOf("import './styles/editorialTheme.css';");
  assert.ok(globalImport >= 0, 'the shared stylesheet must remain imported');
  assert.ok(editorialImport > globalImport, 'Editorial CSS must load after the shared stylesheet');
});

test('Editorial CSS defines the high-contrast modernist surface and type system', async () => {
  const css = await readCss();
  const theme = ruleBody(css, ':root[data-theme="editorial"]');
  const shell = ruleBody(css, ':root[data-theme="editorial"] .single-page-shell');
  const brand = ruleBody(css, ':root[data-theme="editorial"] .single-page-brand');
  const cards = ruleBody(
    css,
    ':root[data-theme="editorial"] :is(.wallpaper-card, .settings-panel, .source-panel, .select-field-content, .context-menu, .wallpaper-details, .feedback-overlay__card)',
  );

  assert.match(theme, /color-scheme:\s*light/);
  assert.match(theme, /--bg:\s*#f4f4f0/i);
  assert.match(theme, /--text:\s*#0a0a0a/i);
  assert.match(theme, /--editorial-ease-out:\s*cubic-bezier\(\.16,\s*1,\s*\.3,\s*1\)/);
  assert.match(theme, /--editorial-ease-settle:\s*cubic-bezier\(\.19,\s*1,\s*\.22,\s*1\)/);
  assert.match(shell, /background:\s*var\(--bg\)/);
  assert.doesNotMatch(shell, /gradient|shadow|blur/);
  assert.match(brand, /font-size:\s*clamp\(/);
  assert.match(brand, /letter-spacing:\s*-0\.0[4-9]em/);
  assert.match(brand, /text-transform:\s*uppercase/);
  assert.match(cards, /border-radius:\s*0/);
  assert.match(cards, /box-shadow:\s*none/);
  assert.doesNotMatch(css, /(?<![-\w])(?:-webkit-)?backdrop-filter\s*:/);
  assert.doesNotMatch(css, /@font-face|url\s*\(|cursor:\s*(?:none|url)/i);
});

test('Editorial CSS makes wallpaper media numbered, monochrome at rest, and structurally active', async () => {
  const css = await readCss();
  const card = ruleBody(css, ':root[data-theme="editorial"] .wallpaper-card');
  const image = ruleBody(css, ':root[data-theme="editorial"] .wallpaper-thumb img');
  const activeImage = ruleBody(
    css,
    ':root[data-theme="editorial"] .wallpaper-card:is(:hover, :focus-visible, :focus-within, .selected, .current) .wallpaper-thumb img',
  );
  const selected = ruleBody(
    css,
    ':root[data-theme="editorial"] .wallpaper-card:is(.selected, .current)',
  );
  const reveal = ruleBody(css, ':root[data-theme="editorial"] .wallpaper-card::after');
  const index = ruleBody(css, ':root[data-theme="editorial"] .wallpaper-index');
  const selectedInfo = ruleBody(
    css,
    ':root[data-theme="editorial"] .wallpaper-card.selected .wallpaper-info',
  );
  const currentLabel = ruleBody(
    css,
    ':root[data-theme="editorial"] .wallpaper-card.current .wallpaper-info::after',
  );

  assert.match(card, /border-radius:\s*0/);
  assert.doesNotMatch(card, /(?:height|min-height|max-height|padding|margin)\s*:/);
  assert.match(image, /filter:\s*grayscale\(1\)\s+contrast\(/);
  assert.match(image, /opacity:\s*0\.[6-9]/);
  assert.match(activeImage, /filter:\s*grayscale\(0\)\s+contrast\(1\)/);
  assert.match(activeImage, /opacity:\s*1/);
  assert.match(selected, /border-color:\s*var\(--text\)/);
  assert.match(selected, /box-shadow:\s*inset 0 0 0 1px var\(--text\)/);
  assert.match(reveal, /content:\s*attr\(data-editorial-action\)/);
  assert.match(reveal, /background:\s*var\(--text\)/);
  assert.match(reveal, /color:\s*var\(--bg\)/);
  assert.match(index, /font-variant-numeric:\s*tabular-nums/);
  assert.match(selectedInfo, /background:\s*var\(--text\)/);
  assert.match(selectedInfo, /color:\s*var\(--bg\)/);
  assert.match(currentLabel, /content:\s*["']current["']/i);
});

test('Editorial CSS has restrained progressive motion and complete accessibility fallbacks', async () => {
  const css = await readCss();
  const focus = ruleBody(
    css,
    ':root[data-theme="editorial"] :is(button, input, select, [role="button"], [role="menuitem"], [tabindex]:not([tabindex="-1"])):focus-visible',
  );
  const settings = ruleBody(css, ':root[data-theme="editorial"] .settings-panel');
  const section = ruleBody(css, ':root[data-theme="editorial"] .settings-section');
  const sectionHeading = ruleBody(
    css,
    ':root[data-theme="editorial"] .settings-section > :is(h2, h3):first-child::before',
  );
  const progressive = atRuleBody(css, '@supports (animation-timeline: view())');
  const reduced = atRuleBody(css, '@media (prefers-reduced-motion: reduce)');
  const reducedFlowMedia = ruleBody(
    reduced,
    ':root[data-theme="editorial"] .flow-preview-item__media',
  );
  const forced = atRuleBody(css, '@media (forced-colors: active)');
  const medium = atRuleBody(css, '@media (max-width: 760px)');
  const compact = atRuleBody(css, '@media (max-width: 420px)');
  const short = atRuleBody(css, '@media (max-width: 760px) and (max-height: 480px)');
  const wide = atRuleBody(css, '@media (min-width: 1100px)');
  const shortTopbar = ruleBody(short, ':root[data-theme="editorial"] .single-page-topbar');
  const shortBrand = ruleBody(short, ':root[data-theme="editorial"] .single-page-brand');
  const shortScanTitle = ruleBody(
    short,
    ':root[data-theme="editorial"] .single-page-shell:has(.wallpaper-flow) > .scan-activity .scan-activity__title',
  );
  const shortScanMeta = ruleBody(
    short,
    ':root[data-theme="editorial"] .single-page-shell:has(.wallpaper-flow) > .scan-activity .scan-activity__meta',
  );

  assert.match(focus, /outline:\s*2px solid var\(--text\)/);
  assert.match(focus, /outline-offset:\s*3px/);
  assert.match(focus, /box-shadow:\s*0 0 0 2px var\(--bg\)/);
  assert.match(settings, /counter-reset:\s*editorial-section/);
  assert.match(section, /counter-increment:\s*editorial-section/);
  assert.match(sectionHeading, /content:\s*["']0["'] counter\(editorial-section\)/);
  assert.match(progressive, /animation-timeline:\s*view\(/);
  assert.match(progressive, /animation-range:/);
  assert.match(reduced, /animation:\s*none\s*!important/);
  assert.match(reduced, /transition:\s*none\s*!important/);
  assert.match(reduced, /filter:\s*none/);
  assert.match(reduced, /\.settings-overlay\[data-presentation-phase="open"\]\s+\.settings-panel/);
  assert.match(reduced, /transform:\s*none\s*!important/);
  assert.match(reducedFlowMedia, /transform:\s*none\s*!important/);
  assert.match(forced, /\bCanvas\b/);
  assert.match(forced, /\bCanvasText\b/);
  assert.match(forced, /\bButtonText\b/);
  assert.match(forced, /forced-color-adjust:\s*auto/);
  assert.match(css, /data-source-action\^=["']request-remove["'][\s\S]*?var\(--danger\)/);
  assert.match(css, /data-source-action\^=["']confirm-remove["'][\s\S]*?var\(--danger\)/);
  assert.match(medium, /\.single-page-brand/);
  assert.match(compact, /\.single-page-topbar/);
  assert.match(shortTopbar, /grid-template-rows:\s*auto/);
  assert.match(shortBrand, /display:\s*none/);
  assert.match(shortScanTitle, /text-overflow:\s*ellipsis/);
  assert.match(shortScanTitle, /white-space:\s*nowrap/);
  assert.match(shortScanMeta, /display:\s*none\s*!important/);
  assert.match(wide, /\.single-page-brand/);
});

test('Editorial Flow is a strict monochrome portfolio composition', async () => {
  const css = await readCss();
  const flow = ruleBody(css, ':root[data-theme="editorial"] .wallpaper-flow');
  const rail = ruleBody(
    css,
    ':root[data-theme="editorial"] :is(.flow-index-rail, .flow-metadata-rail)',
  );
  const media = ruleBody(css, ':root[data-theme="editorial"] .flow-preview-item__media');
  const restingMedia = ruleBody(
    css,
    ':root[data-theme="editorial"] .flow-preview-item:not([data-centered]) .flow-preview-item__media img',
  );
  const centeredMedia = ruleBody(
    css,
    ':root[data-theme="editorial"] .flow-preview-item[data-centered] .flow-preview-item__media img',
  );
  const metadata = ruleBody(css, ':root[data-theme="editorial"] .flow-metadata-rail');
  const metadataTitle = ruleBody(
    css,
    ':root[data-theme="editorial"] .flow-metadata-rail__title',
  );

  assert.match(flow, /background:\s*var\(--bg\)/);
  assert.match(flow, /border-top:\s*1px solid var\(--text\)/);
  assert.match(rail, /font-family:\s*var\(--editorial-mono\)/);
  assert.match(rail, /text-transform:\s*uppercase/);
  assert.match(media, /border-radius:\s*0/);
  assert.match(media, /box-shadow:\s*none/);
  assert.match(restingMedia, /filter:\s*grayscale\(1\)/);
  assert.match(centeredMedia, /filter:\s*grayscale\(0\)/);
  assert.match(metadata, /animation:\s*editorial-flow-metadata-enter 180ms/);
  const lineHeight = Number(metadataTitle.match(/line-height:\s*([\d.]+)/)?.[1]);
  assert.ok(lineHeight >= 1, `wrapped CJK title line-height must be at least 1, got ${lineHeight}`);
});

test('Editorial Flow keeps native scrolling free of decorative transition work', async () => {
  const css = await readCss();
  const unsettled = ruleBody(
    css,
    ':root[data-theme="editorial"] .flow-preview-item:not([data-settled])',
  );

  assert.match(unsettled, /transition:\s*none/);
});

test('Editorial Flow keeps state, controls, and dialogs in the same visual language', async () => {
  const css = await readCss();
  const switchGroup = ruleBody(css, ':root[data-theme="editorial"] .library-view-switch');
  const filterSwitch = ruleBody(
    css,
    ':root[data-theme="editorial"] .single-page-filters > .library-view-switch',
  );
  const pressed = ruleBody(
    css,
    ':root[data-theme="editorial"] .library-view-switch__button[aria-pressed="true"]',
  );
  const actions = ruleBody(css, ':root[data-theme="editorial"] .flow-metadata-rail__action');
  const dialog = ruleBody(css, ':root[data-theme="editorial"] .flow-index-dialog');

  assert.match(switchGroup, /border:\s*1px solid var\(--text\)/);
  assert.match(switchGroup, /border-radius:\s*0/);
  assert.match(filterSwitch, /border-inline-start:\s*1px solid var\(--border-subtle\)/);
  assert.match(pressed, /background:\s*var\(--text\)/);
  assert.match(pressed, /color:\s*var\(--bg\)/);
  assert.match(actions, /border-radius:\s*0/);
  assert.match(actions, /text-transform:\s*uppercase/);
  assert.match(dialog, /border-radius:\s*0/);
  assert.match(dialog, /box-shadow:\s*none/);
});

test('Editorial source close control is a visible square target', async () => {
  const css = await readCss();
  const close = ruleBody(css, ':root[data-theme="editorial"] .source-panel__close');

  assert.match(close, /border:\s*1px solid/);
  assert.match(close, /border-radius:\s*0\s*!important/);
  assert.match(close, /display:\s*grid/);
  assert.match(close, /place-items:\s*center/);
  assert.match(close, /line-height:\s*1/);
});
