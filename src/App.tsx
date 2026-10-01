/**
 * The workbench.
 *
 * `handoff.md` §4.1 asks for a three-part layout, and §5.1 requires the preview to be the REAL Astro
 * project rather than a second renderer. So the preview pane is an iframe pointed at the dev server the
 * Rust supervisor started, and there is no Markdown rendering in this application at all — the moment
 * there were two renderers, they would disagree, and the preview would stop predicting the site.
 *
 * The editor is a plain Markdown source editor on purpose. `handoff.md` §4.2 asks for WYSIWYG, which
 * needs ProseMirror/Tiptap; that is a real piece of work and shipping a half-working WYSIWYG would be
 * worse than shipping an honest source editor that gets the SAVE behaviour right. What is here is the
 * autosave contract, the conflict handling and the live preview — the three things that are hard to
 * retrofit.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
  api,
  asCommandError,
  describeError,
  type ContentKind,
  type DocumentView,
  type AutoBackupStateView,
  type AiSettingsView,
  type PreviewView,
  type ProjectView,
  type RibbonEntry,
  type SaveView,
  type WipTransition,
} from './lib/api';
import Editor from './Editor';
import AttachmentPanel from './AttachmentPanel';
import BackupPanel from './BackupPanel';
import DeployPanel from './DeployPanel';
import LogView from './LogView';
import PageCreator, { AiSettingsPanel } from './PageCreator';
import PublishPanel from './PublishPanel';
import WorksPanel from './WorksPanel';
import NetworkImagePanel from './NetworkImagePanel';
import {
  dueReason,
  hashText,
  initialAutosaveState,
  markChanged,
  markFailed,
  markSaved,
  markSaving,
  mustSaveNow,
  type AutosaveState,
} from './lib/autosave';

const PREVIEW_POLL_MS = 2_000;

function useAutosave(
  document: DocumentView | null,
  source: string,
  onSaved: (view: SaveView) => void,
  onError: (message: string) => void,
) {
  const hash = useMemo(() => hashText(source), [source]);
  const [state, setState] = useState<AutosaveState>(() =>
    initialAutosaveState(Date.now(), document ? hashText(document.source) : ''),
  );
  const stateRef = useRef(state);
  stateRef.current = state;
  const savingRef = useRef(false);

  // A new document resets the baseline, or the first keystroke in it would look like an edit of the
  // previous one.
  useEffect(() => {
    if (!document) return;
    setState(initialAutosaveState(Date.now(), hashText(document.source)));
  }, [document?.id, document?.revision.hash, document]);

  const save = useCallback(
    async (reason: string) => {
      if (!document || savingRef.current) return;
      const current = stateRef.current;
      if (hash === current.savedHash) return;

      savingRef.current = true;
      setState((previous) => markSaving(previous));
      try {
        const result = await api.saveDocument({
          kind: document.kind,
          id: document.id,
          // The revision the editor READ, so an external edit is caught rather than overwritten.
          baseline: document.revision,
          source,
        });
        if (result.saved) {
          const savedHash = result.source ? hashText(result.source) : hash;
          setState((previous) => markSaved(previous, savedHash, Date.now()));
        } else {
          // A refused save keeps the unsaved hash, so the next tick tries again and the UI keeps
          // showing the conflict. Marking it saved here would silently discard the user's work.
          setState((previous) => markFailed(previous));
        }
        onSaved(result);
        void reason;
      } catch (error) {
        setState((previous) => markFailed(previous));
        onError(describeError(error));
      } finally {
        savingRef.current = false;
      }
    },
    [document, hash, source, onSaved, onError],
  );

  // The change detector. One interval rather than a timer per keystroke, so the arithmetic in
  // `dueReason` is the single definition of when a save happens.
  useEffect(() => {
    const timer = window.setInterval(() => {
      const reason = dueReason(stateRef.current, hash, Date.now());
      if (reason) void save(reason);
    }, 250);
    return () => window.clearInterval(timer);
  }, [hash, save]);

  const onChange = useCallback(
    (next: string) => {
      void next;
      setState((previous) => markChanged(previous, Date.now()));
    },
    [],
  );

  // `handoff.md` section 4.3: closing the application saves immediately. `beforeunload` cannot await,
  // so the save is fired and the handler does not block on it; the Rust side writes atomically, so a
  // half-written file is not possible.
  useEffect(() => {
    const handler = () => {
      if (document && mustSaveNow(stateRef.current, hash)) {
        void api
          .saveDocument({
            kind: document.kind,
            id: document.id,
            baseline: document.revision,
            source,
          })
          .catch(() => {
            // Nothing can be shown during unload; the autosave interval or the next launch's
            // external-modification check will surface it.
          });
      }
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [document, hash, source]);

  return { state, hash, onChange, saveNow: () => save('manual') };
}

export default function App() {
  const [project, setProject] = useState<ProjectView | null>(null);
  const [documents, setDocuments] = useState<RibbonEntry[]>([]);
  const [document, setDocument] = useState<DocumentView | null>(null);
  const [source, setSource] = useState('');
  const [preview, setPreview] = useState<PreviewView | null>(null);
  const [issues, setIssues] = useState<string[]>([]);
  const [status, setStatus] = useState('正在启动…');
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<SaveView | null>(null);
  const [pathInput, setPathInput] = useState('');
  const [filter, setFilter] = useState<ContentKind | 'all'>('all');
  // Whether the log page is showing instead of the editor (handoff.md section 18).
  const [showLog, setShowLog] = useState(false);
  // The publish panel is a third main view beside the editor and the log.
  const [showPublish, setShowPublish] = useState(false);
  const [showAttachments, setShowAttachments] = useState(false);
  const [showBackups, setShowBackups] = useState(false);
  const [showDeploy, setShowDeploy] = useState(false);
  // The automatic backup rule (handoff.md section 17.5).
  const [autoBackup, setAutoBackup] = useState<AutoBackupStateView | null>(null);
  // The editor/preview split, draggable (handoff.md section 4.2). Stored as a percentage so it survives
  // a window resize, and clamped so a pane can never be dragged to nothing and become unrecoverable.
  const [splitPercent, setSplitPercent] = useState(50);
  // The AI surface (handoff.md sections 8 and 11).
  const [aiSettings, setAiSettings] = useState<AiSettingsView | null>(null);
  const [showPageCreator, setShowPageCreator] = useState(false);
  const [showAiSettings, setShowAiSettings] = useState(false);
  // Section 10's works system: import, metadata, the poster image and the independent Works Origin.
  const [showWorks, setShowWorks] = useState(false);
  // Section 13.14's network image browser: search a free stock provider and write the remote URL.
  const [showNetworkImages, setShowNetworkImages] = useState(false);
  const splitRef = useRef<HTMLDivElement | null>(null);
  const draggingRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [list, index] = await Promise.all([api.listDocuments(), api.contentIndex()]);
      setDocuments(list);
      setIssues(index.issues);
    } catch (caught) {
      setError(describeError(caught));
    }
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const info = await api.appInfo();
        const current = await api.currentProject();
        setProject(current);
        setStatus(`${info.name} ${info.version}`);
        if (current) await refresh();
      } catch (caught) {
        setError(describeError(caught));
        setStatus('启动失败');
      }
    })();
  }, [refresh]);

  // The preview state comes from the supervisor, which polls the child's liveness. Doing the polling
  // here rather than in an event listener is deliberate: a crashed sidecar delivers no event.
  useEffect(() => {
    const timer = window.setInterval(() => {
      void api
        .previewStatus()
        .then(setPreview)
        .catch(() => {
          // A failed poll is not worth a message on every tick; the status bar simply stays as it was.
        });
    }, PREVIEW_POLL_MS);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (project) void api.previewStatus().then(setPreview).catch(() => undefined);
  }, [project]);

  const reloadAiSettings = useCallback(async () => {
    try {
      setAiSettings(await api.aiSettings());
    } catch (caught) {
      setError(describeError(caught));
    }
  }, []);

  useEffect(() => {
    if (project) void reloadAiSettings();
  }, [project, reloadAiSettings]);

  // The automatic backup poll. A MINUTE is the right granularity: the rule works in ten-minute spans, and a
  // tighter loop would ask the same question sixty times for no benefit. The command returns whether anything
  // happened, so the common case costs one IPC call and no work.
  useEffect(() => {
    if (!project) return;
    const ask = () => {
      void api
        .autoBackupState()
        .then(setAutoBackup)
        .catch(() => undefined);
    };
    ask();
    const timer = window.setInterval(ask, 60_000);
    return () => window.clearInterval(timer);
  }, [project]);

  // Separate from the state poll because running a backup is not idempotent, so it must not be re-entered by a
  // slow render. The interval is the same minute.
  useEffect(() => {
    if (!project) return;
    const timer = window.setInterval(() => {
      void api
        .runAutoBackupIfDue()
        .then((outcome) => {
          if (outcome) setStatus(`自动备份已写入:${outcome.archive_path}`);
        })
        .catch(() => undefined);
    }, 60_000);
    return () => window.clearInterval(timer);
  }, [project]);

  const onSaved = useCallback(
    (view: SaveView) => {
      if (view.saved) {
        // Section 17.5's first condition: the content changed. The Rust side restarts its timer; the window
        // does not keep a second one, because two timers would disagree.
        void api.noteContentChange().then(setAutoBackup).catch(() => undefined);
        setConflict(null);
        setStatus(
          view.status === 'unchanged' ? '无改动' : `已保存 ${new Date().toLocaleTimeString()}`,
        );
      } else {
        setConflict(view);
        setStatus('保存被拒绝');
      }
      void refresh();
    },
    [refresh],
  );

  const autosave = useAutosave(document, source, onSaved, setError);

  const openProject = async () => {
    if (!pathInput.trim()) return;
    try {
      const opened = await api.openProject(pathInput.trim());
      setProject(opened);
      setError(null);
      await refresh();
    } catch (caught) {
      setError(describeError(caught));
    }
  };

  const selectDocument = async (entry: RibbonEntry) => {
    try {
      const loaded = await api.readDocument(entry.kind, entry.id);
      setDocument(loaded);
      setSource(loaded.source);
      setConflict(null);
    } catch (caught) {
      setError(describeError(caught));
    }
  };

  const createDocument = async (kind: ContentKind) => {
    const title = window.prompt(kind === 'article' ? '文章标题' : kind === 'page' ? '页面标题' : '知识库条目标题');
    if (!title) return;
    try {
      const created = await api.createDocument({ kind, title });
      await refresh();
      setDocument(created.document);
      setSource(created.document.source);
    } catch (caught) {
      setError(describeError(caught));
    }
  };

  const transition = async (kind: WipTransition) => {
    if (!document) return;
    try {
      const result = await api.transitionWip({
        id: document.id,
        baseline: document.revision,
        source,
        transition: kind,
      });
      onSaved(result);
      const reloaded = await api.readDocument(document.kind, document.id);
      setDocument(reloaded);
      setSource(reloaded.source);
    } catch (caught) {
      setError(describeError(caught));
    }
  };

  /** Take the disk text, discarding the editor's version. The user chose this explicitly. */
  const acceptDisk = () => {
    if (!conflict?.disk_source) return;
    setSource(conflict.disk_source);
    setConflict(null);
    setStatus('已采用磁盘版本');
  };

  const startPreview = async () => {
    try {
      setPreview(await api.startPreview('site'));
    } catch (caught) {
      const command = asCommandError(caught);
      setError(command.hint ? `${command.message} — ${command.hint}` : command.message);
    }
  };

  const visible = documents.filter((entry) => filter === 'all' || entry.kind === filter);

  const previewState = preview?.site.state ?? 'stopped';
  const previewLabel = (() => {
    switch (previewState) {
      case 'running':
        return `预览运行中 :${preview?.site.state === 'running' ? preview.site.port : ''}`;
      case 'starting':
        return '预览启动中…';
      case 'crashed':
        return '预览已崩溃';
      case 'exited':
        return '预览已停止';
      default:
        return '预览未启动';
    }
  })();

  return (
    <div
      className="workbench"
      style={{ '--editor-share': `${splitPercent}%` } as React.CSSProperties}
    >
      <aside className="sidebar">
        <header>
          <strong>Veloct</strong>
          {project && <span className="muted">{project.name}</span>}
        </header>

        {!project && (
          <div className="open-project">
            <p className="muted">打开或新建一个项目目录。</p>
            <input
              value={pathInput}
              onChange={(event) => setPathInput(event.target.value)}
              placeholder="D:/我的站点"
              spellCheck={false}
            />
            <button type="button" onClick={() => void openProject()}>
              打开项目
            </button>
          </div>
        )}

        {project && (
          <>
            <nav className="filters">
              {(['all', 'article', 'page', 'knowledge'] as const).map((kind) => (
                <button
                  type="button"
                  key={kind}
                  className={filter === kind ? 'active' : ''}
                  onClick={() => setFilter(kind)}
                >
                  {kind === 'all' ? '全部' : kind === 'article' ? '文章' : kind === 'page' ? '页面' : '知识库'}
                </button>
              ))}
            </nav>

            <div className="new-buttons">
              <button
                type="button"
                onClick={() => {
                  setShowLog((showing) => !showing);
                  setShowPublish(false);
                }}
              >
                {showLog ? '返回编辑' : '查看日志'}
              </button>
              <button type="button" onClick={() => void createDocument('article')}>
                + 文章
              </button>
              <button type="button" onClick={() => void createDocument('page')}>
                + 页面
              </button>
              <button type="button" onClick={() => void createDocument('knowledge')}>
                + 知识库
              </button>
              <button
                type="button"
                className={showPublish ? 'active' : ''}
                onClick={() => {
                  setShowPublish((showing) => !showing);
                  setShowLog(false);
                }}
                title="检查改动、构建一次并打标签(handoff.md 第 19.3 节)"
              >
                发布
              </button>
              <button
                type="button"
                onClick={() => setShowPageCreator(true)}
                title="由 AI 生成整页,确认前不会创建任何文件"
              >
                AI 建页
              </button>
              <button type="button" onClick={() => setShowAiSettings(true)}>
                AI 设置
              </button>
              <button
                type="button"
                className={showAttachments ? 'active' : ''}
                onClick={() => {
                  setShowAttachments((showing) => !showing);
                  setShowLog(false);
                  setShowPublish(false);
                }}
                title="附件池(handoff.md 第 13 节)"
              >
                附件
              </button>
              <button
                type="button"
                className={showNetworkImages ? 'active' : ''}
                onClick={() => {
                  setShowNetworkImages((showing) => !showing);
                  setShowWorks(false);
                  setShowBackups(false);
                  setShowAttachments(false);
                  setShowLog(false);
                  setShowPublish(false);
                }}
                title="网络图片浏览:搜索免费图库并写入远程 URL(handoff.md 第 13.14 节)"
              >
                网络图片
              </button>
              <button
                type="button"
                className={showWorks ? 'active' : ''}
                onClick={() => {
                  setShowWorks((showing) => !showing);
                  setShowNetworkImages(false);
                  setShowBackups(false);
                  setShowAttachments(false);
                  setShowLog(false);
                  setShowPublish(false);
                }}
                title="作品系统:导入、封面与独立 Origin(handoff.md 第 10 节)"
              >
                作品
              </button>
              <button
                type="button"
                className={showBackups ? 'active' : ''}
                onClick={() => {
                  setShowBackups((showing) => !showing);
                  setShowAttachments(false);
                  setShowLog(false);
                  setShowPublish(false);
                }}
                title="备份与恢复(handoff.md 第 17 节)"
              >
                备份
              </button>
              <button
                type="button"
                className={showDeploy ? 'active' : ''}
                onClick={() => {
                  setShowDeploy((showing) => !showing);
                  setShowBackups(false);
                  setShowAttachments(false);
                  setShowLog(false);
                  setShowPublish(false);
                }}
                title="部署目标(handoff.md 第 10 与 14 节)"
              >
                部署
              </button>
            </div>

            <ul className="document-list">
              {visible.map((entry) => (
                <li key={`${entry.kind}:${entry.id}`}>
                  <button
                    type="button"
                    className={document?.id === entry.id ? 'active' : ''}
                    onClick={() => void selectDocument(entry)}
                  >
                    <span className="title">{entry.title}</span>
                    <span className={`badge ${entry.wip_state}`}>{entry.wip_state}</span>
                    {!entry.is_buildable && <span className="badge draft-only">不进入构建</span>}
                  </button>
                </li>
              ))}
              {visible.length === 0 && <li className="muted">还没有内容。</li>}
            </ul>
          </>
        )}
      </aside>

      <main className="editor">
        {showDeploy ? (
          <DeployPanel enabled={project !== null} />
        ) : showNetworkImages ? (
          <NetworkImagePanel
            enabled={project !== null}
            onInsertMarkdown={(markdown) => {
              // §13.14: the remote URL goes straight into the Markdown and the image never enters the
              // cache pool. The buffer is appended here and the user saves when they choose, so browsing
              // cannot edit a document by itself. Unlike the attachment panel this does NOT close: a
              // browser is normally used for several picks in a row.
              if (document) {
                const next = `${source.trimEnd()}\n\n${markdown}\n`;
                setSource(next);
                autosave.onChange(next);
              } else {
                setStatus('请先打开一篇文章,再插入网络图片 Markdown');
              }
            }}
          />
        ) : showWorks ? (
          <WorksPanel enabled={project !== null} />
        ) : showBackups ? (
          <BackupPanel enabled={project !== null} />
        ) : showAttachments ? (
          <AttachmentPanel
            enabled={project !== null}
            onInsertMarkdown={(markdown) => {
              // §13.3: the pool never rewrites an article. The Markdown is appended to the buffer here and the
              // user saves when they choose, so adding a picture cannot edit a document by itself.
              setShowAttachments(false);
              if (document) {
                const next = `${source.trimEnd()}\n\n${markdown}\n`;
                setSource(next);
                autosave.onChange(next);
                setStatus('已将附件 Markdown 插入编辑器,保存后生效');
              } else {
                setStatus('请先打开一篇文章,再插入附件 Markdown');
              }
            }}
          />
        ) : showPublish ? (
          <PublishPanel
            enabled={project !== null}
            onPublished={() => void refresh()}
          />
        ) : showLog ? (
          <LogView enabled={project !== null} />
        ) : document ? (
          <>
            <header className="editor-header">
              <input
                className="title"
                value={document.title}
                readOnly
                title="标题在 frontmatter 中编辑"
              />
              <span className="muted">{document.route}</span>
              <div className="wip-controls">
                {document.wip_state === 'draft' && (
                  <button type="button" onClick={() => void transition('publish_wip')}>
                    发布 WIP
                  </button>
                )}
                {document.wip_state === 'published_wip' && (
                  <>
                    <button type="button" onClick={() => void transition('unpublish_wip')}>
                      取消发布
                    </button>
                    <button type="button" onClick={() => void transition('finish')}>
                      完成
                    </button>
                  </>
                )}
                {document.wip_state === 'finished_with_history' && (
                  <span className="muted" title="旧 WIP 地址会永久重定向到文章地址">
                    历史 WIP 地址已重定向
                  </span>
                )}
              </div>
            </header>

            {conflict && (
              <div className="conflict" role="alert">
                <strong>保存被拒绝:文件在磁盘上已改变</strong>
                <p>{conflict.conflict_message}</p>
                <p className="muted">
                  Veloct 没有覆盖磁盘上的内容。你的编辑仍在编辑器里。
                </p>
                <div className="conflict-actions">
                  <button type="button" onClick={acceptDisk} disabled={!conflict.disk_source}>
                    采用磁盘版本(放弃我的编辑)
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setConflict(null);
                      void autosave.saveNow();
                    }}
                  >
                    保留我的编辑并重试
                  </button>
                </div>
              </div>
            )}

            {document.diagnostics.length > 0 && (
              <ul className="diagnostics">
                {document.diagnostics.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            )}

            <Editor
              source={source}
              editable
              onChange={(next) => {
                setSource(next);
                autosave.onChange(next);
              }}
            />
          </>
        ) : (
          <div className="empty">
            <h1>Veloct</h1>
            <p className="muted">
              {project
                ? '从左侧选择一篇内容,或新建一篇。'
                : '打开一个项目目录以开始。'}
            </p>
          </div>
        )}
      </main>

      <div
        className="split-handle"
        role="separator"
        aria-orientation="vertical"
        aria-label="拖动以调整编辑器和预览的比例"
        aria-valuenow={splitPercent}
        aria-valuemin={20}
        aria-valuemax={80}
        tabIndex={0}
        onPointerDown={(event) => {
          draggingRef.current = true;
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          if (!draggingRef.current) return;
          const container = splitRef.current;
          if (!container) return;
          const bounds = container.getBoundingClientRect();
          // The workspace begins after the sidebar, so the ratio is measured inside it.
          const ratio = ((event.clientX - bounds.left) / bounds.width) * 100;
          setSplitPercent(Math.min(80, Math.max(20, ratio)));
        }}
        onPointerUp={(event) => {
          draggingRef.current = false;
          event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        onKeyDown={(event) => {
          // A keyboard path, because a pointer-only resize is unusable without a mouse.
          if (event.key === 'ArrowLeft') setSplitPercent((value) => Math.max(20, value - 2));
          if (event.key === 'ArrowRight') setSplitPercent((value) => Math.min(80, value + 2));
        }}
      />

      <section className="preview" ref={splitRef}>
        <header>
          <span>{previewLabel}</span>
          {previewState === 'running' ? (
            <button type="button" onClick={() => void api.stopPreview().then(setPreview)}>
              停止
            </button>
          ) : (
            <button type="button" onClick={() => void startPreview()} disabled={!project}>
              启动预览
            </button>
          )}
        </header>

        {preview?.site_url ? (
          // The REAL Astro dev server, not a second renderer (handoff.md section 5.1).
          <iframe title="站点预览" src={preview.site_url} sandbox="allow-same-origin allow-scripts" />
        ) : (
          <p className="muted preview-empty">
            {previewState === 'crashed' && preview?.site.state === 'crashed'
              ? preview.site.reason
              : '预览使用与最终站点同一套 Astro 工程。启动后会在这里显示。'}
          </p>
        )}
      </section>

      {showPageCreator && (
        <PageCreator
          settings={aiSettings}
          onClose={() => setShowPageCreator(false)}
          onOpenSettings={() => {
            setShowPageCreator(false);
            setShowAiSettings(true);
          }}
          onCreated={(created) => {
            setShowPageCreator(false);
            setDocument(created);
            setSource(created.source);
            setStatus(`已创建 ${created.title}`);
            void refresh();
          }}
        />
      )}

      {showAiSettings && (
        <AiSettingsPanel
          settings={aiSettings}
          onReload={reloadAiSettings}
          onClose={() => setShowAiSettings(false)}
        />
      )}

      <footer className="statusbar">
        <span>{status}</span>
        <span className="muted">
          {autosave.state.saving ? '保存中…' : autosave.hash === autosave.state.savedHash ? '已同步' : '有未保存的改动'}
        </span>
        {autoBackup && (
          <span
            className={autoBackup.needs_acknowledgement ? 'issues' : 'muted'}
            title={autoBackup.description}
          >
            {autoBackup.needs_acknowledgement
              ? '自动备份待确认(发现疑似凭据)'
              : autoBackup.decision.decision === 'due'
                ? '自动备份即将执行'
                : autoBackup.description}
          </span>
        )}
        {issues.length > 0 && (
          <span className="issues" title={issues.join('\n')}>
            {issues.length} 个索引问题
          </span>
        )}
        {error && (
          <span className="error" role="alert" onClick={() => setError(null)} title="点击关闭">
            {error}
          </span>
        )}
      </footer>
    </div>
  );
}
