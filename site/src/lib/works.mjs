// Reading the works truth source.
//
// `handoff.md` section 10.1: a work's metadata truth is `works/<stable-id>/work.json` and its body
// truth is the sibling `site/` directory. SQLite only ever holds an index of either, so the site
// reads the files directly and stays correct even with the database removed.
//
// This module exists so the works index and the shell page cannot disagree about what a work is, and
// so the path derivation is in ONE place. The level count is easy to get wrong: this file lives in
// `site/src/lib/`, so the project root is three levels up.

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * The `works/` directory of the project this site belongs to.
 *
 * The path is derived from the build's working directory rather than from `import.meta.url`. That is
 * deliberate and load-bearing: `getStaticPaths` runs during the prerender phase from a bundled chunk
 * under `dist/.prerender/`, where `import.meta.url` points at the chunk rather than at this file, so
 * a URL-relative path silently resolves to the wrong directory and the route produces no pages at
 * all. Astro runs `astro build` and `astro dev` with the site directory as the working directory, so
 * `../works` is stable across dev, build and preview.
 *
 * `VELOCT_PROJECT_ROOT` is honoured first so the desktop app can point a frozen template at the
 * project it is building.
 */
export function projectRoot() {
  if (process.env.VELOCT_PROJECT_ROOT) {
    return resolve(process.env.VELOCT_PROJECT_ROOT);
  }
  return resolve(process.cwd(), '..');
}

/** The `works/` directory of the project. */
export function worksDirectory() {
  return join(projectRoot(), 'works');
}

/** Where the site configuration's exported files live. */
export function dataDirectory() {
  return join(projectRoot(), 'data');
}

/**
 * The site directory the build is running from.
 *
 * Exported for the same reason: anything that needs a project path must use these helpers rather
 * than `import.meta.url`.
 */
export function siteDirectory() {
  return process.cwd();
}

/** Kept for callers that still want the module's own URL; not used for path derivation. */
export const moduleUrl = fileURLToPath(import.meta.url);

/**
 * Read every work, sorted by completion time descending.
 *
 * A `work.json` that cannot be parsed is a hard failure rather than a skipped entry: silently
 * dropping a work from the site would hide a real problem, which `handoff.md` forbids.
 */
export function readWorks() {
  const directory = worksDirectory();
  if (!existsSync(directory)) return [];

  const works = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;

    const metaPath = join(directory, entry.name, 'work.json');
    if (!existsSync(metaPath)) continue;

    let meta;
    try {
      meta = JSON.parse(readFileSync(metaPath, 'utf8'));
    } catch (error) {
      throw new Error(`could not read ${metaPath}: ${error.message}`);
    }

    const siteDir = join(directory, entry.name, 'site');
    works.push({
      /** The permanent directory name, which is the stable id. */
      id: entry.name,
      /** The mutable public slug. */
      slug: meta.slug ?? entry.name,
      title: meta.title ?? entry.name,
      author: meta.author ?? '',
      summary: meta.summary ?? '',
      completedAt: meta.completed_at ?? null,
      importedAt: meta.imported_at ?? null,
      entry: meta.entry ?? 'index.html',
      screenshot: meta.screenshot ?? null,
      cover: meta.cover ?? null,
      tags: Array.isArray(meta.tags) ? meta.tags : [],
      notes: meta.notes ?? '',
      /** Whether the static body exists at all, so the UI can say so instead of showing a blank. */
      hasBody: existsSync(siteDir),
    });
  }

  works.sort((a, b) => String(b.completedAt ?? '').localeCompare(String(a.completedAt ?? '')));
  return works;
}

/** Find one work by its public slug. */
export function findWorkBySlug(slug) {
  return readWorks().find((work) => work.slug === slug) ?? null;
}
