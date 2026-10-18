/** Website configuration, iframe-safe routing, and independent desktop entry motion. */


import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import { PAGE_TRANSITIONS, normalizeSiteConfig } from '../site/src/lib/site-config.mjs';

const SITE = new URL('../site/src/', import.meta.url);
const ROOT = new URL('../', import.meta.url);

/**
 * CSS with its comments removed.
 *
 * Both stylesheets explain themselves at length, and a comment here names the very selectors a check
 * below is asking about — "the old `.content[data-page-transition] > .page` rules are gone" is a
 * sentence this file wants to keep, and matching it as if it were a rule would make the assertion
 * pass for the wrong reason (or fail for one). Assertions are therefore made about the CSS the browser
 * would parse, not about the file's prose.
 */
function rules(css) {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

test('the transition is part of the site configuration and defaults to the moving one', () => {
  // A reader must not get "no animation" because a configuration predates the field.
  assert.equal(normalizeSiteConfig({}).pageTransition, 'rise');
  assert.deepEqual(PAGE_TRANSITIONS, ['rise', 'dissolve', 'none']);
  for (const value of PAGE_TRANSITIONS) {
    assert.equal(normalizeSiteConfig({ pageTransition: value }).pageTransition, value);
  }
  // Refused rather than defaulted: a site that silently rendered differently from its configuration
  // is the failure this project treats as worse than a clear error. `null` is the exception, and it is
  // the documented rule of this file rather than a special case here — `merge` reads null as "not
  // specified" everywhere, so it falls back to the default like a missing key.
  assert.throws(() => normalizeSiteConfig({ pageTransition: 'zoom' }), /pageTransition/);
  assert.throws(() => normalizeSiteConfig({ pageTransition: '' }), /pageTransition/);
  assert.equal(normalizeSiteConfig({ pageTransition: null }).pageTransition, 'rise');
});

test('the layout publishes the setting on the document, where the transition can read it', () => {
  const layout = readFileSync(new URL('layouts/BaseLayout.astro', SITE), 'utf8');
  assert.match(layout, /import \{ readSiteConfig \} from '\.\.\/lib\/site-config\.mjs'/, 'the layout must read the site configuration');
  // The attribute has to be on <html>: the ::view-transition-* pseudo-elements hang off the root
  // element, so a value written anywhere lower could not be selected by the stylesheet.
  assert.match(layout, /<html lang="zh-CN" data-page-transition=\{site\.pageTransition\}>/, 'the effect must be an attribute of the root element');
  assert.match(layout, /<ClientRouter fallback="animate"/, 'iframe navigation uses the same-document router');
  assert.match(layout, /data-astro-rerun/, 'editor bridge is reinitialized after a route swap');
});

test('the stylesheet draws all three effects and respects a reduced-motion request', () => {
  const css = rules(readFileSync(new URL('styles/global.css', SITE), 'utf8'));
  // Cross-document view transitions: the plain navigation is the fallback on an older engine.
  assert.match(css, /@view-transition\s*\{\s*navigation:\s*auto;?\s*\}/, 'cross-document transitions must be enabled');
  for (const value of PAGE_TRANSITIONS) {
    assert.match(css, new RegExp(`html\\[data-page-transition='${value}'\\]::view-transition-(old|new)\\(root\\)`), `${value} must be drawn`);
  }
  // 'none' is the one that must be INSTANT, and it is the one an implementation is most likely to
  // leave to the user agent's default cross-fade.
  assert.match(css, /html\[data-page-transition='none'\]::view-transition-old\(root\),\s*html\[data-page-transition='none'\]::view-transition-new\(root\)\s*\{\s*animation:\s*none;?\s*\}/, "'none' must switch instantly");
  // The blanket reduced-motion rule in this file matches elements, not pseudo-elements, so the
  // snapshots need their own rule or a reader who asked for less motion would still get all of it.
  const reduced = css.slice(css.lastIndexOf('prefers-reduced-motion'));
  assert.match(reduced, /::view-transition-new\(root\)/, 'the transition snapshots must honour reduced motion');
});

test('the window offers exactly the effects the template knows', () => {
  const panel = readFileSync(new URL('src/SiteSettingsPanel.tsx', ROOT), 'utf8');
  const offered = [...panel.matchAll(/value:\s*'(rise|dissolve|none)'/g)].map((match) => match[1]);
  assert.deepEqual(offered, PAGE_TRANSITIONS, 'the picker and the template must offer the same three');
  assert.match(panel, /pageTransition:\s*'rise'/, 'the form default must match the site default');
  // It writes the shared configuration field, not a local preference.
  assert.match(panel, /update\(\{\s*pageTransition:\s*option\.value\s*\}\)/, 'choosing an effect must update the site setting');
  assert.doesNotMatch(panel, /localStorage/, 'the setting must not live only in this application');
});

test('外观 owns 站点信息, 站点美化 and 备案管理, and the settings surface no longer repeats 备案管理', () => {
  const app = readFileSync(new URL('src/App.tsx', ROOT), 'utf8');
  for (const label of ['站点信息', '站点美化', '备案管理']) {
    assert.ok(app.includes(`'${label}'`), `外观 must offer ${label}`);
  }
  // Each appearance tab renders ONE section through the shared panel, so the three tabs cannot hold
  // conflicting drafts of the same file.
  assert.match(app, /onlySection=\{appearanceSection\}/, 'the appearance tabs must select a single section');

  const panel = readFileSync(new URL('src/SiteSettingsPanel.tsx', ROOT), 'utf8');
  const subtabs = panel.slice(panel.indexOf('settings-subtabs'), panel.indexOf('settings-subtabs') + 700);
  assert.ok(!subtabs.includes('备案管理'), '备案管理 must not remain a subtab of 站点设置');
  assert.match(panel, /activeSection === 'filing'/, 'the panel must still be able to render the filing section');
});

test('desktop navigation keeps its independent rise without a permanent transform', () => {
  const css = rules(readFileSync(new URL('src/styles.css', ROOT), 'utf8'));
  const app = readFileSync(new URL('src/App.tsx', ROOT), 'utf8');
  assert.match(app, /content content--rise/);
  assert.match(css, /\.content--rise\s*\{\s*animation:\s*app-rise-in 260ms ease-out;/);
  assert.match(css, /to\s*\{\s*opacity:\s*1;\s*transform:\s*none;/);
  assert.doesNotMatch(css, /animation:\s*app-rise-in[^;]*both/);
});

