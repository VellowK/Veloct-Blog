// The works panel (handoff.md section 10).
//
// Section 10's whole model — `works/<stable-id>/work.json` plus the sibling `site/`, imported from a
// ZIP or a folder, shown on a poster wall, with the body running on its own origin — existed only in
// `veloct-content`'s tests until this panel and the commands behind it. PROCESSING.md section 5.9
// records exactly that gap, and this is the window half of closing it.
//
// Three things this panel deliberately does NOT do:
//
//  * **It does not edit the body.** The body is a truth source that lives in Git; the panel imports it,
//    exports it and removes it, and never rewrites a file inside `site/`.
//  * **It does not generate a poster.** `handoff.md` section 10.3 wants a screenshot and this build has
//    no headless browser, so the panel takes the file the user chose and says so. Inventing a capture
//    would be a fake success, which section 20 forbids.
//  * **It does not import before showing the plan.** `planWorkImport` writes nothing, so the entry
//    file, the file count, the size warning and the skipped directories are shown first.
//
// Every rule the panel applies lives in `src/lib/workForm.ts` and is covered by
// `tests/workForm.test.ts`, because this repository has no React test renderer: a rule that lives in
// JSX is a rule nothing checks.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import {
  api,
  describeError,
  type WorkImportOutcomeView,
  type WorkImportPlanView,
  type WorkView,
} from './lib/api';
import {
  DEFAULT_SCREENSHOT_LIMIT,
  draftFromWork,
  draftIsDirty,
  formatBytes,
  importReadiness,
  metadataFromDraft,
  planKey,
  posterReadiness,
  servableReason,
  skippedNote,
  validateScreenshotChoice,
  validateWorkDraft,
  type WorkDraft,
} from './lib/workForm';

interface Props {
  /** The projects section 10 needs an open project: a work's truth source is a file IN one. */
  enabled: boolean;
}

/** The poster preview URL for a work, or `null` when nothing will be emitted for it. */
function posterUrl(work: WorkView): string | null {
  return work.screenshot_url;
}

