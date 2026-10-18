import { test, expect } from 'vitest';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { normalizeSiteConfig } from '../site/src/lib/site-config.mjs';
import { profileContacts, safePublicUrl, blogActivity, calendarCells, calendarEnd, readGithubActivity } from '../site/src/lib/profile-home.mjs';

test('old settings gain a profile config without changing description, introduction, contacts or unknown keys', () => {
  const config = normalizeSiteConfig({ description: '站点介绍', author: { bio: '独立个人介绍' }, homepage: { githubUsername: 'octocat', custom: 42 }, custom: true });
  expect(config.homepage).toEqual({ cover: '', githubUsername: 'octocat', showBlogHeatmap: true, showGithubHeatmap: true, custom: 42 });
  expect(config.author.bio).toBe('独立个人介绍'); expect(config.description).toBe('站点介绍'); expect(config.custom).toBe(true);
  expect(normalizeSiteConfig({ homepage: { githubUsername: 'profile123' } }).homepage.githubUsername).toBe('profile123');
  for (const githubUsername of ['https://github.com/user', 'name--bad', '-leading', 'bad_user']) expect(() => normalizeSiteConfig({ homepage: { githubUsername } })).toThrow();
  expect(() => normalizeSiteConfig({ homepage: { showBlogHeatmap: 'false' } })).toThrow();
});
test('contact projection keeps ID-only entries, rejects active URLs and limits display without deleting source', () => {
  const source = [{ label: '微信', id: 'wechat-public-id', url: '' }, { label: 'unsafe', url: 'javascript:alert(1)' }, ...Array.from({ length: 18 }, (_, i) => ({ label: String(i), url: 'https://example.com' }))];
  const contacts = profileContacts(source);
  expect(contacts).toHaveLength(16); expect(source).toHaveLength(20); expect(contacts[0].id).toBe('wechat-public-id'); expect(contacts.some(c => c.label === 'unsafe')).toBe(false);
  for (const url of ['javascript:alert(1)', '//evil.example', '/\\evil.example', 'data:image/svg+xml,x', 'https://user:secret@example.com', 'https://x y']) expect(safePublicUrl(url)).toBe('');
  expect(safePublicUrl('mailto:hello@example.com')).toBe('mailto:hello@example.com'); expect(safePublicUrl('mailto:hello@example.com', true)).toBe('');
});
test('blog calendar counts public article and knowledge creation, not hidden drafts or page updates', () => {
  const entry = (type, wip, publish, date) => ({ data: { type, wip, wip_publish: publish, created_at: date } });
  expect(blogActivity([entry('article', false, false, '2026-01-02T00:00:00Z'), entry('article', true, false, '2026-01-02T00:00:00Z'), entry('article', true, true, '2026-01-02T12:00:00Z'), entry('knowledge', false, false, '2026-01-03T00:00:00Z'), entry('knowledge', false, false, '2026-02-30')])).toEqual({ '2026-01-02': 2, '2026-01-03': 1 });
});
test('365-day calendar is ordered, Sunday aligned, deterministic and does not invent GitHub contribution counts', () => {
  const cells = calendarCells({ '2026-10-15': 7 }, '2026-10-15');
  expect(cells.filter(c => c.date)).toHaveLength(365);
  expect(cells.find(c => c.date).date).toBe('2025-10-16'); expect(cells.at(-1)).toMatchObject({ date: '2026-10-15', level: 4, value: 7 });
  expect(cells.find(c => c.date)?.row).toBe(new Date(`${cells.find(c => c.date).date}T00:00:00Z`).getUTCDay());
  expect(calendarCells({ '2026-10-15': 2 }, '2026-10-15', true).at(-1).level).toBe(2);
  expect(calendarEnd(['2026-02-30', '2026-01-02'], '2026-01-03T12:00:00Z')).toBe('2026-01-03');
});
test('rolling window excludes old values and handles leap days and a year boundary in UTC', () => {
  const cells = calendarCells({ '2025-10-15': 99, '2025-10-16': 1 }, '2026-10-15');
  expect(cells.reduce((sum, c) => sum + c.value, 0)).toBe(1);
  expect(calendarCells({}, '2024-03-01').filter(c => c.date).map(c => c.date)).toContain('2024-02-29');
  expect(calendarCells({}, '2026-01-01').find(c => c.date).date).toBe('2025-01-02');
  for (const count of [0, -1, 400, 1.5, NaN]) expect(() => calendarCells({}, '2026-10-15', false, count)).toThrow();
  expect(() => calendarCells({}, '2026-02-30')).toThrow();
});
test('blank and whitespace contacts are hidden without changing the saved source', () => {
  const source = [{ label: 'Blank', id: '  ', url: '' }, { label: 'Name only' }, { label: 'ID', id: ' public-id ' }, { label: 'Link', url: ' https://example.com ' }];
  expect(profileContacts(source)).toEqual([{ label: 'ID', icon: '', id: 'public-id', url: '' }, { label: 'Link', icon: '', id: '', url: 'https://example.com' }]);
  expect(source[0].id).toBe('  '); expect(source).toHaveLength(4);
});
test('only validated complete snapshots for the configured account are exposed, with no raw metadata', () => {
  const root = mkdtempSync(join(tmpdir(), 'veloct-profile-'));
  try {
    mkdirSync(join(root, 'data'));
    const path = join(root, 'data/github-activity.json');
    const days = calendarCells({}, '2026-10-15', false, 365).filter(c => c.date).map(c => ({ date: c.date, level: 1, secret: 'never-render' }));
    writeFileSync(path, JSON.stringify({ schemaVersion: 1, username: 'Octocat', fetchedAt: '2026-10-15T00:00:00Z', days, extra: 'never-render' }));
    expect(readGithubActivity(root, 'octocat').days['2026-10-15']).toBe(1); expect(JSON.stringify(readGithubActivity(root, 'octocat'))).not.toContain('never-render');
    expect(readGithubActivity(root, 'other-user')).toBe(null);
    days[1].date = days[0].date; writeFileSync(path, JSON.stringify({ schemaVersion: 1, username: 'octocat', days }));
    expect(readGithubActivity(root, 'octocat')).toBe(null);
    writeFileSync(path, JSON.stringify({ schemaVersion: 1, username: 'octocat', days: days.slice(0, 100) }));
    expect(readGithubActivity(root, 'octocat')).toBe(null);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
test('home owns its profile area and shared layout independently excludes original sidebar on legacy home routes', () => {
  const home = readFileSync(new URL('../site/src/pages/index.astro', import.meta.url), 'utf8');
  const layout = readFileSync(new URL('../site/src/layouts/BaseLayout.astro', import.meta.url), 'utf8');
  expect(home).toContain('showRightSidebar={false}'); expect(home).toContain('<ProfileHome'); expect(home).not.toContain('<RightSidebar');
  expect(layout).toContain("canonical === '/'"); expect(layout).toContain('showRightSidebar && !isHome');
});
