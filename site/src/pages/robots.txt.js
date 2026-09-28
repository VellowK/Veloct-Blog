// robots.txt.
//
// handoff.md section 15 requires a "禁止搜索引擎开关" that moves TOGETHER with the per-page meta
// robots. One switch, two effects, because a page that says `index` while robots.txt says `Disallow`
// is a contradiction.
//
// WIP pages are excluded here as well as carrying their own noindex, so a crawl cannot reach them
// even if it ignores the meta tag.

import { readSiteConfig } from '../lib/site-config.mjs';

export function GET() {
  const site = readSiteConfig();
  const allowIndexing = site.seo?.allowIndexing !== false;

  const lines = [];

  if (allowIndexing) {
    lines.push('User-agent: *');
    lines.push('Allow: /');
    // Belt and braces with the pages' own noindex, because a WIP URL must not be discoverable.
    lines.push('Disallow: /wip/');
  } else {
    // The site-wide switch is off: nothing is crawlable at all.
    lines.push('User-agent: *');
    lines.push('Disallow: /');
  }

  lines.push('');

  const sitemap = new URL('/sitemap-index.xml', site.url).toString();
  if (allowIndexing) {
    lines.push(`Sitemap: ${sitemap}`);
  }

  lines.push('');
  return new Response(lines.join('\n'), {
    headers: { 'content-type': 'text/plain; charset=utf-8' },
  });
}
