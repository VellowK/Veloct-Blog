// The Veloct remark directive plugin.
//
// `handoff.md` section 3.3 requires the custom Markdown capabilities to be structured AST rather
// than fragile string matching, and names what must be covered: knowledge reference cards,
// download link cards, hyperlink cards and other page sections. This plugin turns the directive
// syntax into element nodes, and the pages map those elements to components through the documented
// `<Content components={{ ... }} />` prop.
//
// Two deliberate choices:
//
//  * The emitted `hName` values are real HTML element names, not invented capitalised tags. The
//    Astro documentation only sanctions mapping HTML element names, so `aside`/`figure`/`a` are
//    used together with `data-veloct-*` attributes to say which component should render them.
//  * The directive names and required attributes must match `veloct-markdown`'s table in
//    `docs/spec/01-data-model.md` section 2.2. The `DIRECTIVES` map below is the single place to
//    change if that table ever moves.

import { visit } from 'unist-util-visit';

/**
 * Directive name to emitted element and the attributes it carries.
 *
 * Each directive maps to a DISTINCT element name, because a page's `components` object maps element
 * to component: two directives sharing an element would collide and one would silently render as
 * the other. The mapping is:
 *
 *   knowledgeref -> aside    (knowledge reference card)
 *   download     -> a        (download card, resolved together with linkcard by hostname)
 *   linkcard     -> a
 *   note         -> details  (collapsible callout, no JavaScript needed)
 *   image        -> figure   (placeholder: handoff.md section 8.1 forbids generating real files)
 *   section      -> section  (a page section, "other page blocks" in section 3.3)
 *   panel        -> div      (a generic card)
 */
export const DIRECTIVES = {
  knowledgeref: { element: 'aside', marker: 'knowledgeref', required: ['content', 'paragraph'] },
  download: { element: 'a', marker: 'download', required: ['url'] },
  linkcard: { element: 'a', marker: 'linkcard', required: ['url'] },
  note: { element: 'details', marker: 'note', required: [] },
  image: { element: 'figure', marker: 'image', required: [] },
  section: { element: 'section', marker: 'section', required: [] },
  panel: { element: 'div', marker: 'panel', required: [] },
  /** Accepted for compatibility with earlier page-creator output. */
  card: { element: 'panel', marker: 'panel', required: [] },
};

/** Turn `{ key: 'value' }` into `data-veloct-<key>` attributes. */
function directiveAttributes(node, marker) {
  const properties = { 'data-veloct': marker };
  for (const [key, value] of Object.entries(node.attributes ?? {})) {
    if (value === undefined || value === null || value === '') {
      properties[`data-veloct-${key}`] = true;
    } else {
      properties[`data-veloct-${key}`] = String(value);
    }
  }
  return properties;
}

/**
 * The remark plugin.
 *
 * Turns `remark-directive`'s container (`:::name{...}`) and leaf (`::name{...}`) directives into
 * element nodes that a page maps to components through the documented
 * `<Content components={{ ... }} />` prop.
 *
 * It emits real HTML element names plus `data-veloct-*` markers. The block ordinals that let a
 * knowledge reference open the cited location are a separate concern handled by
 * `veloct-block-ordinals.mjs` on the HAST tree, because a remark paragraph's `hProperties` do not
 * survive Astro's markdown rendering.
 *
 * Anything unrecognised is left as its own node rather than dropped: an unknown extension must not
 * silently lose the author's words.
 */
export function veloctDirectives() {
  return (tree) => {
    visit(tree, (node) => {
      if (node.type !== 'containerDirective' && node.type !== 'leafDirective') return;

      const definition = DIRECTIVES[node.name];
      if (!definition) return;

      const data = node.data ?? (node.data = {});
      data.hName = definition.element;
      data.hProperties = directiveAttributes(node, definition.marker);

      // A knowledge reference carries no body; a component renders it.
      if (definition.marker === 'knowledgeref') {
        node.children = [];
      }
    });
  };
}

export default veloctDirectives;
