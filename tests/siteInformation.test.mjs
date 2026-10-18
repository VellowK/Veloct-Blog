import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import { normalizeSiteConfig } from '../site/src/lib/site-config.mjs';

test('site information survives template normalization, including public contact IDs and unknown keys', () => {
  const input = {
    name: '我的博客', description: '站点简介', url: 'https://blog.example.com',
    author: { name: '作者', bio: '作者简介', avatar: '/assets/avatar.png', custom: 42 },
    hero: { title: '首页标题', subtitle: 'Keep subtitle', ctaHref: '/article/', custom: true },
    social: [
      { label: 'GitHub', url: 'https://github.com/user', id: 'user', icon: 'github' },
      { label: '微信', url: '', id: 'public-id' },
    ],
    customRoot: { preserved: true },
  };
  const config = normalizeSiteConfig(input);
  assert.equal(config.name, input.name);
  assert.equal(config.description, input.description);
  assert.equal(config.url, input.url);
  assert.deepEqual(config.author, input.author);
  assert.equal(config.hero.title, input.hero.title);
  assert.equal(config.hero.subtitle, input.hero.subtitle);
  assert.equal(config.hero.ctaHref, input.hero.ctaHref);
  assert.equal(config.hero.custom, true);
  assert.deepEqual(config.social, input.social);
  assert.deepEqual(config.customRoot, input.customRoot);
});

test('default/empty homepage title and author name follow the blog name in their consumers', () => {
  for (const input of [{ name: 'New blog' }, { name: 'New blog', hero: { title: '' }, author: { name: '' } }]) {
    const config = normalizeSiteConfig(input);
    assert.equal(config.hero.title || config.name, 'New blog');
    assert.equal(config.author.name || config.name, 'New blog');
  }
  const hero = readFileSync(new URL('../site/src/components/Hero.astro', import.meta.url), 'utf8');
  assert.match(hero, /const title = hero\.title \|\| site\.name/);
  assert.match(hero, /const avatar = site\.author\?\.avatar/);
  const sidebar = readFileSync(new URL('../site/src/components/LeftSidebar.astro', import.meta.url), 'utf8');
  assert.match(sidebar, /entry\.id/);
  assert.match(sidebar, /entry\.url \? \(/);
});

test('SEO injection canonical DTO key wins and old camelCase config still reads', () => {
  assert.equal(normalizeSiteConfig({ seo: { html_injection: '<meta name="new">', htmlInjection: 'old' } }).seo.htmlInjection, '<meta name="new">');
  assert.equal(normalizeSiteConfig({ seo: { htmlInjection: 'legacy' } }).seo.htmlInjection, 'legacy');
});
