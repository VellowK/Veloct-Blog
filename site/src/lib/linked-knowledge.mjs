import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { projectRoot } from './site-config.mjs';

function sourceUrl(provider, value) {
  const url = new URL(value);
  const parts = url.pathname.split('/').filter(Boolean);
  const id = /^[A-Za-z0-9_-]+$/;
  if (url.protocol !== 'https:' || url.username || url.password || url.port || url.search || url.hash || parts.length !== 2 || !parts.every((p) => id.test(p))) throw new Error('Invalid linked knowledge source URL');
  if (provider === 'yuque' ? !(url.hostname === 'yuque.com' || url.hostname.endsWith('.yuque.com')) : provider !== 'feishu' || !url.hostname.endsWith('.feishu.cn') || parts[0] !== 'wiki') throw new Error('Unsupported linked knowledge provider/host');
  return url;
}
function safeLink(provider, source, value) {
  if (typeof value !== 'string' || !value.startsWith('https://')) return null;
  try {
    const url = new URL(value);
    if (url.origin !== source.origin || url.username || url.password || url.port || url.search || url.hash) return null;
    return (provider === 'yuque' ? url.pathname.startsWith(`${source.pathname.replace(/\/$/, '')}/`) : /^\/wiki\/[A-Za-z0-9_-]+\/?$/.test(url.pathname)) ? url.href : null;
  } catch { return null; }
}
export function parseLinkedKnowledge(value) {
  if (!Array.isArray(value) || value.length > 100) throw new Error('Invalid linked knowledge directory');
  return value.map((item) => {
    const source = sourceUrl(item.provider, item.source_url);
    let count = 0;
    const seen = new Set();
    const nodes = (list, depth = 0) => {
      if (!Array.isArray(list) || depth > 32) throw new Error('Invalid linked knowledge tree');
      return list.map((node) => {
        if (++count > 5000 || typeof node.id !== 'string' || seen.has(node.id) || typeof node.title !== 'string') throw new Error('Invalid linked knowledge node');
        seen.add(node.id);
        return { id: node.id, title: node.title.slice(0, 512), url: safeLink(item.provider, source, node.url), children: nodes(node.children ?? [], depth + 1) };
      });
    };
    if (typeof item.name !== 'string' || !item.name.trim()) throw new Error('Invalid linked knowledge title');
    // An allowlisted projection: do not spread arbitrary metadata/credentials into rendered data.
    return { id: String(item.id), name: item.name.slice(0, 256), provider: item.provider, source_url: source.href, nodes: nodes(item.nodes) };
  });
}
export function readLinkedKnowledge(root = projectRoot) {
  let raw;
  try { raw = readFileSync(join(root, 'data/linked-knowledge.json'), 'utf8'); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
  return parseLinkedKnowledge(JSON.parse(raw));
}
/** The data file lives outside the site directory; explicitly watch it for preview reloads. */
export function linkedKnowledgeReload() {
  return { name: 'veloct-linked-knowledge', configureServer(server) {
    const file = join(projectRoot, 'data/linked-knowledge.json');
    const normalized = file.replaceAll('\\', '/');
    server.watcher.add(file);
    const reload = (path) => { if (path.replaceAll('\\', '/') === normalized) server.ws.send({ type: 'full-reload' }); };
    for (const event of ['add', 'change', 'unlink']) server.watcher.on(event, reload);
    server.httpServer?.once('close', () => { for (const event of ['add', 'change', 'unlink']) server.watcher.off(event, reload); });
  } };
}
