/**
 * The network image browser's decisions (`handoff.md` §13.14).
 *
 * The panel's rules live in `src/lib/networkImageForm.ts` and are asserted here, because this repository
 * has no React test renderer — deliberately — so a rule that lives in JSX is a rule nothing checks.
 *
 * Two things these tests are really about, beyond the individual functions:
 *
 * 1. **A source named by the contract that this build does not offer is a SENTENCE, not a silence.** The
 *    refusal reason comes from the crate and is surfaced in the window.
 * 2. **The window never decides whether a credit line is optional.** It reads `attribution_optional`,
 *    which the crate derives from the provider's own terms. A second implementation of "does Unsplash
 *    require credit" would be a second chance to publish a page that violates them.
 */

import { describe, expect, it } from 'vitest';

import type {
  ImageResultView,
  ImageSearchView,
  NetworkImageChoiceResultView,
  NetworkImageSourceView,
} from '../src/lib/api';
import {
  attributionNotice,
  attributionToggle,
  choiceNotice,
  hasKey,
  insertReadiness,
  markdownFor,
  moreLabel,
  pageSizeFor,
  refusedSummary,
  resultMeta,
  searchNotice,
  searchReadiness,
  sourceOptions,
  suggestedAlt,
  validateKeyInput,
} from '../src/lib/networkImageForm';

function source(overrides: Partial<NetworkImageSourceView> = {}): NetworkImageSourceView {
  return {
    source: 'unsplash',
    label: 'Unsplash',
    implemented: true,
    unimplemented_reason: null,
    needs_key: true,
    has_key: true,
    attribution_rule: '必须注明 Unsplash 与摄影师。',
    notice: 'Unsplash:必须注明 Unsplash 与摄影师。图片为免费素材。',
    hotlink: 'required',
    attribution_optional: false,
    default_per_page: 10,
    max_per_page: 30,
    ...overrides,
  };
}

const UNSPLASH = source();
const PEXELS = source({
  source: 'pexels',
  label: 'Pexels',
  attribution_rule: '必须显著标明来源 Pexels。',
  hotlink: 'permitted',
  attribution_optional: true,
  default_per_page: 15,
  max_per_page: 80,
});
const PIXABAY = source({
  source: 'pixabay',
  label: 'Pixabay',
  implemented: false,
  unimplemented_reason: 'Pixabay 不接入:它的条款禁止永久热链。',
  hotlink: 'forbidden',
  attribution_optional: false,
  has_key: false,
  default_per_page: 0,
  max_per_page: 0,
});

function result(overrides: Partial<ImageResultView> = {}): ImageResultView {
  return {
    source: 'pexels',
    source_id: '3573351',
    url: 'https://images.pexels.com/photos/1/x.png?h=650&w=940',
    thumbnail_url: 'https://images.pexels.com/photos/1/x.png?h=130',
    width: 3066,
    height: 3968,
    author: 'Lukas Rodriguez',
    licence: null,
    source_page: 'https://www.pexels.com/photo/trees-3573351/',
    author_url: 'https://www.pexels.com/@lukas-rodriguez-1845331',
    alt: 'Trees during day',
    download_location: null,
    ...overrides,
  };
}

function search(overrides: Partial<ImageSearchView> = {}): ImageSearchView {
  return {
    source: 'pexels',
    label: 'Pexels',
    query: 'trees',
    results: [result()],
    has_more: true,
    dropped: 0,
    attribution_rule: '必须显著标明来源 Pexels。',
    attribution_optional: true,
    ...overrides,
  };
}

function chosen(overrides: Partial<NetworkImageChoiceResultView> = {}): NetworkImageChoiceResultView {
  return {
    choice: {
      source: 'pexels',
      source_id: '3573351',
      url: 'https://images.pexels.com/photos/1/x.png?h=650&w=940',
      alt: 'Trees during day',
      attribution: 'Photo by [Lukas Rodriguez](https://www.pexels.com/@x) on [Pexels](https://www.pexels.com/photo/1/)',
    },
    markdown: '![Trees during day](https://images.pexels.com/photos/1/x.png)\n\n*credit*',
    attribution: 'Photo by ... on [Pexels](...)',
    download_reported: null,
    download_note: null,
    ...overrides,
  };
}

describe('which sources are offered', () => {
  it('offers the implemented sources and keeps the refused ones with their reason', () => {
    const options = sourceOptions([UNSPLASH, PEXELS, PIXABAY]);
    expect(options.offered.map((entry) => entry.source)).toEqual(['unsplash', 'pexels']);
    expect(options.refused.map((entry) => entry.source)).toEqual(['pixabay']);
    expect(options.refused[0]?.unimplemented_reason).toContain('热链');
  });

  it('never offers a source that is implemented but carries a reason', () => {
    // The crate derives one from the other, so this state should be impossible — which is exactly why
    // the window must not treat it as offered if it ever appears.
    const contradictory = source({
      implemented: true,
      unimplemented_reason: 'a reason that should not be here',
    });
    const options = sourceOptions([contradictory]);
    expect(options.offered).toHaveLength(0);
    expect(options.refused).toHaveLength(1);
  });

  it('explains every refused source in one sentence, or says nothing when none is refused', () => {
    expect(refusedSummary([])).toBeNull();
    expect(refusedSummary(sourceOptions([UNSPLASH]).refused)).toBeNull();

    const summary = refusedSummary(sourceOptions([UNSPLASH, PIXABAY]).refused);
    expect(summary).toContain('Pixabay');
    expect(summary).toContain('热链');
    expect(summary).toContain('未接入');
  });
});

