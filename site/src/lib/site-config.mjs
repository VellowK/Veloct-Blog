// Site configuration for the build.
//
// The configuration's truth source is SQLite (handoff.md section 2.3: YAML is not used for site
// configuration). Before a build, the desktop app exports it to `data/site.json`; this module is
// the build's only reader, so the site never queries the database and the template can also run on
// its own with defaults.

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
/**
 * The project root this site belongs to.
 *
 * Derived from the build's working directory rather than from `import.meta.url`. That matters: this
 * module is bundled, and during the prerender phase `import.meta.url` points into
 * `dist/.prerender/`, so a URL-relative path resolves to the wrong place and the configuration
 * silently falls back to defaults. Astro runs `astro dev` and `astro build` with the site directory
 * as the working directory, so `..` is stable. `VELOCT_PROJECT_ROOT` lets the desktop app point a
 * frozen template at the project it is building.
 */
export const projectRoot = process.env.VELOCT_PROJECT_ROOT
  ? resolve(process.env.VELOCT_PROJECT_ROOT)
  : resolve(process.cwd(), '..');

/** The site directory itself. */
export const siteDir = here;

/**
 * Defaults that keep a bare template runnable and byte-stable.
 *
 * Every value here is deliberately boring: the point is that `astro build` works and produces a
 * deterministic result before any project configuration exists.
 */
const DEFAULTS = {
  name: 'Veloct',
  description: 'A personal site built with Veloct.',
  url: 'https://example.com',
  base: '/',
  language: 'zh-CN',
  articleDisplayCount: 10,
  archiveDisplayCount: 10,
  categoryDisplayCount: 10,
  tagDisplayCount: 10,
  author: {
    name: '',
    bio: '',
    avatar: '',
  },
  social: [],
  homepage: { cover: '', githubUsername: '', showBlogHeatmap: true, showGithubHeatmap: true },
  filing: {},
  comments: {
    enabled: false,
    // handoff.md section 16: when comments are on they REQUIRE a GitHub connection and live in GitHub
    // Discussions. These name the repository and the giscus identifiers for it.
    //
    // There is no token field, and that is the point: giscus authenticates the reader through GitHub's own
    // window, so the static site holds no secret and cannot leak one.
    repo: '',
    repoId: '',
    category: 'Announcements',
    categoryId: '',
    mapping: 'pathname',
    reactionsEnabled: true,
    inputPosition: 'bottom',
    theme: 'preferred_color_scheme',
  },
  seo: {
    // A single switch that must be able to flip robots.txt AND every page's robots meta together
    // (handoff.md section 15).
    allowIndexing: true,
    engines: [],
  },
  hero: {
    // Empty means Hero follows the site name rather than a hard-coded template brand.
    title: '',
    subtitle: '',
    ctaLabel: '',
    ctaHref: '',
  },
  // The effect the site uses between two navigations. This is CONFIGURATION rather than a local
  // preference: the thing that animates is the reader's browser, so it has to travel with the build.
  // `rise` settles the new page up into place, `dissolve` cross-fades, `none` switches instantly.
  pageTransition: 'rise',
};

/** The three effects the template knows how to draw, in the order the form offers them. */
export const PAGE_TRANSITIONS = ['rise', 'dissolve', 'none'];

/** Merge `override` onto `base`, one level deep for plain objects. */
function merge(base, override) {
  if (override === null || override === undefined) return base;
  if (Array.isArray(base) || typeof base !== 'object') return override;
  if (typeof override !== 'object' || Array.isArray(override)) return override;

  const out = { ...base };
  for (const [key, value] of Object.entries(override)) {
    out[key] = key in base ? merge(base[key], value) : value;
  }
  return out;
}

