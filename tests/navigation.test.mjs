import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { test } from 'vitest';

const app = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
const api = readFileSync(new URL('../src/lib/api.ts', import.meta.url), 'utf8');
const commands = readFileSync(new URL('../src-tauri/src/commands.rs', import.meta.url), 'utf8');
const registry = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');

test('every declared view has exactly one explicit rendering branch and navigation metadata', () => {
  const type = app.match(/type ViewId =([\s\S]*?);/)[1];
  const views = [...type.matchAll(/'([^']+)'/g)].map(match => match[1]);
  const render = app.slice(app.indexOf("{view === 'dashboard' ?"));
  for (const view of views) {
    assert.equal([...render.matchAll(new RegExp(`\\) : view === '${view}' \\? \\(|\\{view === '${view}' \\? \\(`, 'g'))].length, 1, `${view} has an explicit branch`);
    assert.ok(app.includes(`{ id: '${view}', label:`), `${view} has breadcrumb metadata`);
  }
  assert.ok(!views.includes('deploy'), 'the duplicate top-level deployment route is removed');
  assert.match(app, /settingsSection === 'deploy' && <DeployPanel/);
});

test('AI page creation, model settings and callable commands are removed without removing normal editing', () => {
  assert.doesNotMatch(app, /PageCreator|AiSettings|page-creator|AI 建页|AI 与模型/);
  assert.doesNotMatch(api, /generatePage|createPageFromDraft|aiSettings|saveAiProvider/);
  assert.doesNotMatch(commands + registry, /page_creator|generate_page|create_page_from_draft|ai_settings|save_ai_provider/);
  assert.equal(existsSync(new URL('../src/PageCreator.tsx', import.meta.url)), false);
  assert.equal(existsSync(new URL('../src-tauri/src/page_creator.rs', import.meta.url)), false);
  const select = app.match(/const selectDocument = async[\s\S]*?const createDocument/)[0];
  assert.match(select, /await api\.readDocument[\s\S]*?setView\('editor'\)/);
});

test('selected content uses one full-width editor and site preview stays separate', () => {
  assert.match(app, /CONTENT_VIEWS\[document\.kind\]/);
  assert.match(app, /onClick=\{\(\) => setView\(activeNavId\)\}/);
  // Latest owner request: WYSIWYG already renders the document; do not duplicate it on the right.
  const writing = app.slice(app.indexOf(") : view === 'editor' ? ("));
  assert.equal([...writing.matchAll(/<section className="pane">/g)].length, 1);
  assert.match(writing, /<Editor[\s\S]*?source=\{source\}/);
  assert.doesNotMatch(app, /MarkdownPreview|DocumentPreview|splitPercent|splitRef|draggingRef|split-handle|--editor-share/);
  assert.match(writing, /setView\('preview'\)\}>打开站点预览/);
  const styles = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8');
  assert.match(styles, /\.workspace \{[\s\S]*?grid-template-columns: minmax\(0, 1fr\);/);
  assert.match(app, /const previewInUse = view === 'preview' \|\| showingSiteInfo/);
  assert.match(app, /view === 'preview' \? \([\s\S]*?<PreviewPanel/);
  const crumb = app.match(/<p className="crumb">([\s\S]*?)<\/p>/)[1];
  assert.doesNotMatch(crumb, /\\n/);
  assert.match(crumb, /sectionLabel/);
});
