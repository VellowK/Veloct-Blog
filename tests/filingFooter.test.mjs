import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import { normalizeSiteConfig } from '../site/src/lib/site-config.mjs';

test('filing links accept queries and fragments and preserve custom keys', () => {
  const filing = { icp: 'record', icpUrl: 'https://example.com/?record=123', policeUrl: 'https://example.com/#/456', moeUrl: 'https://icp.gov.moe/?keyword=789', custom: true };
  assert.deepEqual(normalizeSiteConfig({ filing }).filing, filing);
  assert.equal(normalizeSiteConfig({ filing: { icp: 'legacy' } }).filing.icpUrl, '');
  for (const key of ['icpUrl', 'policeUrl', 'moeUrl']) {
    for (const value of ['javascript:alert(1)', '//example.com/', 'https://', 'https://user:password@example.com/', 'https://example.com/a b', 42]) {
      assert.throws(() => normalizeSiteConfig({ filing: { [key]: value } }), new RegExp(key));
    }
  }
});

test('footer consumes the editable link for every filing entry', () => {
  const footer = readFileSync(new URL('../site/src/components/SiteFooter.astro', import.meta.url), 'utf8');
  for (const key of ['icpUrl', 'policeUrl', 'moeUrl']) assert.ok(footer.includes(`filing.${key}`));
  assert.match(footer, /rel="noopener noreferrer"/);
});

test('short-page footer uses a stretching column, not a fixed overlay', () => {
  const layout = readFileSync(new URL('../site/src/layouts/BaseLayout.astro', import.meta.url), 'utf8');
  assert.match(layout, /flex min-h-screen w-full/);
  assert.match(layout, /flex min-w-0 flex-1 flex-col/);
  assert.match(layout, /class="min-w-0 flex-1" data-veloct-page-content/);
});
