/**
 * The works panel's decisions (`PROCESSING.md` §5.9 / H10).
 *
 * `handoff.md` §10 is a locked section, and this repository has no React test renderer — deliberately.
 * So the panel's rules live in `src/lib/workForm.ts` and are asserted here: which fields a work's
 * metadata edit may change, why a draft cannot be saved, why a work will not be served, and whether an
 * import or a poster may be sent at all.
 *
 * The `CRATE_RULE` transcription is the same device `storageTargetForm.test.ts` uses for §13: the
 * crate is the authority, the window has its own copy so the user is told in the field they typed, and
 * the pair is held together by shapes whose verdicts were copied from the crate. A one-sided change is
 * a failing test rather than an oversight.
 */

import { describe, expect, it } from 'vitest';

import type { WorkImportPlanView, WorkView } from '../src/lib/api';
import {
  CRATE_RULE,
  DEFAULT_SCREENSHOT_LIMIT,
  draftFromWork,
  draftIsDirty,
  formatBytes,
  formatTags,
  importReadiness,
  parseTags,
  planKey,
  posterReadiness,
  servableReason,
  skippedNote,
  slugify,
  SUPPORTED_IMAGE_FORMATS,
  validateScreenshotChoice,
  validateWorkDraft,
  type WorkDraft,
} from '../src/lib/workForm';

/** One work view, with only the interesting fields overridden. */
function work(overrides: Partial<WorkView['metadata']> = {}): WorkView {
  return {
    metadata: {
      id: '01J8Z5Q0K3W7M9V2B4N6P8R2AG',
      slug: 'demo-counter',
      title: '计数器 Demo',
      author: 'Vellow',
      summary: '一个最小的静态作品。',
      completed_at: '2026-02-14T00:00:00Z',
      imported_at: '2026-02-14T00:00:00Z',
      source: { kind: 'folder', original_name: 'demo-counter' },
      entry: 'index.html',
      tags: ['demo', 'ui'],
      cover: 'preview/cover.webp',
      screenshot: 'preview/screenshot.webp',
      size_bytes: 512,
      notes: '用于验证作品系统。',
      ...overrides,
    },
    has_body: true,
    file_count: 3,
    is_servable: true,
    has_screenshot: true,
    screenshot_url: '/work-previews/demo-counter/screenshot.webp',
    screenshot_drift: false,
  };
}

function draft(overrides: Partial<WorkDraft> = {}): WorkDraft {
  return { ...draftFromWork(work()), ...overrides };
}

describe('the editable draft', () => {
  it('reads every editable field out of the work and leaves the identity alone', () => {
    const source = work();
    const result = draftFromWork(source);

    expect(result.id).toBe(source.metadata.id);
    expect(result.title).toBe('计数器 Demo');
    expect(result.tags).toBe('demo\nui');
    expect(result.completedAt).toBe('2026-02-14T00:00:00Z');
    expect(result.cover).toBe('preview/cover.webp');
  });

  it('treats a null completion time and a null cover as blank rather than as the text "null"', () => {
    const result = draftFromWork(work({ completed_at: null, cover: null }));
    expect(result.completedAt).toBe('');
    expect(result.cover).toBe('');
  });

  it('is not dirty when nothing changed, and dirty for every field that can change', () => {
    const source = work();
    expect(draftIsDirty(draftFromWork(source), source)).toBe(false);

    const editable: Array<Partial<WorkDraft>> = [
      { title: '别的名字' },
      { author: 'someone' },
      { slug: 'other-slug' },
      { summary: '改过的简介' },
      { entry: 'main.html' },
      { tags: 'demo' },
      { notes: '改过的备注' },
      { completedAt: '2026-03-01T00:00:00Z' },
      { cover: '' },
    ];
    for (const change of editable) {
      expect(draftIsDirty({ ...draftFromWork(source), ...change }, source)).toBe(true);
    }
  });

  it('does not treat the id as an editable difference, because the form never shows it', () => {
    const source = work();
    const changed = { ...draftFromWork(source), id: 'nonsense' };
    // The id is carried for the save call, not edited, and it is not part of what "changed" means:
    // a form that could make the draft dirty by changing something it does not render would offer
    // Save for a change nobody made.
    expect(draftIsDirty(changed, source)).toBe(false);
  });
});

