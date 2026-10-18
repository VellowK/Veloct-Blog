// Exercise the real Astro production output in Edge, including ClientRouter page swaps.
// No user project or existing server is touched; only built files are served through an isolated route.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';

const root = path.resolve('site/dist/site');
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  for (const width of [1440, 1024]) {
    const height = width === 1440 ? 900 : 640;
    const page = await browser.newPage({ viewport: { width, height } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('http://empty-home.test/**', async route => {
      const url = new URL(route.request().url());
      const relative = decodeURIComponent(url.pathname).replace(/^\/+/, '');
      const file = path.resolve(root, relative + (url.pathname.endsWith('/') ? 'index.html' : ''));
      if (!file.startsWith(root + path.sep)) return route.fulfill({ status: 403, body: '' });
      try {
        await route.fulfill({ body: await readFile(file), contentType: ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.webp': 'image/webp', '.svg': 'image/svg+xml' })[path.extname(file)] || 'application/octet-stream' });
      } catch { await route.fulfill({ status: 404, body: '' }); }
    });
    const assertBlank = async () => {
      const canvas = page.locator('[data-veloct-page-content]');
      await canvas.waitFor();
      assert.equal(await canvas.evaluate(element => element.childElementCount), 0);
      assert.equal((await canvas.textContent()).trim(), '');
      assert.equal(await page.locator('#veloct-right-sidebar').count(), 0);
      assert.equal(await page.locator('#veloct-left-sidebar').isVisible(), true);
      assert.equal(await page.locator('main footer').isVisible(), true);
      const bounds = await canvas.boundingBox();
      assert.ok(bounds.width > width * 0.65 && bounds.height > 100, JSON.stringify(bounds));
      assert.equal(await page.locator('head link[rel="canonical"]').count(), 1);
    };
    await page.goto('http://empty-home.test/');
    await assertBlank();
    await page.screenshot({ path: `target/empty-home-${width}.png` });
    await page.locator('#veloct-left-sidebar a[href="/article/"]').first().click();
    await page.waitForURL('**/article/');
    await page.locator('#veloct-right-sidebar').waitFor({ state: 'attached' });
    assert.ok((await page.locator('[data-veloct-page-content]').innerText()).length > 0);
    await page.locator('#veloct-left-sidebar a[href="/"]').first().click();
    await page.waitForURL('http://empty-home.test/');
    await assertBlank();
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, height, homeEmpty: true, noHomeSidebar: true, navigationAndFooterKept: true, otherPageSidebarKept: true, routerReturnStillBlank: true, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
