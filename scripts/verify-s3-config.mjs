// Real Edge + production bundle; all IPC/account checks are fixtures. No cloud writes or real credentials.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  for (const width of [1440, 1024]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.route('http://workbench.test/**', async route => {
      const file = new URL(route.request().url()).pathname.slice(1) || 'index.html';
      try { await route.fulfill({ body: await readFile(path.join(process.cwd(), 'dist-ui', file)), contentType: ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png' })[path.extname(file)] || 'application/octet-stream' }); }
      catch { await route.fulfill({ status: 404, body: '' }); }
    });
    await page.addInitScript(() => {
      window.calls = []; window.verifyFailure = false;
      const row = (id, overrides = {}) => ({ id, name: id, kind: 's3', implemented: true, unimplemented_reason: null,
        path_prefix: null, public_url_prefix: 'https://cdn.example.com', repository: null, branch: null,
        endpoint: 'https://account.r2.cloudflarestorage.com', region: 'auto', bucket: 'picbed', access_key: 'fixture-access-id',
        instance: null, credential_ref: 'fixture-handle', has_credential: true, ...overrides });
      const targets = [row('old'), row('high', { upload_priority: 80 }), row('hidden', { hidden_from_upload: true, upload_priority: 999 })];
      window.__TAURI_INTERNALS__ = { invoke: async (cmd, args = {}) => {
        window.calls.push({ cmd, args });
        if (cmd === 'app_info') return { name: 'Veloct', version: '1.0.0' };
        if (cmd === 'current_project') return { path: 'D:/isolated/s3', name: 'S3 验证项目' };
        if (cmd === 'list_documents' || cmd === 'list_attachments' || cmd === 'attachment_objects') return [];
        if (cmd === 'content_index') return { entries: [], issues: [], is_buildable: true };
        if (cmd === 'ai_settings') return { page_create_ready: false, providers: [], features: [], fallback: null };
        if (cmd === 'preview_status') return { site: { state: 'stopped' }, works: { state: 'stopped' }, site_url: null, works_url: null };
        if (cmd === 'attachment_messages') return { upload_errors: [], health_errors: [] };
        if (cmd === 'attachment_orphans') return { safe: [], needs_confirmation: [] };
        if (cmd === 'attachment_cache_directory') return 'D:/isolated/s3/.attachments-cache';
        if (cmd === 'attachment_sync_settings') return { target_ids: [] };
        if (cmd === 'storage_targets') return structuredClone(targets);
        if (cmd === 'storage_kinds') return ['local', 's3', 'github', 'lsky_pro'].map(kind => ({ kind, implemented: true, unimplemented_reason: null }));
        if (cmd === 'validate_storage_target') return null;
        if (cmd === 'test_storage_target') { if (window.verifyFailure) throw new Error('连接拒绝（fixture），未保存配置'); return 'bucket reachable'; }
        if (cmd === 'save_storage_target') {
          const { credential, ...request } = args.request;
          const index = targets.findIndex(t => t.id === request.id);
          const saved = row(request.id, { ...request, has_credential: !!credential || index >= 0 });
          if (index < 0) targets.push(saved); else targets[index] = saved;
          return null;
        }
        return null;
      } };
    });
    const calls = cmd => page.evaluate(cmd => window.calls.filter(c => c.cmd === cmd), cmd);
    await page.goto('http://workbench.test');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page.getByRole('button', { name: '附件存储', exact: true }).click();
    const panel = page.locator('.attachment-panel--settings');
    await panel.getByLabel(/^类型/).selectOption('s3');
    await panel.getByLabel(/^标识\(id\)/).fill('new-r2');
    await panel.getByLabel(/^名称/).fill('R2 fixture');
    const fields = panel.locator('.s3-config');
    assert.equal(await fields.getByLabel(/^上传时重命名文件方式/).inputValue(), 'uuid');
    assert.equal(await fields.getByLabel(/^重复文件名处理方式/).inputValue(), 'random_alphanumeric');
    assert.equal(await fields.getByLabel(/^Endpoint 访问风格/).inputValue(), 'path');
    await fields.getByLabel(/^Bucket 桶名称/).fill('picbed');
    await fields.locator('input[placeholder="账户ID.r2.cloudflarestorage.com"]').fill('https://account.r2.cloudflarestorage.com');
    await fields.getByLabel(/^Access Key ID/).fill('fixture-access-id');
    await fields.getByLabel(/^Access Key Secret/).fill('fixture-secret-not-real');
    assert.equal(await fields.getByLabel(/^Access Key ID/).getAttribute('type'), 'password');
    assert.equal(await fields.getByLabel(/^Access Key Secret/).getAttribute('type'), 'password');
    await fields.getByLabel(/^Region/).fill('auto');
    await fields.getByLabel(/^上传目录/).fill('images/${year}/${month}');
    await fields.getByLabel(/^绑定域名（CDN 域名）/).fill('cdn.example.com');
    await fields.getByLabel(/^在上传界面隐藏/).check();
    await fields.getByLabel(/^上传界面显示优先级/).fill('42');
    await fields.getByRole('button', { name: '添加网址后缀', exact: true }).click();
    await fields.getByLabel('文件类型 1', { exact: true }).fill('png,jpg');
    await fields.getByLabel('网址后缀 1', { exact: true }).fill('!webp');
    await fields.getByRole('button', { name: '添加网址后缀', exact: true }).click();
    await fields.getByLabel('文件类型 2', { exact: true }).fill('*');
    await fields.getByLabel('网址后缀 2', { exact: true }).fill('?raw=1');
    await fields.getByRole('button', { name: '上移后缀规则 2', exact: true }).click();
    await fields.getByLabel(/^缩略图参数/).selectOption('custom');
    await fields.getByLabel(/^自定义缩略图参数/).fill('?width={width}');
    await page.evaluate(() => { window.verifyFailure = true; });
    await panel.getByRole('button', { name: '验证对象存储', exact: true }).click();
    await page.locator('.toast--error').filter({ hasText: '连接拒绝' }).waitFor();
    assert.equal((await calls('save_storage_target')).length, 0);
    await page.evaluate(() => { window.verifyFailure = false; });
    await panel.getByRole('button', { name: '验证对象存储', exact: true }).click();
    await page.locator('.toast--success').getByText(/对象存储验证通过/).waitFor();
    const verified = (await calls('test_storage_target')).at(-1).args.request;
    assert.equal(verified.endpoint, 'https://account.r2.cloudflarestorage.com');
    assert.equal(verified.public_url_prefix, 'https://cdn.example.com');
    assert.equal(verified.region, 'auto'); assert.equal(verified.s3_policy.thumbnail, '?width={width}');
    assert.deepEqual(verified.s3_policy.url_suffixes.map(r => r.extensions), ['*', 'png,jpg']);
    assert.equal((await calls('save_storage_target')).length, 0);
    await panel.getByRole('button', { name: '创建目标', exact: true }).click();
    await page.locator('.toast--success').getByText(/存储目标已保存/).last().waitFor();
    assert.equal((await calls('save_storage_target')).length, 1);
    const item = panel.locator('.row-list > li').filter({ hasText: 'R2 fixture' });
    await item.getByRole('button', { name: '修改', exact: true }).click();
    assert.equal(await panel.locator('.s3-config').getByLabel(/^Access Key Secret/).inputValue(), '');
    assert.equal(await panel.locator('.s3-config').getByLabel(/^上传目录/).inputValue(), 'images/${year}/${month}');
    assert.equal(await panel.locator('.s3-config').getByLabel(/^上传界面显示优先级/).inputValue(), '42');
    await panel.getByRole('button', { name: '保存修改', exact: true }).click();
    await page.locator('.toast--success').getByText(/存储目标已保存/).last().waitFor();
    assert.equal((await calls('save_storage_target')).at(-1).args.request.credential, null);
    await page.getByRole('button', { name: '附件管理', exact: true }).click();
    const manager = page.locator('.attachment-panel:not(.attachment-panel--settings)');
    const options = await manager.locator('select').first().locator('option').allTextContents();
    assert.deepEqual(options, ['(选择目标)', 'high', 'old']);
    assert.equal((await calls('upload_attachment')).length, 0); assert.equal((await calls('sync_upload_attachment')).length, 0);
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, protocolComposition: true, secretMaskedAndNotReturned: true, policyRoundtrip: true, firstMatchRuleOrder: true, readOnlyVerificationAndFailure: true, hiddenAndPriorityInUpload: true, noRealUpload: true, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
