import { describe, expect, it } from 'vitest';
import type { RibbonEntry } from '../src/lib/api';
import { menuPageOptions } from '../src/lib/menuPages';

const page = (changes: Partial<RibbonEntry> = {}): RibbonEntry => ({
  id: 'about', kind: 'page', slug: 'about', title: '关于', summary: null,
  wip_state: 'finished', updated_at: '2026-10-16', category: null, tags: [],
  is_buildable: true, route: '/about/', ...changes,
});

describe('menu page links', () => {
  it('lists buildable pages only, not articles, knowledge or invalid pages', () => {
    const valid = page();
    expect(menuPageOptions([valid, page({ kind: 'article' }), page({ kind: 'knowledge' }), page({ is_buildable: false })])).toEqual([valid]);
  });
  it('uses canonical routes, including home and custom pages, rather than guessing from slugs', () => {
    const home = page({ id: 'home', slug: 'home', title: '首页', route: '/' });
    const custom = page({ id: 'custom', slug: 'custom', route: '/custom/' });
    expect(menuPageOptions([home, custom]).map(entry => entry.route)).toEqual(expect.arrayContaining(['/', '/custom/']));
  });
  it('searches title and route case-insensitively, trims whitespace and leaves source order intact', () => {
    const entries = [page({ title: 'Zeta', route: '/Zeta/' }), page({ title: 'Alpha' })];
    expect(menuPageOptions(entries, '  zETA ')).toEqual([entries[0]]);
    expect(menuPageOptions(entries, '/ABOUT/')).toEqual([entries[1]]);
    expect(menuPageOptions(entries, 'missing')).toEqual([]);
    expect(menuPageOptions(entries).map(entry => entry.title)).toEqual(['Alpha', 'Zeta']);
    expect(entries[0]?.title).toBe('Zeta');
  });
  it('rejects external, scheme-relative and backslash routes', () => {
    expect(menuPageOptions(['https://evil.test/', '//evil.test/', '/\\evil.test/', 'javascript:alert(1)', ''].map(route => page({ route })))).toEqual([]);
  });
});
