// The RSS feed.
//
// handoff.md section 6.7 lists RSS among the required capabilities, and section 14.9 requires
// time-derived fields to be stable across a rebuild of one release. Two consequences:
//
//   * `pubDate` comes from the article's own frontmatter, never from `new Date()`.
//   * `trailingSlash: false` is passed so the feed's links match the site's actual URLs rather than
//     Astro's default, which always adds a trailing slash.

import rss from '@astrojs/rss';
import { getCollection } from 'astro:content';
import { readSiteConfig } from '../lib/site-config.mjs';
import { byNewest, isProduction } from '../lib/format.ts';
import { articleRoute, WipRoute } from '../lib/content.ts';

export async function GET(context) {
  const site = readSiteConfig();
  const articles = await getCollection('articles');

  const items = articles
    .filter((entry) => articleRoute(entry, { isProduction }).kind === WipRoute.Article)
    .sort(byNewest)
    .map((entry) => ({
      title: entry.data.title,
      description: entry.data.summary ?? '',
      // The article's own publication time, so the feed does not change on a rebuild.
      pubDate: new Date(entry.data.published_at ?? entry.data.created_at),
      link: `/article/${entry.data.slug}/`,
      categories: entry.data.tags ?? [],
    }));

  return rss({
    title: site.name,
    description: site.description,
    site: context.site ?? site.url,
    items,
    trailingSlash: false,
    customData: `<language>${site.language ?? 'zh-CN'}</language>`,
  });
}