export default function WorksPanel({ enabled }: Props) {
  const [works, setWorks] = useState<WorkView[]>([]);
  const [servable, setServable] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // ---- Import (section 10.2) ------------------------------------------------------------------
  const [sourcePath, setSourcePath] = useState('');
  const [sourceKind, setSourceKind] = useState<'zip' | 'folder'>('folder');
  const [importTitle, setImportTitle] = useState('');
  const [importAuthor, setImportAuthor] = useState('');
  /** The plan the user has SEEN, with the key it was computed from so it cannot go stale. */
  const [plan, setPlan] = useState<{ key: string; value: WorkImportPlanView } | null>(null);
  const [lastImport, setLastImport] = useState<WorkImportOutcomeView | null>(null);

  // ---- Metadata editing -----------------------------------------------------------------------
  const [editing, setEditing] = useState<{ id: string; draft: WorkDraft } | null>(null);
  /**
   * The CRATE's ruling on the draft, or `null` when it can be saved.
   *
   * It arrives from the `validate_work_metadata` command rather than from a copy of the rules in the window
   * (PROCESSING.md §5.10.1). The effect below asks on every change, debounced; the save path asks again,
   * because a debounce is a delay and not a guarantee.
   */
  const [saveReason, setSaveReason] = useState<string | null>(null);

  // ---- Poster image (section 10.1's `preview/`) ------------------------------------------------
  const [posterTarget, setPosterTarget] = useState<string | null>(null);
  const [posterPath, setPosterPath] = useState('');
  const [posterProblem, setPosterProblem] = useState<string | null>(null);
  const [posterLimit, setPosterLimit] = useState(DEFAULT_SCREENSHOT_LIMIT);
  /**
   * Whether the chosen path came from the file picker, so the window KNOWS the file's name and size.
   *
   * A path typed by hand cannot be read by the window, so the format and size checks only run on a
   * file it was handed; a typed path is sent and the crate reports a missing or unusable file. That
   * is the honest split rather than a refusal based on a guess.
   */
  const [posterChosen, setPosterChosen] = useState(false);

  /** The work whose removal is being confirmed. An inline step, so it cannot be dismissed by accident. */
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      const list = await api.listWorks();
      setWorks(list.works);
      setServable(list.servable);
    } catch (caught) {
      setError(describeError(caught));
    }
  }, [enabled]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // The poster ceiling comes from the crate rather than from a constant in the window, so the panel
  // cannot advertise a size the crate then refuses.
  useEffect(() => {
    if (!enabled) return;
    void (async () => {
      try {
        setPosterLimit(await api.workScreenshotLimit());
      } catch {
        // A failed read leaves the crate's documented default in place rather than a guess.
        setPosterLimit(DEFAULT_SCREENSHOT_LIMIT);
      }
    })();
  }, [enabled]);

  const run = useCallback(
    async (action: () => Promise<void>, done: string) => {
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        await action();
        await reload();
        setNotice(done);
      } catch (caught) {
        setError(describeError(caught));
      } finally {
        setBusy(false);
      }
    },
    [reload],
  );

  // ---- Import ---------------------------------------------------------------------------------

  const currentPlanKey = useMemo(
    () => planKey({ path: sourcePath, kind: sourceKind, title: importTitle }),
    [sourcePath, sourceKind, importTitle],
  );
  const seenPlan = plan && plan.key === currentPlanKey ? plan.value : null;

  /** Inspect a source without importing it, so the confirmation is about a real plan. */
  const inspect = useCallback(
    async (path: string, kind: 'zip' | 'folder') => {
      if (!importTitle.trim()) {
        setError('请先填写作品名称,预览需要一个标题来计算地址片段。');
        setPlan(null);
        return;
      }
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        const value = await api.planWorkImport({
          sourcePath: path,
          sourceKind: kind,
          title: importTitle.trim(),
          author: importAuthor.trim(),
        });
        setPlan({ key: planKey({ path, kind, title: importTitle }), value });
        if (!importAuthor.trim() && value.metadata.author) {
          // The plan carries the author the crate used, so an empty field is filled from what actually
          // happened rather than left for the user to wonder about.
          setImportAuthor(value.metadata.author);
        }
      } catch (caught) {
        setPlan(null);
        setError(describeError(caught));
      } finally {
        setBusy(false);
      }
    },
    [importTitle, importAuthor],
  );

  const chooseSource = useCallback(
    async (kind: 'zip' | 'folder') => {
      setError(null);
      try {
        const chosen = await open(
          kind === 'folder'
            ? { directory: true, multiple: false, title: '选择作品文件夹' }
            : {
                directory: false,
                multiple: false,
                title: '选择作品 ZIP',
                filters: [{ name: 'ZIP', extensions: ['zip'] }],
              },
        );
        const path = typeof chosen === 'string' ? chosen : Array.isArray(chosen) ? chosen[0] : null;
        if (!path) return;
        // What the path IS decides the kind, rather than what the button said: a folder named
        // `demo.zip` must not be handed to a ZIP reader.
        const described = await api.describeWorkSource(path);
        setSourcePath(described.path);
        setSourceKind(described.kind);
        await inspect(described.path, described.kind);
      } catch (caught) {
        setError(describeError(caught));
      }
    },
    [inspect],
  );

  const readiness = importReadiness({
    sourcePath,
    title: importTitle,
    planned: seenPlan !== null,
    blockedReason: seenPlan?.blocked_reason ?? null,
    busy,
  });

  const doImport = () =>
    run(async () => {
      // The plan has already been shown and the crate re-validates everything it was told, so this
      // sends the same inputs the plan was computed from and reports whatever the crate says.
      const outcome = await api.importWork({
        sourcePath,
        sourceKind,
        title: importTitle.trim(),
        author: importAuthor.trim(),
      });
      setLastImport(outcome);
      setPlan(null);
      setSourcePath('');
      setImportTitle('');
    }, '作品已导入。作品本体是真相源,会随 Git 一起提交;构建时进入 dist/works。');

  // ---- Metadata editing -----------------------------------------------------------------------

  const beginEdit = (work: WorkView) => {
    setError(null);
    setNotice(null);
    setEditing({ id: work.metadata.id, draft: draftFromWork(work) });
  };

  const setDraftField = (key: keyof WorkDraft, value: string) => {
    setEditing((current) =>
      current ? { ...current, draft: { ...current.draft, [key]: value } } : current,
    );
  };

  const currentWork = useMemo(
    () => (editing ? works.find((work) => work.metadata.id === editing.id) ?? null : null),
    [editing, works],
  );
  const dirty = editing && currentWork ? draftIsDirty(editing.draft, currentWork) : false;

  /**
   * Ask the crate whether this draft may be saved, WITHOUT saving it.
   *
   * This is what replaced the window's copy of `WorkMetadata::validate`. The object asked about is exactly the
   * object the save would write (`metadataFromDraft`), so the answer cannot be about a different shape.
   */
  useEffect(() => {
    if (!editing || !currentWork) {
      setSaveReason(null);
      return;
    }
    let live = true;
    setSaveReason(null);
    const handle = window.setTimeout(() => {
      void validateWorkDraft(editing.draft, currentWork.metadata, api.validateWorkMetadata).then(
        (reason) => {
          if (live) setSaveReason(reason);
        },
      );
    }, 250);
    return () => {
      live = false;
      window.clearTimeout(handle);
    };
  }, [editing, currentWork]);

  const saveMetadata = () =>
    run(async () => {
      if (!editing) return;
      if (!currentWork) throw new Error('这个作品已经不在列表里了,请刷新后重试。');
      // Asked AGAIN here, rather than trusting the debounced hint: a save must never be sent on a draft the
      // crate has not accepted, and the crate's message is what the user sees either way.
      const reason = await validateWorkDraft(
        editing.draft,
        currentWork.metadata,
        api.validateWorkMetadata,
      );
      if (reason) throw new Error(reason);
      // The whole metadata object is sent back, so a field this form does not show keeps its value — and it is
      // built by the SAME function the validation above used, so the two cannot judge different objects.
      await api.updateWork(editing.id, metadataFromDraft(editing.draft, currentWork.metadata));
      setEditing(null);
    }, '作品元数据已保存。稳定 ID 与导入来源不会被改动。');

  // ---- Poster image ---------------------------------------------------------------------------

  const choosePoster = useCallback(async () => {
    setError(null);
    setPosterProblem(null);
    try {
      const chosen = await open({
        directory: false,
        multiple: false,
        title: '选择封面截图',
        filters: [{ name: '图片', extensions: ['webp', 'png', 'jpg', 'jpeg'] }],
      });
      const path = typeof chosen === 'string' ? chosen : Array.isArray(chosen) ? chosen[0] : null;
      if (!path) return;
      setPosterPath(path);
      setPosterProblem(null);
      // The picker does not report a size, so the format check waits for the `<input type="file">`
      // reading, and the crate reports a file it cannot use.
      setPosterChosen(false);
    } catch (caught) {
      setError(describeError(caught));
    }
  }, []);

  const posterState = posterReadiness({
    screenshotPath: posterPath,
    hasFile: posterChosen,
    fileProblem: posterProblem,
  });

  const savePoster = () =>
    run(async () => {
      if (!posterTarget) return;
      if (posterPath.trim() === '') throw new Error('请先选择一个图片文件。');
      const problem = posterReadiness({
        screenshotPath: posterPath,
        hasFile: posterChosen,
        fileProblem: posterProblem,
      }).reason;
      if (problem) throw new Error(problem);
      await api.setWorkScreenshot(posterTarget, posterPath.trim());
      setPosterPath('');
      setPosterChosen(false);
      setPosterTarget(null);
    }, '封面截图已保存到作品的 preview/ 目录,构建时会输出到 /work-previews/<地址>/screenshot.webp。');

  /** Check a poster the browser handed us, so an unusable file is refused before the copy. */
  const checkPosterFile = useCallback(
    (name: string, bytes: number) => {
      setPosterChosen(true);
      setPosterProblem(validateScreenshotChoice({ fileName: name, bytes, limit: posterLimit }));
    },
    [posterLimit],
  );

  // ---- Export and removal ---------------------------------------------------------------------

  const doExport = (work: WorkView) =>
    run(async () => {
      const chosen = await open({
        directory: true,
        multiple: false,
        title: '选择导出目录',
      });
      const directory = typeof chosen === 'string' ? chosen : Array.isArray(chosen) ? chosen[0] : null;
      if (!directory) return;
      const separator = directory.includes('\\') ? '\\' : '/';
      const destination = `${directory}${directory.endsWith(separator) ? '' : separator}${work.metadata.slug}.zip`;
      const written = await api.exportWork(work.metadata.id, destination);
      setNotice(`已导出 ${written} 个文件到 ${destination}。导出可以被重新导入(§10.5)。`);
    }, '导出完成。');

  const doRemove = (work: WorkView) =>
    run(async () => {
      await api.removeWork(work.metadata.id);
      setConfirmRemove(null);
      if (editing?.id === work.metadata.id) setEditing(null);
    }, '作品已删除。作品本体属于 Git,请提交这次删除。');

  if (!enabled) {
    return (
      <div className="works-panel">
        <p className="muted">打开一个项目后可以管理作品。</p>
      </div>
    );
  }

  return (
    <div className="works-panel">
      <header className="works-header">
        <strong>作品</strong>
        <span className="muted">
          共 {works.length} 个,其中 {servable} 个会进入构建
        </span>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新
        </button>
      </header>

      {error && (
        <p className="works-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="works-notice">{notice}</p>}

      <section className="works-import">
        <h3>导入作品(§10.2)</h3>
        <div className="row">
          <label>
            <span>作品名称</span>
            <input
              value={importTitle}
              onChange={(event) => {
                setImportTitle(event.target.value);
                setPlan(null);
              }}
              placeholder="Demo Counter"
              spellCheck={false}
            />
          </label>
          <label>
            <span>作者</span>
            <input
              value={importAuthor}
              onChange={(event) => setImportAuthor(event.target.value)}
              placeholder="留空则使用项目默认作者"
              spellCheck={false}
            />
          </label>
        </div>

        <div className="row">
          <button type="button" onClick={() => void chooseSource('folder')} disabled={busy}>
            选择文件夹…
          </button>
          <button type="button" onClick={() => void chooseSource('zip')} disabled={busy}>
            选择 ZIP…
          </button>
        </div>

        <div className="row">
          <label className="grow">
            <span>来源路径({sourceKind === 'zip' ? 'ZIP 文件' : '文件夹'})</span>
            <input
              value={sourcePath}
              onChange={(event) => {
                setSourcePath(event.target.value);
                setPlan(null);
              }}
              placeholder="D:/作品/demo-counter"
              spellCheck={false}
            />
          </label>
          <button
            type="button"
            onClick={() => void inspect(sourcePath.trim(), sourceKind)}
            disabled={busy || sourcePath.trim() === '' || importTitle.trim() === ''}
            title="预演这次导入,不写入任何文件"
          >
            查看导入预览
          </button>
        </div>

        {/* Section 10.1: what the import will leave out is part of what the user is agreeing to. */}
        {seenPlan && (
          <div className="works-plan">
            <p>
              将导入 <strong>{seenPlan.title}</strong>,地址片段 <code>{seenPlan.slug}</code>,入口{' '}
              <code>{seenPlan.entry}</code>,{seenPlan.file_count} 个文件,共{' '}
              {formatBytes(seenPlan.total_bytes)}。
            </p>
            {seenPlan.stripped_prefix && (
              <p className="muted">
                已识别并会剥离外层目录 <code>{seenPlan.stripped_prefix}</code>,让作品本体在根路径提供。
              </p>
            )}
            {skippedNote(seenPlan.skipped_directories) && (
              <p className="muted">{skippedNote(seenPlan.skipped_directories)}</p>
            )}
            {seenPlan.unused_images.length > 0 && (
              <p className="muted">
                这些图片不会被当作封面截图:
                {seenPlan.unused_images.map((image) => (
                  <code key={image}>{image}</code>
                ))}
                。导入后可以用「设置封面」把它保存为 <code>preview/screenshot.webp</code>。
              </p>
            )}
            {seenPlan.warning && <p className="works-warning">{seenPlan.warning.message}</p>}
            {seenPlan.blocked_reason && <p className="works-error">{seenPlan.blocked_reason}</p>}
          </div>
        )}

        {readiness.reason && <p className="muted hint">{readiness.reason}</p>}

        <button type="button" onClick={() => void doImport()} disabled={!readiness.ready}>
          导入
        </button>

        {lastImport && (
          <p className="works-result">
            已导入 <strong>{lastImport.work.metadata.title}</strong>({lastImport.files_written} 个文件,
            {formatBytes(lastImport.total_bytes)})到 <code>{lastImport.directory}</code>。
            {lastImport.warning && <span className="works-warning"> {lastImport.warning.message}</span>}
          </p>
        )}
      </section>

      <section>
        <h3>作品列表</h3>
        {works.length === 0 && <p className="muted">还没有导入作品。</p>}
        <ul className="works-list">
          {works.map((work) => {
            const blocked = servableReason(work);
            const poster = posterUrl(work);
            return (
              <li key={work.metadata.id} data-work={work.metadata.id}>
                <div className="works-row">
                  <span className="works-poster" aria-hidden={poster === null}>
                    {poster ? (
                      <img src={poster} alt={`${work.metadata.title} 截图`} loading="lazy" />
                    ) : (
                      <span className="muted">暂无截图</span>
                    )}
                  </span>
                  <span className="works-main">
                    <span className="works-title">
                      {work.metadata.title} <code>{work.metadata.slug}</code>
                    </span>
                    <span className="muted">
                      {work.metadata.author || '(未填写作者)'} · {work.file_count} 个文件 ·{' '}
                      {formatBytes(work.metadata.size_bytes)} · 入口 <code>{work.metadata.entry}</code>
                    </span>
                    {work.metadata.completed_at && (
                      <span className="muted">完成于 {work.metadata.completed_at}</span>
                    )}
                    {work.metadata.tags.length > 0 && (
                      <span className="muted">
                        {work.metadata.tags.map((tag) => (
                          <span key={tag} className="works-tag">
                            #{tag}
                          </span>
                        ))}
                      </span>
                    )}
                    {blocked && <span className="works-warning">{blocked}</span>}
                    {work.screenshot_drift && (
                      <span className="works-warning">
                        封面文件存在,但 work.json 指向的路径不是 <code>preview/screenshot.webp</code>,
                        构建只会输出这个固定路径。
                      </span>
                    )}
                  </span>
                  <span className="works-actions">
                    <button type="button" onClick={() => beginEdit(work)} disabled={busy}>
                      编辑元数据
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setPosterTarget(work.metadata.id);
                        setPosterPath('');
                        setPosterChosen(false);
                        setPosterProblem(null);
                      }}
                      disabled={busy}
                    >
                      设置封面
                    </button>
                    <button type="button" onClick={() => void doExport(work)} disabled={busy}>
                      导出 ZIP
                    </button>
                    {confirmRemove === work.metadata.id ? (
                      <>
                        <button
                          type="button"
                          className="danger"
                          onClick={() => void doRemove(work)}
                          disabled={busy}
                        >
                          确认删除作品本体
                        </button>
                        <button type="button" onClick={() => setConfirmRemove(null)} disabled={busy}>
                          取消
                        </button>
                      </>
                    ) : (
                      <button
                        type="button"
                        onClick={() => setConfirmRemove(work.metadata.id)}
                        disabled={busy}
                      >
                        删除
                      </button>
                    )}
                  </span>
                </div>

                {posterTarget === work.metadata.id && (
                  <div className="works-poster-form">
                    <p className="muted">
                      选择一个图片文件作为封面截图。它会被复制到作品的{' '}
                      <code>preview/screenshot.webp</code>,随项目一起备份和提交。这个构建没有无头浏览器,
                      因此不会自动截图(§10.3 的封面来自这里)。
                    </p>
                    <div className="row">
                      <label className="grow">
                        <span>图片路径</span>
                        <input
                          value={posterPath}
                          onChange={(event) => {
                            setPosterPath(event.target.value);
                            setPosterProblem(null);
                          }}
                          placeholder="D:/截图/screenshot.webp"
                          spellCheck={false}
                        />
                      </label>
                      <button type="button" onClick={() => void choosePoster()} disabled={busy}>
                        选择图片…
                      </button>
                    </div>
                    <p className="muted hint">
                      上限 {formatBytes(posterLimit)};支持 webp / png / jpg。文件不会被重新编码。
                    </p>
                    {/* A picked file is checked here, so an unusable one is refused before the copy. */}
                    <input
                      type="file"
                      accept=".webp,.png,.jpg,.jpeg"
                      onChange={(event) => {
                        const file = event.target.files?.[0];
                        if (file) checkPosterFile(file.name, file.size);
                      }}
                    />
                    {posterProblem && <p className="works-warning">{posterProblem}</p>}
                    {posterState.reason && <p className="muted hint">{posterState.reason}</p>}
                    <div className="row">
                      <button
                        type="button"
                        onClick={() => void savePoster()}
                        disabled={busy || posterPath.trim() === ''}
                      >
                        保存封面
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setPosterTarget(null);
                          setPosterPath('');
                          setPosterChosen(false);
                          setPosterProblem(null);
                        }}
                        disabled={busy}
                      >
                        取消
                      </button>
                    </div>
                  </div>
                )}

                {editing?.id === work.metadata.id && (
                  <div className="works-edit">
                    <div className="row">
                      <label>
                        <span>名称</span>
                        <input
                          value={editing.draft.title}
                          onChange={(event) => setDraftField('title', event.target.value)}
                        />
                      </label>
                      <label>
                        <span>作者</span>
                        <input
                          value={editing.draft.author}
                          onChange={(event) => setDraftField('author', event.target.value)}
                        />
                      </label>
                      <label>
                        <span>地址片段</span>
                        <input
                          value={editing.draft.slug}
                          onChange={(event) => setDraftField('slug', event.target.value)}
                          spellCheck={false}
                        />
                      </label>
                    </div>
                    <div className="row">
                      <label className="grow">
                        <span>简介</span>
                        <input
                          value={editing.draft.summary}
                          onChange={(event) => setDraftField('summary', event.target.value)}
                        />
                      </label>
                      <label>
                        <span>入口文件</span>
                        <input
                          value={editing.draft.entry}
                          onChange={(event) => setDraftField('entry', event.target.value)}
                          spellCheck={false}
                        />
                      </label>
                    </div>
                    <div className="row">
                      <label>
                        <span>完成时间(RFC 3339)</span>
                        <input
                          value={editing.draft.completedAt}
                          onChange={(event) => setDraftField('completedAt', event.target.value)}
                          placeholder="2026-02-14T00:00:00Z"
                          spellCheck={false}
                        />
                      </label>
                      <label>
                        <span>封面路径(作品目录内)</span>
                        <input
                          value={editing.draft.cover}
                          onChange={(event) => setDraftField('cover', event.target.value)}
                          placeholder="preview/cover.webp"
                          spellCheck={false}
                        />
                      </label>
                    </div>
                    <div className="row">
                      <label className="grow">
                        <span>标签(每行一个)</span>
                        <textarea
                          value={editing.draft.tags}
                          onChange={(event) => setDraftField('tags', event.target.value)}
                          rows={2}
                        />
                      </label>
                      <label className="grow">
                        <span>备注</span>
                        <textarea
                          value={editing.draft.notes}
                          onChange={(event) => setDraftField('notes', event.target.value)}
                          rows={2}
                        />
                      </label>
                    </div>
                    <p className="muted hint">
                      稳定 ID <code>{work.metadata.id}</code> 与导入来源不会被修改:ID 就是目录名。
                    </p>
                    {saveReason && <p className="works-warning">{saveReason}</p>}
                    <div className="row">
                      <button
                        type="button"
                        onClick={() => void saveMetadata()}
                        disabled={busy || saveReason !== null || !dirty}
                      >
                        保存元数据
                      </button>
                      <button type="button" onClick={() => setEditing(null)} disabled={busy}>
                        取消
                      </button>
                      {!dirty && <span className="muted hint">没有改动。</span>}
                    </div>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
}
