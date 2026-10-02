/**
 * The workbench shell.
 *
 * `handoff.md` §4.1 asks for a three-part layout, and §5.1 requires the preview to be the REAL Astro
 * project rather than a second renderer. So the preview pane is an iframe pointed at the dev server the
 * Rust supervisor started, and there is no Markdown rendering in this application at all — the moment
 * there were two renderers, they would disagree, and the preview would stop predicting the site.
 *
 * The editor is a WYSIWYG Tiptap surface over the Markdown body (`Editor.tsx`). What is here is the shell
 * around it: the autosave contract, the conflict handling and the live preview — the three things that are
 * hard to retrofit.
 *
 * ## The shell's structure
 *
 * `DESIGN.md` is the visual system (warm cream canvas, serif display, coral accent, dark product surfaces)
 * and Halo's admin console is the structural reference. Four regions, each with ONE job:
 *
 *  1. **A persistent left sidebar** (`aside.sidebar`) — the brand, the open project, the navigation, and the
 *     content browser. EVERY top-level view lives in the navigation, grouped (`内容` / `资源` / `发布与部署` /
 *     `维护` / `AI`); no feature was dropped to make the layout tidy, and the AI surfaces that used to be
 *     loose buttons are navigation entries that open their modal.
 *  2. **A top bar** (`header.topbar`) — the current page's CONTEXT as a breadcrumb (project › section ›
 *     document) plus the controls that must be reachable from anywhere: the preview start/stop, and the
 *     save / automatic-backup / index-issue chips that used to be crammed into a 28px status strip.
 *  3. **One content container** (`div.content`) — a single scrollable column of cards, capped at a readable
 *     width. A page is `PageHeader` + one card per subject; never a wall of raw controls.
 *  4. **A status bar** (`footer.statusbar`) — the last thing that happened, in one line.
 *
 * Everything visible is built from the tokens in `styles.css` and the four primitives in
 * `components/ui.tsx`. No dependency was added: no component library, no CSS framework, no icon package.
 *
 * ## What this turn changed, and what it did not
 *
 * The SHELL is new and the ATTACHMENTS page is converted as the sample (`AttachmentPanel.tsx`). The other
 * panels are unchanged inside — they are now wrapped by the shell's content column and inherit the token
 * re-colouring, which is deliberate: they are the "before" the owner compares the sample against.
 *
 * Three behaviour points worth stating rather than hiding:
 *
 *  * **The views are mutually exclusive.** They used to be seven independent booleans, so two panels could be
 *    open at once and the render order silently decided which one you saw. A single `view` fixes that.
 *  * **Clicking a document now returns to the editor.** It used to load the document behind whichever panel
 *    was open, so the click appeared to do nothing.
 *  * **The split handle is measured on the workspace** rather than on the preview pane. `--editor-share` is a
 *    percentage of the workspace, so measuring the ratio anywhere else made the handle move at the wrong
 *    speed — roughly 3× the pointer. Same property, same grid, correct frame of reference.
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
import { Chip, Icon, PageHeader, SpikeMark, StateBlock, type IconName } from './components/ui';
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

/** Every top-level view. Nothing that used to be a button is missing from this list. */
type ViewId =
  | 'editor'
  | 'logs'
  | 'publish'
  | 'attachments'
  | 'network-images'
  | 'works'
  | 'backups'
  | 'deploy';

interface NavEntry {
  id: ViewId;
  /** The label in the sidebar, which is also the section name in the breadcrumb. */
  label: string;
  icon: IconName;
  /** The explanation the old button carried as a `title`, kept verbatim. */
  title: string;
}

const NAV_GROUPS: Array<{ label: string; entries: NavEntry[] }> = [
  {
    label: '内容',
    entries: [
      { id: 'editor', label: '编辑器', icon: 'edit', title: '编辑选中的内容' },
      { id: 'logs', label: '运行日志', icon: 'logs', title: '运行日志(handoff.md 第 18 节)' },
    ],
  },
  {
    label: '资源',
    entries: [
      { id: 'attachments', label: '附件', icon: 'clip', title: '附件池(handoff.md 第 13 节)' },
      {
        id: 'network-images',
        label: '网络图片',
        icon: 'image',
        title: '网络图片浏览:搜索免费图库并写入远程 URL(handoff.md 第 13.14 节)',
      },
      { id: 'works', label: '作品', icon: 'grid', title: '作品系统:导入、封面与独立 Origin(handoff.md 第 10 节)' },
    ],
  },
  {
    label: '发布与部署',
    entries: [
      { id: 'publish', label: '发布', icon: 'tag', title: '检查改动、构建一次并打标签(handoff.md 第 19.3 节)' },
      { id: 'deploy', label: '部署', icon: 'deploy', title: '部署目标(handoff.md 第 10 与 14 节)' },
    ],
  },
  {
    label: '维护',
    entries: [
      { id: 'backups', label: '备份与恢复', icon: 'archive', title: '备份与恢复(handoff.md 第 17 节)' },
    ],
  },
];

