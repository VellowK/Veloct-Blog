// The WIP publication rules, in one place.
//
// `handoff.md` section 7.1 defines a four-way matrix, and getting it wrong is the difference
// between leaking an unfinished draft to the public internet and hiding a published page. It is
// therefore implemented once here and consumed by every route.
//
//  wip = true,  wip_publish = false  -> preview only: no production URL at all
//  wip = true,  wip_publish = true   -> /wip/<stable-id>, noindex, not in the sitemap, canonical
//                                       to itself, and NOT also /article/<slug>
//  wip = false                       -> /article/<slug>, indexable, in the sitemap
//  wip = false, wip_ever_published   -> additionally a permanent redirect from /wip/<stable-id>
//                                       (handoff.md section 7.2)
//
// The stable WIP URL uses the internal id, never the slug: a draft's slug can change while people
// hold a link to it.

/** Where an emitted page lives, in both preview and production. */
export const WipRoute = Object.freeze({
  /** A published article, served at /article/<slug>/. */
  Article: 'article',
  /** A published-but-unfinished WIP, served at /wip/<id>/. */
  Wip: 'wip',
  /** A finished article whose old /wip/<id> must redirect. */
  WipRedirect: 'wip-redirect',
  /** Preview only: reachable under `astro dev`, absent from a production build. */
  PreviewOnly: 'preview-only',
  /** Never emitted anywhere. */
  None: 'none',
});

/**
 * Whether this document is currently a WIP.
 *
 * Only articles can be WIP (`handoff.md` section 3.2), so a stray `wip` flag on a page is ignored
 * rather than changing how it is published.
 */
export function isWip(entry) {
  return entry.data.type === 'article' && entry.data.wip === true;
}

/**
 * Decide how one article is published.
 *
 * `isProduction` is what makes the difference between the preview server and a release build: a
 * publishable-but-unpublished WIP is reachable while previewing and absent from `dist`.
 */
export function articleRoute(entry, { isProduction }) {
  const { slug, wip, wip_publish: wipPublish, wip_ever_published: everPublished } = entry.data;
  const id = entry.id;

  if (!wip) {
    return {
      kind: WipRoute.Article,
      slug,
      id,
      // A finished article that was once public keeps a redirect entry (section 7.2).
      redirectFrom: everPublished ? `/wip/${id}/` : null,
    };
  }

  if (wipPublish) {
    return { kind: WipRoute.Wip, slug, id, redirectFrom: null };
  }

  // Not publishable. In preview it is reachable; in a production build it must not exist.
  return {
    kind: isProduction ? WipRoute.None : WipRoute.PreviewOnly,
    slug,
    id,
    redirectFrom: null,
  };
}

/** Articles that should appear as published articles, in production or preview. */
export function publishedArticles(entries, options) {
  return entries.filter((entry) => articleRoute(entry, options).kind === WipRoute.Article);
}

/** WIP entries that are emitted into the current output, in production or preview. */
export function emittedWips(entries, options) {
  return entries.filter((entry) => {
    const route = articleRoute(entry, options);
    return route.kind === WipRoute.Wip || route.kind === WipRoute.PreviewOnly;
  });
}

/**
 * Whether an entry may be indexed.
 *
 * WIP pages are always noindex, and a site-wide indexing switch in the configuration must be able
 * to turn everything off (handoff.md section 7.3, 15).
 */
export function isIndexable(entry, { allowIndexing }) {
  if (!allowIndexing) return false;
  return !isWip(entry);
}

/** The canonical URL for an entry, or `null` when the page must point at itself or nothing. */
export function canonicalPath(entry, options) {
  const route = articleRoute(entry, options);
  switch (route.kind) {
    case WipRoute.Article:
      return `/article/${route.slug}/`;
    case WipRoute.Wip:
      // A WIP page's canonical points at itself (handoff.md section 7.3).
      return `/wip/${route.id}/`;
    case WipRoute.PreviewOnly:
      return `/wip/${route.id}/`;
    default:
      return null;
  }
}

/**
 * The WIP waterfall only shows WIP that is actually in the current static output
 * (`handoff.md` section 7.1).
 */
export function waterfallWips(entries, options) {
  return entries.filter((entry) => articleRoute(entry, options).kind === WipRoute.Wip);
}
