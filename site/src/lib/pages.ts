// Built-in pages are ordinary Markdown documents, addressed by their frontmatter slug.
// Fixed Astro routes render them; no catch-all is registered for the same URLs.
import { getCollection, render } from 'astro:content';

export async function readManagedPage(slug: 'home' | 'about' | 'links' | 'archive') {
  const matches = (await getCollection('pages')).filter((entry) => entry.data.slug === slug);
  if (matches.length > 1) {
    throw new Error(`Duplicate page slug "${slug}": ${matches.map((entry) => entry.id).join(', ')}`);
  }
  const entry = matches[0] ?? null;
  const rendered = entry ? await render(entry) : null;
  return { entry, Content: rendered?.Content ?? null, headings: rendered?.headings ?? [] };
}
