/**
 * Backup and restore.
 *
 * `handoff.md` §17 is locked, and the two-step shape of this panel is the contract rather than a design
 * preference:
 *
 *  * **§17.10: a restore is staged, verified and CONFIRMED before it replaces anything.** The plan is shown
 *    first — which files will be replaced, which will be left alone with reasons, and whether a publish is
 *    needed afterwards — and only then is the confirm button enabled. A single button that restored would make
 *    the confirmation a formality.
 *  * **§17.3: the secret scan runs before the archive is written, and a finding blocks the UPLOAD without
 *    failing the backup.** A finding is reported by PATH and KIND; the value is never shown, because printing
 *    it in a dialog would be the same mistake as writing it into the archive.
 *
 * The restore result names the credential HANDLES that must be rebound (§17.10) and never a value, matching
 * how the rest of the application treats credentials.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  api,
  describeError,
  type BackupArchiveView,
  type BackupOutcomeView,
  type RestoreAppliedView,
  type StagedRestoreView,
} from './lib/api';

type Mode = 'full' | 'content_only';

const MODE_LABEL: Record<Mode, string> = {
  full: '完整恢复(内容、配置、作品)',
  content_only: '仅内容恢复(保留配置与凭据)',
};

export default function BackupPanel({ enabled }: { enabled: boolean }) {
  const [archives, setArchives] = useState<BackupArchiveView[]>([]);
  const [directory, setDirectory] = useState('');
  const [exportDirectory, setExportDirectory] = useState('');
  const [lastBackup, setLastBackup] = useState<BackupOutcomeView | null>(null);
  const [staged, setStaged] = useState<StagedRestoreView | null>(null);
  const [mode, setMode] = useState<Mode>('full');
  const [applied, setApplied] = useState<RestoreAppliedView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      const [list, dir] = await Promise.all([api.listBackups(), api.backupDirectory()]);
      setArchives(list);
      setDirectory(dir);
      setExportDirectory((current) => current || dir);
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

  if (!enabled) {
    return (
      <div className="backup-panel">
        <p className="muted">打开一个项目后可以备份。</p>
      </div>
    );
  }

  return (
    <div className="backup-panel">
      <header className="backup-header">
        <strong>备份与恢复</strong>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新
        </button>
      </header>

      {error && (
        <p className="backup-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="backup-notice">{notice}</p>}

      <section>
        <h3>立即备份</h3>
        <div className="row">
          <button
            type="button"
            onClick={() =>
              void run(async () => {
                setStaged(null);
                setApplied(null);
                setLastBackup(await api.manualBackup());
              }, '备份已写入项目自己的备份目录。')
            }
            disabled={busy}
          >
            手动备份
          </button>
          <label>
            <span>导出到</span>
            <input
              value={exportDirectory}
              onChange={(event) => setExportDirectory(event.target.value)}
              spellCheck={false}
            />
          </label>
          <button
            type="button"
            onClick={() =>
              void run(async () => {
                setLastBackup(await api.exportBackup(exportDirectory.trim()));
              }, '已导出到指定目录。')
            }
            disabled={busy}
          >
            导出
          </button>
          <button
            type="button"
            onClick={() => void run(async () => {
              const removed = await api.pruneBackups();
              setNotice(`已按保留策略删除 ${removed} 个旧备份。`);
            }, '保留策略已应用。')}
            disabled={busy}
          >
            应用保留策略
          </button>
        </div>
        <p className="muted hint">
          备份目录:<code>{directory || '(未知)'}</code>
        </p>

        {lastBackup && (
          <div className="backup-result">
            <p>
              <strong>{lastBackup.file_count}</strong> 个文件 ·{' '}
              {(lastBackup.archive_bytes / 1024).toFixed(1)} KB · 指纹{' '}
              <code>{lastBackup.archive_sha256.slice(0, 12)}…</code>
            </p>
            {lastBackup.secret_findings.length > 0 ? (
              <div className="secret-block" role="alert">
                <p>
                  <strong>发现 {lastBackup.secret_findings.length} 处疑似凭据。</strong>
                  本地备份已写入,但自动上传会被阻止,直到你确认。
                </p>
                <ul>
                  {lastBackup.secret_findings.map((finding) => (
                    // The finding names the path and the kind. The VALUE is deliberately not shown: printing
                    // it here would be the same mistake as writing it into the archive.
                    <li key={finding}>{finding}</li>
                  ))}
                </ul>
              </div>
            ) : (
              <p className="muted">
                未发现凭据。{lastBackup.history_note ? `(${lastBackup.history_note})` : 'Git 历史也已扫描。'}
              </p>
            )}
            {lastBackup.skipped.length > 0 && (
              <details>
                <summary>{lastBackup.skipped.length} 个路径未包含(点击查看原因)</summary>
                <ul className="skip-list">
                  {lastBackup.skipped.map((skip) => (
                    <li key={skip.path}>
                      <code>{skip.path}</code> — {skip.reason}
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </div>
        )}
      </section>

      <section>
        <h3>历史备份({archives.length})</h3>
        {archives.length === 0 && <p className="muted">还没有备份。</p>}
        <ul className="archive-list">
          {archives.map((archive) => (
            <li key={archive.path}>
              <span className="archive-name">{archive.file_name}</span>
              <span className="muted">{archive.file_count} 文件</span>
              <span className="muted">{(archive.bytes / 1024).toFixed(1)} KB</span>
              <span className="muted">{archive.trigger}</span>
              {archive.secret_findings > 0 && (
                <span className="missing">{archive.secret_findings} 处凭据</span>
              )}
              <button
                type="button"
                onClick={() =>
                  void run(async () => {
                    setApplied(null);
                    // STAGE first. Nothing is replaced until the user confirms, which is the whole point of
                    // section 17.10's ordering.
                    setStaged(await api.stageRestore(archive.path, mode));
                  }, '恢复已暂存并通过校验。请检查下面的计划,然后确认。')
                }
                disabled={busy}
              >
                暂存恢复
              </button>
            </li>
          ))}
        </ul>
      </section>

      {staged && (
        <section className="restore-plan">
          <h3>恢复计划(尚未改动任何文件)</h3>
          <label>
            <span>恢复模式</span>
            <select value={mode} onChange={(event) => setMode(event.target.value as Mode)}>
              {(['full', 'content_only'] as Mode[]).map((option) => (
                <option key={option} value={option}>
                  {MODE_LABEL[option]}
                </option>
              ))}
            </select>
          </label>
          <p>{staged.plan.summary}</p>
          <p className="muted">
            将替换 {staged.plan.file_count} 个文件;保留 {staged.plan.will_preserve.length} 个。
          </p>
          {staged.plan.needs_publish_afterwards && (
            <p className="restore-note">
              恢复后需要重新发布,线上站点不会自动更新。
            </p>
          )}
          {staged.plan.credentials_to_rebind.length > 0 && (
            <p className="restore-note">
              以下凭据需要重新绑定(仅显示句柄,不显示值):
              {staged.plan.credentials_to_rebind.map((handle) => (
                <code key={handle}>{handle}</code>
              ))}
            </p>
          )}
          <details>
            <summary>保留的路径与原因</summary>
            <ul className="skip-list">
              {staged.plan.will_preserve.map((preserved) => (
                <li key={preserved.path}>
                  <code>{preserved.path}</code> — {preserved.reason}
                </li>
              ))}
            </ul>
          </details>
          <div className="row">
            <button type="button" onClick={() => setStaged(null)} disabled={busy}>
              取消
            </button>
            <button
              type="button"
              className="primary"
              onClick={() =>
                void run(async () => {
                  const result = await api.confirmRestore(
                    staged.plan.archive_path,
                    staged.plan.mode,
                  );
                  setApplied(result);
                  setStaged(null);
                }, '恢复已完成。')
              }
              disabled={busy}
            >
              确认恢复(会替换文件)
            </button>
          </div>
        </section>
      )}

      {applied && (
        <section className="restore-applied">
          <h3>恢复结果</h3>
          <p>{applied.summary}</p>
          <p className="muted">
            写入 {applied.files_written} 个文件;保留 {applied.preserved.length} 个。
          </p>
          {applied.working_tree_changed && (
            <p className="restore-note">工作区已变更,需要重新发布。</p>
          )}
          {applied.needs_index_rebuild && (
            <p className="restore-note">内容索引需要重建。</p>
          )}
        </section>
      )}
    </div>
  );
}
