// Production UI in real Edge. IPC and every cloud response are isolated fixtures, not a real deployment.
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
      const name = new URL(route.request().url()).pathname;
      const file = name === '/' ? 'index.html' : name.slice(1);
      try { await route.fulfill({ body: await readFile(path.join(process.cwd(), 'dist-ui', file)), contentType: ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png' })[path.extname(file)] || 'application/octet-stream' }); }
      catch { await route.fulfill({ status: 404, body: '' }); }
    });
    await page.addInitScript(() => {
      window.calls = []; window.cloudFailure = false; window.holdDeploy = false;
      const row = (id, kind, project, works) => ({ id, kind, kind_label: kind === 'edgeone_cn' ? 'EdgeOne(中国大陆)' : 'EdgeOne(国际)', enabled: true, manual: false, project, works_project: works, works_origin: works ? 'https://works.example.com' : null, credential_ref: 'fixture-handle', has_credential: true });
      const targets = [row('cn', 'edgeone_cn', 'main-cn', 'works-cn'), row('global', 'edgeone_global', 'main-global', 'works-global')];
      window.__TAURI_INTERNALS__ = { invoke: async (cmd, args = {}) => {
        window.calls.push({ cmd, args });
        if (cmd === 'app_info') return { name: 'Veloct', version: '1.0.0' };
        if (cmd === 'current_project') return { path: 'D:/isolated/edgeone', name: 'EdgeOne 测试项目' };
        if (cmd === 'list_documents') return [];
        if (cmd === 'content_index') return { entries: [], issues: [], is_buildable: true };
        if (cmd === 'ai_settings') return { page_create_ready: false, providers: [], features: [], fallback: null };
        if (cmd === 'preview_status') return { site: { state: 'stopped' }, works: { state: 'stopped' }, site_url: null, works_url: null };
        if (cmd === 'attachment_messages' || cmd === 'check_attachment_images') return { upload_errors: [], health_errors: [] };
        if (cmd === 'deploy_targets') return { enabled: window.deployEnabled ?? true, targets: structuredClone(targets) };
        if (cmd === 'save_deploy_target') {
          const request = args.request;
          const saved = row(request.id, request.kind, request.project, request.works_project);
          const at = targets.findIndex(t => t.id === request.id);
          if (at < 0) targets.push(saved); else targets[at] = saved;
          return null;
        }
        if (cmd === 'set_deploy_enabled') { window.deployEnabled = args.enabled; return null; }
        if (cmd === 'deploy_now') {
          if (window.holdDeploy) await new Promise(resolve => { window.finishDeploy = resolve; });
          const failed = window.cloudFailure;
          return { release_id: 'release-fixture', pushed_files: 12, all_succeeded: !failed, needing_attention: failed ? ['cn'] : [], started_at: '', finished_at: '', seo_state: 'not_requested', targets: targets.map((t, i) => ({ target_id: t.id, kind: t.kind, status: failed && i === 0 ? 'partial' : 'succeeded', main_status: 'succeeded', main_detail: 'Production confirmed', works_status: failed && i === 0 ? 'failed' : 'succeeded', works_detail: failed && i === 0 ? 'EdgeOne 部署未成功' : 'Production confirmed', works_origin: t.works_origin, needs_attention: failed && i === 0 })) };
        }
        return null;
      } };
    });
    await page.goto('http://workbench.test');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page.getByRole('button', { name: '部署目标', exact: true }).click();
    const panel = page.locator('.deploy-panel');
    await panel.getByText('EdgeOne(中国大陆)', { exact: true }).waitFor();
    const calls = cmd => page.evaluate(cmd => window.calls.filter(c => c.cmd === cmd), cmd);
    assert.equal((await calls('deploy_now')).length, 0, 'Opening settings must not deploy');
    const kind = panel.locator('select');
    await kind.selectOption('edgeone_mainland');
    await panel.getByText('主站项目 ID', { exact: true }).waitFor();
    await kind.selectOption('edgeone_international');
    assert.equal(await kind.inputValue(), 'edgeone_international');
    await panel.getByText(/不需要腾讯云 SecretId/).waitFor();
    const fields = panel.locator('input:not([type=checkbox])');
    await fields.nth(0).fill('new-global');
    await fields.nth(1).fill('same-id');
    await fields.nth(2).fill('same-id');
    await panel.getByRole('button', { name: '保存目标', exact: true }).click();
    await page.locator('.toast--error').filter({ hasText: '互相覆盖' }).waitFor();
    assert.equal((await calls('save_deploy_target')).length, 0);
    await fields.nth(2).fill('works-new');
    await fields.nth(3).fill('https://works-new.example.com');
    await fields.nth(4).fill('fixture-api-token');
    await panel.getByRole('button', { name: '保存目标', exact: true }).click();
    await page.locator('.toast--success').getByText('目标已保存。', { exact: false }).waitFor();
    assert.equal((await calls('save_deploy_target')).length, 1);
    assert.equal((await calls('deploy_now')).length, 0, 'Saving credentials must not deploy');
    assert.equal(await fields.nth(4).inputValue(), '', 'Token is cleared, never reloaded from OS store');
    await panel.locator('.target-list li').filter({ hasText: 'cn' }).first().getByRole('button', { name: '编辑', exact: true }).click();
    assert.equal(await kind.inputValue(), 'edgeone_mainland', 'Stored kind alias must load into region selector');
    assert.equal(await fields.nth(1).inputValue(), 'main-cn');
    assert.equal(await fields.nth(4).inputValue(), '');
    await page.evaluate(() => { window.holdDeploy = true; });
    await panel.getByRole('button', { name: '部署当前构建', exact: true }).click();
    await page.waitForFunction(() => typeof window.finishDeploy === 'function');
    assert.equal((await calls('deploy_now')).length, 1);
    assert.equal(await panel.getByRole('button', { name: '推送中…', exact: true }).isDisabled(), true);
    assert.equal(await panel.getByRole('checkbox').isDisabled(), true);
    await page.evaluate(() => { window.holdDeploy = false; window.finishDeploy(); });
    await page.locator('.toast--success').getByText('所有已启用目标部署成功。', { exact: true }).waitFor();
    assert.equal(await panel.locator('.target-results li').count(), 3);
    await page.evaluate(() => { window.cloudFailure = true; });
    await panel.getByRole('button', { name: '部署当前构建', exact: true }).click();
    await page.locator('.toast').getByText(/部署检查已结束，部分目标未成功/).waitFor();
    await panel.getByText('需要重发的目标:cn', { exact: true }).waitFor();
    assert.equal(await page.locator('.toast').getByText('所有已启用目标部署成功。', { exact: true }).count(), 0);
    assert.equal((await calls('deploy_now')).length, 2);
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ width, regionSelection: true, explicitDeployOnly: true, secretsNotRedisplayed: true, sameProjectRefused: true, allTargetsVisible: true, partialFailureHonest: true, asyncBusyControls: true, pageErrors: errors }));
    await page.close();
  }
} finally { await browser.close(); }
