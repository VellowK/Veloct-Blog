import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

export function safePublicUrl(value, image = false) {
  if (typeof value !== 'string' || !value || /[\s\\\u0000-\u001f]/.test(value)) return '';
  if (value.startsWith('/') && !value.startsWith('//')) return value;
  try {
    const url = new URL(value);
    if (url.username || url.password) return '';
    if (['https:', 'http:'].includes(url.protocol) && url.hostname) return value;
    if (!image && ['mailto:', 'tel:'].includes(url.protocol) && url.pathname) return value;
  } catch { /* Invalid or active schemes never become links. */ }
  return '';
}
export function profileContacts(contacts) {
  return (Array.isArray(contacts) ? contacts : []).filter(contact => contact && typeof contact === 'object')
    .map(contact => ({ label: String(contact.label || contact.icon || '联系方式'), icon: typeof contact.icon === 'string' ? contact.icon : '',
      id: typeof contact.id === 'string' ? contact.id.trim() : '', url: safePublicUrl(typeof contact.url === 'string' ? contact.url.trim() : '') }))
    .filter(contact => contact.url || contact.id).slice(0, 16);
}
export function validDay(value) {
  return typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value) && !Number.isNaN(Date.parse(`${value}T00:00:00Z`)) && new Date(`${value}T00:00:00Z`).toISOString().slice(0, 10) === value;
}
export function blogActivity(entries) {
  const days = {};
  for (const entry of entries) {
    if (!['article', 'knowledge'].includes(entry.data?.type)) continue;
    // Unpublished articles must not leak even activity metadata to a public home.
    if (entry.data?.type === 'article' && entry.data.wip && !entry.data.wip_publish) continue;
    const date = String(entry.data?.created_at ?? '').slice(0, 10);
    if (validDay(date)) days[date] = (days[date] ?? 0) + 1;
  }
  return days;
}
export function readGithubActivity(root, username) {
  if (!/^[a-z\d](?:[a-z\d-]{0,37}[a-z\d])?$/i.test(username || '') || username.includes('--')) return null;
  try {
    const snapshot = JSON.parse(readFileSync(resolve(root, 'data/github-activity.json'), 'utf8'));
    if (snapshot.schemaVersion !== 1 || snapshot.username?.toLowerCase() !== username.toLowerCase() || !Array.isArray(snapshot.days) || snapshot.days.length < 300 || snapshot.days.length > 371) return null;
    const seen = new Set();
    for (const day of snapshot.days) {
      if (!validDay(day.date) || !Number.isInteger(day.level) || day.level < 0 || day.level > 4 || seen.has(day.date)) return null;
      seen.add(day.date);
    }
    const sorted = [...seen].sort();
    if (sorted.some((date, index) => index > 0 && Date.parse(date) - Date.parse(sorted[index - 1]) !== 86400000)) return null;
    return { username, fetchedAt: typeof snapshot.fetchedAt === 'string' ? snapshot.fetchedAt : '', days: Object.fromEntries(snapshot.days.map(day => [day.date, day.level])) };
  } catch (error) {
    if (error.code !== 'ENOENT') console.warn('GitHub contribution snapshot unavailable; previous site content is unaffected.');
    return null;
  }
}
export function calendarEnd(dates, buildTime = process.env.VELOCT_BUILD_TIME || '') {
  // No wall clock: rebuilding the same release keeps its calendar byte-identical.
  const candidates = [...dates, buildTime.slice(0, 10)].filter(validDay).sort();
  return candidates.at(-1) || '1970-01-01';
}
export function calendarCells(days, end, intensity = false, dayCount = 365) {
  if (!validDay(end) || !Number.isInteger(dayCount) || dayCount < 1 || dayCount > 366) throw new RangeError('Invalid calendar date or day count');
  const endTime = Date.parse(`${end}T00:00:00Z`);
  const start = new Date(endTime - (dayCount - 1) * 86400000);
  const offset = start.getUTCDay();
  return Array.from({ length: offset + dayCount }, (_, index) => {
    if (index < offset) return { date: '', level: 0, value: 0, column: Math.floor(index / 7), row: index % 7 };
    const date = new Date(start.getTime() + (index - offset) * 86400000).toISOString().slice(0, 10);
    const value = Math.max(0, Number(days[date]) || 0);
    const level = intensity ? Math.min(4, value) : value === 0 ? 0 : value === 1 ? 1 : value === 2 ? 2 : value < 5 ? 3 : 4;
    return { date, value, level, column: Math.floor(index / 7), row: index % 7 };
  });
}
