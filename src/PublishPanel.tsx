/**
 * The publish panel.
 *
 * `handoff.md` §19.3 is the sequence; §14.4 makes the release a `release.json` plus a Git tag; §14.13 allows a
 * re-send of a failed sub-target and §14.14 a rollback. This panel is the window onto those facts, and it
 * shows them as FACTS rather than as a spinner:
 *
 *  * every release on disk, with its tag and whether that tag still RESOLVES. A tag that does not resolve is
 *    the one piece of release state a user cannot recover from by looking at the filesystem, so it is
 *    reported explicitly rather than implied by a green tick;
 *  * the publish result, including the reason it could not be anchored. §14.4's ordering writes the
 *    artefacts BEFORE the commit, so "built but not committed" is a real state — reporting it as a plain
 *    failure would hide that the files are already on disk;
 *  * the build log, because a failed Astro build is the most likely failure and its own output says why.
 *
 * The publish button is deliberately NOT optimistic. It shows the running state, then whatever the Rust side
 * reports, because a publish that silently half-succeeded would be worse than one that failed loudly.
 */

import { useCallback, useEffect, useState } from 'react';

import { api, describeError, type PublishView, type ReleaseView } from './lib/api';

export default function PublishPanel({
  enabled,
  onPublished,
}: {
  enabled: boolean;
  onPublished?: () => void;
}) {
  const [releases, setReleases] = useState<ReleaseView[]>([]);
  const [result, setResult] = useState<PublishView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showLog, setShowLog] = useState(false);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      setReleases(await api.listReleases());
    } catch (caught) {
      setError(describeError(caught));
    }
  }, [enabled]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const publish = useCallback(
    async (createdFrom?: string) => {
      setBusy(true);
      setError(null);
      setResult(null);
      try {
        const view = await api.publishProject(createdFrom);
        setResult(view);
        setShowLog(!view.anchored);
        await reload();
        onPublished?.();
      } catch (caught) {
        setError(describeError(caught));
      } finally {
        setBusy(false);
      }
    },
    [reload, onPublished],
  );

  if (!enabled) {
    return (
      <div className="publish-panel">
        <p className="muted">打开一个项目后可以发布。</p>
      </div>
    );
  }

  return (
    <div className="publish-panel">
      <header className="publish-header">
        <strong>发布</strong>
        <button type="button" className="primary" onClick={() => void publish()} disabled={busy}>
          {busy ? '构建中…' : '发布'}
        </button>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新
        </button>
      </header>

      {error && (
        <p className="publish-error" role="alert">
          {error}
        </p>
      )}

      {result && (
        <div className={`publish-result ${result.anchored ? 'ok' : 'unanchored'}`}>
          <p>
            <strong>{result.release_id}</strong>
            <span className="muted"> · {result.routes} 条路由</span>
          </p>
          {result.anchored ? (
            <p className="muted">
              已提交 {result.committed ? `${result.changed_files} 个文件` : '(无改动,直接打标签)'} ·
              tag <code>{result.tag_name}</code> · <code>{result.commit_sha?.slice(0, 8)}</code>
            </p>
          ) : (
            <p className="unanchored-note">
              <strong>已构建,但未能锚定到 Git。</strong> 发布产物已写入
              <code> .veloct/releases/</code>,但没有 commit 和 tag:
              {result.anchor_error}
            </p>
          )}
          {result.fingerprint && (
            <p className="muted fingerprint" title="发布指纹:输入的哈希,与运行无关">
              {result.fingerprint.slice(0, 16)}…
            </p>
          )}
          <button type="button" onClick={() => setShowLog((showing) => !showing)}>
            {showLog ? '隐藏构建输出' : '查看构建输出'}
          </button>
          {showLog && <pre className="build-log">{result.build_log || '(空)'}</pre>}
        </div>
      )}

      <h3>历史发布</h3>
      {releases.length === 0 && <p className="muted">还没有发布过。</p>}
      <ul className="release-list">
        {releases.map((release) => (
          <li key={release.release_id}>
            <span className="release-id">{release.release_id}</span>
            <span className="muted">{release.routes} 路由</span>
            {release.tag_name ? (
              <span className={release.tag_resolves ? 'ok' : 'missing'}>
                {/* A tag that no longer resolves is the one failure a user cannot see from the filesystem. */}
                {release.tag_resolves ? `tag ${release.tag_name}` : `tag 无法解析:${release.tag_name}`}
              </span>
            ) : (
              <span className="missing">未锚定</span>
            )}
            {release.created_from_release_id && (
              <span className="muted" title="由另一个发布重发或回滚而来">
                ← {release.created_from_release_id}
              </span>
            )}
            <button
              type="button"
              onClick={() => void publish(release.release_id)}
              disabled={busy}
              title="以这个发布为来源重新发布(handoff.md 第 14.13 与 14.14 节)"
            >
              重发 / 回滚到此
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
