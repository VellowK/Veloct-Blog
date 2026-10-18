import { test } from 'vitest';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseLinkedKnowledge, readLinkedKnowledge, linkedKnowledgeReload } from '../src/lib/linked-knowledge.mjs';
const fixture = () => [{id:'source',name:'External <script>',provider:'yuque',source_url:'https://www.yuque.com/team/book',nodes:[{id:'group',title:'Group',url:null,children:[{id:'doc',title:'Title only',url:'https://www.yuque.com/team/book/doc',children:[]}]}],token:'do-not-publish',app_secret:'never-render'}];
test('projects only an outline, preserving hierarchy and safe source links', () => {
  const value = parseLinkedKnowledge(fixture());
  assert.equal(value[0].nodes[0].children[0].url, 'https://www.yuque.com/team/book/doc');
  assert.equal(value[0].name, 'External <script>'); // Escaped by Astro, never interpreted as HTML.
  assert.ok(!JSON.stringify(value).includes('do-not-publish'));
  assert.ok(!JSON.stringify(value).includes('app_secret'));
});
test('unsafe or unrelated URLs become unclickable', () => {
  for (const url of ['javascript:alert(1)','https://evil.test/doc','https://secret@www.yuque.com/team/book/doc','https://www.yuque.com/other/book/doc']) {
    const input = fixture(); input[0].nodes[0].children[0].url = url;
    assert.equal(parseLinkedKnowledge(input)[0].nodes[0].children[0].url, null);
  }
});
test('rejects invalid sources and duplicate directory nodes', () => {
  const input = fixture(); input[0].source_url = 'https://www.yuque.com.evil.test/team/book';
  assert.throws(() => parseLinkedKnowledge(input));
  const duplicate = fixture(); duplicate[0].nodes.push(duplicate[0].nodes[0]);
  assert.throws(() => parseLinkedKnowledge(duplicate));
});
test('Feishu nodes keep the supplied tenant and node token URL', () => {
  const input = fixture(); input[0].provider = 'feishu'; input[0].source_url = 'https://tenant.feishu.cn/wiki/root'; input[0].nodes[0].children[0].url = 'https://tenant.feishu.cn/wiki/child';
  assert.equal(parseLinkedKnowledge(input)[0].nodes[0].children[0].url, 'https://tenant.feishu.cn/wiki/child');
});
test('missing data defaults empty; corrupt data is reported without overwrite', () => {
  const root = mkdtempSync(join(tmpdir(), 'veloct-linked-'));
  try { assert.deepEqual(readLinkedKnowledge(root), []); mkdirSync(join(root, 'data')); writeFileSync(join(root,'data/linked-knowledge.json'),'broken'); assert.throws(() => readLinkedKnowledge(root)); }
  finally { rmSync(root,{recursive:true,force:true}); }
});
test('outside-project data edits and removal trigger live preview reloads', () => {
  const callbacks = {}; const sent = []; let file;
  const watcher = { add: (path) => {file=path;}, on: (event, callback) => {callbacks[event]=callback;}, off: () => {} };
  linkedKnowledgeReload().configureServer({ watcher, ws:{send:value=>sent.push(value)}, httpServer:{once:()=>{}} });
  callbacks.change(file); callbacks.add(file); callbacks.unlink(file);
  assert.equal(sent.length,3); assert.ok(sent.every(value=>value.type==='full-reload'));
  callbacks.change('unrelated.json'); assert.equal(sent.length,3);
});
