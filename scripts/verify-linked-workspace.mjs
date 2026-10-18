// Browser regression against the production workbench bundle; all IPC is isolated, no credentials.
// Requires Playwright in target/browser-validation (validation-only, not an application dependency).
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const browser = await chromium.launch({channel:'msedge',headless:true});
try {
  const page = await browser.newPage({viewport:{width:1440,height:900}});
  const errors = []; page.on('pageerror', e=>errors.push(e.message));
  await page.route('http://workbench.test/**', async route=>{
    const name = new URL(route.request().url()).pathname;
    try { await route.fulfill({body:await readFile(path.join(process.cwd(),'dist-ui',name==='/'?'index.html':name)),contentType:({'.html':'text/html','.js':'text/javascript','.css':'text/css','.png':'image/png'})[path.extname(name==='/'?'index.html':name)] || 'application/octet-stream'}); }
    catch { await route.fulfill({status:404,body:''}); }
  });
  await page.addInitScript(()=>{
    window.calls=[];
    let docs=[{id:'article1',kind:'article',title:'Delete fixture',wip_state:'finished',is_buildable:true},{id:'knowledge1',kind:'knowledge',title:'Local knowledge',wip_state:'finished',is_buildable:true}];
    const make = input=>({id:'linked1',name:input.name,provider:input.provider,source_url:input.source_url,has_credential:true,synced_at:'2026-10-02T12:00:00Z',nodes:[{id:'group',title:'A group',url:null,children:[{id:'node',title:'Remote document',url:'https://www.yuque.com/team/book/slug',children:[]}]}]});
    let links=[]; let rev=0;
    window.__TAURI_INTERNALS__={invoke:async(cmd,args)=>{
      window.calls.push({cmd,args});
      if(cmd==='app_info') return {name:'Veloct',version:'1.0.0'};
      if(cmd==='current_project') return {path:'D:/isolated/workspace',name:'test'};
      if(cmd==='list_documents') return docs;
      if(cmd==='content_index') return {entries:[],issues:[],is_buildable:true};
      if(cmd==='preview_status') return {site:{state:'stopped'},works:{state:'stopped'},site_url:null,works_url:null};
      if(cmd==='ai_settings') return {page_create_ready:false,providers:[],features:[],fallback:null};
      if(cmd==='attachment_messages'||cmd==='check_attachment_images') return {upload_errors:[],health_errors:[]};
      if(cmd==='delete_document') {docs=docs.filter(d=>d.id!==args.id);return null;}
      if(cmd==='linked_knowledge') return {items:links,revision:rev?{hash:String(rev),size:rev}:null};
      if(cmd==='link_knowledge') { links=[make(args.input)];rev++;return {items:links,revision:{hash:String(rev),size:rev}};}
      if(cmd==='refresh_linked_knowledge') throw {code:'network',message:'模拟权限失效；旧目录已保留'};
      if(cmd==='unlink_knowledge') {links=[];rev++;return {items:links,revision:{hash:String(rev),size:rev}};}
      return null;
    }};
  });
  await page.goto('http://workbench.test/');
  const nav = label=>page.locator('.nav-item').filter({hasText:label});
  await nav('文章管理').click();
  const remove = page.getByRole('button',{name:'删除文章：Delete fixture',exact:true}); await remove.waitFor(); await page.waitForTimeout(400);
  const original = await remove.boundingBox();
  await remove.click();
  const confirm = page.getByRole('button',{name:'确认删除文章：Delete fixture',exact:true}); await confirm.waitFor();
  await page.waitForTimeout(200);
  const changed = await confirm.boundingBox();
  assert.equal(changed.x,original.x);assert.equal(changed.y,original.y);assert.equal(changed.width,original.width);
  assert.equal(await page.locator('.state-block').count(),0);
  assert.equal(await confirm.evaluate(el=>getComputedStyle(el).animationName),'inline-delete-switch');
  assert.equal((await page.evaluate(()=>window.calls)).filter(c=>c.cmd==='delete_document').length,0);
  await page.keyboard.press('Escape'); await remove.waitFor();
  await remove.click(); await confirm.click();
  await page.getByText('还没有文章',{exact:true}).waitFor();
  assert.equal((await page.evaluate(()=>window.calls)).filter(c=>c.cmd==='delete_document').length,1);
  await nav('知识库管理').click();
  await page.getByRole('button',{name:'链接知识库',exact:true}).click();
  await page.getByLabel('显示名称',{exact:true}).fill('My remote outline');
  await page.getByLabel('知识库链接',{exact:true}).fill('https://www.yuque.com/team/book');
  await page.getByLabel('语雀个人 Token',{exact:true}).fill('fixture-not-real');
  await page.getByRole('button',{name:'链接并获取目录',exact:true}).click();
  await page.getByText('Remote document',{exact:false}).waitFor();
  assert.equal(await page.locator('.manager-item').filter({hasText:'Local knowledge'}).count(),1);
  assert.equal(await page.locator('.linked-outline .linked-outline').count(),1);
  await page.getByRole('button',{name:'Remote document ↗',exact:true}).click();
  assert.ok((await page.evaluate(()=>window.calls)).some(c=>c.cmd==='open_linked_knowledge'&&c.args.id==='linked1'&&c.args.nodeId==='node'));
  await page.getByRole('button',{name:'更新目录',exact:true}).click();
  await page.getByText('模拟权限失效；旧目录已保留',{exact:false}).waitFor();
  assert.equal(await page.getByRole('button',{name:'Remote document ↗',exact:true}).count(),1);
  await page.getByRole('button',{name:'名称与授权',exact:true}).click();
  assert.equal(await page.getByLabel('语雀个人 Token',{exact:true}).inputValue(),'');
  await page.getByRole('button',{name:'取消',exact:true}).click();
  await page.getByRole('button',{name:'解除链接',exact:true}).click();
  await page.getByRole('button',{name:'确认解除',exact:true}).click();
  await page.getByText('还没有链接外部知识库；本地知识库仍在上方独立管理。',{exact:true}).waitFor();
  assert.equal(await page.locator('.manager-item').filter({hasText:'Local knowledge'}).count(),1);
  assert.deepEqual(errors,[]);
  console.log(JSON.stringify({inlineDeleteSamePosition:true,animation:true,escapeCancels:true,requiresSecondClick:true,linkedHierarchy:true,sourceJump:true,failedRefreshPreservesOutline:true,secretsCleared:true,unlinkPreservesLocal:true,pageErrors:errors}));
} finally {await browser.close();}