describe('hasKey', () => {
  it('reads the boolean and treats a missing source as having none', () => {
    expect(hasKey(UNSPLASH)).toBe(true);
    expect(hasKey(source({ has_key: false }))).toBe(false);
    expect(hasKey(undefined)).toBe(false);
  });
});

describe('what a search still needs', () => {
  const query = 'trees';

  it('is ready for a source with a key and a keyword', () => {
    const readiness = searchReadiness({ source: PEXELS, query, busy: false });
    expect(readiness.ready).toBe(true);
    expect(readiness.reason).toBeNull();
  });

  it('asks for a key BEFORE a search, and says where it will be stored', () => {
    const readiness = searchReadiness({ source: source({ has_key: false }), query, busy: false });
    expect(readiness.ready).toBe(false);
    expect(readiness.needsKey).toBe(true);
    expect(readiness.reason).toContain('操作系统凭据库');
    expect(readiness.reason).toContain('§12.2');
  });

  it('refuses a refused source with the provider\'s own reason rather than a generic message', () => {
    const readiness = searchReadiness({ source: PIXABAY, query, busy: false });
    expect(readiness.ready).toBe(false);
    expect(readiness.reason).toContain('热链');
    expect(readiness.needsKey).toBe(false);
  });

  it('asks for a keyword, and for a source', () => {
    expect(searchReadiness({ source: PEXELS, query: '   ', busy: false }).reason).toContain('关键词');
    expect(searchReadiness({ source: undefined, query, busy: false }).reason).toContain('来源');
  });

  it('is not ready while a search is running', () => {
    const readiness = searchReadiness({ source: PEXELS, query, busy: true });
    expect(readiness.ready).toBe(false);
    expect(readiness.reason).toContain('正在搜索');
  });
});

describe('validateKeyInput', () => {
  it('accepts an ordinary opaque key', () => {
    expect(validateKeyInput('a-real-looking-key-0123456789')).toBeNull();
  });

  it('refuses an empty or whitespace-only key and points at the delete command', () => {
    expect(validateKeyInput('')).toContain('删除密钥');
    expect(validateKeyInput('    ')).toContain('删除密钥');
  });

  it('refuses a key with INTERNAL whitespace, which a sloppy copy produces', () => {
    expect(validateKeyInput('abc def')).not.toBeNull();
    expect(validateKeyInput('abc\ndef')).not.toBeNull();
    expect(validateKeyInput('a b c')).not.toBeNull();
  });

  it('tolerates whitespace AROUND a key, because that is the common paste mistake', () => {
    // Trimming is the whole point: a trailing newline from a copy is not a reason to refuse a key the
    // user has correctly copied.
    expect(validateKeyInput('  abcdef0123456789  ')).toBeNull();
    expect(validateKeyInput('\tabcdef0123456789\n')).toBeNull();
  });
});

describe('pageSizeFor', () => {
  it('uses the source\'s own default when nothing was asked for', () => {
    expect(pageSizeFor(UNSPLASH, 0)).toBe(10);
    expect(pageSizeFor(PEXELS, 0)).toBe(15);
    expect(pageSizeFor(UNSPLASH, Number.NaN)).toBe(10);
  });

  it('clamps to the ceiling the crate reports, so the window cannot ask for a refused page size', () => {
    expect(pageSizeFor(UNSPLASH, 500)).toBe(30);
    expect(pageSizeFor(PEXELS, 500)).toBe(80);
    expect(pageSizeFor(PEXELS, 40)).toBe(40);
    expect(pageSizeFor(PEXELS, 40.7)).toBe(40);
  });

  it('reports nothing for a source with no page size, rather than guessing one', () => {
    expect(pageSizeFor(PIXABAY, 20)).toBe(0);
    expect(pageSizeFor(undefined, 20)).toBe(0);
  });
});

