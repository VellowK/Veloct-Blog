/**
 * Deployment targets.
 *
 * `handoff.md` §28 names the three logical targets and their main/works sub-targets, and §10.5 makes the works
 * sub-target a SEPARATE origin. Two distinctions this panel keeps visible rather than flattening:
 *
 *  * **Automatic versus manual.** EdgeOne uses the official Makers direct-upload protocol only on an explicit
 *    Deploy click. 1Panel remains a manual export; cloud acceptance alone is not deployment success.
 *  * **Succeeded, partial, failed, skipped.** §14.13 requires the failure reason to be shown, and a target whose
 *    works sub-target was skipped because it has no works origin is DEGRADED, not broken — showing that as a
 *    failure would send the user looking for a problem that does not exist.
 *
 * The statuses come from the Rust roll-up, so "partial" means exactly what `roll_up` means by it rather than a
 * second interpretation assembled here.
 */

import { useCallback, useEffect, useState } from 'react';
import { Feedback } from './components/Notifications';
import { Card, PageHeader, StateBlock } from './components/ui';

import {
  api,
  describeError,
  type DeployOutcomeView,
  type TargetListView,
  type TargetResultView,
} from './lib/api';

import { deployFormKind, deployOutcomeNotice, isEdgeOneKind, validateEdgeOneProjects } from './lib/deployForm';

const KIND_OPTIONS: Array<{ value: string; label: string }> = [
  { value: 'cloudflare_pages', label: 'Cloudflare Pages(自动)' },
  { value: 'edgeone_mainland', label: 'EdgeOne 中国大陆(自动直传)' },
  { value: 'edgeone_international', label: 'EdgeOne 国际(自动直传)' },
  { value: 'onepanel', label: '1Panel(手动上传)' },
];

const STATUS_LABEL: Record<string, string> = {
  succeeded: '成功',
  partial: '部分成功',
  failed: '失败',
  skipped: '已跳过',
};

const SUB_STATUS_LABEL: Record<string, string> = {
  succeeded: '成功',
  failed: '失败',
  skipped: '跳过',
  pending: '进行中',
};

function statusClass(status: string): string {
  switch (status) {
    case 'succeeded':
      return 'ok';
    case 'partial':
      return 'warn';
    case 'failed':
      return 'bad';
    default:
      return 'muted';
  }
}

function TargetResult({ result }: { result: TargetResultView }) {
  return (
    <li className={statusClass(result.status)}>
      <strong>{result.kind || result.target_id}</strong>
      <span className={statusClass(result.status)}>{STATUS_LABEL[result.status] ?? result.status}</span>
      <span className="sub">
        主站:{SUB_STATUS_LABEL[result.main_status] ?? result.main_status}
        {result.main_detail && <em title={result.main_detail}> · {result.main_detail}</em>}
      </span>
      {result.works_status && (
        <span className="sub">
          作品:{SUB_STATUS_LABEL[result.works_status] ?? result.works_status}
          {result.works_detail && <em title={result.works_detail}> · {result.works_detail}</em>}
        </span>
      )}
      {result.works_origin && <span className="muted">{result.works_origin}</span>}
    </li>
  );
}

