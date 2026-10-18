// Production UI in real Edge; only IPC is isolated from user projects.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';

const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  for (const width of [1440, 1024]) {
    const page = await browser.newPage({ viewport: { width, height: width === 1440 ? 900 : 640 } });
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.route('http://workbench.test/**', async route => {
      const name = new URL(route.request().url()).pathname;
      const file = name === '/' ? 'index.html' : name.slice(1);
      try { await route.fulfill({ body: await readFile(path.join(process.cwd(), 'dist-ui', file)), contentType: ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png' })[path.extname(file)] || 'application/octet-stream' }); }
      catch { await route.fulfill({ status: 404, body: '' }); }
    });
    await page.addInitScript(() => {
      const doc = (id, title, route, kind = 'page', is_buildable = true) => ({ id, title, route, kind, is_buildable, slug: id, summary: null, wip_state: 'finished', updated_at: '2026-10-16', category: null, tags: [] });
      window.fixturePages = [doc('home', '首页', '/'), doc('about', '关于', '/about/'), doc('custom', '自定义页面', '/custom/'), doc('article', '不应显示文章', '/article/article/', 'article'), doc('knowledge', '不应显示知识', '/docs/knowledge/', 'knowledge'), doc('broken', '不应显示无效页', '/broken/', 'page', false), ...Array.from({ length: 50 }, (_, i) => doc('p' + i, '测试页面' + i, '/p' + i + '/'))];
      window.__TAURI_INTERNALS__ = { invoke: async cmd => {
        if (cmd === 'app_info') return { name: 'Veloct', version: '1.0.0' };
        if (cmd === 'current_project') return { path: 'D:/isolated/menu', name: '菜单测试' };
        if (cmd === 'list_documents') return new URL(location.href).searchParams.has('empty') ? [] : window.fixturePages;
        if (cmd === 'content_index') return { entries: [], issues: [], is_buildable: true };
        if (cmd === 'preview_status') return { site: { state: 'stopped' }, works: { state: 'stopped' }, site_url: null, works_url: null };
        if (cmd === 'ai_settings') return { page_create_ready: false, providers: [], features: [], fallback: null };
        if (cmd === 'attachment_messages' || cmd === 'check_attachment_images') return { upload_errors: [], health_errors: [] };
        return null;
      } };
    });
    await page.goto('http://workbench.test/');
    await page.locator('.nav-item').filter({ hasText: '菜单管理' }).click();
    const picker = page.getByRole('button', { name: '选择页面', exact: true });
    const name = page.getByLabel('菜单名称', { exact: true });
    const url = page.getByLabel('链接地址/页面', { exact: true });
    const rows = page.locator('.navigation-row');
    const initialCount = await rows.count();
    await name.fill('我的菜单'); await url.fill('https://example.com/keep');
    await picker.click();
    const dialog = page.getByRole('dialog', { name: '选择页面链接' });
    await dialog.waitFor();
    const search = dialog.getByRole('searchbox', { name: '搜索页面' });
    assert.ok(await search.evaluate(el => el === document.activeElement));
    assert.equal(await dialog.locator('.navigation-page-options button').count(), 53);
    assert.equal(await dialog.getByText(/不应显示/).count(), 0);
    assert.ok(await dialog.locator('.navigation-page-dialog__body').evaluate(el => el.scrollHeight > el.clientHeight));
    const box = await dialog.boundingBox();
    assert.ok(box.x >= 0 && box.y >= 0 && box.x + box.width <= width && box.y + box.height <= (width === 1440 ? 900 : 640));
    for (let i = 0; i < 58; i++) {
      await page.keyboard.press('Tab');
      assert.ok(await dialog.evaluate(el => el.contains(document.activeElement)), 'native modal traps focus');
    }
    await search.fill('没有这个页面');
    await dialog.getByText('没有匹配的页面。').waitFor();
    await page.keyboard.press('Escape');
    await dialog.waitFor({ state: 'detached' });
    assert.equal(await url.inputValue(), 'https://example.com/keep');
    assert.equal(await name.inputValue(), '我的菜单');
    assert.ok(await picker.evaluate(el => el === document.activeElement));
    await picker.click(); await search.fill(' /ABOUT/ ');
    const about = dialog.locator('.navigation-page-options button');
    assert.equal(await about.count(), 1);
    await about.click();
    assert.equal(await url.inputValue(), '/about/');
    assert.equal(await name.inputValue(), '我的菜单');
    assert.equal(await rows.count(), initialCount, 'selection does not add a menu');
    await page.getByRole('button', { name: '新增菜单', exact: true }).click();
    assert.equal(await rows.count(), initialCount + 1);
    assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem('veloct.navigation:D:/isolated/menu')).at(-1).url), '/about/');
    await picker.click(); await search.fill('首页');
    await dialog.locator('.navigation-page-options button').click();
    assert.equal(await url.inputValue(), '/'); assert.equal(await name.inputValue(), '首页');
    await picker.click(); await search.fill('自定义页面');
    await dialog.locator('.navigation-page-options button').press('Enter');
    assert.equal(await url.inputValue(), '/custom/'); assert.equal(await name.inputValue(), '首页');
    await picker.click(); await dialog.getByRole('button', { name: '取消', exact: true }).click();
    assert.equal(await url.inputValue(), '/custom/');
    await url.fill('https://example.com/manual');
    await name.fill('外部链接'); await page.getByRole('button', { name: '新增菜单', exact: true }).click();
    assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem('veloct.navigation:D:/isolated/menu')).at(-1).url), 'https://example.com/manual');
    const layout = await page.locator('.navigation-link-input').evaluate(el => ({ client: el.clientWidth, scroll: el.scrollWidth }));
    assert.ok(layout.scroll <= layout.client);
    // A second isolated page exercises the no-pages state through the same application.
    await page.goto('http://workbench.test/?empty'); await page.locator('.nav-item').filter({ hasText: '菜单管理' }).click();
    await picker.click(); await dialog.getByText('暂无可选页面，请先在页面管理中创建页面。').waitFor();
    await dialog.getByRole('button', { name: '关闭页面选择' }).click();
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, canonicalRoutes: true, searchAndFilter: true, cancelPreservesDraft: true, selectionNotSubmission: true, manualLink: true, keyboardAndFocus: true, scrollingAndEmpty: true, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
