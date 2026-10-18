// Real Edge + production bundle; isolated IPC/cloud fixtures only, never a user's blog.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
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
    await page.addInitScript(() => {
      window.calls = []; window.failImage = true; window.holdImage = false;
      let settings = { name: '验证', description: '', url: '', base: '/', language: 'zh-CN', author: { name: '旧名字', bio: '保留简介', avatar: 'https://cdn.example/old-avatar.png' }, social: [], hero: { title: '' }, homepage: { cover: 'https://cdn.example/old-cover.png', githubUsername: '', showBlogHeatmap: true, showGithubHeatmap: true }, articleDisplayCount: 10, archiveDisplayCount: 10, categoryDisplayCount: 10, tagDisplayCount: 10, aliasPolicy: 'redirect', pageTransition: 'rise', seo: { allow_indexing: true, engines: [] }, filing: { icp: '', icpUrl: '', police: '', policeUrl: '', moe: '', moeUrl: '' } };
      window.__TAURI_INTERNALS__ = { invoke: async (cmd, args = {}) => {
        window.calls.push({ cmd, args });
        if (cmd === 'app_info') return { name: 'Veloct', version: '1.0.0' };
        if (cmd === 'current_project') return { path: 'D:/isolated/site-images', name: '图片验证' };
        if (cmd === 'list_documents') return [];
        if (cmd === 'content_index') return { entries: [], issues: [], is_buildable: true };
        if (cmd === 'preview_status') return { site: { state: 'stopped' }, works: { state: 'stopped' }, site_url: null, works_url: null };
        if (cmd === 'attachment_messages') return { upload_errors: [], health_errors: [] };
        if (cmd === 'site_settings') return { settings: structuredClone(settings), revision: { hash: 'fixture', size: 1 } };
        if (cmd === 'save_site_settings') { settings = structuredClone(args.settings); return { settings, revision: { hash: 'saved', size: 1 }, changed: true }; }
        if (cmd === 'save_site_avatar' || cmd === 'save_site_cover') {
          if (window.holdImage) await new Promise(resolve => { window.finishImage = resolve; });
          if (window.failImage) throw new Error('图片已保留在附件池，但上传首选目标失败；原图片地址未更改。');
          return cmd === 'save_site_avatar' ? 'https://cdn.example/pool-avatar.png' : 'https://cdn.example/pool-cover.png';
        }
        return null;
      }};
    });
    const calls = cmd => page.evaluate(cmd => window.calls.filter(call => call.cmd === cmd), cmd);
    const choose = async (button, name) => {
      const chooser = page.waitForEvent('filechooser'); await button.click();
      await (await chooser).setFiles({ name, mimeType: 'image/png', buffer: validPng });
    };
    await page.goto('https://workbench.test');
    // Canvas-generated PNG: tolerantly decoded test icons can fail the stricter ImageBitmap decoder.
    const validPng = Buffer.from(await page.evaluate(() => { const canvas = document.createElement('canvas'); canvas.width = 16; canvas.height = 8; canvas.getContext('2d').fillRect(0, 0, 16, 8); return canvas.toDataURL('image/png').split(',')[1]; }), 'base64');
    await page.getByRole('button', { name: '外观', exact: true }).click();
    const avatar = page.getByLabel('首页头像', { exact: true });
    const cover = page.getByLabel('首页头图', { exact: true });
    const save = page.getByRole('button', { name: '保存站点信息', exact: true });
    await avatar.waitFor();
    await choose(page.getByRole('button', { name: '上传并裁切', exact: true }), 'avatar.png');
    const confirm = page.getByRole('button', { name: '使用这张头像', exact: true });
    await confirm.click(); await page.locator('.toast--error').last().waitFor();
    assert.equal(await avatar.inputValue(), 'https://cdn.example/old-avatar.png');
    assert.equal((await calls('save_site_settings')).length, 0);
    await page.evaluate(() => { window.failImage = false; window.holdImage = true; });
    await confirm.click();
    assert.equal(await save.isDisabled(), true);
    await page.getByLabel('作者名称', { exact: true }).fill('上传中编辑的新名字');
    await page.evaluate(() => window.finishImage());
    await page.waitForFunction(() => document.querySelector('#author-avatar')?.value.includes('pool-avatar'));
    assert.equal(await page.getByLabel('作者名称', { exact: true }).inputValue(), '上传中编辑的新名字');
    assert.equal((await calls('save_site_settings')).length, 0);
    const bytes = Buffer.from((await calls('save_site_avatar')).at(-1).args.dataBase64, 'base64');
    assert.ok(bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])));
    assert.equal(bytes.readUInt32BE(16), 512); assert.equal(bytes.readUInt32BE(20), 512);
    await page.evaluate(() => { window.holdImage = false; window.failImage = true; });
    await choose(page.getByRole('button', { name: '上传头图', exact: true }), 'cover.png');
    await page.locator('.toast--error').last().waitFor();
    assert.equal(await cover.inputValue(), 'https://cdn.example/old-cover.png');
    await page.evaluate(() => { window.failImage = false; });
    await choose(page.getByRole('button', { name: '上传头图', exact: true }), 'cover.png');
    await page.waitForFunction(() => document.querySelector('#home-cover')?.value.includes('pool-cover'));
    assert.equal((await calls('save_site_settings')).length, 0);
    await save.click(); await page.locator('.toast--success').last().waitFor();
    assert.equal((await calls('save_site_settings')).at(-1).args.settings.homepage.cover, 'https://cdn.example/pool-cover.png');
    assert.equal((await calls('save_site_settings')).at(-1).args.settings.author.avatar, 'https://cdn.example/pool-avatar.png');
    // A late response must not update a re-opened panel, trigger another upload, or save automatically.
    await page.evaluate(() => { window.holdImage = true; });
    await choose(page.getByRole('button', { name: '上传头图', exact: true }), 'late.png');
    await page.waitForFunction(() => typeof window.finishImage === 'function' && window.calls.filter(c => c.cmd === 'save_site_cover').length === 3);
    await page.getByRole('button', { name: '仪表盘', exact: true }).click();
    await page.evaluate(() => window.finishImage());
    await page.getByRole('button', { name: '外观', exact: true }).click();
    await avatar.waitFor(); assert.equal(await cover.inputValue(), 'https://cdn.example/pool-cover.png');
    assert.equal((await calls('save_site_settings')).length, 1);
    assert.equal((await calls('add_attachment_bytes')).length, 0); // The backend owns the project-bound pool/upload workflow.
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, avatarCrop512Png: true, bothImagesPreferredUpload: true, failureKeepsOldUrls: true, retry: true, noAutoSave: true, authorEditsPreserved: true, lateResponsesIgnored: true, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
