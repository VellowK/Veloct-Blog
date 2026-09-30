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
  type CloudBackupResultView,
  type CloudBackupRunView,
  type CloudBackupTargetView,
  type RestoreAppliedView,
  type StagedRestoreView,
} from './lib/api';

type Mode = 'full' | 'content_only';

const MODE_LABEL: Record<Mode, string> = {
  full: '完整恢复(内容、配置、作品)',
  content_only: '仅内容恢复(保留配置与凭据)',
};

/** How the per-target results are drawn (section 17.7). */
function targetMark(target: { succeeded: boolean; skipped: boolean }): string {
  if (target.succeeded) return '✅';
  return target.skipped ? '⏭' : '❌';
}

/**
 * A cloud run's per-target report (handoff.md section 17.7).
 *
 * Three states rather than two: a target that was deliberately not attempted has nothing to retry, and
 * drawing it with the same mark as one that broke would send the user looking for a fault that is not
 * there.
 */
function CloudRunReport({ run }: { run: CloudBackupRunView }) {
  return (
    <div className="backup-result">
      <p>{run.summary}</p>
      <ul className="archive-list">
        {run.targets.map((target) => (
          <li key={target.target_id}>
            <span className="archive-name">
              {targetMark(target)} {target.target_id}
            </span>
            {target.succeeded && <span className="muted">该目标现保留 {target.kept} 份</span>}
            {!target.succeeded && target.error && <span className="muted">{target.error}</span>}
            {target.skipped && <span className="muted">未执行(无需重试)</span>}
          </li>
        ))}
      </ul>
      {!run.all_succeeded && run.nothing_stored && (
        <p className="restore-note">没有任何云端目标保存了本次备份;本地备份仍然有效。</p>
      )}
      {run.index_note && <p className="restore-note">{run.index_note}</p>}
    </div>
  );
}

