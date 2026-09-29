// Build verification.
//
// This is the executable form of the parts of `handoff.md` that a build must satisfy, and it is the
// evidence for rows A8, A9 and A10 of `docs/spec/02-test-matrix.md`. It inspects the REAL `dist/`
// rather than the source, because the emitted output is the thing that gets published.
//
// Run it with `npm run verify` after `npm run build`. It exits non-zero and names the failure, so a
// wrong build cannot be mistaken for a good one.

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const siteDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const projectRoot = resolve(siteDir, '..');
const dist = join(siteDir, 'dist');
const siteDist = join(dist, 'site');
const worksDist = join(dist, 'works');

// `dist/site` is the main publish root and `dist/works` the works root: the two roots
// `handoff.md` section 10.5 names. The fallback keeps the script usable if a future config emits the
// main site directly into `dist`.
const siteRoot = existsSync(siteDist) ? siteDist : dist;

/** Fixture ids, kept in sync with the Markdown in `content/articles`. */
const FIXTURES = {
  // wip = true, wip_publish = false: preview only, so it must NOT exist in a production build.
  unpublishedWip: '01J8Z5Q0K3W7M9V2B4N6P8R2AC',
  // wip = true, wip_publish = true: emitted at its fixed address with noindex.
  publishedWip: '01J8Z5Q0K3W7M9V2B4N6P8R2AD',
  // wip = false, wip_ever_published = true: its old WIP URL becomes a permanent redirect.
  finishedWithHistory: '01J8Z5Q0K3W7M9V2B4N6P8R2AE',
  finishedSlug: 'finished-and-migrated',
  publishedSlug: 'hello-veloct',
  // A work, so the two publish roots can be checked against each other.
  workSlug: 'demo-counter',
};

const results = [];

function check(name, condition, detail = '') {
  results.push({ name, passed: Boolean(condition), detail });
  if (!condition) {
    console.error(`  FAIL  ${name}${detail ? `\n        ${detail}` : ''}`);
  } else {
    console.log(`  ok    ${name}`);
  }
}

function read(path) {
  return readFileSync(path, 'utf8');
}

function exists(path) {
  return existsSync(path);
}

/** Every route the build emitted, as a sorted list of `/…/` or `/…` paths. */
function emittedRoutes() {
  const routes = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const full = join(directory, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.isFile()) {
        routes.push(`/${relative(siteRoot, full).replace(/\\/g, '/')}`);
      }
    }
  };
  if (exists(siteRoot)) walk(siteRoot);
  return routes.sort();
}

/** Route existence, normalised so `/a/` and `/a/index.html` are the same thing. */
function routeExists(route) {
  const clean = route.replace(/\/$/, '');
  return (
    exists(join(siteRoot, `${clean}/index.html`)) ||
    exists(join(siteRoot, `${clean}.html`)) ||
    exists(join(siteRoot, clean))
  );
}

/** Read a route's HTML, or null when it does not exist. */
function routeHtml(route) {
  const clean = route.replace(/\/$/, '');
  for (const candidate of [
    join(siteRoot, `${clean}/index.html`),
    join(siteRoot, `${clean}.html`),
  ]) {
    if (exists(candidate)) return read(candidate);
  }
  return null;
}

console.log(`\nverifying ${siteRoot}\n`);

// ---------------------------------------------------------------------------------------------
// 1. WIP publication matrix — handoff.md section 7.1
// ---------------------------------------------------------------------------------------------
console.log('WIP matrix (handoff.md 7.1)');

const unpublishedHtml = routeHtml(`/wip/${FIXTURES.unpublishedWip}/`);
check(
  'wip_publish=false produces no production URL',
  unpublishedHtml === null,
  `expected /wip/${FIXTURES.unpublishedWip}/ to be absent from a production build`,
);

check(
  'wip_publish=false also produces no /article/<slug>/',
  !routeExists('/article/unfinished-draft/'),
  'an unpublished WIP must not leak through the article route either',
);

