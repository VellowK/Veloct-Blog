// A rehype plugin that numbers the document's top-level paragraphs.
//
// Why this is separate from `veloct-directives.mjs`: Veloct needs each rendered prose block to say
// which identified paragraph it came from, so an incoming knowledge reference can open the right
// location. The remark-side approach (`data.hProperties` on a paragraph) works in a plain unified
// pipeline but is lost inside Astro's markdown rendering, so it is done here on the HAST tree
// instead, where it survives.
//
// The numbering is positional and matches the knowledge index exactly: both count
// blank-line-separated blocks in source order, so block N is the Nth identified paragraph. It is
// deliberately NOT text matching, which would move the highlight the moment an author reworded a
// paragraph — the "guess a different paragraph" behaviour `handoff.md` section 9.3 forbids.

import { visit } from 'unist-util-visit';

/** Tags that count as one citable prose block. */
const BLOCK_TAGS = new Set(['p']);

export function veloctBlockOrdinals() {
  return (tree) => {
    let ordinal = 0;

    visit(tree, 'element', (node, index, parent) => {
      if (!BLOCK_TAGS.has(node.tagName)) return;

      // Only top-level prose counts. A paragraph nested in a blockquote or a list item is inside
      // another block, and counting it would shift every following ordinal.
      if (parent && parent.type === 'element' && parent.tagName !== 'div') return;
      if (parent && parent.type !== 'root' && parent.type !== 'element') return;

      ordinal += 1;
      const properties = node.properties ?? (node.properties = {});
      properties['data-veloct-block'] = String(ordinal);
    });
  };
}

export default veloctBlockOrdinals;