describe('tags', () => {
  it('splits on newlines and commas, trims, drops the leading hash and de-duplicates', () => {
    expect(parseTags('demo\n#ui, demo , ,  rust ')).toEqual(['demo', 'ui', 'rust']);
  });

  it('round-trips through the textarea form', () => {
    expect(parseTags(formatTags(['a', 'b']))).toEqual(['a', 'b']);
  });

  it('produces no tags for empty or whitespace-only input', () => {
    expect(parseTags('')).toEqual([]);
    expect(parseTags('   \n , \n')).toEqual([]);
  });
});

describe('validateWorkDraft, against the shapes the crate refuses', () => {
  it('accepts a plain draft', () => {
    expect(validateWorkDraft(draft())).toBeNull();
  });

  it('refuses each transcribed shape the crate refuses', () => {
    const shapes: Array<[string, Partial<WorkDraft>]> = [
      ['an empty title', { title: '' }],
      ['a whitespace-only title', { title: '   ' }],
      ['an empty slug', { slug: '' }],
      ['a slug with a forward slash', { slug: 'a/b' }],
      ['a slug with a backslash', { slug: 'a\\b' }],
      ['an entry containing ..', { entry: '../index.html' }],
      ['an entry starting with /', { entry: '/index.html' }],
      ['an entry containing a drive colon', { entry: 'C:index.html' }],
      ['a cover containing ..', { cover: '../cover.webp' }],
    ];

    for (const [label, change] of shapes) {
      const problem = validateWorkDraft({ ...draft(), ...change });
      expect(problem, `${label} was accepted`).not.toBeNull();
      expect(problem).toMatch(/[\u4e00-\u9fff]/u);
    }
  });

  it('agrees with the transcribed crate rule for every shape', () => {
    // The transcription's `accepted` column is the crate's verdict. A shape the crate accepts must
    // not be refused here, or the window would block a save the crate would have allowed.
    const byShape: Record<string, Partial<WorkDraft>> = {
      'a plain title and slug': {},
      'an empty title': { title: '' },
      'a whitespace-only title': { title: '  ' },
      'an empty slug': { slug: '' },
      'a slug with a forward slash': { slug: 'a/b' },
      'a slug with a backslash': { slug: 'a\\b' },
      'an entry containing ..': { entry: '..' },
      'an entry starting with /': { entry: '/x.html' },
      'an entry containing a drive colon': { entry: 'D:x.html' },
      'a cover or screenshot containing ..': { cover: '..' },
    };

    for (const rule of CRATE_RULE) {
      const change = byShape[rule.shape];
      expect(change, `no draft shape is transcribed for ${rule.shape}`).toBeDefined();
      const problem = validateWorkDraft({ ...draft(), ...change });
      expect(problem === null, `${rule.shape}: the crate says ${rule.why}`).toBe(rule.accepted);
    }
  });

  it('does not refuse a slug or entry that is merely unusual', () => {
    // Over-refusing is the failure mode this test exists for: the crate accepts these, so the window
    // must not block them.
    expect(validateWorkDraft(draft({ slug: 'demo-2', entry: 'sub/dir/page.html' }))).toBeNull();
    expect(validateWorkDraft(draft({ title: '带空格 的 名称', slug: 'demo-counter-2' }))).toBeNull();
  });
});

