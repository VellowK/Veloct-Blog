// Production bundle in real Edge; IPC/dialog/storage are isolated fixtures, never a user project.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';

const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  for (const width of [1440, 1024]) {
    const page = await browser.newPage({ viewport: { width, height: width === 1440 ? 900 : 720 } });
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.route('http://workbench.test/**', async route => {
      const name = new URL(route.request().url()).pathname;
      const file = name === '/' ? 'index.html' : name.slice(1);
      try { await route.fulfill({ body: await readFile(path.join(process.cwd(), 'dist-ui', file)), contentType: ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png' })[path.extname(file)] || 'application/octet-stream' }); }
      catch { await route.fulfill({ status: 404, body: '' }); }
    });
    await page.route('https://assets.test/**', route => route.fulfill({ status: 404, body: '' }));
    await page.addInitScript(() => {
      window.calls = []; window.dialogPaths = null; window.uploadFails = true; window.holdMarkdown = false;
      const prefix = '---\nid: article1\ntitle: 资源测试\nsummary: 原摘要\nunknown: keep-me\n---\n';
      const sources = { article1: prefix + '# 资源正文\n\n中间内容 end\n\n[旧附件](/.attachments-cache/shared/legacy.pdf)\n\n![远程图](https://assets.test/remote.png)\n', article2: '---\nid: article2\ntitle: 第二篇\n---\n# 第二篇正文\n' };
      const doc = id => ({ id, kind: 'article', title: id === 'article1' ? '资源测试' : '第二篇', slug: id, wip: true, wip_state: 'draft', route: '/wip/' + id + '/', revision: { hash: id, size: 100 }, source: sources[id], diagnostics: [], matches_disk: true });
      const attachment = (id, name, namespace) => ({ id, original_name: name, content_hash: id, size_bytes: 1024, cache_path: id + '/' + name, storage_target_id: 'local', storage_namespace: namespace, remote_url: null, remote_key: null, upload_status: 'PENDING', reference_status: 'UNAPPLIED', cache_status: 'PRESENT', last_error: null, created_at: '', reference_count: 0 });
      const pool = [attachment('paper', 'own.pdf', 'document-article-article1'), attachment('photo', 'own.png', 'document-article-article1'), attachment('other', 'other.pdf', 'document-article-article2'), attachment('shared', 'legacy.pdf', 'default'), attachment('global', 'global.pdf', 'default')];
      window.__TAURI_INTERNALS__ = { invoke: async (cmd, args = {}) => {
        window.calls.push({ cmd, args });
        if (cmd === 'app_info') return { name: 'Veloct', version: '1.0.0' };
        if (cmd === 'current_project') return { path: 'D:/isolated/resources', name: '资源栏验证项目' };
        if (cmd === 'list_documents') return [doc('article1'), doc('article2')];
        if (cmd === 'read_document') return doc(args.id);
        if (cmd === 'content_index') return { entries: [], issues: [], is_buildable: true };
        if (cmd === 'ai_settings') return { page_create_ready: false, providers: [], features: [], fallback: null };
        if (cmd === 'preview_status') return { site: { state: 'stopped' }, works: { state: 'stopped' }, site_url: null, works_url: null };
        if (cmd === 'attachment_messages' || cmd === 'check_attachment_images') return { upload_errors: [], health_errors: [] };
        if (cmd === 'list_attachments') return structuredClone(pool);
        if (cmd === 'storage_targets') return [{ id: 's3', name: '测试存储', implemented: true }];
        if (cmd === 'attachment_sync_settings') return { target_ids: [] };
        if (cmd === 'upload_eligibility') return { can_upload: true, upload_status: 'PENDING', reason: null };
        if (cmd === 'plugin:dialog|open') {
          if (window.holdDialog) await new Promise(resolve => { window.releaseDialog = resolve; });
          return window.dialogPaths;
        }
        if (cmd === 'add_attachment') {
          const name = args.sourcePath.split(/[\\/]/).pop();
          const item = attachment('imported-' + pool.length, name, args.namespace); pool.push(item);
          return { attachment: item, deduplicated: false };
        }
        if (cmd === 'attachment_markdown') {
          if (window.holdMarkdown) await new Promise(resolve => { window.releaseMarkdown = resolve; });
          const item = pool.find(item => item.id === args.id);
          return (item.original_name.endsWith('.png') ? '!' : '') + '[' + item.original_name + '](' + (item.remote_url || '/.attachments-cache/' + item.cache_path) + ')';
        }
        if (cmd === 'sync_upload_attachment') {
          const item = pool.find(item => item.id === args.id);
          if (window.uploadFails) { item.last_error = '模拟上传失败'; return [{ target_id: args.storageTargetId, error: '模拟上传失败' }]; }
          item.last_error = null; item.remote_url = 'https://assets.test/' + item.original_name;
          return [{ target_id: args.storageTargetId, error: null }];
        }
        if (cmd === 'save_document') {
          sources[args.id] = args.source;
          return { saved: true, status: 'saved', source: args.source, revision: { hash: 'saved-' + window.calls.length, size: args.source.length }, diagnostics: [] };
        }
        return null;
      } };
    });
    await page.goto('http://workbench.test/');
    const articles = page.locator('.nav-item').filter({ hasText: '文章管理' });
    const openArticle = async name => { await articles.click(); await page.getByRole('button', { name, exact: false }).first().click(); };
    await openArticle('资源测试');
    const panel = page.getByRole('complementary', { name: '本文资源' });
    const rows = panel.locator('.document-resources__list > li');
    const row = name => rows.filter({ has: page.locator('strong', { hasText: name }) });
    await row('own.pdf').waitFor();
    assert.equal(await rows.count(), 2, 'Own imports plus existing references, not the global pool');
    assert.equal(await rows.filter({ hasText: 'other.pdf' }).count(), 0);
    assert.equal(await panel.getByRole('combobox').inputValue(), '');
    assert.equal((await page.evaluate(() => window.calls)).filter(c => c.cmd === 'save_document').length, 0);
    const tabs = panel.getByRole('tab');
    await tabs.first().focus(); await page.keyboard.press('ArrowRight');
    assert.equal(await tabs.nth(1).getAttribute('aria-selected'), 'true');
    assert.equal(await rows.count(), 2, 'A local owned image plus a remote body image');
    await row('远程图').scrollIntoViewIfNeeded();
    await panel.locator('.document-resource__thumbnail span').filter({ hasText: '图片暂不可预览' }).waitFor();
    await panel.locator('.document-resources__body').evaluate(el => { el.scrollTop = 0; });
    assert.ok(await page.locator('.title-input').evaluate(el => el.getBoundingClientRect().width > 300), 'Title must not collapse behind header actions');
    await page.screenshot({ path: `target/document-resources-${width}.png` });
    await page.keyboard.press('Home');
    await panel.getByRole('tab', { name: /附件池/ }).click();
    const mdToggle = page.locator('.wysiwyg .toolbar').getByRole('button', { name: 'MD', exact: true });
    await mdToggle.click();
    const source = page.locator('textarea.markdown-source');
    const before = await source.inputValue();
    await source.evaluate(input => { const start = input.value.indexOf('中间内容'); input.focus(); input.setSelectionRange(start, start + '中间内容'.length); });
    await row('own.pdf').getByRole('button', { name: '插入', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('textarea.markdown-source').value.includes('[own.pdf]'));
    const inserted = await source.inputValue();
    assert.ok(inserted.includes('[own.pdf](/.attachments-cache/paper/own.pdf)'));
    assert.ok(!inserted.includes('中间内容') && inserted.includes('end'));
    await page.waitForFunction(() => window.calls.some(c => c.cmd === 'save_document' && c.args.source.includes('[own.pdf]')));
    assert.ok((await page.evaluate(() => window.calls)).filter(c => c.cmd === 'save_document').at(-1).args.source.startsWith('---\nid: article1\ntitle: 资源测试\nsummary: 原摘要\nunknown: keep-me\n---\n'));
    await page.evaluate(() => { window.dialogPaths = ['D:/fixtures/new.pdf']; });
    await panel.getByRole('button', { name: '＋ 添加附件', exact: true }).click();
    await row('new.pdf').waitFor();
    assert.equal(await source.inputValue(), inserted, 'Import must not change the writing buffer');
    const added = (await page.evaluate(() => window.calls)).find(c => c.cmd === 'add_attachment');
    assert.equal(added.args.namespace, 'document-article-article1');
    assert.equal(added.args.storageTargetId, 'local');
    await panel.getByRole('combobox').selectOption('s3');
    await row('new.pdf').getByRole('button', { name: '上传', exact: true }).click();
    await page.locator('.toast--error').filter({ hasText: '模拟上传失败' }).waitFor();
    assert.equal(await source.inputValue(), inserted);
    await page.evaluate(() => { window.uploadFails = false; });
    await row('new.pdf').getByRole('button', { name: '上传', exact: true }).click();
    await page.locator('.toast--success').getByText('上传完成，正文未改动。需要新地址时请点击「插入」。', { exact: true }).waitFor();
    assert.equal(await source.inputValue(), inserted, 'Upload must never rewrite old references');
    await mdToggle.click();
    const rich = page.locator('.wysiwyg [contenteditable="true"]');
    await rich.click(); await page.keyboard.press('Control+End');
    await row('new.pdf').getByRole('button', { name: '插入', exact: true }).click();
    await rich.locator('a[href="https://assets.test/new.pdf"]').waitFor();
    await panel.getByRole('tab', { name: /图片池/ }).click();
    await panel.getByText('插入网络图片', { exact: true }).first().click();
    await panel.getByLabel('网络图片地址').fill('https://assets.test/added.png');
    await panel.getByLabel('网络图片描述').fill('新网络图');
    await panel.getByRole('button', { name: '插入网络图片', exact: true }).click();
    await rich.locator('img[src="https://assets.test/added.png"]').waitFor();
    await rows.filter({ hasText: '新网络图' }).waitFor();
    assert.equal((await page.evaluate(() => window.calls)).some(c => ['download_network_image', 'remove_attachment', 'delete_remote_object', 'start_preview'].includes(c.cmd)), false);
    const layout = await page.locator('.workspace').evaluate(el => {
      const writing = el.querySelector('.pane').getBoundingClientRect(), side = el.querySelector('.document-resources').getBoundingClientRect();
      return { sidebarWidth: side.width, writingWidth: writing.width, ordered: side.x >= writing.right, overflow: el.scrollWidth > el.clientWidth };
    });
    assert.ok(layout.sidebarWidth >= 239 && layout.sidebarWidth <= 301 && layout.ordered && !layout.overflow, JSON.stringify(layout));
    assert.ok(await page.locator('.pane').evaluate(el => el.scrollWidth <= el.clientWidth));
    await panel.getByRole('tab', { name: /附件池/ }).click();
    await page.evaluate(() => { window.holdMarkdown = true; });
    await row('new.pdf').getByRole('button', { name: '插入', exact: true }).click();
    await page.waitForFunction(() => typeof window.releaseMarkdown === 'function');
    await openArticle('第二篇');
    await page.evaluate(() => { window.holdMarkdown = false; window.releaseMarkdown(); });
    await page.locator('.wysiwyg').getByRole('heading', { name: '第二篇正文' }).waitFor();
    await row('other.pdf').waitFor();
    assert.equal(await rows.count(), 1);
    assert.equal(await page.locator('.wysiwyg a[href="https://assets.test/new.pdf"]').count(), 0, 'Late insertion cannot leak across articles');
    await page.evaluate(() => { window.holdDialog = true; window.dialogPaths = ['D:/fixtures/late.pdf']; });
    await panel.getByRole('button', { name: '＋ 添加附件', exact: true }).click();
    await page.waitForFunction(() => typeof window.releaseDialog === 'function');
    await openArticle('资源测试');
    await page.evaluate(() => { window.holdDialog = false; window.releaseDialog(); });
    await row('new.pdf').waitFor();
    assert.equal((await page.evaluate(() => window.calls)).some(c => c.cmd === 'add_attachment' && c.args.sourcePath.endsWith('late.pdf')), false);
    await page.getByRole('button', { name: '收起资源栏', exact: true }).click();
    assert.equal(await panel.isVisible(), false);
    await page.getByRole('button', { name: '展开资源栏', exact: true }).click();
    await panel.waitFor();
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, currentArticleIsolation: true, tabsAndKeyboard: true, importWithoutInsertion: true, uploadFailureAndRetry: true, noAutomaticReferenceRewrite: true, cursorInsertionBothModes: true, frontmatterPreserved: true, remoteImagesNoDownload: true, lateActionsGuarded: true, poolSurvivesArticleSwitch: true, collapsible: true, layout, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
