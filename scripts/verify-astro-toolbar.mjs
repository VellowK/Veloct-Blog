// Exercise the rebuilt sidecar against an isolated old project with its toolbar explicitly enabled.
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, symlinkSync, rmSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import net from 'node:net';
import path from 'node:path';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const root = mkdtempSync(path.resolve('target/toolbar-fixture-'));
let child, browser;
try {
  const site = path.join(root, 'site');
  mkdirSync(path.join(site, 'src/pages'), {recursive:true}); mkdirSync(path.join(site, '.astro'));
  writeFileSync(path.join(site, 'package.json'), '{"type":"module"}');
  writeFileSync(path.join(site, 'astro.config.mjs'), 'export default {devToolbar:{enabled:true}};');
  writeFileSync(path.join(site, '.astro/settings.json'), '{"devToolbar":{"enabled":true},"checkUpdates":false}');
  writeFileSync(path.join(site, 'src/pages/index.astro'), '<html><body>toolbar-fixture</body></html>');
  symlinkSync(path.resolve('site/node_modules'), path.join(site, 'node_modules'), 'junction');
  const listener = net.createServer(); await new Promise(resolve=>listener.listen(0,'127.0.0.1',resolve));
  const port = listener.address().port; await new Promise(resolve=>listener.close(resolve));
  let log = '';
  child = spawn(path.resolve('src-tauri/binaries/veloct-astro-sidecar-x86_64-pc-windows-msvc.exe'), ['dev','--host','127.0.0.1','--port',String(port)], {cwd:root,env:{...process.env,VELOCT_PROJECT_ROOT:root,VELOCT_NODE:process.execPath},windowsHide:true});
  child.stdout.on('data', data=>log+=data); child.stderr.on('data', data=>log+=data);
  const url = `http://127.0.0.1:${port}/`;
  let ready = false;
  for (let n=0; n<100; n++) {
    if (child.exitCode !== null) throw new Error(`Sidecar exited: ${log}`);
    try { if ((await fetch(url)).ok) { ready=true; break; } } catch {}
    await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert.ok(ready, log);
  browser = await chromium.launch({channel:'msedge',headless:true});
  const page = await browser.newPage(); await page.goto(url); assert.ok((await page.locator('body').innerText()).includes('toolbar-fixture'), `${await page.content()}\n${log}`);
  await page.waitForTimeout(1000);
  assert.equal(await page.locator('astro-dev-toolbar').count(),0);
  const settings = JSON.parse(readFileSync(path.join(site,'.astro/settings.json'),'utf8'));
  assert.equal(settings.devToolbar.enabled,false); assert.equal(settings.checkUpdates,false);
  assert.ok(readFileSync(path.join(site,'astro.config.mjs'),'utf8').includes('enabled:true'));
  console.log(JSON.stringify({oldProjectToolbarDisabled:true,configUnmodified:true,otherPreferencesPreserved:true}));
} finally {
  if (browser) await browser.close();
  if (child && child.exitCode === null) { const result=spawnSync('taskkill.exe',['/pid',String(child.pid),'/T','/F']); assert.equal(result.status,0,result.stderr?.toString()); }
  assert.equal(path.dirname(path.resolve(root)),path.resolve('target'));
  assert.ok(path.basename(root).startsWith('toolbar-fixture-'));
  rmSync(root,{recursive:true,force:true});
}
