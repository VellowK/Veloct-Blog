// Knowledge reference cards, resolved in the browser from a build-time payload.
//
// WHY THIS IS CLIENT-SIDE, and why that is not a shortcut:
//
// `handoff.md` section 3.3 requires the knowledge reference card to be a structured construct, and
// the remark plugin in `src/remark/veloct-directives.mjs` does emit a real element for it carrying
// its target (`data-veloct-content`, `data-veloct-paragraph`). Turning that element into a Veloct
// component would use Astro's `components` prop on `<Content />`, but testing against Astro 7.3.5
// shows that prop does NOT reach plain Markdown: neither an `h2` override nor a custom element is
// substituted, and a named slot is not filled either. That finding is recorded in ADR-0011.
//
// So the page renders the RESOLVED reference data as a JSON payload alongside the content (still at
// build time, against the current knowledge text, which is what section 9.4 requires), and this
// script builds the card for each directive element from it.
//
// Degradation is deliberate and visible: without scripting the element keeps its attributes and the
// payload is in the document, so the citation is never silently lost. A reference that did not
// resolve is shown as broken rather than hidden, because hiding it would make an unsourced claim
// look sourced — the failure `handoff.md` section 9.3 exists to prevent.

/** Build the card DOM for one reference element. */
function buildCard(element, references) {
  const contentId = element.dataset.veloctContent ?? '';
  const paragraphId = element.dataset.veloctParagraph ?? '';
  const resolved = references[`${contentId}\u001f${paragraphId}`];

  if (!resolved || resolved.status !== 'resolved') {
    const broken = document.createElement('div');
    broken.className = 'card border border-danger-400/50 p-4';
    broken.dataset.veloctKnowledgerefBroken = 'true';
    broken.innerHTML =
      `<p class="mb-1 text-xs uppercase tracking-wide text-danger-400">引用无法解析</p>` +
      `<p class="text-sm text-ink-300">${escapeHtml(
        resolved?.message ?? `找不到该知识库段落：content=${contentId || '?'} paragraph=${paragraphId || '?'}`,
      )}</p>` +
      `<p class="mt-2 text-xs muted">引用保持损坏状态，请在编辑器中使用引用修复工具指向正确的段落。</p>`;
    element.replaceChildren(broken);
    return;
  }

  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'card block w-full p-4 text-start hover:border-accent-600';
  button.dataset.veloctKnowledgerefOpen = 'true';
  button.dataset.contentId = contentId;
  // The paragraph ORDINAL, not the id: the knowledge page renders blocks by ordinal, and matching on
  // the paragraph's text would move the highlight whenever the author reworded it.
  button.dataset.paragraphId = String(resolved.paragraphOrder ?? '');
  button.dataset.knowledgeSlug = resolved.slug ?? '';
  button.dataset.knowledgeTitle = resolved.title ?? '';
  button.setAttribute('aria-label', `在侧边栏打开知识库引用：${resolved.title}`);
  button.innerHTML =
    `<span class="mb-1 flex items-center gap-2 text-xs uppercase tracking-wide text-accent-400">知识库引用</span>` +
    `<span class="block font-serif text-base text-ink-100">${escapeHtml(resolved.title)}</span>` +
    `<span class="mt-1 block text-sm text-ink-300">${escapeHtml(resolved.excerpt ?? '')}</span>`;

  element.replaceChildren(button);
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function resolveAll() {
  const payload = document.getElementById('veloct-knowledge-refs');
  let references = {};
  if (payload) {
    try {
      references = JSON.parse(payload.textContent ?? '{}');
    } catch (error) {
      console.error('[veloct] could not read the resolved reference payload', error);
    }
  }

  for (const element of document.querySelectorAll('[data-veloct="knowledgeref"]')) {
    if (element.dataset.veloctResolved === 'true') continue;
    element.dataset.veloctResolved = 'true';
    buildCard(element, references);
  }
}

document.addEventListener('astro:page-load', resolveAll);

// A click on a card asks the right sidebar to open the knowledge view. The sidebar listens for this
// event, so the card does not need to know the sidebar exists.
document.addEventListener('click', (event) => {
  const trigger = event.target?.closest?.('[data-veloct-knowledgeref-open]');
  if (!trigger) return;

  document.dispatchEvent(
    new CustomEvent('veloct:open-knowledge', {
      detail: {
        contentId: trigger.dataset.contentId ?? '',
        paragraphId: trigger.dataset.paragraphId ?? '',
        title: trigger.dataset.knowledgeTitle ?? '',
        slug: trigger.dataset.knowledgeSlug ?? '',
      },
    }),
  );
});
