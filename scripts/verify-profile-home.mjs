// Isolated real Astro production build + Edge. Never opens or edits a user's project.
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync, symlinkSync, rmSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { stageTemplate } from './prepare-template.mjs';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const root = mkdtempSync(path.resolve('target/profile-home-'));
let browser;
const config = { name: 'Veloct Profile', description: '独立站点描述 · 记录创作与生活', author: { name: 'Vellow', bio: '这是独立的个人介绍。\n喜欢设计、开源和记录日常。', avatar: '/assets/test-avatar.svg' }, homepage: { cover: '/assets/test-cover.svg', githubUsername: 'octocat', showBlogHeatmap: true, showGithubHeatmap: true }, social: [{ label: '微信', icon: 'wechat', id: 'my-public-wechat', url: '' }, { label: 'GitHub', icon: 'github', id: 'octocat', url: 'https://github.com/octocat' }] };
function build() {
  const result = spawnSync(process.execPath, ['node_modules/astro/bin/astro.mjs', 'build'], { cwd: path.join(root, 'site'), encoding: 'utf8', timeout: 120000, env: { ...process.env, VELOCT_BUILD_TIME: '2026-10-15T00:00:00Z' } });
  assert.equal(result.status, 0, result.stdout + result.stderr);
}
async function openBuild(page, output) {
  await page.route('http://profile-home.test/**', async route => {
    const url = new URL(route.request().url());
    const file = path.resolve(output, decodeURIComponent(url.pathname).replace(/^\/+/, '') + (url.pathname.endsWith('/') ? 'index.html' : ''));
    if (!file.startsWith(output + path.sep)) return route.fulfill({ status: 403 });
    try { await route.fulfill({ body: await readFile(file), contentType: ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' })[path.extname(file)] || 'application/octet-stream' }); }
    catch { await route.fulfill({ status: 404 }); }
  });
  await page.goto('http://profile-home.test/');
}
try {
  stageTemplate(path.resolve('.'), root);
  const site = path.join(root, 'site');
  symlinkSync(path.resolve('site/node_modules'), path.join(site, 'node_modules'), 'junction');
  for (const directory of ['content/articles', 'content/pages', 'content/knowledge', 'works', 'data', 'site/public/assets']) mkdirSync(path.join(root, directory), { recursive: true });
  writeFileSync(path.join(root, 'data/site.json'), JSON.stringify(config));
  writeFileSync(path.join(site, 'public/assets/test-cover.svg'), '<svg xmlns="http://www.w3.org/2000/svg" width="1400" height="400"><defs><linearGradient id="g"><stop stop-color="#5b596f"/><stop offset="1" stop-color="#b17a62"/></linearGradient></defs><path fill="url(#g)" d="M0 0h1400v400H0z"/><path fill="#1d2538" opacity=".5" d="M0 350 350 100 600 400 1000 80 1400 400H0z"/></svg>');
  writeFileSync(path.join(site, 'public/assets/test-avatar.svg'), '<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><rect width="128" height="128" rx="20" fill="#bc8c6d"/><text x="64" y="89" text-anchor="middle" font-size="85" fill="#fdf7f0" font-family="serif">V</text></svg>');
  const markdown = (id, type, privateDraft = false) => `---\nid: ${id}\ntype: ${type}\nslug: ${id}\ntitle: Fixture ${id}\ncreated_at: '2026-10-14T00:00:00Z'\nupdated_at: '2026-10-15T00:00:00Z'\n${type === 'article' ? `wip: ${privateDraft}\nwip_publish: false\n` : ''}---\n\nFixture body.\n`;
  writeFileSync(path.join(root, 'content/articles/public-article.md'), markdown('public-article', 'article'));
  writeFileSync(path.join(root, 'content/articles/private-draft.md'), markdown('private-draft', 'article', true));
  writeFileSync(path.join(root, 'content/knowledge/knowledge.md'), markdown('knowledge', 'knowledge'));
  const days = Array.from({ length: 365 }, (_, i) => ({ date: new Date(Date.UTC(2026, 9, 15) - (364 - i) * 86400000).toISOString().slice(0, 10), level: i % 5 }));
  writeFileSync(path.join(root, 'data/github-activity.json'), JSON.stringify({ schemaVersion: 1, username: 'octocat', fetchedAt: '2026-10-15T00:00:00Z', days }));
  build();
  const output = path.join(site, 'dist/site');
  browser = await chromium.launch({ channel: 'msedge', headless: true });
  for (const width of process.env.PROFILE_WIDTH ? [Number(process.env.PROFILE_WIDTH)] : [1440, 1024, 390]) {
    const page = await browser.newPage({ viewport: { width, height: width === 390 ? 844 : width === 1440 ? 900 : 640 } });
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => { Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async value => { window.__copied = value; } } }); });
    await openBuild(page, output);
    await page.locator('[data-profile-home]').waitFor();
    await page.evaluate(() => document.fonts.ready);
    assert.equal(await page.locator('#veloct-right-sidebar').count(), 0);
    assert.equal(await page.locator('.profile-cover img').evaluate(img => img.complete && img.naturalWidth > 0), true);
    assert.equal(await page.locator('.profile-avatar img').evaluate(img => img.complete && img.naturalWidth > 0), true);
    assert.equal(await page.locator('.profile-description').innerText(), config.description);
    assert.equal(await page.locator('.profile-about > p').innerText(), config.author.bio);
    assert.equal(await page.locator('.profile-contact-grid > *').count(), 2);
    assert.equal(await page.locator('.profile-contact--empty').count(), 0);
    assert.equal(await page.locator('[data-activity="blog"] rect').count(), 365);
    assert.equal(await page.locator('[data-activity]:visible').count(), 1);
    assert.match(await page.locator('[data-activity-summary="blog"]').innerText(), /2 篇创作/);
    assert.equal(await page.locator('[data-activity="github"] rect').count(), 365);
    const toggle = page.locator('[data-activity-toggle]');
    assert.equal(await toggle.getAttribute('title'), '切换至 GitHub 贡献');
    assert.equal(await toggle.locator('xpath=ancestor::header').count(), 1);
    assert.equal(await page.locator('[data-activity-title]').innerText(), '博客创作');
    assert.equal(await page.locator('.profile-calendar-panel .activity-card__heading').count(), 0);
    assert.ok(await page.locator('[data-activity-switcher]').evaluate(element => element.lastElementChild.classList.contains('profile-calendar-panels')), 'No separate bottom button row remains');
    await toggle.scrollIntoViewIfNeeded();
    const beforeToggle = await toggle.boundingBox();
    const titleBox = await page.locator('[data-activity-title]').boundingBox();
    assert.ok(Math.abs(beforeToggle.y + beforeToggle.height / 2 - titleBox.y - titleBox.height / 2) < 1, 'Switch sits on the title line');
    assert.ok(beforeToggle.x - titleBox.x - titleBox.width >= 0 && beforeToggle.x - titleBox.x - titleBox.width <= 12, 'Switch is adjacent to the title');
    const cardBefore = await page.locator('[data-activity-switcher]').boundingBox();
    assert.equal(await page.locator('[data-activity-panel="github"]').getAttribute('aria-hidden'), 'true');
    assert.equal(await page.locator('[data-activity-panel="github"]').evaluate(node => node.inert), true);
    assert.equal(await page.locator('[data-activity-summary="github"]').getByRole('link').count(), 0);
    const layoutSnapshot = () => page.locator('[data-activity-switcher]').evaluate(element => [...element.querySelectorAll('.profile-activity__heading, .profile-activity__meta, [data-activity-summary], .profile-calendar-panels, [data-activity-panel], .activity-card__footer')].map(node => ({ class: node.className, data: { ...node.dataset }, ariaHidden: node.getAttribute('aria-hidden'), inert: node.inert, width: node.getBoundingClientRect().width, height: node.getBoundingClientRect().height, display: getComputedStyle(node).display, visibility: getComputedStyle(node).visibility })));
    const layoutBefore = await layoutSnapshot();
    const requests = []; page.on('request', request => requests.push(request.url()));
    await toggle.click();
    assert.equal(await page.locator('[data-activity="github"]:visible').count(), 1);
    assert.equal(await page.locator('[data-activity="blog"]:visible').count(), 0);
    assert.match(await page.locator('[data-activity="github"]').innerText(), /2026-10-15/);
    assert.ok(!/篇创作/.test(await page.locator('[data-activity-summary="github"]').innerText()));
    assert.equal(await page.locator('[data-activity-title]').innerText(), 'GitHub 贡献');
    assert.equal(await toggle.getAttribute('title'), '切换至 博客创作');
    assert.equal(await page.locator('[data-activity-panel="blog"]').getAttribute('aria-hidden'), 'true');
    assert.equal(await page.locator('[data-activity-panel="blog"]').evaluate(node => node.inert), true);
    assert.equal(await page.locator('[data-activity-summary="github"]').getByRole('link').count(), 1);
    assert.equal(await toggle.evaluate(element => document.activeElement === element), true, 'Same button retains focus');
    const cardAfter = await page.locator('[data-activity-switcher]').boundingBox();
    if (cardAfter.height !== cardBefore.height) console.log(JSON.stringify({ width, layoutBefore, layoutAfter: await layoutSnapshot(), hiddenRules: await page.evaluate(() => [...document.styleSheets].map(sheet => [...sheet.cssRules].map(rule => rule.cssText).join('\n')).join('\n').match(/[^{}]*\[hidden\][^{]*\{[^}]*\}/g)) }));
    assert.deepEqual(cardAfter, cardBefore);
    assert.deepEqual(await toggle.boundingBox(), beforeToggle);
    await toggle.press('Enter');
    assert.equal(await page.locator('[data-activity="blog"]:visible').count(), 1);
    await toggle.press('Space');
    assert.equal(await page.locator('[data-activity="github"]:visible').count(), 1);
    await toggle.click();
    assert.deepEqual(requests, [], 'Switching charts must not fetch remote data or reload');
    await page.getByRole('button', { name: '复制 微信：my-public-wechat', exact: true }).click();
    await page.waitForFunction(() => window.__copied === 'my-public-wechat');
    assert.match(await page.locator('.profile-contact-status').innerText(), /已复制/);
    const dimensions = await page.evaluate(() => ({ width: document.documentElement.clientWidth, scrollWidth: document.documentElement.scrollWidth, contactColumns: getComputedStyle(document.querySelector('.profile-contact-grid')).gridTemplateColumns.split(' ').length }));
    assert.ok(dimensions.scrollWidth <= dimensions.width + 1, JSON.stringify(dimensions)); assert.equal(dimensions.contactColumns, 4);
    await page.mouse.move(0, 0); await page.locator('.profile-contact').first().blur();
    await page.screenshot({ path: `target/profile-home-${width}.png`, fullPage: true, animations: 'disabled' });
    if (width !== 390) {
      await page.locator('#veloct-left-sidebar a[href="/article/"]').first().click(); await page.waitForURL('**/article/');
      await page.locator('#veloct-right-sidebar').waitFor({ state: 'attached' });
      await page.locator('#veloct-left-sidebar a[href="/"]').first().click(); await page.waitForURL('http://profile-home.test/');
      assert.equal(await page.locator('#veloct-right-sidebar').count(), 0);
      await page.locator('[data-activity-toggle]').click();
      assert.equal(await page.locator('[data-activity="github"]:visible').count(), 1, 'ClientRouter return must not duplicate toggle handlers');
    }
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, coverAndAvatar: true, separateDescriptionAndBio: true, singleVisibleCalendar: true, annualRange: true, titleAdjacentToggle: true, noBottomButtonRow: true, stableCardAndFocus: true, keyboardToggle: true, noToggleNetwork: true, privateDraftExcluded: true, noEmptyContacts: true, fourColumns: true, idCopy: true, noGlobalSidebarOnHome: true, noOverflow: true, pageErrors: errors }));
    await page.close();
  }
  // Both switches and account mismatch must affect the actual rendered site, not only a form state.
  config.homepage.showBlogHeatmap = false; config.homepage.githubUsername = 'other-user';
  config.social = Array.from({ length: 18 }, (_, i) => ({ label: `Contact ${i}`, id: `id-${i}`, url: '' }));
  writeFileSync(path.join(root, 'data/site.json'), JSON.stringify(config)); build();
  let html = await readFile(path.join(output, 'index.html'), 'utf8');
  assert.ok(!html.includes('data-activity="blog"')); assert.match(html, /尚未同步此账号/); assert.ok(!html.match(/<article\b[^>]*data-profile-home[\s\S]*?<\/article>/)?.[0].includes('Contact 16'));
  assert.ok(!/<button[^>]*data-activity-toggle/.test(html), 'One enabled source needs no toggle');
  const singleSource = await browser.newPage({ viewport: { width: 1024, height: 640 } });
  await openBuild(singleSource, output);
  assert.equal(await singleSource.locator('[data-activity]:visible').count(), 1);
  assert.equal(await singleSource.locator('[data-activity="github"]:visible rect').count(), 365);
  assert.equal(await singleSource.locator('[data-activity-toggle]').count(), 0);
  await singleSource.close();
  config.homepage.showGithubHeatmap = false; config.social = [{ label: 'Empty', id: '  ', url: '' }]; writeFileSync(path.join(root, 'data/site.json'), JSON.stringify(config)); build();
  html = await readFile(path.join(output, 'index.html'), 'utf8'); assert.ok(!html.includes('data-activity="github"')); assert.match(html, /欢迎来到我的个人空间/);
  assert.ok(!html.includes('class="profile-contacts"')); assert.ok(html.includes('profile-content--no-contacts'));
  assert.ok(!/<button[^>]*data-activity-toggle/.test(html));
  const emptyState = await browser.newPage({ viewport: { width: 1024, height: 640 } });
  await openBuild(emptyState, output);
  assert.equal(await emptyState.locator('[data-activity]:visible').count(), 0);
  assert.equal(await emptyState.locator('.profile-contacts').count(), 0);
  assert.equal(await emptyState.locator('[data-activity-toggle]').count(), 0);
  assert.match(await emptyState.locator('.profile-content').textContent(), /欢迎来到我的个人空间/);
  assert.ok(await emptyState.locator('.profile-content').evaluate(element => getComputedStyle(element).gridTemplateColumns.split(' ').length === 1));
  await emptyState.close();
  console.log(JSON.stringify({ switchesAffectProduction: true, accountMismatchNotReused: true, extraContactsPreservedButNotRendered: true, singleSourceAndEmptyBrowser: true }));
} finally {
  await browser?.close();
  assert.equal(path.dirname(root), path.resolve('target')); assert.ok(path.basename(root).startsWith('profile-home-'));
  rmSync(root, { recursive: true, force: true });
}
