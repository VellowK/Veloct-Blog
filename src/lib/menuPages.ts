import type { RibbonEntry } from './api';

/** Use the content service's canonical route (especially home=/), never guess from a slug. */
export function menuPageOptions(documents: readonly RibbonEntry[], query = ''): RibbonEntry[] {
  const search = query.trim().toLocaleLowerCase();
  return documents.filter((entry) => entry.kind === 'page' && entry.is_buildable
    && entry.route.startsWith('/') && !entry.route.startsWith('//') && !entry.route.includes('\\')
    && (!search || `${entry.title}\n${entry.route}`.toLocaleLowerCase().includes(search)))
    .sort((a, b) => a.title.localeCompare(b.title, 'zh-CN') || a.route.localeCompare(b.route));
}