const publishedPath = `/wip/${FIXTURES.publishedWip}/`;
const publishedHtml = routeHtml(publishedPath);
check('wip_publish=true emits /wip/<stable-id>/', publishedHtml !== null, `missing ${publishedPath}`);

if (publishedHtml) {
  check(
    'a published WIP page is noindex',
    /<meta\s+name="robots"\s+content="[^"]*noindex/i.test(publishedHtml),
    'the robots meta must contain noindex',
  );
  check(
    'a published WIP page is canonical to itself',
    publishedHtml.includes(`rel="canonical" href=`) &&
      publishedHtml.includes(`/wip/${FIXTURES.publishedWip}/`),
    'its canonical must point at its own fixed address',
  );
  check(
    'a published WIP is NOT also emitted at /article/<slug>/',
    !routeExists('/article/shared-wip/'),
    'a WIP has exactly one public address',
  );
  check(
    'a published WIP uses the stable id, not the slug',
    !publishedHtml.includes('/wip/shared-wip'),
    'the URL must be derived from the internal id',
  );
}

// ---------------------------------------------------------------------------------------------
// 2. Finished WIP with history — handoff.md section 7.2
// ---------------------------------------------------------------------------------------------
console.log('\nFinished WIP redirect (handoff.md 7.2)');

const historyPath = `/wip/${FIXTURES.finishedWithHistory}/`;
const historyHtml = routeHtml(historyPath);
check('a once-public WIP address still exists after finishing', historyHtml !== null, `missing ${historyPath}`);

if (historyHtml) {
  check(
    'the old WIP address redirects permanently to /article/<slug>/',
    historyHtml.includes('http-equiv="refresh"') &&
      historyHtml.includes(`/article/${FIXTURES.finishedSlug}/`),
    'expected a meta refresh pointing at the article',
  );
  check(
    'the redirect page is itself noindex',
    /<meta\s+name="robots"\s+content="[^"]*noindex/i.test(historyHtml),
    'a redirect entry must not be indexed as content',
  );
}

check(
  'the finished article is published at /article/<slug>/',
  routeExists(`/article/${FIXTURES.finishedSlug}/`),
);

// ---------------------------------------------------------------------------------------------
// 3. A finished article is indexable — handoff.md section 7.1 third case
// ---------------------------------------------------------------------------------------------
console.log('\nPublished article');

const articleHtml = routeHtml(`/article/${FIXTURES.publishedSlug}/`);
check('a finished article is emitted', articleHtml !== null);
if (articleHtml) {
  const robots = articleHtml.match(/<meta\s+name="robots"\s+content="([^"]*)"/i);
  check(
    'a finished article is indexable',
    robots !== null && !robots[1].includes('noindex'),
    `robots meta was ${robots ? robots[1] : '(absent)'}`,
  );
  check(
    'the article shows a word count and a reading time',
    /\d+\s*字/.test(articleHtml) && /约\s*\d+\s*分钟/.test(articleHtml),
    'handoff.md section 4.4 requires both at the top of the article',
  );
}

// ---------------------------------------------------------------------------------------------
// 4. Sitemap — handoff.md section 7.3 and 15
// ---------------------------------------------------------------------------------------------
console.log('\nSitemap');

const sitemapFiles = readdirSync(siteRoot).filter((name) => name.startsWith('sitemap') && name.endsWith('.xml'));
const sitemap = sitemapFiles.map((name) => read(join(siteRoot, name))).join('\n');

check('a sitemap is emitted', sitemapFiles.length > 0, `found ${sitemapFiles.join(', ') || 'none'}`);
// Only the WIP ROUTE is forbidden, matched on the `/wip/` path segment with a following character.
// A plain `includes('/wip/')` would also match the legitimate `/tags/wip/` tag page, which is a
// false positive that made this check fail for the wrong reason.
check(
  'no WIP page URL appears in the sitemap',
  !/\/wip\/[^<\s]/.test(sitemap),
  'WIP pages never enter the sitemap (handoff.md section 7.3)',
);
check(
  'a published article is in the sitemap',
  sitemap.includes(`/article/${FIXTURES.publishedSlug}/`),
  'a finished article should be discoverable',
);

