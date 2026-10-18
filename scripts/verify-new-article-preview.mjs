// Actual Astro dev/build regression, including a draft created AFTER the preview has started.
// Copies only template sources into an isolated target fixture; never edits a user's content.
import assert from 'node:assert/strict';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import net from 'node:net';
import path from 'node:path';
import { stageTemplate } from './prepare-template.mjs';
const root = mkdtempSync(path.resolve('target/new-article-preview-'));
let child;
let log = '';
const ids = ['01J8Z5Q0K3W7M9V2B4N6P8R2AA', '01J8Z5Q0K3W7M9V2B4N6P8R2AB', '01J8Z5Q0K3W7M9V2B4N6P8R2AC'];
function article(id, slug, shared = false, finished = false) {
  return `---\ntype: article\nid: ${id}\nslug: ${slug}\ntitle: Preview ${slug}\ncreated_at: '2026-01-01T00:00:00Z'\nupdated_at: '2026-01-01T00:00:00Z'\nwip: ${!finished}\nwip_publish: ${shared && !finished}\nwip_ever_published: ${shared}\n---\n\nPreview body ${id}.\n`;
}
async function stop() {
  if (child && child.exitCode === null) {
    const ended = new Promise(resolve => child.once('exit', resolve));
    const result = spawnSync('taskkill.exe', ['/pid', String(child.pid), '/T', '/F']);
    assert.equal(result.status, 0, result.stderr?.toString());
    await ended;
  }
}
async function get(url) { const response = await fetch(url); return { status: response.status, html: await response.text() }; }
try {
  stageTemplate(path.resolve('.'), root);
  const site = path.join(root, 'site');
  symlinkSync(path.resolve('site/node_modules'), path.join(site, 'node_modules'), 'junction');
  for (const dir of ['content/articles', 'content/pages', 'content/knowledge', 'works']) mkdirSync(path.join(root, dir), { recursive: true });
  writeFileSync(path.join(root, 'content/articles', ids[0] + '.md'), article(ids[0], 'existing-draft'));
  writeFileSync(path.join(root, 'content/articles', ids[2] + '.md'), article(ids[2], 'shared-draft', true));
  const listener = net.createServer(); await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve));
  const port = listener.address().port; await new Promise(resolve => listener.close(resolve));
  const base = `http://127.0.0.1:${port}`;
  child = spawn(process.execPath, ['node_modules/astro/bin/astro.mjs', 'dev', '--host', '127.0.0.1', '--port', String(port)], { cwd: site, windowsHide: true });
  child.stdout.on('data', chunk => log += chunk); child.stderr.on('data', chunk => log += chunk);
  let ready = false;
  for (let n = 0; n < 200; n++) {
    if (child.exitCode !== null) throw new Error(log);
    try { if ((await get(base + '/')).status === 200) { ready = true; break; } } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.ok(ready, log);
  const existing = await get(`${base}/wip/${ids[0]}/`);
  assert.equal(existing.status, 200, existing.html + '\n' + log);
  assert.ok(existing.html.includes('Preview body ' + ids[0]));
  assert.match(existing.html, /name="robots"[^>]*noindex/);
  // Warm the getStaticPaths cache, then create exactly the default WIP state emitted by Rust.
  const missing = await get(`${base}/wip/${ids[1]}/`); assert.equal(missing.status, 404);
  writeFileSync(path.join(root, 'content/articles', ids[1] + '.md'), article(ids[1], 'new-draft'));
  let created;
  const statuses = [];
  for (let n = 0; n < 100; n++) {
    created = await get(`${base}/wip/${ids[1]}/`); statuses.push(created.status);
    if (created.status === 200) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.equal(created.status, 200, log); assert.ok(created.html.includes('Preview body ' + ids[1]));
  assert.ok(created.html.includes(`/wip/${ids[1]}/`));
  // Slug changes cannot invalidate the permanent draft address.
  writeFileSync(path.join(root, 'content/articles', ids[1] + '.md'), article(ids[1], 'renamed-draft'));
  await new Promise(resolve => setTimeout(resolve, 250));
  assert.equal((await get(`${base}/wip/${ids[1]}/`)).status, 200);
  await stop();
  const build = spawnSync(process.execPath, ['node_modules/astro/bin/astro.mjs', 'build'], { cwd: site, encoding: 'utf8', timeout: 120000 });
  assert.equal(build.status, 0, build.stdout + build.stderr);
  assert.ok(!existsSync(path.join(site, 'dist/site/wip', ids[0], 'index.html')));
  assert.ok(!existsSync(path.join(site, 'dist/site/wip', ids[1], 'index.html')));
  assert.ok(!existsSync(path.join(site, 'dist/site/article/renamed-draft/index.html')));
  assert.ok(existsSync(path.join(site, 'dist/site/wip', ids[2], 'index.html')));
  console.log(JSON.stringify({ existingDraft: 200, newDraft: 200, initialHotCreateStatuses: statuses, stableIdAfterSlugChange: true, privateDraftsAbsentFromProduction: true, sharedWipPresent: true }));
} finally {
  await stop();
  assert.equal(path.dirname(root), path.resolve('target'));
  assert.ok(path.basename(root).startsWith('new-article-preview-'));
  rmSync(root, { recursive: true, force: true });
}
