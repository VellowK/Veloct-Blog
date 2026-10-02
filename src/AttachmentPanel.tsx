/**
 * The attachment pool.
 *
 * `handoff.md` §13 is a locked section, and three of its rules are visible in this component's shape rather
 * than merely honoured by it:
 *
 *  * **§13.1's three axes are shown separately.** An attachment can be cached but not uploaded, uploaded but
 *    not referenced by any document, and so on. A single "status" column would flatten three independent
 *    facts into one and make it impossible to tell "the picture is on the server but no article uses it" from
 *    "an article uses it but the upload failed" — which are different problems with different fixes.
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
 * test renderer on purpose, so a rule that lives in JSX is a rule nothing checks.
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
  const [busy, setBusy] = useState(false);
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
    } catch (caught) {
      setError(describeError(caught));
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
      <div className="attachment-panel">
        <p className="muted">打开一个项目后可以管理附件。</p>
      </div>
    );
  }

  return (
    <div className="attachment-panel">
      <header className="attachment-header">
        <strong>附件</strong>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新
        </button>
        <button type="button" onClick={() => void syncIndex()} disabled={busy}>
          同步索引
        </button>
      </header>

      {error && (
        <p className="attachment-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="attachment-notice">{notice}</p>}

      <section className="attachment-add">
        <h3>添加文件</h3>
        <div className="row">
          <label>
            <span>文件路径</span>
            <input
              value={sourcePath}
              onChange={(event) => setSourcePath(event.target.value)}
              placeholder="D:/图片/photo.png"
              spellCheck={false}
            />
          </label>
          <label>
            <span>存储目标</span>
            <input value={target} onChange={(event) => setTarget(event.target.value)} />
          </label>
          <label>
            <span>命名空间</span>
            <input value={namespace} onChange={(event) => setNamespace(event.target.value)} />
          </label>
        </div>
        <button type="button" onClick={() => void add()} disabled={busy}>
          加入附件池
        </button>
        {lastAdd && (
          <p className="attachment-result">
            {lastAdd.outcome === 'reused' ? '相同内容已存在,复用了原记录: ' : '已添加: '}
            <code>{lastAdd.markdown}</code>
            {onInsertMarkdown && (
              <button type="button" onClick={() => onInsertMarkdown(lastAdd.markdown)}>
                插入到文章
              </button>
            )}
          </p>
        )}
        <p className="muted hint">
          缓存目录:<code>{cacheDirectory || '(未知)'}</code>
        </p>
      </section>

      <section>
        <h3>附件池({attachments.length})</h3>
        {attachments.length === 0 && <p className="muted">还没有附件。</p>}
        <ul className="attachment-list">
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
              <li key={attachment.id}>
                <span className="name" title={attachment.id}>
                  {attachment.original_name}
                </span>
                <span className="muted">{formatBytes(attachment.size_bytes)}</span>
                {/* The three axes, each with its own label, because they fail independently. */}
                {AXES.map((axis) => {
                  const value = String(attachment[axis.key]);
                  return (
                    <span key={axis.key} className={`axis axis-${value.toLowerCase()}`}>
                      {axis.label}:{axis.values[value] ?? value}
                    </span>
                  );
                })}
                <span className={attachment.reference_count > 0 ? 'ok' : 'muted'}>
                  被 {attachment.reference_count} 篇引用
                </span>
                {attachment.remote_url && (
                  <span className="muted" title={attachment.remote_url}>
                    远程已上传
                  </span>
                )}
                {attachment.last_error && (
                  <span className="missing" title={attachment.last_error}>
                    最近错误
                  </span>
                )}
                <button
                  type="button"
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
                {control.reason && (
                  <span className="eligibility-reason" role="status">
                    现在不能上传:{control.reason}
                  </span>
                )}
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
                      // A referenced attachment is removed only after the user is told how many articles cite it,
                      // because section 13.2's reference set exists to make that consequence visible.
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
                  <>
                    <button
                      type="button"
                      onClick={() => {
                        const targetId = cleanup.storageTargetId;
                        if (targetId === null || !window.confirm(`确认删除远程对象?\n\n${cleanup.wording}`)) {
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
                    <span className="muted hint">
                      {cleanup.disabled ? cleanup.disabledReason : cleanup.wording}
                    </span>
                  </>
                )}
              </li>
            );
          })}
        </ul>
      </section>

      <section>
        <h3>存储目标</h3>
        {targets.length === 0 && (
          <p className="muted">
            还没有存储目标。附件会一直留在缓存池里,直到你上传到某个目标。
          </p>
        )}
        <ul className="storage-list">
          {targets.map((view) => (
            <li key={view.id}>
              <span className="name">{view.name}</span>
              <span className="muted">{view.kind}</span>
              <span className="muted">{view.id}</span>
              {/* Every field the view carries is shown as it is, with no kind check: a value that is set is a
                  value the user should be able to see. A credential value has no field to arrive in (§12). */}
              {view.endpoint && <span className="muted">{view.endpoint}</span>}
              {view.region && <span className="muted">{view.region}</span>}
              {view.bucket && <span className="muted">Bucket:{view.bucket}</span>}
              {view.repository && (
                <span className="muted">
                  {view.repository}
                  {view.branch ? `@${view.branch}` : ''}
                </span>
              )}
              {view.path_prefix && <span className="muted">前缀:{view.path_prefix}</span>}
              {view.public_url_prefix && (
                <span className="muted">公开地址前缀:{view.public_url_prefix}</span>
              )}
              {!view.implemented && <span className="warn">本构建无适配器</span>}
              {view.credential_ref && (
                <span className={view.has_credential ? 'ok' : 'bad'}>
                  {view.has_credential ? '已保存凭据' : '缺少凭据'}
                </span>
              )}
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
              {!view.implemented && view.unimplemented_reason && (
                <p className="muted hint">{view.unimplemented_reason}</p>
              )}
              {confirmDelete === view.id && (
                <div className="target-confirm">
                  {/* Deleting a target also removes its credential from the operating system store. This is
                      destructive and irreversible, so it is an explicit step rather than a `confirm()` that a
                      stray keypress could dismiss. */}
                  <p className="bad" role="alert">
                    {deletionWarning(view)}
                  </p>
                  <div className="row">
                    <button
                      type="button"
                      className="danger"
                      onClick={() => void removeTarget(view.id)}
                      disabled={busy}
                    >
                      确认删除 {view.id}
                    </button>
                    {/* Focused, so a repeated keypress dismisses the step instead of confirming it. */}
                    <button type="button" autoFocus onClick={() => setConfirmDelete(null)} disabled={busy}>
                      取消
                    </button>
                  </div>
                </div>
              )}
            </li>
          ))}
        </ul>
        <div className="row">
          <label>
            <span>上传到</span>
            <select value={uploadTarget} onChange={(event) => setUploadTarget(event.target.value)}>
              <option value="">(选择目标)</option>
              {targets.map((view) => (
                <option key={view.id} value={view.id} disabled={!view.implemented}>
                  {view.name}
                  {view.implemented ? '' : '(无适配器)'}
                </option>
              ))}
            </select>
          </label>
        </div>
        {objects.length > 0 && (
          <details>
            <summary>已上传 {objects.length} 个远程对象</summary>
            <ul className="skip-list">
              {objects.map((object) => (
                <li key={object.dedup_key}>
                  <code>{object.key}</code> — {object.url}
                  <span className="muted">目标:{object.storage_target_id}</span>
                </li>
              ))}
            </ul>
          </details>
        )}

        <h4>{editingId === null ? '新建存储目标' : `修改存储目标:${editingId}`}</h4>
        <div className="storage-form">
          {/* The BUILD's own answer, before anything is saved: a kind with no adapter is reported here even
              though a target list can say nothing about it (§5.3/§5.4). Kept out of JSX logic: the sentence
              comes from `kindGapSummary`, which the tests drive. */}
          {gapSummary && (
            <p className="kind-notice warn" role="status">
              {gapSummary}
            </p>
          )}
          <div className="row">
            <label>
              <span>标识(id)</span>
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
              <span className="field-hint">
                {editingId === null
                  ? '附件按这个标识分区去重;保存后再改会让同一个文件看起来像新的(§13.5)。'
                  : '标识已经写进附件记录,修改它会让已上传的对象看起来属于别的地方(§13.5),所以这里不能改。'}
              </span>
            </label>
            <label>
              <span>名称</span>
              <input
                value={draft.name}
                spellCheck={false}
                onChange={(event) => {
                  setDirty(true);
                  setDraft((current) => ({ ...current, name: event.target.value }));
                }}
              />
            </label>
            <label>
              <span>类型</span>
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
                    {option.implemented === false ? `${option.label}(本构建无适配器)` : option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>

          {noticeForKind && (
            <p className={`kind-notice ${noticeForKind.level}`} role={noticeForKind.level === 'warn' ? 'alert' : 'status'}>
              {noticeForKind.text}
            </p>
          )}

          {formFields.length > 0 && (
            <div className="row">
              {formFields.map((field) => (
                <label key={field.key}>
                  <span>{field.label}</span>
                  <input
                    type={field.secret ? 'password' : 'text'}
                    value={draft[field.key]}
                    placeholder={field.placeholder}
                    spellCheck={false}
                    autoComplete={field.secret ? 'off' : 'on'}
                    onChange={(event) => setField(field.key, event.target.value)}
                  />
                  <span className="field-hint">{field.hint}</span>
                </label>
              ))}
            </div>
          )}

          {hiddenNote && <p className="kind-notice info">{hiddenNote}</p>}

          <div className="row">
            <button type="button" onClick={() => void saveTarget()} disabled={busy}>
              {editingId === null ? '创建目标' : '保存修改'}
            </button>
            {editingId !== null && (
              <button type="button" onClick={cancelEdit} disabled={busy}>
                取消修改
              </button>
            )}
          </div>
          {dirty && saveReason && <p className="muted hint">还差一步:{saveReason}</p>}
          <p className="muted hint">
            留空一个凭据字段表示保留已保存的凭据;填写的凭据只写入操作系统的凭据库,不进入项目文件、日志或备份(§12.2)。
          </p>
        </div>
      </section>

      {orphans && (orphans.safe.length > 0 || orphans.needs_confirmation.length > 0) && (
        <section>
          <h3>可以清理的附件</h3>
          {orphans.safe.length > 0 && (
            <>
              <p className="muted">
                以下附件没有任何引用,且满足第 13.2.4 节的全部条件,可以安全删除:
              </p>
              <ul className="orphan-list safe">
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
            </>
          )}
          {orphans.needs_confirmation.length > 0 && (
            <>
              <p className="muted">
                以下附件需要你确认:它们没有引用,但不满足自动清理的全部条件(例如可能正在上传)。
              </p>
              <ul className="orphan-list confirm">
                {orphans.needs_confirmation.map((attachment) => (
                  <li key={attachment.id}>
                    <span>{attachment.original_name}</span>
                    <span className="muted">上传状态:{attachment.upload_status}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>
      )}
    </div>
  );
}