// ---------------------------------------------------------------------------------------------
// 5. robots.txt — handoff.md section 15
// ---------------------------------------------------------------------------------------------
console.log('\nrobots.txt');

const robotsPath = join(siteRoot, 'robots.txt');
check('robots.txt is emitted', exists(robotsPath));
if (exists(robotsPath)) {
  const robots = read(robotsPath);
  check('robots.txt disallows /wip/', robots.includes('Disallow: /wip/'));
  check('robots.txt advertises the sitemap', robots.includes('Sitemap:'));
}

// ---------------------------------------------------------------------------------------------
// 6. Two publish roots from one build — handoff.md section 10.5 and 14.1
// ---------------------------------------------------------------------------------------------
console.log('\nPublish roots (handoff.md 10.5)');

check('the works publish root exists', exists(worksDist), `missing ${worksDist}`);
check(
  'the work body is emitted into dist/works/<slug>/',
  exists(join(worksDist, FIXTURES.workSlug, 'index.html')),
  'the static body of a work must land in the works root',
);
check(
  'the work body assets are copied too',
  exists(join(worksDist, FIXTURES.workSlug, 'assets', 'app.js')),
);
check(
  'a frozen works manifest is written',
  exists(join(worksDist, 'works-manifest.json')),
  'so a rebuild can be compared without walking the tree',
);

// The main site contains the SHELL page only. If a work's own files appeared under the site root the
// two publish roots would be conflated and the isolation claim would be false.
check(
  'the work body is not served from the main site root',
  !exists(join(siteRoot, FIXTURES.workSlug, 'index.html')),
  'a work body must only exist under the works origin',
);
check(
  'the works shell page IS part of the main site',
  routeExists(`/works/${FIXTURES.workSlug}/`),
  'the shell page is what embeds the work from the works origin',
);

// ---------------------------------------------------------------------------------------------
// 7. The sandbox contract — handoff.md section 10.4
// ---------------------------------------------------------------------------------------------
console.log('\nWork isolation (handoff.md 10.4)');

const shellHtml = routeHtml(`/works/${FIXTURES.workSlug}/`);
if (shellHtml) {
  const sandbox = shellHtml.match(/sandbox="([^"]*)"/i);
  check('the shell page embeds the work in a sandboxed iframe', sandbox !== null);
  if (sandbox) {
    check(
      'the sandbox does NOT grant allow-same-origin',
      !sandbox[1].includes('allow-same-origin'),
      `sandbox was "${sandbox[1]}"`,
    );
    check(
      'the sandbox does NOT grant allow-top-navigation',
      !sandbox[1].includes('allow-top-navigation'),
      `sandbox was "${sandbox[1]}"`,
    );
    check(
      'the sandbox does NOT grant allow-popups or allow-forms',
      !sandbox[1].includes('allow-popups') && !sandbox[1].includes('allow-forms'),
      `sandbox was "${sandbox[1]}"`,
    );
  }
}

// ---------------------------------------------------------------------------------------------
// 8. SEO essentials — handoff.md section 15
// ---------------------------------------------------------------------------------------------
console.log('\nSEO (handoff.md 15)');

const homeHtml = routeHtml('/');
if (homeHtml) {
  check('the home page declares a canonical URL', homeHtml.includes('rel="canonical"'));
  check('the home page has Open Graph metadata', homeHtml.includes('property="og:title"'));
  check('the home page emits structured data', homeHtml.includes('application/ld+json'));
  check('the home page links the RSS feed', homeHtml.includes('/rss.xml'));
}

