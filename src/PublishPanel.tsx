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
 *
 * ## The site's dependencies, and why they are on THIS page
 *
 * A build of a project whose `site/` has no dependencies fails with "no Astro CLI in
 * `<project>/site/node_modules/astro`; run the site's dependency install first (npm install in site/)".
 * The message is correct and actionable and still leaves the user to do it in a console, so the ONE-CLICK
 * install is rendered HERE — directly under the publish control, in the same column, where the error
 * appears — rather than in a settings page nobody would look at.
 *
 * The install's rules are in `src/lib/siteInstall.ts` (pure, and covered by
 * `tests/siteInstallForm.test.ts`); this file renders them, polls while an install runs, and offers the
 * cancel that kills npm's whole tree.
 */

import { useCallback, useEffect, useState } from 'react';

import { api, describeError, type PublishView, type ReleaseView, type SiteInstallView } from './lib/api';
import { StateBlock } from './components/ui';
import {
  describeSiteInstall,
  npmSourceLabel,
  publishBlockedReason,
  siteInstallStateLabel,
} from './lib/siteInstall';

/** How often the panel asks about a running install. Fast enough to look alive, slow enough to be free. */
const INSTALL_POLL_MS = 800;

export default function PublishPanel({
  enabled,
  projectPath,
  onPublished,
}: {
  enabled: boolean;
  /**
   * The open project's path, or `null`.
   *
   * Only used to know when the project CHANGED: the Rust side stops and forgets an install when
   * another project replaces this one, so a panel that only watched `enabled` would keep showing the
   * closed project's install, its output and its `site_dir`.
   */
  projectPath: string | null;
  onPublished?: () => void;
}) {
  const [releases, setReleases] = useState<ReleaseView[]>([]);
  const [result, setResult] = useState<PublishView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showLog, setShowLog] = useState(false);

  const [install, setInstall] = useState<SiteInstallView | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);
  const [installBusy, setInstallBusy] = useState(false);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      setReleases(await api.listReleases());
    } catch (caught) {
      setError(describeError(caught));
    }
  }, [enabled, projectPath]);

  /**
   * Read the install's state.
   *
   * Reading IS the poll: the Rust side judges an installer whose process has exited while answering this,
   * so a finished install cannot be reported as running — and the answer carries the output tail, which is
   * what keeps a minutes-long install from being a frozen UI.
   */
  const loadInstall = useCallback(async () => {
    if (!enabled) return;
    try {
      setInstall(await api.siteInstallStatus());
      setInstallError(null);
    } catch (caught) {
      setInstallError(describeError(caught));
    }
  }, [enabled, projectPath]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    // A different project invalidates what is on screen before the new answer arrives, so the panel
    // never shows the previous project's installer as if it were this one's.
    setInstall(null);
    setInstallError(null);
    void loadInstall();
  }, [loadInstall]);

  const installing = install?.state.state === 'running';

  useEffect(() => {
    // Polled only while something is running, so an idle panel asks for nothing.
    if (!installing) return undefined;
    const timer = setInterval(() => {
      void loadInstall();
    }, INSTALL_POLL_MS);
    return () => clearInterval(timer);
  }, [installing, loadInstall]);

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
        // A publish that got as far as building proves the CLI is on disk, and an install run outside
        // this panel would otherwise stay invisible until the page was reopened.
        await loadInstall();
        onPublished?.();
      } catch (caught) {
        setError(describeError(caught));
      } finally {
        setBusy(false);
      }
    },
    [reload, loadInstall, onPublished],
  );

  const startInstall = useCallback(async () => {
    setInstallBusy(true);
    setInstallError(null);
    try {
      // Returns as soon as npm has STARTED, so the window stays responsive and the polling effect above
      // takes over. Asking while one is already running does not start a second npm: the Rust side
      // refuses and answers with the running state, which is what this renders.
      setInstall(await api.installSiteDependencies());
    } catch (caught) {
      // A refusal is a real answer — no system npm, or a project with no site/ — and it carries the
      // hint. The state is NOT re-read here, so the refusal's own message is not wiped by a successful
      // refresh.
      setInstallError(describeError(caught));
    } finally {
      setInstallBusy(false);
    }
  }, []);

  const cancelInstall = useCallback(async () => {
    setInstallBusy(true);
    try {
      setInstall(await api.cancelSiteInstall());
      setInstallError(null);
    } catch (caught) {
      setInstallError(describeError(caught));
    } finally {
      setInstallBusy(false);
    }
  }, []);

  if (!enabled) {
    return (
      <div className="publish-panel">
        <p className="muted">打开一个项目后可以发布。</p>
      </div>
    );
  }

  const display = install ? describeSiteInstall(install) : null;
  // Why a publish would fail right now, or `null` when the build can find its Astro CLI. It is the same
  // `astro_ready` the build's own check produces, so the tooltip cannot disagree with the build.
  const blocked = install ? publishBlockedReason(install) : null;

  return (
    <div className="publish-panel">
      <header className="publish-header">
        <strong>发布</strong>
        <button
          type="button"
          className="primary"
          onClick={() => void publish()}
          disabled={busy}
          title={blocked ?? '检查改动、构建一次并打标签(handoff.md 第 19.3 节)'}
        >
          {busy ? '构建中…' : '发布'}
        </button>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新
        </button>
      </header>

      {error && (
        <StateBlock
          tone="error"
          title="发布没有完成"
          description={error}
          onDismiss={() => setError(null)}
        />
      )}

      {/* The site's dependencies, where the build error appears. The one-click install lives here. */}
      {installError && (
        <StateBlock
          tone="error"
          title="依赖安装没有开始"
          description={installError}
          onDismiss={() => setInstallError(null)}
        />
      )}

      {!install && !installError && (
        <StateBlock tone="loading" title="正在读取站点依赖状态…" />
      )}

      {install && display && (
        <>
          <h3>站点依赖 · {siteInstallStateLabel(install)}</h3>
          <StateBlock
            tone={display.tone}
            title={display.title}
            description={
              <>
                <p>{display.detail}</p>
                <p className="muted">
                  {npmSourceLabel(install)} · {install.site_dir}
                </p>
                {display.showOutput && (
                  <>
                    <p className="muted">安装器输出(最后几行):</p>
                    <pre className="build-log">{install.output_tail}</pre>
                  </>
                )}
              </>
            }
            actions={
              <>
                {display.canCancel && (
                  <button
                    type="button"
                    onClick={() => void cancelInstall()}
                    disabled={installBusy}
                    title="终止 npm 及其子进程;node_modules 可能停在半安装状态,再安装一次即可修复"
                  >
                    取消安装
                  </button>
                )}
                <button
                  type="button"
                  className={display.canStart ? 'primary' : ''}
                  onClick={() => void startInstall()}
                  disabled={installBusy || !display.canStart}
                  title="在 site/ 里运行 npm install(使用系统的 npm;Veloct 不自带 npm)"
                >
                  {display.actionLabel}
                </button>
                <button type="button" onClick={() => void loadInstall()} disabled={installBusy}>
                  刷新状态
                </button>
              </>
            }
          />
        </>
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
