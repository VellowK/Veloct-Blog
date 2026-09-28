// Content collections.
//
// The frontmatter contract is `docs/spec/01-data-model.md` section 2, implemented in Rust by
// `veloct-markdown` and mirrored here so a build fails loudly on a document that violates it
// rather than publishing something subtly wrong.
//
// Three Astro 7 specifics that matter:
//
//  * The config file is `src/content.config.ts`. `src/content/config.ts` is the removed legacy
//    location.
//  * Zod is v4: `z` comes from `astro/zod`, and the error option is `{ error }`, not `{ message }`.
//  * There is no `entry.slug`. `entry.id` is the generated id, and a frontmatter `slug` OVERRIDES
//    it — which is exactly the "permanent id, mutable public slug" split handoff.md section 3.2
//    requires. Pages therefore route on `data.slug`.

import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, resolve } from 'node:path';

// `new URL(...).pathname` would produce `/D:/...` on Windows, so the project root is derived with
// `fileURLToPath`. The content directories live OUTSIDE `site/`, at the project root, because
// Markdown is the project's truth rather than the template's (handoff.md section 3.4).
// `import.meta.url` here is `…/site/src/content.config.ts`, so the project root is two levels up
// from `src`.
const srcDir = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(srcDir, '..', '..');

/**
 * `glob()`'s `base` is resolved through `fileURLToPath`, so it must be a file URL rather than a
 * bare platform path: on Windows a `D:\...` string makes Node throw "The URL must be of scheme
 * file". `pathToFileURL` is what turns the project-relative directory into the required form.
 */
const contentBase = (segments: string) => pathToFileURL(resolve(projectRoot, segments)).href;

/**
 * Preserve the file name exactly as the id.
 *
 * The default `glob()` id is SLUGIFIED, which lowercases it. Veloct's stable ids are ULIDs in
 * Crockford base32 and the stable WIP address is `/wip/<id>/` (handoff.md section 7.1), so a
 * lowercased id would produce a different URL from the one the contract names and from the one the
 * Rust side writes into `articleRoute`. Using the bare file name keeps the id byte-identical to the
 * frontmatter `id`.
 */
const stableId = ({ entry }: { entry: string }) => entry.replace(/\.md$/, '');

/**
 * An RFC 3339 UTC timestamp.
 *
 * YAML has a native timestamp type, so `created_at: 2026-01-01T00:00:00Z` arrives as a `Date`
 * rather than a string, while a quoted value arrives as a string. Both are legitimate ways for an
 * author to write the same instant, so the schema normalises them to one canonical text form instead
 * of rejecting one of them. Normalising here is what lets the Rust side (`veloct-markdown`) and the
 * build agree on a single representation.
 */
const rfc3339 = z
  .union([z.string(), z.date()])
  .transform((value, ctx) => {
    const date = value instanceof Date ? value : new Date(value);
    if (Number.isNaN(date.getTime())) {
      ctx.addIssue({ code: 'custom', message: `not a valid timestamp: ${String(value)}` });
      return z.NEVER;
    }
    return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
  })
  .describe('RFC 3339 UTC timestamp');

/** The same, but the value may be absent. */
const rfc3339Optional = z
  .union([z.string(), z.date(), z.null()])
  .optional()
  .transform((value, ctx) => {
    if (value === undefined || value === null) return undefined;
    const date = value instanceof Date ? value : new Date(value);
    if (Number.isNaN(date.getTime())) {
      ctx.addIssue({ code: 'custom', message: `not a valid timestamp: ${String(value)}` });
      return z.NEVER;
    }
    return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
  })
  .describe('optional RFC 3339 UTC timestamp');

/** Shared frontmatter fields. */
const baseFields = {
  id: z.string().min(1),
  slug: z.string().min(1),
  title: z.string().min(1),
  summary: z.string().optional(),
  created_at: rfc3339,
  updated_at: rfc3339,
  published_at: rfc3339Optional,
  tags: z.array(z.string()).default([]),
  category: z.string().optional(),
  cover: z.string().optional(),
  blocks: z.array(z.string()).default([]),
};

/**
 * `type` is declared per collection rather than as a union, so a page file that says
 * `type: article` is a build error instead of a document that routes as the wrong kind.
 */
const articles = defineCollection({
  loader: glob({
    base: contentBase('content/articles'),
    pattern: '**/*.md',
    generateId: stableId,
  }),
  schema: z
    .object({
      ...baseFields,
      type: z.literal('article'),
      // WIP is an attribute of an ordinary article, not a separate content type
      // (handoff.md section 3.1, 7).
      wip: z.boolean().default(false),
      wip_publish: z.boolean().default(false),
      wip_ever_published: z.boolean().default(false),
    })
    .refine((data) => !(data.wip_publish && !data.wip), {
      error: 'wip_publish is true while wip is false; publishable WIP must still be WIP',
      path: ['wip_publish'],
    }),
});

const pages = defineCollection({
  loader: glob({
    base: contentBase('content/pages'),
    pattern: '**/*.md',
    generateId: stableId,
  }),
  schema: z.object({
    ...baseFields,
    type: z.literal('page'),
  }),
});

const knowledge = defineCollection({
  loader: glob({
    base: contentBase('knowledge'),
    pattern: '**/*.md',
    generateId: stableId,
  }),
  schema: z.object({
    ...baseFields,
    type: z.literal('knowledge'),
    author: z.string().optional(),
    year: z.string().optional(),
    url: z.string().optional(),
  }),
});

export const collections = { articles, pages, knowledge };