describe('slugify, which only PREVIEWS the address the import will use', () => {
  it('matches the shapes veloct-core produces', () => {
    expect(slugify('Demo Counter')).toBe('demo-counter');
    expect(slugify('  Hello,  World!  ')).toBe('hello-world');
    expect(slugify('A---B')).toBe('a-b');
    expect(slugify('a_b')).toBe('a_b');
    // The crate's two decisions that are easy to get wrong: CJK is KEPT, and accented letters are
    // kept and lowercased rather than transliterated (so `ø` stays `ø`, not `o`).
    expect(slugify('计数器 Demo')).toBe('计数器-demo');
    expect(slugify('Ünïcøde Tïtle')).toBe('ünïcøde-tïtle');
  });

  it('falls back to the crate\'s `untitled`, which the plan then overrides', () => {
    // `WorkMetadata::new` uses the work's stable id when the slug is `untitled`, and the window
    // cannot know that id before the import, so the PLAN is what carries the real address.
    expect(slugify('!!!')).toBe('untitled');
    expect(slugify('')).toBe('untitled');
  });
});

describe('formatBytes', () => {
  it('reports each magnitude with a unit', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(2048)).toBe('2.0 KiB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MiB');
  });

  it('says it does not know rather than reporting a negative size', () => {
    expect(formatBytes(-1)).toBe('未知大小');
    expect(formatBytes(Number.NaN)).toBe('未知大小');
  });
});

describe('validateScreenshotChoice', () => {
  it('accepts each supported format at a sane size', () => {
    for (const format of SUPPORTED_IMAGE_FORMATS) {
      expect(validateScreenshotChoice({ fileName: `shot.${format}`, bytes: 2048 })).toBeNull();
    }
    // The extension check is case-insensitive, because a file saved as `SHOT.PNG` is a PNG.
    expect(validateScreenshotChoice({ fileName: 'SHOT.PNG', bytes: 2048 })).toBeNull();
  });

  it('refuses a format the site route cannot serve', () => {
    expect(validateScreenshotChoice({ fileName: 'shot.txt', bytes: 10 })).toMatch(/txt/);
    expect(validateScreenshotChoice({ fileName: 'shot.gif', bytes: 10 })).not.toBeNull();
  });

  it('refuses a file with no extension, because its format cannot be known', () => {
    expect(validateScreenshotChoice({ fileName: 'screenshot', bytes: 10 })).not.toBeNull();
  });

  it('refuses an empty file and one above the limit', () => {
    expect(validateScreenshotChoice({ fileName: 'shot.webp', bytes: 0 })).not.toBeNull();
    expect(
      validateScreenshotChoice({ fileName: 'shot.webp', bytes: DEFAULT_SCREENSHOT_LIMIT + 1 }),
    ).not.toBeNull();
    expect(
      validateScreenshotChoice({ fileName: 'shot.webp', bytes: DEFAULT_SCREENSHOT_LIMIT }),
    ).toBeNull();
  });

  it('uses the limit the caller read from the command rather than its own constant', () => {
    expect(validateScreenshotChoice({ fileName: 'shot.webp', bytes: 100, limit: 50 })).not.toBeNull();
  });

  it('refuses an unreadable size rather than passing it through', () => {
    expect(validateScreenshotChoice({ fileName: 'shot.webp', bytes: Number.NaN })).not.toBeNull();
  });
});