/** New snake_case SEO keys win when both old and new names are present. */
export function normalizeSiteConfig(override = {}) {
  const config = merge(DEFAULTS, override);
  const seo = override.seo ?? {};
  config.seo = {
    ...config.seo,
    allowIndexing: seo.allow_indexing ?? seo.allowIndexing ?? true,
    keywords: seo.keywords ?? '',
    htmlInjection: seo.html_injection ?? seo.htmlInjection ?? '',
  };
  for (const key of ['articleDisplayCount', 'archiveDisplayCount', 'categoryDisplayCount', 'tagDisplayCount']) {
    const value = config[key];
    if (!Number.isInteger(value) || value < 1 || value > 100) {
      throw new Error(`${key} must be an integer between 1 and 100`);
    }
  }
  // Refused rather than defaulted, for the reason the whole file exists: a build that quietly
  // ignored the setting would publish a site whose behaviour differs from its configuration.
  if (!PAGE_TRANSITIONS.includes(config.pageTransition)) {
    throw new Error(`pageTransition must be one of ${PAGE_TRANSITIONS.join(', ')}`);
  }
  if (!config.homepage || typeof config.homepage !== 'object' || Array.isArray(config.homepage)) throw new Error('homepage must be an object');
  for (const key of ['cover', 'githubUsername']) if (typeof config.homepage[key] !== 'string') throw new Error(`homepage.${key} must be a string`);
  for (const key of ['showBlogHeatmap', 'showGithubHeatmap']) if (typeof config.homepage[key] !== 'boolean') throw new Error(`homepage.${key} must be a boolean`);
  if (config.homepage.githubUsername && (!/^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$/i.test(config.homepage.githubUsername) || config.homepage.githubUsername.includes('--'))) throw new Error('homepage.githubUsername must be a GitHub username, not a URL');
  const filing = config.filing ?? {};
  config.filing = { ...filing };
  for (const key of ['icpUrl', 'policeUrl', 'moeUrl']) {
    const value = filing[key] ?? '';
    if (typeof value !== 'string') throw new Error(`${key} must be a URL string`);
    if (value) {
      let url;
      try { url = new URL(value); } catch { throw new Error(`${key} must be a valid http or https URL`); }
      if (!['http:', 'https:'].includes(url.protocol) || !url.hostname || url.username || url.password || /\s/.test(value)) {
        throw new Error(`${key} must be a valid http or https URL`);
      }
    }
    config.filing[key] = value;
  }
  return config;
}

/** Read `data/site.json` if the app exported one, then fill in the defaults. */
export function readSiteConfig() {
  const candidates = [
    resolve(projectRoot, 'data', 'site.json'),
    resolve(siteDir, 'data', 'site.json'),
  ];

  for (const path of candidates) {
    try {
      const raw = readFileSync(path, 'utf8');
      if (raw.trim().length === 0) continue;
      return normalizeSiteConfig(JSON.parse(raw));
    } catch (error) {
      if (error && error.code === 'ENOENT') continue;
      // A malformed configuration must fail the build loudly rather than fall back to defaults and
      // quietly publish the wrong site (handoff.md section 0: no silent failure).
      throw new Error(`could not read site configuration at ${path}: ${error.message}`);
    }
  }

  return DEFAULTS;
}

/** Friend links, from `data/links.json`. */
export function readFriendLinks() {
  const candidates = [
    resolve(projectRoot, 'data', 'links.json'),
    resolve(siteDir, 'data', 'links.json'),
  ];

  for (const path of candidates) {
    try {
      const raw = readFileSync(path, 'utf8');
      if (raw.trim().length === 0) continue;
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : (parsed.links ?? []);
    } catch (error) {
      if (error && error.code === 'ENOENT') continue;
      throw new Error(`could not read friend links at ${path}: ${error.message}`);
    }
  }

  return [];
}

/** Navigation items, from `data/nav.json`, falling back to a sensible default set. */
export function readNavigation() {
  const candidates = [
    resolve(projectRoot, 'data', 'nav.json'),
    resolve(siteDir, 'data', 'nav.json'),
  ];

  for (const path of candidates) {
    try {
      const raw = readFileSync(path, 'utf8');
      if (raw.trim().length === 0) continue;
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.length > 0) return parsed;
    } catch (error) {
      if (error && error.code === 'ENOENT') continue;
      throw new Error(`could not read navigation at ${path}: ${error.message}`);
    }
  }

  return [
    { label: '首页', href: '/', icon: 'home' },
    { label: '文章', href: '/article/', icon: 'article' },
    { label: '知识库', href: '/docs/', icon: 'docs' },
    { label: '作品', href: '/works/', icon: 'works' },
    { label: '归档', href: '/archive/', icon: 'archive' },
    { label: '友链', href: '/links/', icon: 'links' },
    { label: '关于', href: '/about/', icon: 'about' },
  ];
}

/** Where the work bodies are served from, when a Works Origin is configured. */
export function readWorksOrigin() {
  const config = readSiteConfig();
  return config.worksOrigin ?? '';
}
