// Production bundle + real Edge; IPC/native/clipboard fixtures only. Never opens a real blog or uploads.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aU1cAAAAASUVORK5CYII=', 'base64');
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  for (const width of [1440, 1024]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.route('https://workbench.test/**', async route => {
      const name = new URL(route.request().url()).pathname.slice(1) || 'index.html';
      try { await route.fulfill({ body: await readFile(path.join(process.cwd(), 'dist-ui', name)), contentType: ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png' })[path.extname(name)] || 'application/octet-stream' }); }
      catch { await route.fulfill({ status: 404, body: '' }); }
    });
    await page.addInitScript(({ png }) => {
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.calls = []; window.failCheck = false; window.eventCallbacks = {}; window.callbackMap = {}; let callback = 0;
      const documentView = { id: 'one', kind: 'article', title: '保留正文测试', slug: 'one', wip: true, wip_state: 'draft', route: '/wip/one/', source: '---\nid: one\ntitle: 保留正文测试\n---\n# 原正文\n', revision: { hash: 'one', size: 50 }, diagnostics: [], matches_disk: true };
      const pool = [];
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWebview: { label: 'main' }, currentWindow: { label: 'main' } },
        transformCallback: handler => { window.callbackMap[++callback] = handler; return callback; }, unregisterCallback: id => { delete window.callbackMap[id]; },
        invoke: async (cmd, args = {}) => {
          window.calls.push({ cmd, args });
          if (cmd === 'app_info') return { name: 'Veloct', version: '1.0.0' };
          if (cmd === 'current_project') return { path: 'D:/isolated/five-ui', name: '界面验证项目' };
          if (cmd === 'list_documents') return [documentView];
          if (cmd === 'read_document') return structuredClone(documentView);
          if (cmd === 'save_document') return { saved: true, status: 'saved', source: args.source, revision: { hash: 'saved', size: args.source.length }, diagnostics: [] };
          if (cmd === 'content_index') return { entries: [], issues: [], is_buildable: true };
          if (cmd === 'preview_status') return { site: { state: 'stopped' }, works: { state: 'stopped' }, site_url: null, works_url: null };
          if (cmd === 'attachment_messages' || cmd === 'check_attachment_images') {
            if (cmd === 'check_attachment_images' && window.failCheck) throw new Error('完整错误详情：' + '请检查存储目标，文件未上传。'.repeat(30));
            return { upload_errors: [], health_errors: [] };
          }
          if (cmd === 'list_attachments') return structuredClone(pool);
          if (cmd === 'attachment_objects') return [];
          if (cmd === 'attachment_orphans') return { safe: [], needs_confirmation: [] };
          if (cmd === 'attachment_cache_directory') return 'D:/isolated/five-ui/.attachments-cache';
          if (cmd === 'attachment_sync_settings') return { target_ids: [] };
          if (cmd === 'storage_targets') return [
            { id: 'low', name: '低优先级', kind: 's3', implemented: true, upload_priority: 1 },
            { id: 'hidden', name: '隐藏策略', kind: 's3', implemented: true, hidden_from_upload: true, upload_priority: 999 },
            { id: 'high', name: '高优先级', kind: 's3', implemented: true, upload_priority: 80 },
          ];
          if (cmd === 'storage_kinds') return [{ kind: 'local', implemented: true }];
          if (cmd === 'upload_eligibility') return { can_upload: true, upload_status: 'PENDING', reason: null };
          if (cmd === 'add_attachment_bytes' || cmd === 'add_attachment') {
            const name = args.name || args.sourcePath.split(/[\\/]/).pop();
            const item = { id: 'item-' + pool.length, original_name: name, content_hash: 'hash', size_bytes: 10, cache_path: 'hash/' + name, storage_target_id: args.storageTargetId, storage_namespace: args.namespace, upload_status: 'PENDING', reference_status: 'UNAPPLIED', cache_status: 'PRESENT', created_at: '', reference_count: 0 };
            pool.push(item); return { attachment: item, outcome: 'added', markdown: '[' + name + '](/.attachments-cache/hash/' + name + ')' };
          }
          if (cmd === 'plugin:dialog|open') return ['D:/isolated/native.png'];
          if (cmd === 'preview_attachment_file') return { name: args.sourcePath.split(/[\\/]/).pop(), size: 68, data_url: 'data:image/png;base64,' + png };
          if (cmd === 'plugin:event|listen') { window.eventCallbacks[args.event] = args.handler; return args.handler; }
          if (cmd === 'plugin:event|unlisten') { delete window.eventCallbacks[args.event]; return null; }
          if (cmd === 'site_settings') return { settings: { name: '验证', description: '', url: '', base: '/', language: 'zh-CN', author: { name: '', bio: '', avatar: '' }, social: [], hero: { title: '' }, homepage: { cover: '', githubUsername: '', showBlogHeatmap: true, showGithubHeatmap: true }, articleDisplayCount: 10, archiveDisplayCount: 10, categoryDisplayCount: 10, tagDisplayCount: 10, aliasPolicy: 'redirect', pageTransition: 'rise', seo: { allow_indexing: true, engines: [] }, filing: { icp: '', icpUrl: '', police: '', policeUrl: '', moe: '', moeUrl: '' } }, baseline: { hash: 'fixture' } };
          return null;
        },
      };
    }, { png: png.toString('base64') });
    const calls = cmd => page.evaluate(cmd => window.calls.filter(call => call.cmd === cmd), cmd);
    await page.goto('https://workbench.test');
    const back = page.getByRole('button', { name: '返回上一页', exact: true });
    await page.getByRole('button', { name: '附件管理', exact: true }).click();
    const manager = page.locator('.attachment-panel');
    const target = manager.getByLabel(/^存储目标/);
    await target.waitFor();
    assert.deepEqual(await target.locator('option').allTextContents(), ['仅本地缓存', '高优先级', '低优先级']);
    assert.equal(await target.inputValue(), 'local'); await target.selectOption('high');
    const picker = manager.locator('.attachment-picker'); const drop = picker.getByRole('button', { name: '选择或拖拽文件' });
    const add = manager.getByRole('button', { name: '加入附件池', exact: true });
    assert.equal(await add.isDisabled(), true);
    const chooser = page.waitForEvent('filechooser'); await drop.click();
    await (await chooser).setFiles({ name: 'chosen.png', mimeType: 'image/png', buffer: png });
    await picker.locator('img').waitFor();
    assert.equal((await calls('add_attachment_bytes')).length, 0);
    assert.equal(await picker.locator('img').evaluate(image => image.complete && image.naturalWidth > 0), true);
    await add.click(); await page.locator('.toast--success').last().waitFor();
    assert.equal((await calls('add_attachment_bytes')).at(-1).args.storageTargetId, 'high');
    assert.ok(Buffer.from((await calls('add_attachment_bytes')).at(-1).args.dataBase64, 'base64').equals(png));
    assert.equal((await calls('upload_attachment')).length, 0);
    // Browser drag and hover-to-paste are independently exercised before any insertion.
    await drop.evaluate((element, bytes) => {
      const transfer = new DataTransfer(); transfer.items.add(new File([Uint8Array.from(atob(bytes), c => c.charCodeAt(0))], 'dropped.png', { type: 'image/png' }));
      element.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    }, png.toString('base64'));
    await picker.getByText('dropped.png', { exact: true }).waitFor();
    await drop.hover();
    await page.evaluate(bytes => {
      const transfer = new DataTransfer(); transfer.items.add(new File([Uint8Array.from(atob(bytes), c => c.charCodeAt(0))], 'pasted.png', { type: 'image/png' }));
      document.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: transfer }));
    }, png.toString('base64'));
    await picker.getByText('pasted.png', { exact: true }).waitFor();
    assert.equal((await calls('add_attachment_bytes')).length, 1);
    await picker.getByRole('button', { name: '移除', exact: true }).click();
    // Native dialog and physical-coordinate Tauri drag event use read-only previews, not an add command.
    await page.evaluate(() => { window.isTauri = true; });
    await page.getByRole('button', { name: '作品管理', exact: true }).click();
    await page.getByRole('button', { name: '附件管理', exact: true }).click();
    await drop.click(); await picker.getByText('native.png', { exact: true }).waitFor();
    assert.equal((await calls('add_attachment')).length, 0);
    const bounds = await drop.boundingBox();
    await page.waitForFunction(() => window.eventCallbacks['tauri://drag-drop']);
    await page.evaluate(({ x, y }) => {
      const id = window.eventCallbacks['tauri://drag-drop'];
      window.callbackMap[id]({ event: 'tauri://drag-drop', id, payload: { paths: ['D:/isolated/native-drop.png'], position: { x: x * devicePixelRatio, y: y * devicePixelRatio } } });
    }, { x: bounds.x + 20, y: bounds.y + 20 });
    await picker.getByText('native-drop.png', { exact: true }).waitFor();
    assert.equal((await calls('add_attachment')).length, 0);
    await add.click(); await page.locator('.toast--success').last().waitFor();
    assert.equal((await calls('add_attachment')).at(-1).args.sourcePath, 'D:/isolated/native-drop.png');
    // Error detail is full/selectable, closes with Escape, progress runs five seconds.
    await page.evaluate(() => { window.failCheck = true; });
    await manager.getByRole('button', { name: '检测图片', exact: true }).click();
    const errorToast = page.locator('.toast--error').last(); await errorToast.waitFor();
    const geometry = await page.locator('.toast-stack').boundingBox(); assert.ok(geometry.x + geometry.width > width - 30 && geometry.y < 100);
    assert.equal(await errorToast.locator('.toast__progress').evaluate(el => getComputedStyle(el).animationDuration), '5s');
    await errorToast.getByRole('button', { name: /操作没有完成/ }).click();
    const details = page.getByRole('dialog', { name: '消息详情' }); await details.waitFor();
    assert.ok((await details.innerText()).length > 300);
    assert.equal(await details.locator('.notification-details__body').evaluate(el => getComputedStyle(el).userSelect), 'text');
    await page.keyboard.press('Escape'); await details.waitFor({ state: 'hidden' });
    await page.evaluate(() => { window.failCheck = false; });
    await manager.getByRole('button', { name: '检测图片', exact: true }).click();
    await page.locator('.toast--success').last().waitFor();
    await page.waitForFunction(() => !document.querySelector('.toast'), null, { timeout: 6500 });
    await manager.getByRole('button', { name: '检测图片', exact: true }).click();
    await page.locator('.toast--success').last().waitFor(); // same message must appear on repeated action
    // Section-aware app history: no external browser back, buffer preserved.
    await page.getByRole('button', { name: '文章管理', exact: true }).click();
    await page.getByRole('button', { name: /保留正文测试/ }).first().click();
    await page.locator('.toolbar').getByRole('button', { name: 'MD', exact: true }).click();
    const source = page.locator('textarea.markdown-source'); await source.fill('# 暂存内容不应丢失\n');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page.getByRole('button', { name: '附件存储', exact: true }).click();
    await back.click(); await page.getByRole('button', { name: '站点设置', exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '站点设置', exact: true }).getAttribute('class'), 'is-active');
    await back.click(); await page.locator('.title-input').waitFor();
    await page.locator('.toolbar').getByRole('button', { name: 'MD', exact: true }).click();
    await source.waitFor(); assert.ok((await source.inputValue()).includes('暂存内容不应丢失'));
    assert.equal(await page.getByText('AI 建页', { exact: true }).count(), 0);
    assert.equal(await page.getByText('AI 与模型', { exact: true }).count(), 0);
    assert.equal((await calls('ai_settings')).length, 0);
    assert.equal((await calls('generate_page')).length, 0);
    assert.deepEqual(errors, []);
    await page.getByRole('button', { name: '附件管理', exact: true }).click();
    await drop.click(); await picker.getByText('native.png', { exact: true }).waitFor();
    await picker.locator('figure').scrollIntoViewIfNeeded();
    await page.locator('.content').evaluate(el => el.getAnimations().forEach(animation => animation.finish()));
    await page.screenshot({ path: `target/five-ui-${width}.png` });
    console.log(JSON.stringify({ width, targetDropdown: true, previewBeforeAdd: true, browserAndNativePickDrop: true, hoverPaste: true, noAutomaticUpload: true, fiveSecondToastAndFullDetails: true, repeatedNotice: true, backPreservesBuffer: true, aiUiAndCallsRemoved: true, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
