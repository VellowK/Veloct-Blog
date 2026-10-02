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
 *
 * **What the window DOES still decide is listed here rather than implied** (`PROCESSING.md` §5.10.1 asked for
 * exactly this list): which source is selected and whether a keyword or a topic is what a click sends
 * (`browseIntent`/`browseAction`), whether a search may be sent yet and why not (`searchReadiness`,
 * `topicsReadiness` — the disabled-button-with-a-reason affordance §13.2.2 requires), and the INTERNAL
 * whitespace a pasted key may not contain (`validateKeyInput`, which the crate states no rule about). None of
 * these is a transcription of a crate rule: the crate has no counterpart to transcribe. The paging rule and the
 * blank-key refusal ARE the crate's, and are no longer restated here — `pageSizeFor` clamps with the ceiling
 * the command reported, and a blank key is left to `save_image_source_key`.
 */

import { describe, expect, it } from 'vitest';

import type {
  ImageResultView,
  ImageSearchView,
  NetworkImageChoiceResultView,
  NetworkImageSourceView,
  NetworkImageTopicView,
} from '../src/lib/api';
import {
  attributionNotice,
  attributionToggle,
  browseAction,
  browseIntent,
  choiceNotice,
  hasKey,
  insertReadiness,
  markdownFor,
  moreLabel,
  pageSizeFor,
  refusedSummary,
  resultMeta,
  resultsEmptyText,
  resultsHeading,
  searchNotice,
  searchReadiness,
  sourceOptions,
  suggestedAlt,
  topicLabel,
  topicNotice,
  topicsReadiness,
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

function topic(overrides: Partial<NetworkImageTopicView> = {}): NetworkImageTopicView {
  return {
    id: 'bo8jQKTaE0Y',
    slug: 'wallpapers',
    title: 'Wallpapers',
    visibility: 'featured',
    total_photos: 1344,
    description: 'A curated list of wallpapers.',
    ...overrides,
  };
}

/**
 * The reference implementation's browse decisions, transcribed from
 * `halo-sigs/plugin-unsplash` (`console/src/components/UnsplashSelectorProvider.vue`).
 *
 * **Not a crate rule and not a two-language copy**: this is the window's own decision about WHICH command a
 * click sends, and its authority is a third-party reference implementation rather than `veloct-attach`. It is
 * kept, and listed as one of the remaining honest window rules (`PROCESSING.md` §5.10.1), because deleting it
 * would delete a UI affordance rather than a transcription.
 *
 * Its query function is a three-branch rule, and the precedence in it is the one thing that is easy to
 * get wrong: **a keyword wins over a selected topic**. Every row was read from that source, and the
 * `mode` column is what the window must decide.
 */
const PLUGIN_BROWSE_RULE: ReadonlyArray<{
  keyword: string;
  topic: string | null;
  mode: 'search' | 'topic' | 'idle';
  why: string;
}> = [
  {
    keyword: 'coffee',
    topic: null,
    mode: 'search',
    why: 'search.getPhotos when there is a keyword',
  },
  {
    keyword: 'coffee',
    topic: 'bo8jQKTaE0Y',
    mode: 'search',
    why: 'a topic is a starting point, NOT a filter: the keyword still wins',
  },
  {
    keyword: '   ',
    topic: 'bo8jQKTaE0Y',
    mode: 'topic',
    why: 'whitespace is not a keyword, so topics.getPhotos runs',
  },
  {
    keyword: '',
    topic: 'bo8jQKTaE0Y',
    mode: 'topic',
    why: 'topics.getPhotos with topicIdOrSlug',
  },
  {
    keyword: '',
    topic: null,
    mode: 'idle',
    why: 'the plugin returns [] with neither, and sends nothing',
  },
];

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

  it('leaves the blank-key refusal to the command, which owns that rule AND its message', () => {
    // `save_image_source_key` refuses an empty key in its own words and points at the delete command; a second
    // copy of that sentence in the window is exactly the duplication PROCESSING.md §5.10.1 removed. The panel
    // sends the key and shows the command's refusal, so a blank one is not a window rule at all.
    expect(validateKeyInput('')).toBeNull();
    expect(validateKeyInput('    ')).toBeNull();
    expect(validateKeyInput('\n\t')).toBeNull();
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

describe('the one browse button, which names the mode it will send', () => {
  const perPage = 30;

  it('is a keyword search while the keyword box has something in it', () => {
    const action = browseAction({
      source: UNSPLASH,
      query: 'coffee',
      topic: 'bo8jQKTaE0Y',
      perPage,
      busy: false,
    });
    expect(action.ready).toBe(true);
    expect(action.label).toBe('搜索');
    // The keyword wins even with a topic selected, so the button must not offer to browse the topic.
    expect(action.label).not.toContain('主题');
  });

  it('offers the topic browse only when the box is empty and a topic is selected', () => {
    const action = browseAction({
      source: UNSPLASH,
      query: '   ',
      topic: 'bo8jQKTaE0Y',
      perPage,
      busy: false,
    });
    expect(action.ready).toBe(true);
    expect(action.label).toBe('浏览主题照片');
  });

  it('refuses with a sentence rather than a silent disabled button', () => {
    // Neither a keyword nor a topic: the reason names BOTH ways to proceed.
    const idle = browseAction({ source: UNSPLASH, query: '', topic: null, perPage, busy: false });
    expect(idle.ready).toBe(false);
    expect(idle.reason).toContain('关键词');
    expect(idle.reason).toContain('主题');

    // A source with no key is asked for one BEFORE the keyword is requested, which is the order the
    // search path already uses.
    const noKey = browseAction({
      source: source({ has_key: false }),
      query: '',
      topic: 'bo8jQKTaE0Y',
      perPage,
      busy: false,
    });
    expect(noKey.ready).toBe(false);
    expect(noKey.reason).toContain('操作系统凭据库');

    // A refused source keeps the provider's own reason, not "enter a keyword".
    const refused = browseAction({ source: PIXABAY, query: 'coffee', topic: null, perPage, busy: false });
    expect(refused.ready).toBe(false);
    expect(refused.reason).toContain('热链');

    expect(
      browseAction({ source: UNSPLASH, query: 'coffee', topic: null, perPage, busy: true }).reason,
    ).toContain('正在搜索');
  });

  it('agrees word for word with searchReadiness wherever a search is what would be sent', () => {
    // The two functions share the text of the four preconditions by transcription, not by a call, and a
    // ONE-SIDED edit to either is what this compares. Whenever a keyword is present the browse button
    // and the search button are the same button, so their verdicts must be identical — including the
    // sentence shown while it is disabled.
    const cases: Array<{
      label: string;
      source: NetworkImageSourceView | undefined;
      query: string;
      busy: boolean;
    }> = [
      { label: 'no source', source: undefined, query: 'coffee', busy: false },
      { label: 'a refused source', source: PIXABAY, query: 'coffee', busy: false },
      { label: 'a source with no key', source: source({ has_key: false }), query: 'coffee', busy: false },
      { label: 'a running search', source: UNSPLASH, query: 'coffee', busy: true },
      { label: 'a ready source', source: UNSPLASH, query: 'coffee', busy: false },
    ];

    for (const one of cases) {
      const search = searchReadiness({ source: one.source, query: one.query, busy: one.busy });
      const browse = browseAction({
        source: one.source,
        query: one.query,
        topic: null,
        perPage,
        busy: one.busy,
      });
      expect(browse.ready, one.label).toBe(search.ready);
      expect(browse.reason, one.label).toBe(search.reason);
      expect(browse.label, one.label).toBe('搜索');
    }
  });

  it('asks for a KEY and not for a keyword when a topic browse is what an empty box would send', () => {
    // The one place the two orders differ, and deliberately: with a topic selected and an empty box the
    // next click browses the topic, so the missing key is what blocks it — not a keyword the browse
    // would never use.
    const withTopic = browseAction({
      source: source({ has_key: false }),
      query: '',
      topic: 'bo8jQKTaE0Y',
      perPage,
      busy: false,
    });
    expect(withTopic.reason).toContain('操作系统凭据库');
    // And with no topic either, the keyword is asked for, because nothing at all can be sent.
    const withNeither = browseAction({
      source: source({ has_key: true }),
      query: '',
      topic: null,
      perPage,
      busy: false,
    });
    expect(withNeither.reason).toContain('关键词');
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

describe('topic browsing, which is added BESIDE the keyword search', () => {
  it('transcribes the reference implementation\'s own precedence for every combination', () => {
    // The table's `mode` column was read from the plugin's source, not reasoned about here. A shape the
    // plugin sends to search.getPhotos must not be sent to topics.getPhotos, or the browse mode would
    // silently search for a topic id that is not a keyword.
    for (const rule of PLUGIN_BROWSE_RULE) {
      const intent = browseIntent({
        topic: rule.topic,
        query: rule.keyword,
        page: 1,
        perPage: 30,
      });
      expect(intent.mode, `${rule.keyword}/${rule.topic}: ${rule.why}`).toBe(rule.mode);
    }
  });

  it('searches by KEYWORD while a topic is selected, which is the rule that is easy to miss', () => {
    const intent = browseIntent({
      topic: 'bo8jQKTaE0Y',
      query: 'coffee',
      page: 1,
      perPage: 30,
    });
    expect(intent.mode).toBe('search');
    if (intent.mode !== 'search') throw new Error('unreachable');
    expect(intent.query).toBe('coffee');
    // The topic id is NOT smuggled into the query: the plugin passes it to a different endpoint, and a
    // window that concatenated the two would send a search Unsplash never receives.
    expect(intent.query).not.toContain('bo8jQKTaE0Y');
  });

  it('treats a whitespace keyword as no keyword, so the topic browse still runs', () => {
    const intent = browseIntent({ topic: 'bo8jQKTaE0Y', query: '   ', page: 2, perPage: 30 });
    expect(intent.mode).toBe('topic');
    if (intent.mode !== 'topic') throw new Error('unreachable');
    expect(intent.topic).toBe('bo8jQKTaE0Y');
    expect(intent.page).toBe(2);
  });

  it('sends nothing at all with neither a keyword nor a topic, and says which is missing', () => {
    const intent = browseIntent({ topic: null, query: '', page: 1, perPage: 30 });
    expect(intent.mode).toBe('idle');
    if (intent.mode !== 'idle') throw new Error('unreachable');
    expect(intent.reason).toContain('关键词');
    expect(intent.reason).toContain('主题');

    // An empty-string topic is the same as no topic, which is what the strip's "cancel" leaves behind.
    expect(browseIntent({ topic: '', query: '', page: 1, perPage: 30 }).mode).toBe('idle');
    expect(browseIntent({ topic: '   ', query: '', page: 1, perPage: 30 }).mode).toBe('idle');
  });

  it('asks for a page size the provider cannot refuse, from the ceiling the COMMAND reported', () => {
    // The window holds no copy of `providers::validate_paging` (PROCESSING.md §5.10.1). A transcription table
    // used to sit here, re-implementing the crate's rule in TypeScript and comparing it against itself; the
    // rule is asserted in Rust (`veloct-attach`'s network tests, and `veloct-desktop`'s
    // `a_topic_page_size_above_the_providers_ceiling_is_refused_rather_than_trimmed`), and what the window
    // does is CLAMP using `default_per_page` / `max_per_page`, which `network_image_sources` fills from the
    // crate's own facts. That is why the reference implementation's 48 must not become a refused request.
    for (const source of [UNSPLASH, PEXELS]) {
      for (const requested of [0, 1, 7, source.default_per_page, source.max_per_page, 48, 500, Number.NaN]) {
        const clamped = pageSizeFor(source, requested);
        expect(clamped, `${source.source} asked for ${requested}`).toBeGreaterThanOrEqual(1);
        expect(clamped, `${source.source} asked for ${requested}`).toBeLessThanOrEqual(source.max_per_page);
      }
    }

    expect(pageSizeFor(UNSPLASH, 48)).toBe(UNSPLASH.max_per_page);
    // A source with no paging at all asks for nothing rather than guessing a number the crate would refuse.
    expect(pageSizeFor(PIXABAY, 20)).toBe(0);
  });

  it('names a topic chip with the provider\'s own photo count', () => {
    expect(topicLabel(topic())).toBe('Wallpapers(1344)');
    // `total_photos` absent is NOT "0 photos": printing 0 would be a claim the provider did not make.
    expect(topicLabel(topic({ total_photos: 0 }))).toBe('Wallpapers');
    // A missing title degrades to the slug rather than to an empty chip.
    expect(topicLabel(topic({ title: '  ' }))).toBe('wallpapers(1344)');
  });

  it('makes the topic strip its own readiness sentence rather than a silent disabled control', () => {
    expect(topicsReadiness({ source: UNSPLASH, busy: false }).ready).toBe(true);

    // Topics are Unsplash's alone. A Pexels user is told which provider has them.
    const pexels = topicsReadiness({ source: PEXELS, busy: false });
    expect(pexels.ready).toBe(false);
    expect(pexels.reason).toContain('Unsplash');
    expect(pexels.reason).toContain('Pexels');

    // A refused source keeps ITS reason, not "no topics".
    expect(topicsReadiness({ source: PIXABAY, busy: false }).reason).toContain('热链');

    const noKey = topicsReadiness({ source: source({ has_key: false }), busy: false });
    expect(noKey.ready).toBe(false);
    expect(noKey.needsKey).toBe(true);
    expect(noKey.reason).toContain('操作系统凭据库');

    expect(topicsReadiness({ source: UNSPLASH, busy: true }).reason).toContain('主题');
    expect(topicsReadiness({ source: undefined, busy: false }).reason).toContain('来源');
  });

  it('names the topic in the results header, because a browse has no keyword to show', () => {
    const view = search({ source: 'unsplash', label: 'Unsplash', query: 'bo8jQKTaE0Y' });
    const header = topicNotice(view, 'Wallpapers');
    expect(header).toContain('主题');
    expect(header).toContain('Wallpapers');
    expect(header).toContain('共 1 条结果');
    expect(header).not.toContain('隐藏');

    // With no title to show, the provider's own identifier is what is named rather than an empty pair
    // of brackets.
    expect(topicNotice(view, '   ')).toContain('bo8jQKTaE0Y');
    // The drop count is explained by the same rule the keyword header uses.
    expect(topicNotice(search({ dropped: 2 }), 'Wallpapers')).toContain('2 条结果因为缺少可用地址被隐藏');
  });

  it('picks the header and the empty-state sentence from ONE mode decision', () => {
    const view = search({ source: 'unsplash', label: 'Unsplash', query: 'wallpapers' });

    // A topic selected with an empty box: the topic is named, and an empty result blames the topic.
    const browsing = { topic: 'bo8jQKTaE0Y', topicTitle: 'Wallpapers', query: '  ' };
    expect(resultsHeading(view, browsing)).toContain('主题「Wallpapers」');
    expect(resultsEmptyText(browsing)).toContain('换一个主题');
    expect(resultsEmptyText(browsing)).not.toContain('关键词');

    // A keyword typed while that topic is still selected: the KEYWORD is what the results show, so the
    // header must not be labelled with the topic and the empty sentence must not blame it.
    const searching = { topic: 'bo8jQKTaE0Y', topicTitle: 'Wallpapers', query: 'coffee' };
    expect(resultsHeading(view, searching)).toBe(searchNotice(view));
    expect(resultsHeading(view, searching)).not.toContain('主题');
    expect(resultsEmptyText(searching)).toContain('关键词');

    // No topic at all is the ordinary search header.
    expect(resultsHeading(view, { topic: null, topicTitle: '', query: '' })).toBe(searchNotice(view));
    expect(resultsEmptyText({ topic: null, query: '' })).toContain('关键词');
  });

  it('keeps the SAME view type for a topic browse, so the grid is not a second implementation', () => {
    // A design assertion, in the shape the round-trip test above uses: a topic browse returns
    // `ImageSearchView`, so `searchNotice`/`moreLabel`/`resultMeta` all apply to it unchanged.
    const fromTopic = search({
      source: 'unsplash',
      label: 'Unsplash',
      query: 'wallpapers',
      attribution_rule: UNSPLASH.attribution_rule,
      attribution_optional: false,
    });
    expect(moreLabel(fromTopic, 1)).toBe('加载第 2 页');
    expect(resultMeta(fromTopic.results[0]!)).toContain('Lukas Rodriguez');
    // And the credit rule is the SOURCE's, not the browse mode's: a topic pick from Unsplash is still
    // a pick that cannot drop the credit line.
    expect(fromTopic.attribution_optional).toBe(false);
  });
});