export default function DeployPanel({ enabled }: { enabled: boolean }) {
  const [targets, setTargets] = useState<TargetListView | null>(null);
  const [outcome, setOutcome] = useState<DeployOutcomeView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [id, setId] = useState('');
  const [kind, setKind] = useState('cloudflare_pages');
  const [projectName, setProjectName] = useState('');
  const [worksProject, setWorksProject] = useState('');
  const [worksOrigin, setWorksOrigin] = useState('');
  const [credential, setCredential] = useState('');

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      setTargets(await api.deployTargets());
    } catch (caught) {
      setError(describeError(caught));
    }
  }, [enabled]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = useCallback(
    async (action: () => Promise<void | string>, done: string) => {
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        const result = await action();
        await reload();
        setNotice(result ?? done);
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
      <div className="deploy-panel admin-panel">
        <p className="muted">打开一个项目后可以配置部署目标。</p>
      </div>
    );
  }

  return (
    <div className="deploy-panel admin-panel">
      <PageHeader level={2} title="部署" description="将已构建的发布推送到启用的目标。保存配置、发布和构建均不会自动上传。" actions={<>
        <label className="check-field">
          <input
            type="checkbox"
            checked={targets?.enabled ?? true}
            disabled={busy}
            onChange={(event) =>
              void run(
                () => api.setDeployEnabled(event.target.checked),
                event.target.checked ? '已开启部署。' : '已关闭部署：点击部署也不会推送。发布和构建始终不会自动上传。',
              )
            }
          />
          <span>启用部署</span>
        </label>
        <button
          type="button"
          className="primary"
          onClick={() =>
            void run(async () => {
              // A deploy does NOT build: it pushes the existing dist, so section 14.13's fingerprint still
              // describes the bytes being pushed.
              setOutcome(null);
              const result = await api.deployNow();
              setOutcome(result);
              return deployOutcomeNotice(result);
            }, '')
          }
          disabled={busy}
        >
          {busy ? '推送中…' : '部署当前构建'}
        </button>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新
        </button>
      </>} />

      <Feedback message={error} tone="error" />
      <Feedback message={notice} tone="success" />

      {outcome && (
        <Card className="deploy-result" title={`部署结果 · ${outcome.release_id}`} description={`推送 ${outcome.pushed_files} 个文件。各目标结果独立保留，便于检查与重试。`}>
          {!outcome.all_succeeded && outcome.needing_attention.length === 0 && (
            <p className="muted">
              没有任何目标被推送(可能未配置目标或已关闭部署)。
            </p>
          )}
          <ul className="target-results">
            {outcome.targets.map((result) => (
              <TargetResult key={result.target_id} result={result} />
            ))}
          </ul>
          {outcome.needing_attention.length > 0 && (
            <p className="warn">
              需要重发的目标:{outcome.needing_attention.join('、')}
            </p>
          )}
          <p className="muted">
            {/* Section 15.2: the SEO push is a separate task and its state is its own. */}
            SEO 推送:{outcome.seo_state === 'not_requested' ? '未请求' : outcome.seo_state}
          </p>
        </Card>
      )}

      <Card title={`已配置的目标 (${targets?.targets.length ?? 0})`} description="每个目标独立记录凭据与推送结果。">
        {(targets?.targets.length ?? 0) === 0 && (
          <StateBlock tone="empty" title="还没有配置任何部署目标" description="添加目标后，仍需点击“部署当前构建”才会上传。" />
        )}
        <ul className="target-list">
          {targets?.targets.map((target) => (
            <li key={target.id}>
              <span className="target-id">{target.id}</span>
              <span>{target.kind_label}</span>
              {target.manual && <span className="manual-badge">手动</span>}
              {target.works_origin && <span className="muted">{target.works_origin}</span>}
              {!target.manual && (
                <span className={target.has_credential ? 'ok' : 'bad'}>
                  {target.has_credential ? '已保存凭据' : '缺少凭据'}
                </span>
              )}
              <button type="button" disabled={busy} onClick={() => {
                setId(target.id);
                setKind(deployFormKind(target.kind));
                setProjectName(target.project ?? '');
                setWorksProject(target.works_project ?? '');
                setWorksOrigin(target.works_origin ?? '');
                setCredential('');
                setNotice('已载入目标。凭据不会回显；留空保留已保存的 Token。');
              }}>编辑</button>
              <button
                type="button"
                onClick={() =>
                  void run(() => api.deleteDeployTarget(target.id), '目标已删除,凭据一并移除。')
                }
                disabled={busy}
              >
                删除
              </button>
            </li>
          ))}
        </ul>
        <p className="muted hint">
          仅点击「部署当前构建」才上传：覆盖当前发布构建的全部主站文件、独立作品站文件，以及全部已启用目标。
          不上传源码、密钥或本地缓存。发布、构建和保存目标不会自动部署。1Panel 仍为手动导出。
        </p>
      </Card>

      <Card title="添加 / 更新目标" description="凭据仅保存到操作系统凭据库，留空保留已有凭据。">
        <div className="row">
          <label>
            <span>id</span>
            <input value={id} onChange={(event) => setId(event.target.value)} />
          </label>
          <label>
            <span>类型</span>
            <select value={kind} onChange={(event) => setKind(event.target.value)}>
              {KIND_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        {isEdgeOneKind(kind) && <p className="muted hint">
          在所选区域的 EdgeOne Pages 控制台创建「直接上传」项目，填写项目 ID（不是名称或网址）和该区域的 API Token。
          不需要腾讯云 SecretId / SecretKey。主站和作品站需使用不同项目；国际与国内 Token 不会自动互换。
          先发布/构建，再手动点击部署；后台会等待云端确认，未确认不会报成功。
        </p>}
        <div className="row">
          <label>
            <span>{isEdgeOneKind(kind) ? '主站项目 ID' : '主站项目 / 账号'}</span>
            <input
              value={projectName}
              onChange={(event) => setProjectName(event.target.value)}
              placeholder={isEdgeOneKind(kind) ? 'EdgeOne 主站 ProjectId' : 'Cloudflare: 账号 id'}
            />
          </label>
          <label>
            <span>{isEdgeOneKind(kind) ? '作品站项目 ID（独立项目；留空则跳过）' : '作品项目(留空则跳过作品子目标)'}</span>
            <input
              value={worksProject}
              onChange={(event) => setWorksProject(event.target.value)}
            />
          </label>
        </div>
        <div className="row">
          <label>
            <span>作品源(独立 origin)</span>
            <input
              value={worksOrigin}
              onChange={(event) => setWorksOrigin(event.target.value)}
              placeholder="https://works.example.com"
            />
          </label>
          <label>
            <span>{isEdgeOneKind(kind) ? 'EdgeOne API Token（留空保留原凭据）' : '凭据(留空表示不修改已保存的凭据)'}</span>
            <input
              type="password"
              value={credential}
              onChange={(event) => setCredential(event.target.value)}
              autoComplete="off"
            />
          </label>
        </div>
        <button
          type="button"
          onClick={() =>
            void run(async () => {
              if (!id.trim()) throw new Error('目标需要一个 id。');
              if (isEdgeOneKind(kind)) {
                const invalid = validateEdgeOneProjects(projectName, worksProject);
                if (invalid) throw new Error(invalid);
              }
              await api.saveDeployTarget({
                id: id.trim(),
                kind,
                enabled: true,
                project: projectName.trim() || undefined,
                worksProject: worksProject.trim() || undefined,
                worksOrigin: worksOrigin.trim() || undefined,
                credential: credential.trim() || undefined,
              });
              setCredential('');
            }, '目标已保存。凭据存放在系统凭据库中,不会写入配置文件。')
          }
          disabled={busy}
        >
          保存目标
        </button>
      </Card>
    </div>
  );
}