check('an RSS feed is emitted', exists(join(siteRoot, 'rss.xml')));
check('a 404 page is emitted', exists(join(siteRoot, '404.html')));

// ---------------------------------------------------------------------------------------------
// 9. Knowledge references — handoff.md section 9
// ---------------------------------------------------------------------------------------------
console.log('\nKnowledge references (handoff.md 9)');

if (articleHtml) {
  check(
    'a resolvable reference renders a card with the knowledge title',
    articleHtml.includes('知识库引用') && articleHtml.includes('SQLite 只是索引'),
    'the card shows the cited document title',
  );
  check(
    'the card carries an ellipsis as required',
    articleHtml.includes('…'),
    'handoff.md section 9.1 requires the excerpt to end with an ellipsis',
  );
  check(
    'the article carries a Harvard-style reference list',
    articleHtml.includes('References') && /Vellow \(20\d\d\)/.test(articleHtml),
    'handoff.md section 9.4 requires an automatically generated reference list',
  );
  check(
    'paragraph ids are not visible as comments in the rendered article',
    !articleHtml.includes('<!--p:'),
    'internal identity must never reach the reader',
  );
  check(
    'blocks carry an ordinal so a reference can open the cited location',
    /data-veloct-block="\d+"/.test(articleHtml),
    'the right sidebar needs this to highlight the cited paragraph',
  );
}

// ---------------------------------------------------------------------------------------------
// Comments (handoff.md section 16)
//
// The contract is mostly a list of things that must NOT exist: no fake local comment system, and an
// explicit unavailable state rather than an empty box. Both are asserted here, because "we did not build
// the wrong thing" is exactly the claim that erodes silently.
// ---------------------------------------------------------------------------------------------
{
  const article = join(siteRoot, 'article');
  const candidates = [];
  const walk = (directory) => {
    let entries = [];
    try {
      entries = readdirSync(directory, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = join(directory, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name === 'index.html') candidates.push(full);
    }
  };
  walk(article);

  // An article page is one that carries the reference block, rather than the article list.
  const articleHtml = candidates
    .map((file) => readFileSync(file, 'utf8'))
    .find((html) => html.includes('data-veloct-article-body')) ?? '';

  check(
    'the article page has a comment section',
    /<section class="comments"/.test(articleHtml),
    'handoff.md section 16 asks for a comment area',
  );

  check(
    'there is no fake local comment system',
    // A textarea anywhere in the page would accept text the static site cannot store.
    !articleHtml.includes('<textarea'),
    'section 16 forbids a local comment system; anything accepting text here would lose it',
  );

  // The class name also appears in Astro's inlined <style>, so the ELEMENT is matched rather than the text.
  const unavailableElement = /<div class="comments-unavailable"/.test(articleHtml);
  const giscusElement = /class="giscus"/.test(articleHtml);

  check(
    'comments are either wired to giscus or explicitly unavailable',
    unavailableElement || giscusElement,
    'section 16 requires an explicit unavailable state rather than an empty box',
  );

  check(
    'giscus is never loaded without a repository to load comments FROM',
    // An iframe with an empty repo-id renders a blank frame and a console error, which is the
    // broken-looking box the contract rules out.
    !articleHtml.includes('giscus.app') || /data-repo="[^"]+"/.test(articleHtml),
    'a partial giscus configuration must fall back to the unavailable state',
  );

  check(
    'the comment section says where comments are stored when it renders one',
    !giscusElement || articleHtml.includes('存储在 GitHub Discussions'),
    'section 16 asks for a hint icon telling the reader the comments live in GitHub Discussions',
  );
}

// ---------------------------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------------------------
const failed = results.filter((result) => !result.passed);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);

if (failed.length > 0) {
  console.error(`\n${failed.length} check(s) failed:`);
  for (const result of failed) {
    console.error(`  - ${result.name}${result.detail ? `: ${result.detail}` : ''}`);
  }
  process.exit(1);
}

console.log('build verification passed\n');