describe('servableReason', () => {
  it('says nothing for a work the build will emit', () => {
    expect(servableReason(work())).toBeNull();
  });

  it('names the missing body rather than hiding the work', () => {
    expect(servableReason({ ...work(), has_body: false })).toMatch(/site\//);
  });

  it('names an empty body', () => {
    expect(servableReason({ ...work(), file_count: 0 })).not.toBeNull();
  });
});

describe('skippedNote', () => {
  it('is absent when nothing was skipped', () => {
    expect(skippedNote([])).toBeNull();
  });

  it('names every skipped directory, because what is left out is part of what was agreed', () => {
    const note = skippedNote(['node_modules', 'dist']);
    expect(note).toContain('node_modules');
    expect(note).toContain('dist');
  });
});

describe('planKey', () => {
  it('changes when any input the plan was computed from changes', () => {
    const base = { path: 'C:/demo', kind: 'folder', title: 'Demo' };
    expect(planKey(base)).toBe(planKey({ ...base }));
    expect(planKey(base)).not.toBe(planKey({ ...base, path: 'C:/other' }));
    expect(planKey(base)).not.toBe(planKey({ ...base, kind: 'zip' }));
    expect(planKey(base)).not.toBe(planKey({ ...base, title: 'Other' }));
    // A title that differs only in surrounding whitespace is the same title.
    expect(planKey(base)).toBe(planKey({ ...base, title: '  Demo  ' }));
  });
});

describe('importReadiness', () => {
  const ready = {
    sourcePath: 'C:/demo',
    title: 'Demo',
    planned: true,
    blockedReason: null,
    busy: false,
  };

  it('is ready only once a plan has been seen', () => {
    expect(importReadiness(ready).ready).toBe(true);
    const unplanned = importReadiness({ ...ready, planned: false });
    expect(unplanned.ready).toBe(false);
    expect(unplanned.reason).toMatch(/预览/);
  });

  it('refuses without a source or a title, and says which', () => {
    expect(importReadiness({ ...ready, sourcePath: '  ' }).reason).toMatch(/ZIP|文件夹/);
    expect(importReadiness({ ...ready, title: '' }).reason).toMatch(/名称/);
  });

  it('carries the crate-visible reason when the plan is blocked', () => {
    const blocked = importReadiness({ ...ready, blockedReason: 'the slug is taken' });
    expect(blocked.ready).toBe(false);
    expect(blocked.reason).toBe('the slug is taken');
  });

  it('is not ready while an import is already running', () => {
    expect(importReadiness({ ...ready, busy: true }).ready).toBe(false);
  });
});

describe('posterReadiness', () => {
  it('refuses with no file chosen', () => {
    expect(posterReadiness({ screenshotPath: '', hasFile: false, fileProblem: null }).ready).toBe(false);
  });

  it('surfaces the file problem the window already found', () => {
    const result = posterReadiness({
      screenshotPath: 'C:/x/shot.gif',
      hasFile: true,
      fileProblem: 'bad format',
    });
    expect(result.ready).toBe(false);
    expect(result.reason).toBe('bad format');
  });

  it('allows a typed path, because the window cannot read a file it was not handed', () => {
    // The crate reports a missing path. Guessing here would mean refusing a path that might be fine.
    expect(
      posterReadiness({ screenshotPath: 'C:/x/shot.webp', hasFile: false, fileProblem: null }).ready,
    ).toBe(true);
  });

  it('is ready for a good file', () => {
    expect(
      posterReadiness({ screenshotPath: 'C:/x/shot.webp', hasFile: true, fileProblem: null }).ready,
    ).toBe(true);
  });
});

describe('the plan view the panel renders', () => {
  it('carries everything the confirmation needs, so nothing is invented in JSX', () => {
    // This is a shape test on purpose: the panel shows these fields and nothing else, so a field
    // missing here is a field the confirmation would have to guess at.
    const plan: WorkImportPlanView = {
      title: 'Demo Counter',
      slug: 'demo-counter',
      id: '01J8Z5Q0K3W7M9V2B4N6P8R2AG',
      entry: 'index.html',
      file_count: 12,
      total_bytes: 4096,
      metadata: work().metadata,
      skipped_directories: ['node_modules'],
      stripped_prefix: 'my-demo',
      unused_images: ['screenshot.png'],
      warning: { total_bytes: 4096, threshold: 1024, message: '很大' },
      blocked_reason: null,
    };

    expect(plan.slug).toBe('demo-counter');
    expect(plan.skipped_directories).toContain('node_modules');
    expect(plan.unused_images).toContain('screenshot.png');
    expect(importReadiness({
      sourcePath: 'C:/demo',
      title: plan.title,
      planned: true,
      blockedReason: plan.blocked_reason,
      busy: false,
    }).ready).toBe(true);
  });
});