describe('what a result shows', () => {
  it('names the author, the size and the licence', () => {
    expect(resultMeta(result())).toBe('Lukas Rodriguez · 3066×3968');
    expect(resultMeta(result({ licence: 'Unsplash License' }))).toContain('Unsplash License');
  });

  it('omits a missing dimension rather than printing 0×0', () => {
    expect(resultMeta(result({ width: 0, height: 0 }))).toBe('Lukas Rodriguez');
  });

  it('suggests the provider\'s own alt text, which is better than a generated line', () => {
    expect(suggestedAlt(result())).toBe('Trees during day');
  });

  it('falls back to naming the author when the provider described nothing', () => {
    expect(suggestedAlt(result({ alt: null }))).toBe('Image by Lukas Rodriguez');
    expect(suggestedAlt(result({ alt: '  ' }))).toBe('Image by Lukas Rodriguez');
  });

  it('suggests nothing when there is neither a description nor an author', () => {
    expect(suggestedAlt(result({ alt: null, author: '' }))).toBe('');
  });
});

describe('what the provider requires of the user', () => {
  it('says the credit is mandatory where the terms make it so', () => {
    const notice = attributionNotice({
      label: 'Unsplash',
      attribution_rule: UNSPLASH.attribution_rule,
      attribution_optional: false,
    });
    expect(notice.required).toBe(true);
    expect(notice.text).toContain('无法取消');
  });

  it('says the credit is the user\'s choice where the terms allow it', () => {
    const notice = attributionNotice({
      label: 'Pexels',
      attribution_rule: PEXELS.attribution_rule,
      attribution_optional: true,
    });
    expect(notice.required).toBe(false);
    expect(notice.text).toContain('可以选择');
  });

  it('does not render a switch that would do nothing', () => {
    // Unsplash forces the credit in the crate, so a switch in the window would be a lie.
    expect(attributionToggle({ attribution_optional: false }).shown).toBe(false);
    expect(attributionToggle({ attribution_optional: false }).label).toContain('必须保留');
    expect(attributionToggle({ attribution_optional: true }).shown).toBe(true);
    expect(attributionToggle({ attribution_optional: true }).label).toContain('写入文章');
  });
});

describe('insertion', () => {
  it('accepts the Markdown the command produced', () => {
    expect(insertReadiness({ markdown: chosen().markdown, busy: false }).ready).toBe(true);
  });

  it('refuses an empty result and something that is not image syntax', () => {
    expect(insertReadiness({ markdown: '', busy: false }).reason).toContain('还没有');
    expect(insertReadiness({ markdown: 'just some prose', busy: false }).reason).toContain('图片语法');
  });

  it('is not ready while a choice is being made', () => {
    expect(insertReadiness({ markdown: chosen().markdown, busy: true }).ready).toBe(false);
  });

  it('inserts exactly what the command returned', () => {
    expect(markdownFor(chosen())).toBe(chosen().markdown);
  });
});

describe('what to say after a choice, including the compliance step', () => {
  it('reports success where the provider required a download report', () => {
    const notice = choiceNotice(chosen({ download_reported: true }));
    expect(notice.level).toBe('ok');
    expect(notice.text).toContain('下载');
  });

  it('states a FAILED report rather than hiding it, and does not claim the image is broken', () => {
    // §20: an explicit, explained failure. Unsplash's guidelines require the report, so a silent one
    // would be a compliance gap; the image itself is still usable, so it is not a blocking error.
    const notice = choiceNotice(
      chosen({ download_reported: false, download_note: 'Unsplash 返回 HTTP 500: upstream boom' }),
    );
    expect(notice.level).toBe('warning');
    expect(notice.text).toContain('下载上报没有成功');
    expect(notice.text).toContain('HTTP 500');
    expect(notice.text).toContain('可以使用');
  });

  it('explains a failure with no note attached', () => {
    const notice = choiceNotice(chosen({ download_reported: false, download_note: null }));
    expect(notice.level).toBe('warning');
    expect(notice.text).toContain('下载上报没有成功');
  });

  it('says nothing about a download for a source that requires none', () => {
    // Pexels reports `null`, so a sentence about a download would be invented.
    const notice = choiceNotice(chosen({ download_reported: null }));
    expect(notice.level).toBe('ok');
    expect(notice.text).not.toContain('下载');
  });
});

describe('the results header and paging', () => {
  it('counts the results and explains a short list', () => {
    expect(searchNotice(search())).toContain('共 1 条结果');
    expect(searchNotice(search())).not.toContain('隐藏');
    expect(searchNotice(search({ dropped: 3 }))).toContain('3 条结果因为缺少可用地址被隐藏');
  });

  it('offers the next page only when there is one, and names it', () => {
    expect(moreLabel(search(), 1)).toBe('加载第 2 页');
    expect(moreLabel(search({ has_more: false }), 1)).toBeNull();
  });
});

describe('the shape the panel sends back to the command', () => {
  it('carries everything the credit line and the compliance step need', () => {
    // A design assertion: the window sends the whole result back rather than an id, because a provider
    // id alone cannot reconstruct the image URL, the author's profile link or the download endpoint.
    const one = result({ source: 'unsplash', download_location: 'https://api.unsplash.com/photos/1/download' });
    for (const field of [
      'url',
      'author',
      'author_url',
      'source_page',
      'download_location',
      'source_id',
    ] as const) {
      expect(one[field], `${field} is what the round trip needs`).toBeTruthy();
    }
  });
});
