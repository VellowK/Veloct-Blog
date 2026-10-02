/**
 * The attachment pool — the ONE page converted by this sample.
 *
 * It was chosen over the works panel because it is the page with the most to demonstrate. It has every state
 * a console needs (loading, empty, error, success), it is the page where an error was most invisible, and it
 * is the only page that carries a genuinely failing sub-target. Before this change a failed upload showed up
 * as a `title` tooltip on the words "最近错误" and as one line of text at the top of a long scrolling column;
 * §13.2.2's reason for a disabled button was a bare sentence wedged into a wrapping flex row. Those are now
 * state blocks next to the row they are about.
 *
 * Three rules from `handoff.md` §13 are still visible in this component's SHAPE rather than merely honoured
 * by it — the conversion changed the presentation and left the data flow alone:
 *
 *  * **§13.1's three axes are shown separately.** An attachment can be cached but not uploaded, uploaded but
 *    not referenced by any document, and so on. A single "status" column would flatten three independent
 *    facts into one and make it impossible to tell "the picture is on the server but no article uses it" from
 *    "an article uses it but the upload failed" — different problems with different fixes.
 *  * **The Markdown comes from the Rust side.** `markdown` arrives in the add result, so there is exactly one
 *    place that formats an attachment URL. A second formatter here would be a second thing that can disagree
 *    with the site about where the file is.
 *  * **§13.8's two deletions are two different actions.** `删除` removes this pool's record and the cached
 *    bytes and deliberately leaves the remote file alone; `仅删除远程对象` is the explicit cleanup that touches
 *    ONLY the remote object. Neither is allowed to do the other's job by accident.
 *
 * The orphan section shows §13.2.4's two outcomes as two lists, because the distinction IS the information:
 * an attachment that fails a condition is not a cleanup candidate but a question for the user.
 *
 * The storage-target FORM (§5.5) is deliberately thin here: which fields a kind asks for, what a blank
 * credential means, why a draft cannot be saved and why an upload button is disabled are all decided in
 * `lib/storageTargetForm.ts` and asserted in `tests/storageTargetForm.test.ts`. This repository has no React
 * test renderer on purpose, so a rule that lives in JSX is a rule nothing checks — which is why the conversion
 * moved no decision into JSX. Every `lib/` module and every test is untouched by this sample.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  api,
  describeError,
  type AddAttachmentView,
  type AttachmentOrphans,
  type AttachmentView,
  type ObjectView,
  type StorageKindView,
  type StorageTargetView,
  type UploadEligibility,
} from './lib/api';
import {
  buildSaveRequest,
  changeKind,
  deletionWarning,
  draftFromTarget,
  eligibilityFallback,
  emptyDraft,
  fieldsForKind,
  hiddenFieldsNote,
  kindGapSummary,
  kindNotice,
  normaliseKind,
  remoteCleanup,
  storageKindOptions,
  uploadControl,
  validateDraft,
  type StorageFieldKey,
  type StorageTargetDraft,
} from './lib/storageTargetForm';
import { Card, Field, PageHeader, StateBlock } from './components/ui';

/** The axes, in the order §13.1 lists them, with the values the crate uses. */
const AXES: Array<{ key: keyof AttachmentView; label: string; values: Record<string, string> }> = [
  {
    key: 'upload_status',
    label: '上传',
    values: {
      PENDING: '未上传',
      UPLOADING: '上传中',
      SUCCEEDED: '已上传',
      FAILED: '上传失败',
    },
  },
  {
    key: 'reference_status',
    label: '引用',
    values: {
      NOT_APPLIED: '未被引用',
      APPLYING: '写入中',
      APPLIED: '已被引用',
      FAILED: '引用失败',
    },
  },
  {
    key: 'cache_status',
    label: '缓存',
    values: { PRESENT: '已缓存', CLEANED: '已清理' },
  },
];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function AttachmentPanel({
  enabled,
  onInsertMarkdown,
}: {
  enabled: boolean;
  /** Called with the Markdown for an attachment, so the editor decides what to do with it. */
  onInsertMarkdown?: (markdown: string) => void;
}) {
  const [attachments, setAttachments] = useState<AttachmentView[]>([]);
  const [orphans, setOrphans] = useState<AttachmentOrphans | null>(null);
  const [targets, setTargets] = useState<StorageTargetView[]>([]);
  /**
   * Every storage kind this BUILD knows, with its adapter status (`storage_kinds`).
   *
   * `null` means the read has not succeeded, and the form then reports each kind's status as unknown rather
   * than guessing: this is the answer a target list cannot give, because §5.3/§5.4's kinds are exactly the ones
   * a user has no target for.
   */
  const [kindReport, setKindReport] = useState<StorageKindView[] | null>(null);
  const [objects, setObjects] = useState<ObjectView[]>([]);
  const [uploadTarget, setUploadTarget] = useState('');
  const [cacheDirectory, setCacheDirectory] = useState<string>('');
  const [sourcePath, setSourcePath] = useState('');
  const [target, setTarget] = useState('local');
  const [namespace, setNamespace] = useState('default');
  const [lastAdd, setLastAdd] = useState<AddAttachmentView | null>(null);
  /** An ACTION is running. Every button that changes something is disabled while it is true. */
  const [busy, setBusy] = useState(false);
  /** The read is in flight, so the page can say so instead of showing an empty pool that is not empty. */
  const [reading, setReading] = useState(false);
  /** Whether the pool has ever been read. Distinguishes "loading" from "nothing here". */
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  /** §13.2.2's reason per attachment, read BEFORE an upload is offered. */
  const [eligibility, setEligibility] = useState<Record<string, UploadEligibility>>({});

  /** The storage-target form (PROCESSING.md §5.5). */
  const [draft, setDraft] = useState<StorageTargetDraft>(() => emptyDraft());
  const [editingId, setEditingId] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  /**
   * The CRATE's ruling on the current draft, or `null` when it can be saved.
   *
   * It arrives from the `validate_storage_target` command rather than from a copy of the rules in the window
   * (PROCESSING.md §5.10.1). The effect below asks on every change, debounced, so the feedback is still
   * immediate in practice; the save path asks again, because a debounce is a delay and not a guarantee.
   */
  const [saveReason, setSaveReason] = useState<string | null>(null);
  /** Filled values a kind switch kept in the form but will not submit. Never silently dropped. */
  const [hiddenFilled, setHiddenFilled] = useState<StorageFieldKey[]>([]);
  /** The target whose deletion is being confirmed. An inline step, so it cannot be dismissed by accident. */
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!enabled) return;
    setReading(true);
    try {
      const [list, found, directory, storage, remote] = await Promise.all([
        api.listAttachments(),
        api.attachmentOrphans(),
        api.attachmentCacheDirectory(),
        api.storageTargets(),
        api.attachmentObjects(),
      ]);
      setAttachments(list);
      setOrphans(found);
      setCacheDirectory(directory);
      setTargets(storage);
      setObjects(remote);
      // The first implemented target is the sensible default, and a target with no adapter is not offered.
      setUploadTarget((current) => current || storage.find((candidate) => candidate.implemented)?.id || '');

      // §5.3/§5.4's gap, for every kind the BUILD knows — read separately, because a failure here must not
      // blank the whole panel: the form then reports the status as unknown (never as working) and says so.
      try {
        setKindReport(await api.storageKinds());
      } catch {
        setKindReport(null);
      }

      // §13.2.2's reason, for every attachment in the pool, so the button can say WHY it is disabled. A read
      // that fails falls back to the attachment's own status axis WITH a reason, which is the behaviour the
      // window had before this reason existed — a transient failure must not silently remove the action.
      const reasons = await Promise.all(
        list.map(async (attachment) => {
          try {
            return [attachment.id, await api.uploadEligibility(attachment.id)] as const;
          } catch {
            return [attachment.id, eligibilityFallback(attachment.upload_status)] as const;
          }
        }),
      );
      const next: Record<string, UploadEligibility> = {};
      for (const [id, reason] of reasons) {
        next[id] = reason;
      }
      setEligibility(next);
      setLoaded(true);
      setError(null);
    } catch (caught) {
      setError(describeError(caught));
    } finally {
      setReading(false);
    }
  }, [enabled]);

  useEffect(() => {
    void reload();
  }, [reload]);

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

  const add = () =>
    run(async () => {
      if (!sourcePath.trim()) {
        throw new Error('请填写要添加的文件的完整路径。');
      }
      const result = await api.addAttachment({
        sourcePath: sourcePath.trim(),
        storageTargetId: target.trim() || 'local',
        namespace: namespace.trim() || 'default',
      });
      setLastAdd(result);
      setSourcePath('');
    }, '已加入附件池。添加不会修改任何文章,请自己决定何时插入 Markdown。');

  const syncIndex = () =>
    run(async () => {
      await api.syncAttachmentIndex();
    }, '附件索引已同步。');

  // ---- The storage-target form (PROCESSING.md §5.5) ------------------------------------------------
  const kindOptions = useMemo(
    () => storageKindOptions(targets, kindReport),
    [targets, kindReport],
  );
  const chosenKind = kindOptions.find((option) => option.kind === normaliseKind(draft.kind));
  const noticeForKind = kindNotice(chosenKind);
  /** §5.3/§5.4's gaps, stated before the first save rather than after a target that cannot exist. */
  const gapSummary = kindGapSummary(kindOptions);
  const formFields = fieldsForKind(draft.kind);
  const hiddenNote = hiddenFieldsNote(hiddenFilled, draft.kind);

  /**
   * Ask the crate whether this draft may be saved, WITHOUT saving it.
   *
   * This is what replaced the window's copy of `StorageConfig::validate`. It is debounced rather than
   * synchronous because the answer comes from Rust, which is the whole point: one authority, one message, no
   * table comparing two hand-written copies. The trade is stated in `storageTargetForm.ts::validateDraft`.
   *
   * The reason is cleared first, so a stale refusal cannot keep blocking a field the user has just fixed; the
   * save path re-asks before writing anything, so a save is never sent on an unvalidated draft.
   */
  useEffect(() => {
    let live = true;
    setSaveReason(null);
    const handle = window.setTimeout(() => {
      void validateDraft(draft, api.validateStorageTarget).then((reason) => {
        if (live) setSaveReason(reason);
      });
    }, 250);
    return () => {
      live = false;
      window.clearTimeout(handle);
    };
  }, [draft]);

  const setField = (key: StorageFieldKey, value: string) => {
    setDirty(true);
    setDraft((current) => ({ ...current, [key]: value }));
  };

  const startEdit = (view: StorageTargetView) => {
    setDraft(draftFromTarget(view));
    setEditingId(view.id);
    setDirty(false);
    setHiddenFilled([]);
    setConfirmDelete(null);
  };

  const cancelEdit = () => {
    setDraft(emptyDraft());
    setEditingId(null);
    setDirty(false);
    setHiddenFilled([]);
  };

  const saveTarget = () =>
    run(async () => {
      // Asked AGAIN here, and not merely trusted from the debounced hint above: a save must never be sent on a
      // draft the crate has not accepted, and the crate's message is what the user sees either way.
      const reason = await validateDraft(draft, api.validateStorageTarget);
      if (reason) throw new Error(reason);
      // `buildSaveRequest` omits every blank field, so an untouched credential field leaves the stored one
      // alone (§12.2) and a value typed for another kind is not written into this target.
      await api.saveStorageTarget(buildSaveRequest(draft));
      setDraft(emptyDraft());
      setEditingId(null);
      setDirty(false);
      setHiddenFilled([]);
    }, '存储目标已保存。留空的凭据字段保持原值;新的凭据只写入操作系统凭据库(§12.2)。');

  const removeTarget = (id: string) =>
    run(async () => {
      await api.deleteStorageTarget(id);
      setConfirmDelete(null);
      if (editingId === id) cancelEdit();
    }, '存储目标已删除,它的凭据也一并从操作系统凭据库移除;已经上传的远程文件没有被删除(§13.8)。');

  if (!enabled) {
    return (
      <div className="page">
        <PageHeader
          title="附件"
          description="附件池、存储目标与远程对象。加入附件只写入缓存池,不会改动任何文章。"
        />
        <StateBlock
          tone="empty"
          title="还没有打开项目"
          description="打开一个项目之后才能管理附件:缓存池、存储目标和远程对象记录都写在这个项目里。"
        />
      </div>
    );
  }

  return (
    <div className="page">
      <PageHeader
        title="附件"
        description="附件池、存储目标与远程对象。加入附件只写入缓存池,不会改动任何文章——Markdown 由你决定何时插入。"
        actions={
          <>
            <button type="button" onClick={() => void reload()} disabled={busy || reading}>
              刷新
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => void syncIndex()}
              disabled={busy || reading}
            >
              同步索引
            </button>
          </>
        }
      />

      {!loaded && !error && (
        <StateBlock
          tone="loading"
          title="正在读取附件池…"
          description="附件、存储目标与远程对象三份列表会一起读取。"
        />
      )}

      {!loaded && error && (
        <StateBlock
          tone="error"
          title="读取附件池失败"
          description={error}
          actions={
            <button type="button" onClick={() => void reload()} disabled={reading}>
              重试
            </button>
          }
          onDismiss={() => setError(null)}
        />
      )}

      {loaded && (
        <>
          {error && (
            <StateBlock
              tone="error"
              title="操作没有完成"
              description={error}
              actions={
                <button type="button" onClick={() => void reload()} disabled={reading}>
                  重试读取
                </button>
              }
              onDismiss={() => setError(null)}
            />
          )}
          {notice && (
            <StateBlock
              tone="success"
              title="已完成"
              description={notice}
              onDismiss={() => setNotice(null)}
            />
          )}

          <Card
            title="添加文件"
            description="把本地文件复制进缓存池。这一步不会修改任何文章。"
          >
            <div className="form-grid">
              <Field
                label="文件路径"
                grow
                hint="完整路径;可以粘贴,也可以先在文件管理器里复制。"
              >
                <input
                  value={sourcePath}
                  onChange={(event) => setSourcePath(event.target.value)}
                  placeholder="D:/图片/photo.png"
                  spellCheck={false}
                />
              </Field>
              <Field
                label="存储目标"
                hint="写进附件记录的目标标识;上传时按这个标识和路径前缀分区去重(§13.5)。"
              >
                <input value={target} onChange={(event) => setTarget(event.target.value)} />
              </Field>
              <Field label="命名空间" hint="同一个目标下再分一层;留空时按 default 处理。">
                <input value={namespace} onChange={(event) => setNamespace(event.target.value)} />
              </Field>
            </div>

            <div className="card-foot">
              <button type="button" className="primary" onClick={() => void add()} disabled={busy}>
                加入附件池
              </button>
            </div>

            {lastAdd && (
              <StateBlock
                tone="success"
                inline
                title={
                  lastAdd.outcome === 'reused' ? '相同内容已存在,复用了原记录' : '已添加'
                }
                description={
                  <>
                    <code>{lastAdd.markdown}</code>
                    {onInsertMarkdown && (
                      <button type="button" onClick={() => onInsertMarkdown(lastAdd.markdown)}>
                        插入到文章
                      </button>
                    )}
                  </>
                }
              />
            )}

            <p className="card-note">
              缓存目录:<code>{cacheDirectory || '(未知)'}</code>
            </p>
          </Card>

          <Card
            title={`附件池(${attachments.length})`}
            description="上传、引用、缓存是三个独立的轴:它们各自失败,所以各自显示(§13.1)。"
          >
            {attachments.length === 0 ? (
              <StateBlock
                tone="empty"
                title="还没有附件"
                description="在上面的「添加文件」里填一个完整路径。附件会先留在缓存池里,直到你把它上传到某个存储目标。"
              />
            ) : (
              <ul className="row-list">
                {attachments.map((attachment) => {
                  // §13.2.2's reason, consulted before the attempt: a disabled button with no explanation is
                  // exactly what this reason exists to prevent.
                  const control = uploadControl({
                    eligibility: eligibility[attachment.id] ?? null,
                    storageTargetId: uploadTarget,
                  });
                  // §13.8's explicit cleanup, on the target the object's own record names.
                  const cleanup = remoteCleanup(attachment, objects, targets);
                  return (
                    <li
                      key={attachment.id}
                      className={`row${attachment.last_error ? ' row--failed' : ''}`}
                    >
                      <div className="row-main">
                        <p className="row-title" title={attachment.id}>
                          {attachment.original_name}
                        </p>
                        <p className="row-meta">
                          {formatBytes(attachment.size_bytes)} · 目标{' '}
                          <code>{attachment.storage_target_id}</code> · 命名空间{' '}
                          <code>{attachment.storage_namespace}</code>
                        </p>
                        {/* The three axes, each with its own label, because they fail independently. */}
                        <div className="row-axes">
                          {AXES.map((axis) => {
                            const value = String(attachment[axis.key]);
                            return (
                              <span key={axis.key} className={`axis axis-${value.toLowerCase()}`}>
                                {axis.label}:{axis.values[value] ?? value}
                              </span>
                            );
                          })}
                          <span
                            className={`axis${attachment.reference_count > 0 ? ' axis-succeeded' : ''}`}
                          >
                            被 {attachment.reference_count} 篇引用
                          </span>
                          {attachment.remote_url && (
                            <span className="axis axis-succeeded" title={attachment.remote_url}>
                              远程已上传
                            </span>
                          )}
                        </div>
                      </div>

                      <div className="row-actions">
                        <button
                          type="button"
                          className="primary"
                          onClick={() =>
                            void run(async () => {
                              if (!uploadTarget) throw new Error('请先选择一个存储目标。');
                              const result = await api.uploadAttachment(attachment.id, uploadTarget);
                              setNotice(
                                result.reused
                                  ? `相同内容已存在于目标,复用了远程对象:${result.remote_url}`
                                  : `已上传:${result.remote_url}`,
                              );
                            }, '上传完成。Markdown 已指向远程地址,插入后保存即可。')
                          }
                          disabled={busy || control.disabled}
                          title={control.reason ?? '上传到这个目标'}
                        >
                          上传
                        </button>
                        <button
                          type="button"
                          onClick={() =>
                            void api
                              .attachmentMarkdown(attachment.id)
                              .then((markdown) => {
                                if (onInsertMarkdown) onInsertMarkdown(markdown);
                                else void navigator.clipboard?.writeText(markdown);
                                setNotice('Markdown 已就绪。');
                              })
                              .catch((caught: unknown) => setError(describeError(caught)))
                          }
                          disabled={busy}
                        >
                          取 Markdown
                        </button>
                        <button
                          type="button"
                          onClick={() =>
                            void run(async () => {
                              // A referenced attachment is removed only after the user is told how many articles
                              // cite it, because section 13.2's reference set exists to make that consequence
                              // visible.
                              if (attachment.reference_count > 0) {
                                const confirmed = window.confirm(
                                  `这个附件被 ${attachment.reference_count} 篇文章引用,删除后这些引用会失效。继续?`,
                                );
                                if (!confirmed) return;
                              }
                              await api.removeAttachment(attachment.id);
                            }, '附件已删除,缓存文件一并移除;远程对象没有被删除(§13.8),要删除它请用“仅删除远程对象”。')
                          }
                          disabled={busy}
                          title="删除记录和缓存文件;远程对象不会被删除(§13.8)"
                        >
                          删除
                        </button>
                        {cleanup.storageTargetId !== null && (
                          <button
                            type="button"
                            className="danger"
                            onClick={() => {
                              const targetId = cleanup.storageTargetId;
                              if (
                                targetId === null ||
                                !window.confirm(`确认删除远程对象?\n\n${cleanup.wording}`)
                              ) {
                                return;
                              }
                              void run(async () => {
                                await api.deleteRemoteObject(attachment.id, targetId);
                              }, '远程对象已删除;文章的 Markdown 引用、附件记录和缓存文件都没有改动(§13.8)。');
                            }}
                            disabled={busy || cleanup.disabled}
                            title={cleanup.disabled ? cleanup.disabledReason ?? '' : cleanup.wording}
                          >
                            仅删除远程对象
                          </button>
                        )}
                      </div>

                      {/* THE fix this page exists to show: a failure is stated where the row is, not in a
                          tooltip and not only as a line at the top of the column. */}
                      {attachment.last_error && (
                        <StateBlock
                          tone="error"
                          inline
                          title="这个附件最近一次失败"
                          description={attachment.last_error}
                        />
                      )}

                      {control.reason && (
                        <StateBlock
                          tone="info"
                          inline
                          role="status"
                          title="现在不能上传"
                          description={control.reason}
                        />
                      )}

                      {cleanup.storageTargetId !== null && (
                        <p className={`row-hint${cleanup.disabled ? ' row-hint--warn' : ''}`}>
                          {cleanup.disabled ? cleanup.disabledReason : cleanup.wording}
                        </p>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
          </Card>

          <Card
            title="存储目标"
            description="附件上传到哪里去。没有适配器的类型也可以保存配置,但现在无法上传到它(§5.3/§5.4)。"
          >
            {targets.length === 0 ? (
              <StateBlock
                tone="empty"
                title="还没有存储目标"
                description="附件会一直留在缓存池里,直到你上传到某个目标。在下面的表单里新建一个。"
              />
            ) : (
              <ul className="row-list">
                {targets.map((view) => (
                  <li key={view.id} className="row">
                    <div className="row-main">
                      <p className="row-title">
                        {view.name}
                        <span className="axis">{view.kind}</span>
                        {!view.implemented && (
                          <span className="axis axis-failed">本构建无适配器</span>
                        )}
                        {view.credential_ref && (
                          <span className={`axis ${view.has_credential ? 'axis-succeeded' : 'axis-failed'}`}>
                            {view.has_credential ? '已保存凭据' : '缺少凭据'}
                          </span>
                        )}
                      </p>
                      {/* Every field the view carries is shown as it is, with no kind check: a value that is
                          set is a value the user should be able to see. A credential value has no field to
                          arrive in (§12). */}
                      <p className="row-meta">
                        <code>{view.id}</code>
                        {view.endpoint && <> · {view.endpoint}</>}
                        {view.region && <> · {view.region}</>}
                        {view.bucket && <> · Bucket:{view.bucket}</>}
                        {view.repository && (
                          <>
                            {' '}
                            · {view.repository}
                            {view.branch ? `@${view.branch}` : ''}
                          </>
                        )}
                        {view.path_prefix && <> · 前缀:{view.path_prefix}</>}
                        {view.public_url_prefix && <> · 公开地址前缀:{view.public_url_prefix}</>}
                      </p>
                    </div>

                    <div className="row-actions">
                      <button type="button" onClick={() => startEdit(view)} disabled={busy}>
                        修改
                      </button>
                      <button
                        type="button"
                        onClick={() => setConfirmDelete(confirmDelete === view.id ? null : view.id)}
                        disabled={busy}
                      >
                        删除
                      </button>
                    </div>

                    {!view.implemented && view.unimplemented_reason && (
                      <StateBlock
                        tone="warning"
                        inline
                        title="这个类型在本构建没有适配器"
                        description={view.unimplemented_reason}
                      />
                    )}

                    {confirmDelete === view.id && (
                      <div className="confirm">
                        {/* Deleting a target also removes its credential from the operating system store. This
                            is destructive and irreversible, so it is an explicit step rather than a
                            `confirm()` that a stray keypress could dismiss. */}
                        <p className="confirm-text" role="alert">
                          {deletionWarning(view)}
                        </p>
                        <div className="confirm-actions">
                          <button
                            type="button"
                            className="danger"
                            onClick={() => void removeTarget(view.id)}
                            disabled={busy}
                          >
                            确认删除 {view.id}
                          </button>
                          {/* Focused, so a repeated keypress dismisses the step instead of confirming it. */}
                          <button
                            type="button"
                            autoFocus
                            onClick={() => setConfirmDelete(null)}
                            disabled={busy}
                          >
                            取消
                          </button>
                        </div>
                      </div>
                    )}
                  </li>
                ))}
              </ul>
            )}

            <div className="form-grid form-grid--single">
              <Field
                label="上传到"
                hint="每个附件的「上传」按钮都写进这个目标。没有适配器的目标不能选,所以它不会变成一个注定失败的动作。"
              >
                <select
                  value={uploadTarget}
                  onChange={(event) => setUploadTarget(event.target.value)}
                >
                  <option value="">(选择目标)</option>
                  {targets.map((view) => (
                    <option key={view.id} value={view.id} disabled={!view.implemented}>
                      {view.name}
                      {view.implemented ? '' : '(无适配器)'}
                    </option>
                  ))}
                </select>
              </Field>
            </div>

            {objects.length > 0 && (
              <details className="disclosure">
                <summary>已上传 {objects.length} 个远程对象</summary>
                <ul className="plain-list">
                  {objects.map((object) => (
                    <li key={object.dedup_key}>
                      <code>{object.key}</code> — {object.url}
                      <span className="muted">目标:{object.storage_target_id}</span>
                    </li>
                  ))}
                </ul>
              </details>
            )}

            <div className="subform">
              <h3 className="subform-title">
                {editingId === null ? '新建存储目标' : `修改存储目标:${editingId}`}
              </h3>

              {/* The BUILD's own answer, before anything is saved: a kind with no adapter is reported here even
                  though a target list can say nothing about it (§5.3/§5.4). Kept out of JSX logic: the sentence
                  comes from `kindGapSummary`, which the tests drive. */}
              {gapSummary && <StateBlock tone="warning" role="status" description={gapSummary} />}

              <div className="form-grid">
                <Field
                  label="标识(id)"
                  hint={
                    editingId === null
                      ? '附件按这个标识分区去重;保存后再改会让同一个文件看起来像新的(§13.5)。'
                      : '标识已经写进附件记录,修改它会让已上传的对象看起来属于别的地方(§13.5),所以这里不能改。'
                  }
                >
                  <input
                    value={draft.id}
                    readOnly={editingId !== null}
                    placeholder="assets"
                    spellCheck={false}
                    onChange={(event) => {
                      setDirty(true);
                      setDraft((current) => ({ ...current, id: event.target.value }));
                    }}
                  />
                </Field>
                <Field label="名称" hint="显示用的名字,不影响任何路径或地址。">
                  <input
                    value={draft.name}
                    spellCheck={false}
                    onChange={(event) => {
                      setDirty(true);
                      setDraft((current) => ({ ...current, name: event.target.value }));
                    }}
                  />
                </Field>
                <Field label="类型" hint="换成别的类型时,已经填好但新类型不用的字段会留在表单里并明确告诉你。">
                  <select
                    value={normaliseKind(draft.kind)}
                    onChange={(event) => {
                      const change = changeKind(draft, event.target.value);
                      setDirty(true);
                      setDraft(change.draft);
                      setHiddenFilled(change.hiddenFilled);
                    }}
                  >
                    {kindOptions.map((option) => (
                      <option key={option.kind} value={option.kind}>
                        {option.implemented === false
                          ? `${option.label}(本构建无适配器)`
                          : option.label}
                      </option>
                    ))}
                  </select>
                </Field>
              </div>

              {noticeForKind && (
                <StateBlock
                  tone={noticeForKind.level === 'warn' ? 'warning' : 'info'}
                  description={noticeForKind.text}
                />
              )}

              {formFields.length > 0 && (
                <div className="form-grid">
                  {formFields.map((field) => (
                    <Field key={field.key} label={field.label} hint={field.hint}>
                      <input
                        type={field.secret ? 'password' : 'text'}
                        value={draft[field.key]}
                        placeholder={field.placeholder}
                        spellCheck={false}
                        autoComplete={field.secret ? 'off' : 'on'}
                        onChange={(event) => setField(field.key, event.target.value)}
                      />
                    </Field>
                  ))}
                </div>
              )}

              {hiddenNote && <StateBlock tone="info" description={hiddenNote} />}

              {dirty && saveReason && (
                <StateBlock tone="warning" description={`还差一步:${saveReason}`} />
              )}

              <div className="card-foot">
                <button
                  type="button"
                  className="primary"
                  onClick={() => void saveTarget()}
                  disabled={busy}
                >
                  {editingId === null ? '创建目标' : '保存修改'}
                </button>
                {editingId !== null && (
                  <button type="button" onClick={cancelEdit} disabled={busy}>
                    取消修改
                  </button>
                )}
              </div>

              <p className="card-note">
                留空一个凭据字段表示保留已保存的凭据;填写的凭据只写入操作系统的凭据库,不进入项目文件、日志或备份(§12.2)。
              </p>
            </div>
          </Card>

          {orphans && (orphans.safe.length > 0 || orphans.needs_confirmation.length > 0) && (
            <Card
              title="可以清理的附件"
              description="第 13.2.4 节把「可以安全删除」和「需要你确认」当作两件事;这里也分成两段,因为它们的处理方式不同。"
            >
              {orphans.safe.length > 0 && (
                <div className="stack">
                  <p className="card-note">
                    以下附件没有任何引用,且满足第 13.2.4 节的全部条件,可以安全删除:
                  </p>
                  <ul className="plain-list">
                    {orphans.safe.map((attachment) => (
                      <li key={attachment.id}>
                        <span>{attachment.original_name}</span>
                        <button
                          type="button"
                          onClick={() =>
                            void run(
                              () => api.removeAttachment(attachment.id).then(() => undefined),
                              '已删除。',
                            )
                          }
                          disabled={busy}
                        >
                          删除
                        </button>
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {orphans.needs_confirmation.length > 0 && (
                <div className="stack">
                  <p className="card-note">
                    以下附件需要你确认:它们没有引用,但不满足自动清理的全部条件(例如可能正在上传)。
                  </p>
                  <ul className="plain-list">
                    {orphans.needs_confirmation.map((attachment) => (
                      <li key={attachment.id}>
                        <span>{attachment.original_name}</span>
                        <span className="muted">上传状态:{attachment.upload_status}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </Card>
          )}
        </>
      )}
    </div>
  );
}
