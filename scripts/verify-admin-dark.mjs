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
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        const fixtures = {
          list_works: {works:[],servable:0}, work_screenshot_limit:8388608,
          list_releases:[], list_backups:[], cloud_backup_targets:[], backup_directory:'D:/isolated/backups',
          site_install_status:{state:'ready',output_tail:'',npm:'npm',site_dir:'D:/isolated/site',astro_ready:true},
          deploy_targets:{enabled:false,targets:[]},
          read_log:{entries:[],total:0,info:0,warning:0,error:0,categories:[]},
          network_image_sources:[], network_image_topics:[],
          linked_knowledge:{items:[],revision:null}
        };
        if(cmd in fixtures) { window.calls.push({cmd,args}); return fixtures[cmd]; }
        return invoke(cmd,args);
      };
    }, { png: png.toString('base64') });

    await page.goto('https://workbench.test');
    const checks = [];
    async function inspect(name, title, screenshot = false) {
      await page.locator('.sidebar').getByRole('button',{name,exact:true}).click();
      await page.locator('.content h1').filter({hasText:title}).first().waitFor();
      await page.evaluate(() => Promise.all(document.getAnimations().map(a=>a.finished.catch(()=>{}))));
      const metrics = await page.evaluate(() => {
        const content=document.querySelector('.content');
        const heading=content.querySelector('h1');
        const h=getComputedStyle(heading);
        return {titleSize:h.fontSize,titleFont:h.fontFamily,overflow:content.scrollWidth>content.clientWidth+1,
          cards:[...content.querySelectorAll('.card')].map(c=>({radius:getComputedStyle(c).borderRadius,bg:getComputedStyle(c).backgroundColor})),
          scrollOwners:[...content.querySelectorAll('.admin-panel')].filter(e=>getComputedStyle(e).overflowY==='auto').length};
      });
      assert.equal(metrics.titleSize,'28px',name);
      assert.equal(metrics.overflow,false,name+' horizontal overflow');
      assert.equal(metrics.scrollOwners,0,name+' nested scroll');
      for(const c of metrics.cards) {assert.equal(c.radius,'12px',name+' radius');assert.equal(c.bg,'rgb(36, 33, 30)',name+' surface');}
      if(screenshot) await page.screenshot({path:'target/admin-dark-'+name+'-'+width+'.png'});
      checks.push({name,...metrics});
    }
    for(const [name,title] of [['仪表盘','仪表盘'],['文章管理','文章'],['页面管理','页面'],['知识库管理','知识库'],['附件管理','附件'],['作品管理','作品管理'],['菜单管理','菜单管理'],['备份与恢复','备份'],['构建与发布','发布'],['运行日志','日志'],['外观','外观'],['设置','设置']]) await inspect(name,title,['作品管理','备份与恢复','设置'].includes(name));
    for(const label of ['部署目标','附件存储','网络图片','备份设置']) {
      await page.getByRole('button',{name:label,exact:true}).click();
      await page.evaluate(() => Promise.all(document.getAnimations().map(a=>a.finished.catch(()=>{}))));
      assert.equal(await page.locator('.content').evaluate(e=>e.scrollWidth>e.clientWidth+1),false,label);
      assert.equal(await page.locator('.content h1').count(),1,label+' single page heading');
      await page.locator('.page-header--section h2').waitFor();
      assert.equal(await page.locator('.page-header--section h2').evaluate(e=>getComputedStyle(e).fontSize),'18px',label+' section heading');
      await page.screenshot({path:'target/admin-dark-'+label+'-'+width+'.png'});
    }
    assert.equal(await page.locator('.settings-tabs .is-active').getAttribute('aria-pressed'),'true');
    assert.deepEqual(errors,[]);
    const commands=await page.evaluate(()=>window.calls.map(c=>c.cmd));
    assert.equal(commands.some(c=>['deploy_now','publish_project','install_site_dependencies','manual_backup','upload_attachment'].includes(c)),false);
    console.log(JSON.stringify({width,checks,pageErrors:errors,noImplicitWrites:true}));
    await page.close();
  }
} finally { await browser.close(); }