/** An empty form for a new cloud target (section 17.4). */
const EMPTY_TARGET = {
  id: '',
  kind: 'github' as 'github' | 's3',
  location: '',
  prefix: '',
  retention: 3,
  privateConfirmed: false,
  credential: '',
  // S3 only. The access key ID is configuration; the SECRET access key goes in `credential`, and
  // section 12 is why the two are asked for separately.
  accessKey: '',
  endpoint: '',
  region: '',
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

  // The cloud backup targets (handoff.md section 17.4).
  const [cloudTargets, setCloudTargets] = useState<CloudBackupTargetView[]>([]);
  const [targetForm, setTargetForm] = useState({ ...EMPTY_TARGET });
  const [selectedTargets, setSelectedTargets] = useState<string[]>([]);
  const [cloudResult, setCloudResult] = useState<CloudBackupResultView | null>(null);
  const [cloudRun, setCloudRun] = useState<CloudBackupRunView | null>(null);
  const [connectionReport, setConnectionReport] = useState<Record<string, string>>({});
  const [acknowledged, setAcknowledged] = useState(false);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      const [list, dir, targets] = await Promise.all([
        api.listBackups(),
        api.backupDirectory(),
        api.cloudBackupTargets(),
      ]);
      setArchives(list);
      setDirectory(dir);
      setExportDirectory((current) => current || dir);
      setCloudTargets(targets);
      // A target that was just removed must not stay selected, or the next run would skip it silently.
      setSelectedTargets((current) =>
        current.filter((id) => targets.some((target) => target.id === id)),
      );
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

      <section className="cloud-backup">
        <h3>云端备份目标({cloudTargets.length}/3 默认保留)</h3>
        <p className="muted hint">
          handoff.md §17.4 只支持 GitHub 与 S3/S3-compatible 两种云端目标;§17.2 要求目标必须是<strong>私有</strong>的,
          因为备份包未加密。凭据保存在操作系统凭据库,不进入 SQLite、日志或备份包(§12)。
        </p>

        {cloudTargets.length === 0 && <p className="muted">还没有配置云端目标,备份只写在本机。</p>}
        <ul className="archive-list">
          {cloudTargets.map((target) => (
            <li key={target.id}>
              <label className="cloud-pick">
                <input
                  type="checkbox"
                  checked={selectedTargets.includes(target.id)}
                  onChange={(event) =>
                    setSelectedTargets((current) =>
                      event.target.checked
                        ? [...current, target.id]
                        : current.filter((id) => id !== target.id),
                    )
                  }
                />
                <span className="archive-name">{target.id}</span>
              </label>
              <span className="muted">{target.label}</span>
              <span className="muted">{target.location}</span>
              {target.access_key && <span className="muted">ID {target.access_key}</span>}
              {target.endpoint && <span className="muted">{target.endpoint}</span>}
              {target.kind === 's3' && target.region && (
                <span className="muted">{target.region}</span>
              )}
              <span className="muted">保留 {target.retention} 份</span>
              {!target.has_credential && <span className="missing">缺少凭据</span>}
              {!target.implemented && <span className="missing">本构建无适配器</span>}
              {target.refusal_reason && <span className="missing">会被拒绝</span>}
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    const said = await api.testCloudBackupTarget(target.id);
                    setConnectionReport((current) => ({ ...current, [target.id]: said }));
                  }, '连接测试已完成。')
                }
              >
                测试连接
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await api.deleteCloudBackupTarget(target.id);
                    setConnectionReport((current) =>
                      Object.fromEntries(
                        Object.entries(current).filter(([key]) => key !== target.id),
                      ),
                    );
                  }, '目标已删除(它的历史备份记录会保留)。')
                }
              >
                删除
              </button>
              {target.unimplemented_reason && (
                <p className="muted hint">{target.unimplemented_reason}</p>
              )}
              {target.refusal_reason && <p className="muted hint">{target.refusal_reason}</p>}
              {connectionReport[target.id] && (
                <p className="muted hint">{connectionReport[target.id]}</p>
              )}
            </li>
          ))}
        </ul>

        <h4>添加 / 修改目标</h4>
        <div className="row">
          <label>
            <span>标识</span>
            <input
              value={targetForm.id}
              spellCheck={false}
              onChange={(event) => setTargetForm({ ...targetForm, id: event.target.value })}
            />
          </label>
          <label>
            <span>类型</span>
            <select
              value={targetForm.kind}
              onChange={(event) =>
                setTargetForm({ ...targetForm, kind: event.target.value as 'github' | 's3' })
              }
            >
              <option value="github">GitHub(私有仓库)</option>
              <option value="s3">S3 / S3-compatible</option>
            </select>
          </label>
          <label>
            <span>{targetForm.kind === 'github' ? 'owner/repo' : 'Bucket'}</span>
            <input
              value={targetForm.location}
              spellCheck={false}
              onChange={(event) => setTargetForm({ ...targetForm, location: event.target.value })}
            />
          </label>
          <label>
            <span>路径前缀</span>
            <input
              value={targetForm.prefix}
              spellCheck={false}
              onChange={(event) => setTargetForm({ ...targetForm, prefix: event.target.value })}
            />
          </label>
          <label>
            <span>保留份数</span>
            <input
              type="number"
              min={1}
              value={targetForm.retention}
              onChange={(event) =>
                setTargetForm({ ...targetForm, retention: Number(event.target.value) || 3 })
              }
            />
          </label>
        </div>
        {targetForm.kind === 's3' && (
          <div className="row">
            <label>
              <span>Access Key ID(标识符,可存配置)</span>
              <input
                value={targetForm.accessKey}
                spellCheck={false}
                onChange={(event) => setTargetForm({ ...targetForm, accessKey: event.target.value })}
              />
            </label>
            <label>
              <span>Endpoint(留空则用 AWS)</span>
              <input
                value={targetForm.endpoint}
                spellCheck={false}
                placeholder="http://127.0.0.1:9000"
                onChange={(event) => setTargetForm({ ...targetForm, endpoint: event.target.value })}
              />
            </label>
            <label>
              <span>Region</span>
              <input
                value={targetForm.region}
                spellCheck={false}
                placeholder="us-east-1"
                onChange={(event) => setTargetForm({ ...targetForm, region: event.target.value })}
              />
            </label>
          </div>
        )}
        <div className="row">
          <label>
            <span>
              {targetForm.kind === 's3'
                ? 'Secret Access Key(仅写入系统凭据库)'
                : 'GitHub Token(仅写入系统凭据库)'}
            </span>
            <input
              type="password"
              value={targetForm.credential}
              autoComplete="off"
              onChange={(event) => setTargetForm({ ...targetForm, credential: event.target.value })}
            />
          </label>
          <label className="cloud-confirm">
            <input
              type="checkbox"
              checked={targetForm.privateConfirmed}
              onChange={(event) =>
                setTargetForm({ ...targetForm, privateConfirmed: event.target.checked })
              }
            />
            <span>我确认该目标是私有仓库 / 私有 bucket(§17.2 必需)</span>
          </label>
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await api.saveCloudBackupTarget({
                  id: targetForm.id.trim(),
                  kind: targetForm.kind,
                  location: targetForm.location.trim(),
                  prefix: targetForm.prefix.trim() || null,
                  retention: targetForm.retention,
                  privateConfirmed: targetForm.privateConfirmed,
                  credential: targetForm.credential.trim() || null,
                  accessKey: targetForm.accessKey.trim() || null,
                  endpoint: targetForm.endpoint.trim() || null,
                  region: targetForm.region.trim() || null,
                });
                setTargetForm({ ...EMPTY_TARGET });
              }, '目标已保存。')
            }
          >
            保存目标
          </button>
        </div>

        <div className="row">
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                setCloudRun(null);
                setAcknowledged(false);
                setCloudResult(
                  await api.backupToCloud({
                    // An empty selection means "every enabled target", which is what an automatic backup
                    // does; a non-empty one is section 17.8's temporary subset.
                    targetIds: selectedTargets.length > 0 ? selectedTargets : null,
                    acknowledgedFindings: acknowledged,
                  }),
                );
              }, '备份已完成。')
            }
          >
            备份并上传到云端
          </button>
          <label className="cloud-confirm">
            <input
              type="checkbox"
              checked={acknowledged}
              onChange={(event) => setAcknowledged(event.target.checked)}
            />
            <span>我已查看 Secret 扫描结果,仍要上传本次备份(§17.3)</span>
          </label>
        </div>
        <p className="muted hint">
          {selectedTargets.length > 0
            ? `本次只上传勾选的 ${selectedTargets.length} 个目标(§17.8)。`
            : '未勾选任何目标:本次会上传全部已启用目标。'}
        </p>

        {cloudResult?.blocked_by_finding && (
          <div className="secret-block" role="alert">
            <p>{cloudResult.blocked_reason}</p>
            <p className="muted">
              确认后可以重新执行“备份并上传到云端”,或对下面已有的备份使用“重发到云端”。
            </p>
          </div>
        )}

        {cloudResult?.run && <CloudRunReport run={cloudResult.run} />}
        {cloudRun && <CloudRunReport run={cloudRun} />}
      </section>

      <section>
        <h3>历史备份({archives.length})</h3>        {archives.length === 0 && <p className="muted">还没有备份。</p>}
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
              <button
                type="button"
                onClick={() =>
                  void run(async () => {
                    // Section 17.7's retry path: an archive that already exists is re-sent rather than
                    // taken again, so a target that was unreachable does not cost a second backup.
                    setCloudResult(null);
                    setCloudRun(
                      await api.resendBackupToCloud({
                        archivePath: archive.path,
                        targetIds: selectedTargets.length > 0 ? selectedTargets : null,
                      }),
                    );
                  }, '已重新发送到云端目标。')
                }
                disabled={busy || cloudTargets.length === 0}
              >
                重发到云端
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