/** The two AI surfaces are modals rather than pages, so they are navigation entries with no active state. */
const AI_ENTRIES: Array<{ label: string; icon: IconName; title: string; opens: 'creator' | 'settings' }> = [
  {
    label: 'AI 建页',
    icon: 'sparkle',
    title: '由 AI 生成整页,确认前不会创建任何文件',
    opens: 'creator',
  },
  { label: 'AI 设置', icon: 'sliders', title: '模型与提供方设置(handoff.md 第 8 与 11 节)', opens: 'settings' },
];

const ALL_ENTRIES: NavEntry[] = NAV_GROUPS.flatMap((group) => group.entries);

const KIND_LABEL: Record<ContentKind | 'all', string> = {
  all: '全部',
  article: '文章',
  page: '页面',
  knowledge: '知识库',
};

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
  /** The single active view. Mutually exclusive, which seven booleans could not guarantee. */
  const [view, setView] = useState<ViewId>('editor');
  // The automatic backup rule (handoff.md section 17.5).
  const [autoBackup, setAutoBackup] = useState<AutoBackupStateView | null>(null);
  // The editor/preview split, draggable (handoff.md section 4.2). Stored as a percentage so it survives
  // a window resize, and clamped so a pane can never be dragged to nothing and become unrecoverable.
  const [splitPercent, setSplitPercent] = useState(50);
  // The AI surface (handoff.md sections 8 and 11).
  const [aiSettings, setAiSettings] = useState<AiSettingsView | null>(null);
  const [showPageCreator, setShowPageCreator] = useState(false);
  const [showAiSettings, setShowAiSettings] = useState(false);
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
      setView('editor');
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

  const currentEntry = ALL_ENTRIES.find((entry) => entry.id === view);
  const savedLabel = autosave.state.saving
    ? '保存中…'
    : autosave.hash === autosave.state.savedHash
      ? '已同步'
      : '有未保存的改动';
  const savedTone = autosave.state.saving
    ? 'neutral'
    : autosave.hash === autosave.state.savedHash
      ? 'success'
      : 'warning';

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <SpikeMark className="brand-mark" />
          <span className="brand-word">Veloct</span>
        </div>

        {!project ? (
          <div className="open-project">
            <p className="muted">打开或新建一个项目目录。</p>
            <input
              value={pathInput}
              onChange={(event) => setPathInput(event.target.value)}
              placeholder="D:/我的站点"
              spellCheck={false}
            />
            <button type="button" className="primary" onClick={() => void openProject()}>
              打开项目
            </button>
          </div>
        ) : (
          <p className="sidebar-project" title={project.name}>
            {project.name}
          </p>
        )}

        {/* Every top-level view is here, grouped, and reachable whether or not a project is open: a page
            that cannot work yet says so in its own empty state rather than disappearing from the nav. */}
        <nav className="nav" aria-label="主导航">
          {NAV_GROUPS.map((group) => (
            <div className="nav-group" key={group.label}>
              <p className="nav-group-label">{group.label}</p>
              <ul className="nav-list">
                {group.entries.map((entry) => (
                  <li key={entry.id}>
                    <button
                      type="button"
                      className={`nav-item${view === entry.id ? ' is-active' : ''}`}
                      aria-current={view === entry.id ? 'page' : undefined}
                      title={entry.title}
                      onClick={() => setView(entry.id)}
                    >
                      <Icon name={entry.icon} />
                      <span className="nav-label">{entry.label}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ))}
          <div className="nav-group">
            <p className="nav-group-label">AI</p>
            <ul className="nav-list">
              {AI_ENTRIES.map((entry) => (
                <li key={entry.label}>
                  <button
                    type="button"
                    className="nav-item"
                    title={entry.title}
                    onClick={() =>
                      entry.opens === 'creator' ? setShowPageCreator(true) : setShowAiSettings(true)
                    }
                  >
                    <Icon name={entry.icon} />
                    <span className="nav-label">{entry.label}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </nav>

        {project && (
          <section className="doc-browser" aria-label="内容">
            <div className="doc-filter">
              {(['all', 'article', 'page', 'knowledge'] as const).map((kind) => (
                <button
                  type="button"
                  key={kind}
                  className={`tab${filter === kind ? ' is-active' : ''}`}
                  onClick={() => setFilter(kind)}
                >
                  {KIND_LABEL[kind]}
                </button>
              ))}
            </div>

            <div className="doc-new">
              <button type="button" onClick={() => void createDocument('article')}>
                + 文章
              </button>
              <button type="button" onClick={() => void createDocument('page')}>
                + 页面
              </button>
              <button type="button" onClick={() => void createDocument('knowledge')}>
                + 知识库
              </button>
            </div>

            <ul className="doc-list">
              {visible.map((entry) => (
                <li key={`${entry.kind}:${entry.id}`}>
                  <button
                    type="button"
                    className={`doc-item${document?.id === entry.id ? ' is-active' : ''}`}
                    onClick={() => {
                      // Selecting content means editing it: the click used to load the document behind
                      // whichever panel was open, so it looked like nothing happened.
                      setView('editor');
                      void selectDocument(entry);
                    }}
                  >
                    <span className="doc-title">{entry.title}</span>
                    <span className="doc-meta">
                      <span className={`badge ${entry.wip_state}`}>{entry.wip_state}</span>
                      {!entry.is_buildable && <span className="badge draft-only">不进入构建</span>}
                    </span>
                  </button>
                </li>
              ))}
              {visible.length === 0 && <li className="doc-empty muted">还没有内容。</li>}
            </ul>
          </section>
        )}
      </aside>

      <div className="main">
        <header className="topbar">
          <div className="topbar-context">
            <p className="crumb">
              <span className="crumb-project">{project ? project.name : '未打开项目'}</span>
              <span className="crumb-sep">/</span>
              <span className="crumb-page">{currentEntry?.label ?? '编辑器'}</span>
              {view === 'editor' && document && (
                <>
                  <span className="crumb-sep">/</span>
                  <span className="crumb-leaf">{document.title}</span>
                </>
              )}
            </p>
          </div>

          <div className="topbar-actions">
            {view === 'editor' &&
              (previewState === 'running' ? (
                <button type="button" onClick={() => void api.stopPreview().then(setPreview)}>
                  停止预览
                </button>
              ) : (
                <button
                  type="button"
                  className="primary"
                  onClick={() => void startPreview()}
                  disabled={!project}
                >
                  启动预览
                </button>
              ))}
            <Chip tone={savedTone}>{savedLabel}</Chip>
            {autoBackup && (
              <Chip
                tone={autoBackup.needs_acknowledgement ? 'warning' : 'neutral'}
                title={autoBackup.description}
              >
                {autoBackup.needs_acknowledgement
                  ? '自动备份待确认(发现疑似凭据)'
                  : autoBackup.decision.decision === 'due'
                    ? '自动备份即将执行'
                    : autoBackup.description}
              </Chip>
            )}
            {issues.length > 0 && (
              <Chip tone="warning" title={issues.join('\n')}>
                {issues.length} 个索引问题
              </Chip>
            )}
          </div>
        </header>

        <div className="content">
          {/* An error appears HERE — at the top of the content column, where the user is looking — rather
              than only in the status bar or a log. It is the same treatment as every other state. */}
          {error && (
            <div className="page">
              <StateBlock
                tone="error"
                title="出错了"
                description={error}
                onDismiss={() => setError(null)}
              />
            </div>
          )}

          {view === 'attachments' ? (
            <AttachmentPanel
              enabled={project !== null}
              onInsertMarkdown={(markdown) => {
                // §13.3: the pool never rewrites an article. The Markdown is appended to the buffer here and the
                // user saves when they choose, so adding a picture cannot edit a document by itself.
                setView('editor');
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
          ) : view === 'network-images' ? (
            <div className="page page--legacy">
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
            </div>
          ) : view === 'works' ? (
            <div className="page page--legacy">
              <WorksPanel enabled={project !== null} />
            </div>
          ) : view === 'backups' ? (
            <div className="page page--legacy">
              <BackupPanel enabled={project !== null} />
            </div>
          ) : view === 'deploy' ? (
            <div className="page page--legacy">
              <DeployPanel enabled={project !== null} />
            </div>
          ) : view === 'publish' ? (
            <div className="page page--legacy">
              {/* `projectPath` is passed so the panel re-reads the install state when a DIFFERENT
                  project is opened: the Rust side clears it with the project, and a panel that only
                  watched `enabled` would keep showing the closed project's install. */}
              <PublishPanel
                enabled={project !== null}
                projectPath={project?.path ?? null}
                onPublished={() => void refresh()}
              />
            </div>
          ) : view === 'logs' ? (
            <div className="page page--legacy">
              <LogView enabled={project !== null} />
            </div>
          ) : (
            <div className="page page--workspace">
              <div
                className="workspace"
                ref={splitRef}
                style={{ '--editor-share': `${splitPercent}%` } as React.CSSProperties}
              >
                <section className="pane">
                  {document ? (
                    <>
                      <PageHeader
                        title={
                          <input
                            className="title-input"
                            value={document.title}
                            readOnly
                            title="标题在 frontmatter 中编辑"
                          />
                        }
                        description={document.route}
                        actions={
                          <div className="wip-controls">
                            {document.wip_state === 'draft' && (
                              <button
                                type="button"
                                className="primary"
                                onClick={() => void transition('publish_wip')}
                              >
                                发布 WIP
                              </button>
                            )}
                            {document.wip_state === 'published_wip' && (
                              <>
                                <button
                                  type="button"
                                  onClick={() => void transition('unpublish_wip')}
                                >
                                  取消发布
                                </button>
                                <button
                                  type="button"
                                  className="primary"
                                  onClick={() => void transition('finish')}
                                >
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
                        }
                      />

                      {conflict && (
                        <StateBlock
                          tone="error"
                          title="保存被拒绝:文件在磁盘上已改变"
                          description={
                            <>
                              <p>{conflict.conflict_message}</p>
                              <p className="muted">
                                Veloct 没有覆盖磁盘上的内容。你的编辑仍在编辑器里。
                              </p>
                            </>
                          }
                          actions={
                            <>
                              <button
                                type="button"
                                onClick={acceptDisk}
                                disabled={!conflict.disk_source}
                              >
                                采用磁盘版本(放弃我的编辑)
                              </button>
                              <button
                                type="button"
                                className="primary"
                                onClick={() => {
                                  setConflict(null);
                                  void autosave.saveNow();
                                }}
                              >
                                保留我的编辑并重试
                              </button>
                            </>
                          }
                        />
                      )}

                      {document.diagnostics.length > 0 && (
                        <StateBlock
                          tone="warning"
                          title="文档诊断"
                          description={
                            <ul className="bullet-list">
                              {document.diagnostics.map((line) => (
                                <li key={line}>{line}</li>
                              ))}
                            </ul>
                          }
                        />
                      )}

                      <div className="pane-body">
                        <Editor
                          source={source}
                          editable
                          onChange={(next) => {
                            setSource(next);
                            autosave.onChange(next);
                          }}
                        />
                      </div>
                    </>
                  ) : (
                    <>
                      <PageHeader
                        title="编辑"
                        description="选中左侧的一篇内容后在这里写作;右侧是真实站点的预览。"
                      />
                      <div className="pane-body pane-body--center">
                        <StateBlock
                          tone="empty"
                          title={project ? '还没有打开任何内容' : '还没有打开项目'}
                          description={
                            project
                              ? '从左侧选择一篇内容,或新建一篇。'
                              : '打开一个项目目录以开始。'
                          }
                        />
                      </div>
                    </>
                  )}
                </section>

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
                    // The share is a percentage OF THE WORKSPACE, so the ratio is measured on the workspace.
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

                <section className="pane">
                  <header className="pane-head">
                    <h2 className="pane-title">站点预览</h2>
                    <span className="pane-note">{previewLabel}</span>
                  </header>
                  <div className="pane-body">
                    {preview?.site_url ? (
                      // The REAL Astro dev server, not a second renderer (handoff.md section 5.1).
                      <iframe
                        title="站点预览"
                        src={preview.site_url}
                        sandbox="allow-same-origin allow-scripts"
                      />
                    ) : (
                      <div className="pane-body--center">
                        <StateBlock
                          tone="empty"
                          title="预览还没有启动"
                          description={
                            previewState === 'crashed' && preview?.site.state === 'crashed'
                              ? preview.site.reason
                              : '预览使用与最终站点同一套 Astro 工程。启动后会在这里显示。'
                          }
                        />
                      </div>
                    )}
                  </div>
                </section>
              </div>
            </div>
          )}
        </div>

        <footer className="statusbar">
          <span>{status}</span>
          <span className="statusbar-end muted">
            {project ? project.name : '未打开项目'}
          </span>
        </footer>
      </div>

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
            setView('editor');
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
    </div>
  );
}
