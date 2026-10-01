// Split one Astro build into the two publish roots Veloct requires.
//
// `handoff.md` section 10.5 and 14.1: one publish runs Astro exactly ONCE and that single build
// produces one `dist/` containing `dist/site/` (the main site) and `dist/works/` (the work bodies
// that are served from the separate Works Origin).
//
// Astro has exactly one `outDir`, and there is no supported way to declare a second one. The
// documented approach is therefore: keep the single output directory, and in the `astro:build:done`
// hook write the second root yourself. Note the hook argument is `pages`, not `routes`, and `dir`
// is a URL.
//
// The work bodies are copied from `works/<stable-id>/site/`, which is the work truth
// (`handoff.md` section 10.1). Nothing is generated from the database, so a build with the SQLite
// index removed still produces the correct works root.

import { cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// `import.meta.url` here is `…/site/src/integrations/split-publish-roots.mjs`, so the site directory
// is two levels up and the project root is three. `fileURLToPath` rather than `.pathname`, which
// would produce `/D:/...` on Windows.
const siteDir = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const projectRoot = resolve(siteDir, '..');

/** Read every work's metadata from its truth source. */
async function readWorks(worksDir) {
  if (!existsSync(worksDir)) return [];

  const entries = await readdir(worksDir, { withFileTypes: true });
  const works = [];

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const workJson = join(worksDir, entry.name, 'work.json');
    if (!existsSync(workJson)) continue;

    try {
      const raw = await readFile(workJson, 'utf8');
      const meta = JSON.parse(raw);
      works.push({
        id: entry.name,
        directory: join(worksDir, entry.name),
        meta,
        // The public slug comes from work.json; the directory name is the permanent stable id.
        slug: meta.slug ?? entry.name,
      });
    } catch (error) {
      // A broken work.json must fail the build rather than silently drop a work from the site.
      throw new Error(`could not read ${workJson}: ${error.message}`);
    }
  }

  works.sort((a, b) => a.slug.localeCompare(b.slug));
  return works;
}

/** Copy one work's static body into the works publish root. */
async function copyWorkBody(work, worksRoot) {
  const source = join(work.directory, 'site');
  const target = join(worksRoot, work.slug);

  if (!existsSync(source)) return { id: work.id, slug: work.slug, entry: work.meta.entry ?? null, files: [] };

  await rm(target, { recursive: true, force: true });
  await mkdir(dirname(target), { recursive: true });
  await cp(source, target, { recursive: true });

  const files = [];
  const walk = async (directory, prefix) => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        await walk(join(directory, entry.name), relative);
      } else if (entry.isFile()) {
        files.push(relative);
      }
    }
  };
  await walk(source, '');
  files.sort();

  return {
    id: work.id,
    slug: work.slug,
    entry: work.meta.entry ?? 'index.html',
    files,
  };
}

/**
 * Copy one work's `preview/` files to the MAIN site root, where the poster wall expects them.
 *
 * `handoff.md` section 10.1 puts `preview/` beside `site/` rather than inside it, and section
 * 10.3's poster wall reads `/work-previews/<slug>/screenshot.webp`. Nothing else writes that route,
 * so without this the poster is always a placeholder even when the work ships a screenshot — which
 * is exactly the gap PROCESSING.md section 5.9 recorded.
 *
 * The copy is driven by what is ON DISK rather than by what `work.json` names, so a work whose
 * metadata points at a file that is not there produces no route at all rather than a route that
 * 404s on a published page.
 */
async function copyWorkPreviews(work, siteRoot) {
  const source = join(work.directory, 'preview');
  if (!existsSync(source)) return [];

  const target = join(siteRoot, 'work-previews', work.slug);
  await rm(target, { recursive: true, force: true });
  await mkdir(target, { recursive: true });
  await cp(source, target, { recursive: true });

  const files = [];
  const walk = async (directory, prefix) => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        await walk(join(directory, entry.name), relative);
      } else if (entry.isFile()) {
        files.push(relative);
      }
    }
  };
  await walk(source, '');
  files.sort();
  return files;
}

export function splitPublishRoots() {
  return {
    name: 'veloct:split-publish-roots',
    hooks: {
      'astro:build:done': async ({ dir, logger }) => {
        // The works root is `dist/works`, resolved from an absolute path the config publishes.
        //
        // It is NOT derived from the hook's `dir`: that argument names the MAIN output root, and
        // computing the sibling `dist/` from it depends on how Astro nested `outDir`. Getting that
        // wrong put the work bodies under `dist/site/works/`, where they collided with the shell
        // pages at `/works/<slug>/` and silently replaced them.
        const siteRoot = fileURLToPath(dir);
        const distRoot = process.env.VELOCT_DIST_ROOT || dirname(siteRoot);
        const worksRoot = join(distRoot, 'works');

        await mkdir(worksRoot, { recursive: true });

        const worksDir = join(projectRoot, 'works');
        const works = await readWorks(worksDir);
        const manifest = [];

        for (const work of works) {
          const entry = await copyWorkBody(work, worksRoot);
          // The poster lives on the MAIN site root, not in the works root: it is a card image the
          // shell page shows, not part of the work that runs on the works origin.
          entry.previews = await copyWorkPreviews(work, siteRoot);
          manifest.push(entry);
          logger.info(
            `works origin: ${work.slug}/ (${entry.files.length} file${entry.files.length === 1 ? '' : 's'})`,
          );
          if (entry.previews.length > 0) {
            logger.info(`work previews: /work-previews/${work.slug}/ (${entry.previews.join(', ')})`);
          }
        }

        // A frozen manifest so a rebuild can be compared without walking the trees
        // (handoff.md section 14.9).
        const manifestPath = join(worksRoot, 'works-manifest.json');
        await writeFile(
          manifestPath,
          `${JSON.stringify({ version: 1, works: manifest }, null, 2)}\n`,
          'utf8',
        );

        logger.info(`works publish root written to ${worksRoot}`);
      },
    },
  };
}

export default splitPublishRoots;
