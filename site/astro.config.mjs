// Veloct site configuration.
//
// This is the ONE Astro project handoff.md section 5 requires: the desktop app runs `astro dev`
// against it for live preview and `astro build` against it for a release, so preview and
// production cannot diverge in config, content pipeline, components or styles.
//
// Choices that are deliberate rather than default:
//
//  * `compressHTML: 'jsx'` is pinned because v7 changed the default from `true`, and the value
//    changes emitted bytes. A reproducible release comparison (handoff.md section 14.9) needs it
//    fixed.
//  * `build.format: 'directory'` with `trailingSlash: 'always'` is the documented pairing, and it
//    keeps `/article/slug/` stable for a static host.
//  * `scopedStyleStrategy: 'attribute'` is pinned for the same byte-stability reason.
//  * `markdown.processor` opts back into the unified pipeline, because Astro 7's default processor
//    no longer runs remark plugins and Veloct's directives must be real AST nodes rather than
//    string matching (handoff.md section 3.3, ADR-0011).
//  * Sitemap `lastmod` is NOT set to `new Date()`. Veloct injects `VELOCT_BUILD_TIME` as the
//    frozen release time, so time-derived output cannot move on a rebuild.

import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import sitemap from '@astrojs/sitemap';
import tailwindcss from '@tailwindcss/vite';
import remarkDirective from 'remark-directive';

import { veloctDirectives } from './src/remark/veloct-directives.mjs';
import { veloctBlockOrdinals } from './src/remark/veloct-block-ordinals.mjs';
import { splitPublishRoots } from './src/integrations/split-publish-roots.mjs';
import { readSiteConfig } from './src/lib/site-config.mjs';
import { linkedKnowledgeReload } from './src/lib/linked-knowledge.mjs';
import { defaultPaths, writeKnowledgeIndex } from './src/lib/knowledge-scan.mjs';

const siteConfig = readSiteConfig();
const site = siteConfig.url ?? 'https://example.com';

/**
 * The site directory as a native path.
 *
 * `new URL(...).pathname` yields `/D:/...` on Windows, and joining that with another path produces
 * `D:\D:\...`. `fileURLToPath` is the only correct way to turn a file URL into a platform path.
 */
const siteDirPath = fileURLToPath(new URL('.', import.meta.url));

/**
 * Generate the knowledge index before anything else is loaded.
 *
 * `handoff.md` section 9.4 requires a reference to resolve to the current text at build time, and a
 * component cannot await a collection query. The config is evaluated first, so this is where a
 * synchronously importable index has to be produced.
 *
 * Diagnostics for a duplicate or missing paragraph id are printed here as well as being surfaced on
 * the page, so a release build's log names the file (section 9.3).
 */
const knowledge = writeKnowledgeIndex(defaultPaths(siteDirPath));
for (const diagnostic of knowledge.diagnostics) {
  const line = `[veloct] ${diagnostic.code}: ${diagnostic.message}`;
  if (diagnostic.severity === 'error') {
    console.error(line);
  } else {
    console.warn(line);
  }
}

/** The frozen build time, or the epoch so a template run is still deterministic. */
const releaseBuildTime = process.env.VELOCT_BUILD_TIME ?? '1970-01-01T00:00:00.000Z';

/**
 * Where the two publish roots live.
 *
 * Published through the environment rather than recomputed inside the build integration, because
 * this is the only place that unambiguously knows the site directory: the integration receives
 * `dir` for the MAIN output root, and deriving `dist/` from it depends on how Astro chose to nest
 * `outDir`, which is exactly the kind of assumption that produced a build where the work bodies
 * landed under `dist/site/works/` and collided with the shell pages.
 */
process.env.VELOCT_DIST_ROOT = resolve(siteDirPath, 'dist');

export default defineConfig({
  site,
  base: siteConfig.base ?? undefined,
  trailingSlash: 'always',
  // The main site goes to `dist/site` and the work bodies to `dist/works`, which is exactly the two
  // publish roots handoff.md section 10.5 and 14.1 name. This is not cosmetic: the shell page route
  // is `/works/<slug>/`, so emitting the main site directly into `dist/` would collide with
  // `dist/works/<slug>/` and the work body would overwrite the shell page (or the reverse).
  outDir: './dist/site',
  compressHTML: 'jsx',
  scopedStyleStrategy: 'attribute',
  // Off, because it exists only in `astro dev` and therefore only in the preview: `handoff.md`
  // section 5 requires the preview to be the same project as the production build so it PREDICTS it,
  // and a floating dev overlay that no reader ever sees works against that. It also sits in the
  // document during a page transition, so the overlay would visibly animate along with the page.
  devToolbar: { enabled: false },
  build: {
    format: 'directory',
    assets: '_astro',
  },
  markdown: {
    processor: unified({
      remarkPlugins: [remarkDirective, veloctDirectives],
      // Block ordinals are applied on the HAST tree rather than in remark: a remark paragraph's
      // `hProperties` do not survive Astro's markdown rendering, while a rehype element's
      // `properties` do. They are what let a knowledge reference open the cited paragraph.
      rehypePlugins: [veloctBlockOrdinals],
    }),
    shikiConfig: {
      // Keep highlighting deterministic: no theme auto-detection and no injected timestamps.
      theme: 'github-dark-default',
      wrap: false,
    },
  },
  vite: {
    plugins: [tailwindcss(), linkedKnowledgeReload()],
  },
  integrations: [
    sitemap({
      // WIP pages are noindex and must never appear in the sitemap (handoff.md section 7.3).
      filter: (page) => !new URL(page).pathname.startsWith('/wip/'),
      // A fixed value, from the frozen release time, so the sitemap is byte-stable on a rebuild.
      lastmod: new Date(releaseBuildTime),
      changefreq: 'weekly',
    }),
    splitPublishRoots(),
  ],
});
