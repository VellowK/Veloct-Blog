// Verify a selected project's actual compiled homepage, without mock settings or user data writes.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, extname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { profileContacts } from '../site/src/lib/profile-home.mjs';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const root = resolve(process.argv[2] || '');
if (!process.argv[2]) throw new Error('Pass the selected project root');
const output = resolve(root, 'site/dist/site');
const settings = JSON.parse(await readFile(resolve(root, 'data/site.json'), 'utf8'));
const html = await readFile(resolve(output, 'index.html'), 'utf8');
assert.ok(html.includes('data-profile-home')); assert.ok(!html.includes('id="veloct-right-sidebar"'));
const types = { '.css':'text/css', '.js':'text/javascript', '.png':'image/png', '.webp':'image/webp', '.svg':'image/svg+xml', '.ico':'image/x-icon', '.html':'text/html' };
const browser = await chromium.launch({ channel:'msedge', headless:true });
try {
  for (const width of [1440,1024]) {
    const page = await browser.newPage({ viewport:{width,height:width === 1440 ? 900 : 640} });
    const errors=[]; page.on('pageerror', error => errors.push(error.message));
    await page.route('http://existing-home.test/**', async route => {
      const pathname=decodeURIComponent(new URL(route.request().url()).pathname);
      const file=resolve(output, '.' + (pathname.endsWith('/') ? pathname+'index.html' : pathname));
      if (!file.startsWith(output+sep)) return route.fulfill({status:403,body:'Forbidden'});
      try { await route.fulfill({contentType:types[extname(file)] || 'application/octet-stream',body:await readFile(file)}); }
      catch { await route.fulfill({status:404,body:'Not found'}); }
    });
    await page.goto('http://existing-home.test/');
    await page.locator('[data-profile-home]').waitFor();
    assert.equal(await page.locator('[data-profile-home] h1').textContent(), settings.author.name || settings.name);
    assert.equal(await page.locator('.profile-description').textContent(), settings.description);
    assert.equal(await page.locator('.profile-about p').textContent(), settings.author.bio || '还没有填写个人介绍。');
    assert.equal(await page.locator('#veloct-right-sidebar').count(),0);
    assert.equal(await page.locator('.profile-contact-grid > *').count(),profileContacts(settings.social).length);
    assert.equal(await page.locator('.profile-contact--empty').count(),0);
    assert.equal(await page.locator('[data-activity]:visible').count(),1);
    assert.equal(await page.locator('[data-activity]:visible rect').count(),365);
    const toggle = page.locator('[data-activity-toggle]');
    assert.equal(await page.locator('.profile-activity__title [data-activity-toggle]').count(),1);
    assert.equal(await page.locator('.profile-activity__controls').count(),0);
    await page.evaluate(() => document.fonts.ready);
    await toggle.scrollIntoViewIfNeeded();
    const cardBefore = await page.locator('[data-activity-switcher]').boundingBox();
    await toggle.click();
    assert.equal(await page.locator('[data-activity="github"]:visible').count(),1);
    assert.deepEqual(await page.locator('[data-activity-switcher]').boundingBox(),cardBefore);
    await toggle.click();
    assert.equal(await page.locator('[data-activity="blog"]:visible').count(),1);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.deepEqual(errors,[]);
    await page.screenshot({path:fileURLToPath(new URL(`../target/existing-home-${width}.png`, import.meta.url)),fullPage:true,animations:'disabled'});
    console.log(JSON.stringify({width,actualProject:true,profileHome:true,ownerSettingsPreserved:true,singleCalendarWithToggle:true,titleAdjacentToggle:true,noBottomButtonRow:true,stableCard:true,annualRange:true,emptyContactsHidden:true,noOriginalRightSidebar:true,pageErrors:errors}));
    await page.close();
  }
} finally { await browser.close(); }
