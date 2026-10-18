// Production UI regression. Only IPC is mocked; typing and full-width layout run in real Edge.
// Latest owner request: WYSIWYG only, no duplicate preview beside the editor.
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
      window.calls = []; window.holdSave = true; window.failSave = false;
      window.running = false;
      const prefix = '---\nid: article1\ntitle: 编辑测试\nsummary: 原始摘要\nwip: true\nunknown: keep-me\n---\n';
      const doc = id => ({ id, kind: 'article', title: id === 'article1' ? '编辑测试' : '第二篇', slug: id, wip: true, wip_publish: false, wip_ever_published: false, wip_state: 'draft', is_buildable: false, route: '/wip/' + id + '/', revision: { hash: id, size: 100 }, source: id === 'article1' ? prefix + '# 初始标题\n\n初始正文 **加粗**。\n' : '---\nid: article2\ntitle: 第二篇\n---\n# 第二篇正文\n', diagnostics: [], matches_disk: true });
      window.__TAURI_INTERNALS__ = { invoke: async (cmd, args) => {
        window.calls.push({ cmd, args });
        const preview = () => ({ site: window.running ? { state: 'running', port: 4399, pid: 99999, restarts: 0 } : { state: 'stopped' }, works: { state: 'stopped' }, site_url: window.running ? 'http://127.0.0.1:4399/' : null, works_url: null });
        if (cmd === 'app_info') return { name: 'Veloct', version: '1.0.0' };
        if (cmd === 'current_project') return { path: 'D:/isolated/editor', name: '用于测试的长项目名称' };
        if (cmd === 'list_documents') return [doc('article1'), doc('article2')];
        if (cmd === 'read_document') return doc(args.id);
        if (cmd === 'content_index') return { entries: [], issues: [], is_buildable: true };
        if (cmd === 'list_attachments' || cmd === 'storage_targets') return [];
        if (cmd === 'attachment_sync_settings') return { target_ids: [] };
        if (cmd === 'preview_status') return preview();
        if (cmd === 'start_preview') { window.running = true; return preview(); }
        if (cmd === 'stop_preview') { window.running = false; return preview(); }
        if (cmd === 'ai_settings') return { page_create_ready: false, providers: [], features: [], fallback: null };
        if (cmd === 'attachment_messages' || cmd === 'check_attachment_images') return { upload_errors: [], health_errors: [] };
        if (cmd === 'save_document') {
          if (window.holdSave) await new Promise(resolve => { window.releaseSave = resolve; });
          if (window.failSave) return { saved: false, status: 'conflict', conflict_message: '模拟磁盘冲突', disk_source: doc('article1').source, diagnostics: [] };
          return { saved: true, status: 'saved', source: args.source, revision: { hash: 'saved', size: args.source.length }, diagnostics: [] };
        }
        return null;
      } };
    });
    await page.goto('http://workbench.test/');
    const articles = page.locator('.nav-item').filter({ hasText: '文章管理' });
    await articles.click(); await page.getByRole('button', { name: '编辑测试', exact: false }).first().click();
    const workspace = page.locator('.workspace');
    const richText = page.locator('.wysiwyg [contenteditable="true"]');
    await richText.getByRole('heading', { name: '初始标题' }).waitFor();
    const singlePane = async () => {
      assert.equal(await workspace.locator(':scope > .pane').count(), 1);
      assert.equal(await workspace.locator('iframe, .split-handle, .markdown-preview, [contenteditable="false"]').count(), 0);
      assert.equal(await page.getByRole('separator', { name: '拖动以调整编辑器和预览的比例' }).count(), 0);
      const layout = await workspace.evaluate(el => {
        const pane = el.firstElementChild.getBoundingClientRect(), area = el.getBoundingClientRect();
        const resources = el.querySelector('.document-resources');
        const sidebar = resources && !resources.hidden ? resources.getBoundingClientRect() : null;
        return { x: pane.x - area.x, width: pane.width - area.width, sidebar: sidebar?.width ?? 0, gap: sidebar ? sidebar.x - pane.right : 0, height: area.height, overflow: el.scrollWidth > el.clientWidth };
      });
      assert.ok(Math.abs(layout.x) < 1 && Math.abs(layout.width + layout.sidebar + layout.gap) < 1, JSON.stringify(layout));
      assert.ok(layout.height > 100 && !layout.overflow, JSON.stringify(layout));
    };
    await singlePane();
    await page.getByRole('button', { name: '收起资源栏', exact: true }).click();
    assert.equal(await page.getByRole('complementary', { name: '本文资源' }).count(), 0);
    await singlePane();
    await page.getByRole('button', { name: '展开资源栏', exact: true }).click();
    await page.getByRole('complementary', { name: '本文资源' }).waitFor();
    await singlePane();
    const chip = page.locator('.topbar-actions .chip').first();
    const geometry = () => page.locator('.topbar-actions').evaluate(el => {
      const rect = element => { const r = element.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; };
      return { bar: rect(el), button: rect(el.querySelector('button')), chip: rect(el.querySelector('.chip')) };
    });
    await page.waitForFunction(() => document.querySelector('.topbar-actions')?.textContent.includes('已同步'));
    const saved = await geometry();
    assert.equal((await page.evaluate(() => window.calls)).filter(c => c.cmd === 'save_document').length, 0, 'Opening must not normalize and save a document');
    await page.locator('.wysiwyg .toolbar').getByRole('button', { name: 'MD', exact: true }).click();
    await page.locator('textarea.markdown-source').fill('# 未保存标题\n\n即时正文 **新加粗**。\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```js\nconst count = 42;\n```\n');
    await page.waitForFunction(() => document.querySelector('.topbar-actions')?.textContent.includes('有未保存的改动'));
    await singlePane();
    assert.equal(await workspace.locator('[contenteditable="true"]').count(), 0);
    assert.equal((await page.evaluate(() => window.calls)).some(c => c.cmd === 'start_preview' || c.cmd === 'preview_document_ready'), false);
    assert.deepEqual(await geometry(), saved, 'Dirty state must not move controls');
    await page.waitForFunction(() => typeof window.releaseSave === 'function');
    assert.match(await chip.innerText(), /保存中/); assert.deepEqual(await geometry(), saved);
    await page.evaluate(() => { window.holdSave = false; window.releaseSave(); });
    await page.waitForFunction(() => document.querySelector('.topbar-actions')?.textContent.includes('已同步'));
    assert.deepEqual(await geometry(), saved);
    assert.match((await page.evaluate(() => window.calls)).find(c => c.cmd === 'save_document').args.source, /^---\nid: article1\ntitle: 编辑测试\nsummary: 原始摘要\nwip: true\nunknown: keep-me\n---\n# 未保存标题/);
    // Source mode and WYSIWYG are the same single pane, not two simultaneous editors.
    await page.locator('.wysiwyg .toolbar').getByRole('button', { name: 'MD', exact: true }).click();
    await richText.getByRole('heading', { name: '未保存标题', exact: true }).waitFor();
    assert.equal(await richText.locator('strong').innerText(), '新加粗');
    assert.equal(await richText.locator('table').count(), 1);
    assert.match(await richText.locator('pre code').innerText(), /const count = 42/);
    await richText.click(); await page.keyboard.press('Control+End'); await page.keyboard.press('Enter'); await page.keyboard.insertText('所见即所得追加');
    await richText.getByText('所见即所得追加', { exact: false }).waitFor();
    await page.getByLabel('摘要', { exact: true }).fill('最新摘要');
    await page.waitForFunction(() => window.calls.some(c => c.cmd === 'save_document' && c.args.source.includes('summary: "最新摘要"')));
    await singlePane();
    await page.screenshot({ path: `target/editor-single-pane-${width}.png` });
    // A rejected save must retain the editing buffer; changing documents must not show stale content.
    await page.evaluate(() => { window.failSave = true; });
    await page.locator('.wysiwyg .toolbar').getByRole('button', { name: 'MD', exact: true }).click();
    await page.locator('textarea.markdown-source').fill('# 冲突后保留正文\n');
    await page.getByText('保存被拒绝:文件在磁盘上已改变', { exact: true }).waitFor();
    assert.match(await page.locator('textarea.markdown-source').inputValue(), /冲突后保留正文/);
    await articles.click(); await page.getByRole('button', { name: '第二篇', exact: false }).first().click();
    await richText.getByRole('heading', { name: '第二篇正文', exact: true }).waitFor();
    assert.equal(await richText.getByRole('heading', { name: '冲突后保留正文' }).count(), 0);
    await singlePane();
    assert.ok(await page.locator('.topbar').evaluate(el => el.scrollWidth <= el.clientWidth));
    assert.ok(await page.locator('.pane').evaluate(el => el.scrollWidth <= el.clientWidth));
    await page.getByRole('button', { name: '打开站点预览', exact: true }).click();
    await page.waitForFunction(() => window.calls.some(c => c.cmd === 'start_preview'));
    await page.locator('.preview-panel iframe').waitFor();
    assert.equal(await workspace.count(), 0, 'Site preview and writing must remain mutually exclusive');
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, singleFullWidthPane: true, noDuplicatePreviewOrDivider: true, toolbarStable: true, richTextAndMarkdown: true, frontmatterAndSummary: true, noServerNeededForWriting: true, conflictPreservesBuffer: true, documentSwitch: true, independentSitePreview: true, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
