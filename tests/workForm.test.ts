/**
 * The works panel's decisions (`PROCESSING.md` §5.9 / H10).
 *
 * `handoff.md` §10 is a locked section, and this repository has no React test renderer — deliberately.
 * So the panel's rules live in `src/lib/workForm.ts` and are asserted here: which fields a work's
 * metadata edit may change, why a work will not be served, and whether an import or a poster may be sent.
 *
 * **What is deliberately NOT here any more is a second copy of any rule.** A ten-shape `CRATE_RULE` table used
 * to live in this file, carrying a hand-written copy of `WorkMetadata::validate`'s verdict in every row, and it
 * could only ever DETECT that the window's restatement had drifted from the crate. Both are gone: the RULES are
 * asserted in Rust, against the authority (`src-tauri/src/works.rs` drives the `validate_work_metadata`
 * command's own body), and this file asserts that the window sends the object the save would write and surfaces
 * the crate's message unchanged.
 */

import { describe, expect, it } from 'vitest';

import type { WorkImportPlanView, WorkMetadataView, WorkView } from '../src/lib/api';
import {
  DEFAULT_SCREENSHOT_LIMIT,
  draftFromWork,
  draftIsDirty,
  formatBytes,
  formatTags,
  importReadiness,
  metadataFromDraft,
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

/**
 * The form's ruling is the CRATE's, and this is where that is asserted.
 *
 * `PROCESSING.md` §5.10.1: the window used to restate `WorkMetadata::validate` field by field, and a
 * ten-shape transcription table compared the two hand-written copies. There is no copy left, so what these
 * tests prove is the SHAPE of what replaced it: the object the save would write goes out, and whatever the
 * crate answers comes back unchanged. The RULES themselves are asserted in Rust
 * (`src-tauri/src/works.rs`), where the authority is.
 */
describe('asking the crate about a draft', () => {
  /** A validator that refuses exactly what the fake "crate" is told to refuse. */
  function crate(says: (metadata: WorkMetadataView) => string | null) {
    return async (metadata: WorkMetadataView) => {
      const problem = says(metadata);
      if (problem !== null) {
        // The shape a rejected `invoke` produces, so the window's own error handling is what is exercised.
        throw { code: 'INVALID', message: problem, actionable: false, hint: null };
      }
    };
  }

  /** A crate that accepts everything, which is what makes the "no rule text here" assertion meaningful. */
  const acceptsEverything = crate(() => null);

  it('accepts a draft the crate accepts, and holds no opinion of its own', async () => {
    expect(await validateWorkDraft(draft(), work().metadata, acceptsEverything)).toBeNull();

    // Every shape the deleted copy refused is accepted here, because the window has no rule to refuse it with.
    // This is the assertion that a copy of the rules cannot come back without a test noticing.
    const wouldBeRefused: Array<Partial<WorkDraft>> = [
      { title: '   ' },
      { slug: 'a/b' },
      { slug: 'a\\b' },
      { entry: '../index.html' },
      { entry: '/index.html' },
      { entry: 'C:index.html' },
      { cover: '../cover.webp' },
    ];
    for (const change of wouldBeRefused) {
      expect(
        await validateWorkDraft({ ...draft(), ...change }, work().metadata, acceptsEverything),
        `${JSON.stringify(change)} was refused by the window`,
      ).toBeNull();
    }
  });

  it("shows the crate's own message, word for word", async () => {
    const message = 'title must not be empty';
    expect(await validateWorkDraft(draft({ title: '' }), work().metadata, crate(() => message))).toBe(message);

    const escaping = 'entry "../index.html" must not escape the work directory';
    expect(await validateWorkDraft(draft({ entry: '..' }), work().metadata, crate(() => escaping))).toBe(
      escaping,
    );
  });

  it("carries the crate's hint when there is one, and invents none when there is not", async () => {
    const withHint = await validateWorkDraft(draft({ slug: 'a/b' }), work().metadata, async () => {
      throw {
        code: 'INVALID',
        message: 'slug "a/b" must not contain a path separator',
        actionable: true,
        hint: '地址片段会成为 URL 的一部分。',
      };
    });
    expect(withHint).toContain('must not contain a path separator');
    expect(withHint).toContain('地址片段');

    expect(await validateWorkDraft(draft(), work().metadata, crate(() => 'slug must not be empty'))).toBe(
      'slug must not be empty',
    );
  });

  it('reports a failed call rather than treating it as an acceptance', async () => {
    const reason = await validateWorkDraft(draft(), work().metadata, async () => {
      throw new Error('窗口与后端之间的连接已断开。');
    });
    expect(reason).toBe('窗口与后端之间的连接已断开。');
  });

  it('asks about exactly the object the save would write', async () => {
    // The other half of one authority: the object judged and the object written are one expression, built by
    // `metadataFromDraft`. A form that validated one shape and saved another would make the answer meaningless.
    const changed: WorkDraft = {
      ...draft(),
      title: '  新名字  ',
      tags: 'demo, #ui',
      cover: '',
      completedAt: '   ',
    };
    const seen: WorkMetadataView[] = [];
    const record = async (metadata: WorkMetadataView) => {
      seen.push(metadata);
    };

    expect(await validateWorkDraft(changed, work().metadata, record)).toBeNull();
    expect(seen).toHaveLength(1);
    const asked = seen[0];
    if (asked === undefined) throw new Error('the validator was not asked about the draft');
    expect(asked).toEqual(metadataFromDraft(changed, work().metadata));
    expect(asked.title).toBe('新名字');
    expect(asked.tags).toEqual(['demo', 'ui']);
    expect(asked.cover).toBeNull();
    expect(asked.completed_at).toBeNull();
    // A field the form does not show keeps its value, because the base metadata is spread first.
    expect(asked.id).toBe(work().metadata.id);
    expect(asked.imported_at).toBe(work().metadata.imported_at);
    expect(asked.screenshot).toBe(work().metadata.screenshot);
    expect(asked.size_bytes).toBe(work().metadata.size_bytes);
  });
});

describe('metadataFromDraft, which is both the validated and the written object', () => {
  it('trims the fields the crate trims and blanks what the user emptied', () => {
    const result = metadataFromDraft(
      { ...draft(), title: '  T  ', author: ' A ', slug: ' s ', entry: ' main.html ', cover: '  ' },
      work().metadata,
    );
    expect(result.title).toBe('T');
    expect(result.author).toBe('A');
    expect(result.slug).toBe('s');
    expect(result.entry).toBe('main.html');
    expect(result.cover).toBeNull();
  });

  it('leaves the identity, the provenance and the poster to the work it came from', () => {
    // §3.2: the id is the directory name, so a form that could change it would move the body. And §10.1's
    // poster is set by its own command, not by this form.
    const base = work({ cover: null, screenshot: 'preview/screenshot.webp' }).metadata;
    const result = metadataFromDraft(draftFromWork(work()), base);
    expect(result.id).toBe(base.id);
    expect(result.slug).toBe(base.slug);
    expect(result.imported_at).toBe(base.imported_at);
    expect(result.source).toEqual(base.source);
    expect(result.size_bytes).toBe(base.size_bytes);
    expect(result.screenshot).toBe('preview/screenshot.webp');
  });

  it('round trips the textarea form of the tags, so the panel does no parsing of its own', () => {
    expect(metadataFromDraft({ ...draft(), tags: formatTags(['a', 'b']) }, work().metadata).tags).toEqual([
      'a',
      'b',
    ]);
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
