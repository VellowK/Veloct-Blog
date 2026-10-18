/**
 * The workbench shell.
 *
 * The owner's latest writing-workflow request keeps only the full-width WYSIWYG editor: a second
 * reading pane duplicates what the editor already shows. The separate site/appearance views still
 * use the REAL Astro project for final theme/layout verification.
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
 *     content browser. Every supported top-level view lives in the navigation; removed features have
 *     no navigation entries or command surface.
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
 *  * **Writing has one full-width pane.** The WYSIWYG surface and its optional MD source mode share the
 *    same editor; final site preview is a separate view, never a second writing column.
 */

import { open as chooseDirectory } from '@tauri-apps/plugin-dialog';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
  api,
  asCommandError,
  describeError,
  type ContentKind,
  type DocumentView,
  type AutoBackupStateView,
  type PreviewView,
  type ProjectView,
  type RibbonEntry,
  type SaveView,
  type WipTransition,
  type SiteSettings,
} from './lib/api';
import Editor, { type EditorHandle } from './Editor';
import DocumentResourcePanel from './DocumentResourcePanel';
import AttachmentPanel from './AttachmentPanel';
import BackupPanel from './BackupPanel';
import SiteSettingsPanel from './SiteSettingsPanel';
import NavigationPanel, { type CustomMenu } from './NavigationPanel';
import DeployPanel from './DeployPanel';
import LogView from './LogView';
import { useNavigationHistory } from './lib/navigationHistory';
import { Feedback, notify, type NoticeTone } from './components/Notifications';
import PublishPanel from './PublishPanel';
import WorksPanel from './WorksPanel';
import NetworkImagePanel from './NetworkImagePanel';
import Dashboard from './Dashboard';
import PreviewPanel from './PreviewPanel';
import LinkedKnowledgePanel from './LinkedKnowledgePanel';
import brandLogo from '../src-tauri/icons-logo/128x128@2x.png';
import { Chip, Icon, PageHeader, StateBlock, type IconName } from './components/ui';
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
const PREVIEW_IDLE_MS = 5 * 60_000;

/** Every top-level view. Nothing that used to be a button is missing from this list. */
type ViewId =
  | 'dashboard'
  | 'articles'
  | 'pages'
  | 'knowledge'
  | 'preview'
  | 'editor'
  | 'logs'
  | 'publish'
  | 'attachments'
  | 'works'
  | 'backups'
  | 'settings'
  | 'appearance'
  | 'navigation';

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
    label: '仪表盘',
    entries: [
      { id: 'dashboard', label: '仪表盘', icon: 'grid', title: '查看内容概览与近期文档' },
    ],
  },
  {
    label: '工作区',
    entries: [
      { id: 'preview', label: '页面预览', icon: 'grid', title: '打开站点预览页' },
      { id: 'articles', label: '文章管理', icon: 'edit', title: '管理和编辑文章' },
      { id: 'pages', label: '页面管理', icon: 'grid', title: '管理和编辑页面' },
      { id: 'knowledge', label: '知识库管理', icon: 'archive', title: '管理和编辑知识库' },
      { id: 'attachments', label: '附件管理', icon: 'clip', title: '管理上传的图片与文件' },
      { id: 'works', label: '作品管理', icon: 'grid', title: '导入和管理独立作品' },
    ],
  },
  {
    label: '其他选项',
    entries: [
      { id: 'appearance', label: '外观', icon: 'sparkle', title: '管理站点信息与站点美化' },
      { id: 'publish', label: '构建与发布', icon: 'tag', title: '检查改动、构建一次并打标签(handoff.md 第 19.3 节)' },
    ],
  },
  {
    label: '维护',
    entries: [
      { id: 'navigation', label: '菜单管理', icon: 'menu', title: '管理网站左侧菜单' },
      { id: 'backups', label: '备份与恢复', icon: 'archive', title: '备份与恢复(handoff.md 第 17 节)' },
      { id: 'logs', label: '运行日志', icon: 'logs', title: '查看运行日志(handoff.md 第 18 节)' },
    ],
  },
];

const ALL_ENTRIES: NavEntry[] = [
  ...NAV_GROUPS.flatMap((group) => group.entries),
  { id: 'editor', label: '编辑器', icon: 'edit', title: '编辑文档' },
  { id: 'settings', label: '设置', icon: 'sliders', title: '应用与站点设置' },
];
const CONTENT_VIEWS: Record<ContentKind, ViewId> = { article: 'articles', page: 'pages', knowledge: 'knowledge' };

const PROJECT_HISTORY_KEY = 'veloct.project-history';
const LAST_PROJECT_KEY = 'veloct.last-project';
const SIDEBAR_COLLAPSED_KEY = 'veloct.sidebar-collapsed';

function readProjectHistory(): Array<{ path: string; name: string }> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(PROJECT_HISTORY_KEY) ?? '[]');
    return Array.isArray(value)
      ? value.filter((item): item is { path: string; name: string } =>
          typeof item?.path === 'string' && typeof item?.name === 'string',
        ).slice(0, 8)
      : [];
  } catch {
    return [];
  }
}

function rememberProject(path: string, name: string) {
  const history = [{ path, name }, ...readProjectHistory().filter((item) => item.path !== path)].slice(0, 8);
  localStorage.setItem(PROJECT_HISTORY_KEY, JSON.stringify(history));
  localStorage.setItem(LAST_PROJECT_KEY, path);
  return history;
}

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

function ContentManager({
  kind,
  documents,
  onSelect,
  onCreate,
  onDelete,
}: {
  kind: ContentKind;
  documents: RibbonEntry[];
  onSelect: (entry: RibbonEntry) => void;
  onCreate: () => void;
  onDelete: (entry: RibbonEntry) => Promise<void>;
}) {
  const [pendingDelete, setPendingDelete] = useState<RibbonEntry | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deletingRef = useRef(false);
  useEffect(() => {
    setPendingDelete(null);
    setDeleteError(null);
  }, [kind]);
  const confirmDelete = async () => {
    if (!pendingDelete || deletingRef.current) return;
    deletingRef.current = true;
    setDeleting(true);
    setDeleteError(null);
    try {
      await onDelete(pendingDelete);
      setPendingDelete(null);
    } catch (caught) {
      setDeleteError(describeError(caught));
    } finally {
      deletingRef.current = false;
      setDeleting(false);
    }
  };
  const entries = documents.filter((entry) => entry.kind === kind);
  return (
    <div className="page content-manager">
      <PageHeader
        title={`${KIND_LABEL[kind]}管理`}
        description={`管理${KIND_LABEL[kind]}内容,选择一项进入编辑器。`}
        actions={<button type="button" className="primary" onClick={onCreate}>新建{KIND_LABEL[kind]}</button>}
      />
      {entries.length === 0 ? (
        <StateBlock tone="empty" title={`还没有${KIND_LABEL[kind]}`} description={`新建一篇${KIND_LABEL[kind]}开始编辑。`} actions={<button type="button" className="primary" onClick={onCreate}>新建{KIND_LABEL[kind]}</button>} />
      ) : (
        <ul className="manager-list">
          {entries.map((entry) => (
            <li className="manager-row" key={`${entry.kind}:${entry.id}`}>
              <button type="button" className="manager-item" disabled={deleting} onClick={() => onSelect(entry)}>
                <span className="manager-item-title">{entry.title}</span>
                <span className="manager-item-meta"><span className={`badge ${entry.wip_state}`}>{entry.wip_state}</span>{!entry.is_buildable && <span className="badge draft-only">不进入构建</span>}</span>
              </button>
              <div className="inline-delete" onKeyDown={(event) => { if (event.key === 'Escape' && !deleting) { setPendingDelete(null); setDeleteError(null); } }}>
                <div className="inline-delete__actions">
                  <span className="inline-delete__cancel">
                    {pendingDelete?.id === entry.id && <button type="button" disabled={deleting} onClick={() => { setPendingDelete(null); setDeleteError(null); }}>取消</button>}
                  </span>
                  {pendingDelete?.id === entry.id ? (
                    <button key="confirm" type="button" autoFocus className="danger inline-delete__button is-confirming" disabled={deleting} aria-label={`确认删除${KIND_LABEL[entry.kind]}：${entry.title}`} title="永久删除本地内容，无法撤销；已发布的网站需重新部署才会更新" onClick={() => void confirmDelete()}>{deleting ? '删除中…' : '确认删除'}</button>
                  ) : (
                    <button key="delete" type="button" className="danger inline-delete__button" disabled={deleting} aria-label={`删除${KIND_LABEL[entry.kind]}：${entry.title}`} onClick={() => { setPendingDelete(entry); setDeleteError(null); }}>删除</button>
                  )}
                </div>
                {pendingDelete?.id === entry.id && <span className="inline-delete__hint" role={deleteError ? 'alert' : 'status'}>{deleteError ?? '永久删除，无法撤销'}</span>}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default function App() {
  const [project, setProject] = useState<ProjectView | null>(null);
  const [documents, setDocuments] = useState<RibbonEntry[]>([]);
  const [document, setDocument] = useState<DocumentView | null>(null);
  const [source, setSource] = useState('');
  const editorHandle = useRef<EditorHandle>(null);
  const [resourceSidebarOpen, setResourceSidebarOpen] = useState(true);
  const [preview, setPreview] = useState<PreviewView | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewReady, setPreviewReady] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  // State alone cannot lock two calls made before React's next render.
  const previewBusyRef = useRef(false);
  const previewOperationRef = useRef(0);
  const previewAutoAttemptRef = useRef<string | null>(null);
  const [issues, setIssues] = useState<string[]>([]);
  const [status, setStatus] = useState('正在启动…');
  const reportStatus = (message: string, tone: NoticeTone = 'info') => { setStatus(message); notify(message, tone); };
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<SaveView | null>(null);
  const [pathInput, setPathInput] = useState('');
  const [projectHistory, setProjectHistory] = useState(() => readProjectHistory());
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === 'true');
  const [settingsSection, setSettingsSection] = useState<'site' | 'deploy' | 'attachments' | 'network-images' | 'backup'>('site');
  const [appearanceSection, setAppearanceSection] = useState<'info' | 'beautify' | 'filing'>('info');
  const [previewRevision, setPreviewRevision] = useState(0);
  const [siteDraft, setSiteDraft] = useState<SiteSettings | null>(null);
  const [attachmentIssueCount, setAttachmentIssueCount] = useState(0);
  useEffect(() => {
    let cancelled = false;
    let checking = false;
    setAttachmentIssueCount(0);
    if (!project) return;
    const refreshMessages = async (check = false) => {
      if (checking) return;
      checking = true;
      try {
        const report = await (check ? api.checkAttachmentImages() : api.attachmentMessages());
        if (!cancelled) {
          setAttachmentIssueCount(new Set([...report.upload_errors, ...report.health_errors].map((m) => m.attachment_id)).size);
          if (check) window.dispatchEvent(new Event('veloct-attachment-messages'));
        }
      } catch { /* Connection errors belong in the attachment manager, not a global modal. */ }
      finally { checking = false; }
    };
    const refresh = () => { void refreshMessages(); };
    void refreshMessages().then(() => { if (!cancelled) void refreshMessages(true); });
    const timer = window.setInterval(() => { if (!window.document.hidden) void refreshMessages(true); }, 300_000);
    window.addEventListener('veloct-attachment-messages', refresh);
    return () => { cancelled = true; window.clearInterval(timer); window.removeEventListener('veloct-attachment-messages', refresh); };
  }, [project?.path]);

  /** The single active view. Mutually exclusive, which seven booleans could not guarantee. */
  const [view, setView] = useState<ViewId>('dashboard');
  const [customMenus, setCustomMenus] = useState<CustomMenu[]>([
    { id: 'home', label: '首页', url: '/', visible: true, builtin: true },
    { id: 'articles', label: '文章', url: '/article/', visible: true, builtin: true },
    { id: 'pages', label: '页面', url: '/about/', visible: true, builtin: true },
    { id: 'archive', label: '归档', url: '/archive/', visible: true, builtin: true },
    { id: 'links', label: '友链', url: '/links/', visible: true, builtin: true },
  ]);
  // The automatic backup rule (handoff.md section 17.5).
  const [autoBackup, setAutoBackup] = useState<AutoBackupStateView | null>(null);

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
        let current = await api.currentProject();
        if (!current) {
          const lastPath = localStorage.getItem(LAST_PROJECT_KEY);
          if (lastPath) {
            try {
              current = await api.openProject(lastPath);
              setProjectHistory(rememberProject(current.path, current.name));
            } catch {
              localStorage.removeItem(LAST_PROJECT_KEY);
            }
          }
        }
        setProject(current);
        setStatus(`${info.name} ${info.version}`);
        if (current) await refresh();
      } catch (caught) {
        setError(describeError(caught));
        setStatus('启动失败');
      }
    })();
  }, [refresh]);

  // Poll child liveness, but never let an older poll overwrite a start/stop response.
  // Changing projects also invalidates any response still in flight for the old project.
  useEffect(() => {
    let cancelled = false;
    let polling = false;
    setPreviewReady(false);
    setPreview(null);
    setPreviewError(null);
    previewAutoAttemptRef.current = null;
    if (!project) return;
    const ask = async () => {
      if (polling || previewBusyRef.current) return;
      polling = true;
      const operation = previewOperationRef.current;
      try {
        const next = await api.previewStatus();
        if (!cancelled && operation === previewOperationRef.current) {
          setPreview(next);
          setPreviewReady(true);
        }
      } catch (caught) {
        if (!cancelled && operation === previewOperationRef.current) {
          setPreviewError(`无法读取预览状态：${describeError(caught)}`);
          setPreviewReady(true);
        }
      } finally {
        polling = false;
      }
    };
    void ask();
    const timer = window.setInterval(() => void ask(), PREVIEW_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [project?.path]);

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

  const openProject = async (requestedPath = pathInput.trim()) => {
    try {
      let selectedPath = requestedPath;
      if (!selectedPath) {
        const choice = await chooseDirectory({ directory: true, multiple: false, title: '选择 Veloct 项目文件夹' });
        if (typeof choice !== 'string') return;
        selectedPath = choice;
        setPathInput(choice);
      }
      const opened = await api.openProject(selectedPath);
      setProject(opened);
      setProjectHistory(rememberProject(opened.path, opened.name));
      setError(null);
      setDocument(null);
      setSource('');
      await refresh();
    } catch (caught) {
      setError(describeError(caught));
    }
  };

  const toggleSidebar = () => {
    setSidebarCollapsed((collapsed) => {
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(!collapsed));
      return !collapsed;
    });
  };

  const selectDocument = async (entry: RibbonEntry) => {
    try {
      const loaded = await api.readDocument(entry.kind, entry.id);
      setDocument(loaded);
      setSource(loaded.source);
      setConflict(null);
      setView('editor');
    } catch (caught) {
      setError(describeError(caught));
    }
  };

  const deleteDocument = async (entry: RibbonEntry) => {
    await api.deleteDocument(entry.kind, entry.id);
    if (document?.kind === entry.kind && document.id === entry.id) {
      setDocument(null);
      setSource('');
      setConflict(null);
    }
    setDocuments((current) => current.filter((item) => item.kind !== entry.kind || item.id !== entry.id));
    reportStatus(`已删除${KIND_LABEL[entry.kind]}“${entry.title}”`, 'success');
    setPreviewRevision((revision) => revision + 1);
    await refresh();
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
    reportStatus('已采用磁盘版本');
  };

  const projectPathRef = useRef(project?.path);
  projectPathRef.current = project?.path;

  const changePreview = useCallback(async (start: boolean) => {
    const projectPath = project?.path;
    if (!projectPath || previewBusyRef.current) return;
    previewBusyRef.current = true;
    previewOperationRef.current += 1;
    setPreviewBusy(true);
    setPreviewError(null);
    try {
      const next = await (start ? api.startPreview('site') : api.stopPreview('site'));
      if (projectPathRef.current === projectPath) {
        setPreview(next);
        setPreviewReady(true);
        if (!start) setStatus('预览已停止');
      }
    } catch (caught) {
      if (projectPathRef.current === projectPath) {
        const command = asCommandError(caught);
        const message = command.hint ? `${command.message} — ${command.hint}` : command.message;
        setPreviewError(message);
        setStatus(`站点预览：${message}`);
      }
    } finally {
      previewBusyRef.current = false;
      setPreviewBusy(false);
    }
  }, [project?.path]);

  const startPreview = useCallback(() => changePreview(true), [changePreview]);

  const projectScope = useRef(project?.path ?? null);
  projectScope.current = project?.path ?? null;
  const route = {
    view,
    settingsSection: view === 'settings' ? settingsSection : null,
    appearanceSection: view === 'appearance' ? appearanceSection : null,
    doc: view === 'editor' && document ? { kind: document.kind, id: document.id } : null,
  };
  const navigation = useNavigationHistory(route, project?.path ?? null, async previous => {
    const scope = projectScope.current;
    if (previous.doc && (document?.id !== previous.doc.id || document?.kind !== previous.doc.kind)) {
      const loaded = await api.readDocument(previous.doc.kind, previous.doc.id);
      if (scope !== projectScope.current) return;
      setDocument(loaded); setSource(loaded.source); setConflict(null);
    }
    if (previous.settingsSection) setSettingsSection(previous.settingsSection);
    if (previous.appearanceSection) setAppearanceSection(previous.appearanceSection);
    setView(previous.view);
  }, caught => setError(describeError(caught)));

  const previewState = preview?.site.state ?? 'stopped';
  const previewRetrying = preview?.site.state === 'crashed' && preview.site.will_retry;
  // Markdown writing no longer consumes Astro. Only the actual site/appearance views keep it alive.
  // Polling updates must not reset an idle timeout.
  const showingSiteInfo = view === 'appearance' && appearanceSection === 'info';
  const previewInUse = view === 'preview' || showingSiteInfo;
  const previewInUseRef = useRef(previewInUse);
  previewInUseRef.current = previewInUse;

  useEffect(() => {
    if (view !== 'preview' && !showingSiteInfo) {
      previewAutoAttemptRef.current = null;
      return;
    }
    const projectPath = project?.path;
    if (!projectPath || !previewReady || previewBusy || previewState === 'running'
      || previewState === 'starting' || previewRetrying) return;
    // One automatic attempt per visit, not a retry loop on every two-second poll.
    // A failed start stays visible and can be retried explicitly from the error state.
    if (previewAutoAttemptRef.current === projectPath) return;
    previewAutoAttemptRef.current = projectPath;
    void startPreview();
  }, [view, showingSiteInfo, project?.path, previewReady, previewBusy, previewState, previewRetrying, startPreview]);

  useEffect(() => {
    if (!project || previewInUse || previewBusy
      || (previewState !== 'running' && previewState !== 'starting' && !previewRetrying)) return;
    const timer = window.setTimeout(() => {
      // Recheck at the boundary in case navigation and the timeout happen together.
      if (!previewInUseRef.current && !previewBusyRef.current) {
        setStatus('预览空闲已满 5 分钟，正在自动停止');
        void changePreview(false);
      }
    }, PREVIEW_IDLE_MS);
    return () => window.clearTimeout(timer);
    // Use liveness, not the polled object/state: starting → running is not a new idle period.
  }, [project?.path, previewInUse, previewBusy,
    previewState === 'running' || previewState === 'starting' || previewRetrying, changePreview]);
  useEffect(() => {
    if (!project?.path) return;
    const key = `veloct.navigation:${project.path}`;
    try {
      const saved = JSON.parse(localStorage.getItem(key) ?? 'null');
      if (Array.isArray(saved)) setCustomMenus(saved);
    } catch { /* use built-in defaults */ }
  }, [project?.path]);

  const updateCustomMenus = (menus: CustomMenu[]) => {
    setCustomMenus(menus);
    if (project?.path) localStorage.setItem(`veloct.navigation:${project.path}`, JSON.stringify(menus));
  };

  const activeNavId: ViewId = view === 'editor' && document
    ? CONTENT_VIEWS[document.kind]
    : view;
  const currentEntry = ALL_ENTRIES.find((entry) => entry.id === activeNavId);
  const sectionLabel = view === 'appearance'
    ? { info: '站点信息', beautify: '站点美化', filing: '备案管理' }[appearanceSection]
    : view === 'settings'
      ? { site: '站点设置', deploy: '部署目标', attachments: '附件存储', 'network-images': '网络图片', backup: '备份设置' }[settingsSection]
      : null;
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
      <aside className={`sidebar${sidebarCollapsed ? ' sidebar--collapsed' : ''}`}>
        <div className="brand">
          <img src={brandLogo} className="brand-mark" alt="Veloct" />
          <span className="brand-word">Veloct</span>
          <button type="button" className="sidebar-toggle" onClick={toggleSidebar} aria-label={sidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'} title={sidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'}>
            <Icon name={sidebarCollapsed ? 'expand' : 'collapse'} />
          </button>
        </div>

        {!project ? (
          <div className="open-project">
            <p className="muted">打开或新建一个项目目录。</p>
            <input
              value={pathInput}
              onChange={(event) => setPathInput(event.target.value)}
              onKeyDown={(event) => { if (event.key === 'Enter') void openProject(); }}
              placeholder="D:/我的站点"
              spellCheck={false}
            />
            <button type="button" className="primary" onClick={() => void openProject()}>
              打开项目
            </button>
            {projectHistory.length > 0 && (
              <div className="recent-projects">
                <p className="nav-group-label">最近打开</p>
                {projectHistory.map((item) => (
                  <button key={item.path} type="button" className="recent-project" title={item.path} onClick={() => void openProject(item.path)}>
                    <span>{item.name}</span><small>{item.path}</small>
                  </button>
                ))}
              </div>
            )}
          </div>
        ) : (
          <div className="project-switcher">
            <p className="sidebar-project" title={project.path}>{project.name}</p>
            <button type="button" onClick={() => void openProject('')}>切换项目</button>
          </div>
        )}

        {/* Every top-level view is here, grouped, and reachable whether or not a project is open: a page
            that cannot work yet says so in its own empty state rather than disappearing from the nav. */}
        <nav className="nav" aria-label="主导航">
          {NAV_GROUPS.map((group) => (
            <div className="nav-group" key={group.label}>
              <p className="nav-group-label">{group.label}</p>
              <ul className="nav-list">
                {group.entries.map((entry) => (
                  <li key={`${entry.id}-${entry.label}`}>
                    <button
                      type="button"
                      className={`nav-item${activeNavId === entry.id ? ' is-active' : ''}`}
                      aria-current={activeNavId === entry.id ? 'page' : undefined}
                      title={entry.title}
                      onClick={() => setView(entry.id)}
                    >
                      <Icon name={entry.icon} />
                      <span className="nav-label">{entry.label}</span>
                      {entry.id === 'attachments' && attachmentIssueCount > 0 && <span className="attachment-message-bubble" aria-label={`${attachmentIssueCount} 个附件需要处理`}>{attachmentIssueCount}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>

        <div className="sidebar-footer">
          <button
            type="button"
            className={`nav-item${view === 'settings' ? ' is-active' : ''}`}
            aria-current={view === 'settings' ? 'page' : undefined}
            title="部署、资源与应用设置"
            onClick={() => setView('settings')}
          >
            <Icon name="settings" />
            <span className="nav-label">设置</span>
          </button>
        </div>
      </aside>

      <div className="main">
        <header className="topbar">
          <div className="topbar-context">
            <button type="button" className="history-back" aria-label="返回上一页" title="返回上一页" disabled={!navigation.canGoBack} onClick={() => void navigation.goBack()}><Icon name="collapse" /></button>
            <p className="crumb">
              <button type="button" className="crumb-link crumb-project" onClick={() => setView('dashboard')}>{project ? project.name : '未打开项目'}</button>
              <span className="crumb-sep">/</span>
              <button type="button" className="crumb-link crumb-page" onClick={() => setView(activeNavId)}>{currentEntry?.label ?? '编辑器'}</button>
              {sectionLabel && <><span className="crumb-sep">/</span><span className="crumb-leaf">{sectionLabel}</span></>}
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
                <button type="button" className="site-preview-toggle" title="仅停止独立站点预览，不影响所见即所得编辑" disabled={previewBusy} onClick={() => void changePreview(false)}>
                  停止预览
                </button>
              ) : (
                <button
                  type="button"
                  className="primary site-preview-toggle"
                  title="启动独立站点预览；所见即所得编辑无需启动站点"
                  onClick={() => void startPreview()}
                  disabled={!project || previewBusy || previewState === 'starting'}
                >
                  启动预览
                </button>
              ))}
            <span className="save-status" role="status" aria-live="polite" aria-atomic="true">
              <Chip tone={savedTone} title={savedLabel}>{savedLabel}</Chip>
            </span>
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

        <div
          key={view}
          className={`content content--rise${view === 'preview' ? ' content--preview' : ''}${showingSiteInfo ? ' content--site-info' : ''}`}
        >
          <Feedback message={error} tone="error" title="出错了" />

          {view === 'dashboard' ? (
            <Dashboard documents={documents} onSelect={(entry) => { void selectDocument(entry); }} />
          ) : view === 'articles' ? (
            <ContentManager kind="article" documents={documents} onDelete={deleteDocument} onCreate={() => void createDocument('article')} onSelect={(entry) => { void selectDocument(entry); }} />
          ) : view === 'pages' ? (
            <ContentManager kind="page" documents={documents} onDelete={deleteDocument} onCreate={() => void createDocument('page')} onSelect={(entry) => { void selectDocument(entry); }} />
          ) : view === 'knowledge' ? (
            <div className="knowledge-workspace">
              <ContentManager kind="knowledge" documents={documents} onDelete={deleteDocument} onCreate={() => void createDocument('knowledge')} onSelect={(entry) => { void selectDocument(entry); }} />
              <LinkedKnowledgePanel key={project?.path ?? 'none'} enabled={project !== null} />
            </div>
          ) : view === 'attachments' ? (
            <AttachmentPanel
              key={project?.path ?? "no-project"}
              onOpenSettings={() => { setSettingsSection("attachments"); setView("settings"); }}
              enabled={project !== null}
              onInsertMarkdown={(markdown) => {
                // §13.3: the pool never rewrites an article. The Markdown is appended to the buffer here and the
                // user saves when they choose, so adding a picture cannot edit a document by itself.
                if (document) {
                  setView('editor');
                  const next = `${source.trimEnd()}\n\n${markdown}\n`;
                  setSource(next);
                  autosave.onChange(next);
                  reportStatus('已将附件 Markdown 插入编辑器,保存后生效', 'success');
                } else {
                  reportStatus('请先打开一篇文章,再插入附件 Markdown', 'warning');
                }
              }}
            />
          ) : view === 'works' ? (
            <div className="page page--legacy">
              <WorksPanel enabled={project !== null} />
            </div>
          ) : view === 'backups' ? (
            <div className="page page--legacy">
              <BackupPanel enabled={project !== null} />
            </div>

          ) : view === 'navigation' ? (
            <NavigationPanel key={project?.path ?? 'no-project'} menus={customMenus} pages={documents} onChange={updateCustomMenus} />
          ) : view === 'publish' ? (
            <div className="page page--legacy">
              {/* `projectPath` is passed so the panel re-reads the install state when a DIFFERENT
                  project is opened: the Rust side clears it with the project, and a panel that only
                  watched `enabled` would keep showing the closed project's install. */}
              <PublishPanel
                enabled={project !== null}
                projectPath={project?.path ?? null}
                                 onInitializeProject={async () => {
                   try {
                     const initialized = await api.initializeCurrentProject();
                     setProject(initialized);
                     setStatus('项目站点模板已初始化,正在刷新');
                     await refresh();
                   } catch (caught) {
                     setError(describeError(caught));
                   }
                 }}
                 onPublished={() => {
                  void refresh();
                  setPreviewRevision((revision) => revision + 1);
                  setStatus('发布完成,正在刷新预览');
                }}
              />
            </div>
          ) : view === 'appearance' ? (
            <div className="page page--legacy settings-page">
              <PageHeader title="外观" description="管理站点信息与站点美化" />
              <div className="settings-tabs" role="group" aria-label="外观分类">
                {([
                  ['info', '站点信息'],
                  ['beautify', '站点美化'],
                  ['filing', '备案管理'],
                ] as const).map(([key, label]) => (
                  <button key={key} type="button" className={appearanceSection === key ? 'is-active' : ''} aria-pressed={appearanceSection === key} onClick={() => setAppearanceSection(key)}>{label}</button>
                ))}
              </div>
              {appearanceSection === 'info' ? (
                <div className="site-info-workspace">
                  <div className="site-info-workspace__settings">
                  <SiteSettingsPanel
                    key={project?.path ?? 'no-project'}
                    enabled={project !== null}
                    projectPath={project?.path ?? null}
                    mode="info"
                    onEditHome={() => {
                      const home = documents.find((entry) => entry.kind === 'page' && entry.slug === 'home');
                      if (!home) {
                        setError('找不到 slug 为 home 的页面');
                        return;
                      }
                       void selectDocument(home);
                    }}
                    onDraftChange={setSiteDraft}
                  />
                  </div>
                  <PreviewPanel editorMode enabled={project !== null} preview={preview} busy={previewBusy || !previewReady} error={previewError} draftSettings={siteDraft} revision={previewRevision} onRetry={() => void startPreview()} />
                </div>
              ) : (
                <SiteSettingsPanel
                  key={`${project?.path ?? 'no-project'}:${appearanceSection}`}
                  enabled={project !== null}
                  projectPath={project?.path ?? null}
                  onlySection={appearanceSection}
                />
              )}
            </div>
          ) : view === 'settings' ? (
            <div className="page page--legacy settings-page">
              <PageHeader title="设置" description="部署、资源与应用配置" />
              <div className="settings-tabs" role="tablist" aria-label="设置分类">
                {([
                   ['site', '站点设置'],
                   ['deploy', '部署目标'],
                  ['attachments', '附件存储'],
                  ['network-images', '网络图片'],
                   ['backup', '备份设置'],

                ] as const).map(([key, label]) => (
                  <button key={key} type="button" className={settingsSection === key ? 'is-active' : ''} onClick={() => setSettingsSection(key)}>{label}</button>
                ))}
              </div>
              {settingsSection === 'site' && <SiteSettingsPanel key={project?.path ?? 'no-project'} enabled={project !== null} projectPath={project?.path ?? null} />}
               {settingsSection === 'deploy' && <DeployPanel enabled={project !== null} />}
              {settingsSection === 'attachments' && <AttachmentPanel key={project?.path ?? 'no-project'} enabled={project !== null} mode="settings" />}
              {settingsSection === 'network-images' && <NetworkImagePanel key={project?.path ?? 'no-project'} enabled={project !== null} mode="settings" onInsertMarkdown={(markdown) => {
                if (!document) { reportStatus('请先打开一篇文章,再插入网络图片 Markdown', 'warning'); return; }
                const next = `${source.trimEnd()}\n\n${markdown}\n`;
                setSource(next); autosave.onChange(next); reportStatus('已插入网络图片 Markdown', 'success');
              }} />}
               {settingsSection === 'backup' && <BackupPanel enabled={project !== null} mode="settings" />}

              <div className="settings-resource-links">
                <button type="button" onClick={() => setView('attachments')}><Icon name="clip" /> 打开附件管理</button>
              </div>
            </div>
          ) : view === 'preview' ? (
            <PreviewPanel
              enabled={project !== null}
              preview={preview}
              busy={previewBusy || !previewReady}
              error={previewError}
              revision={previewRevision}
              onRetry={() => void startPreview()}
            />
          ) : view === 'logs' ? (
            <div className="page page--legacy">
              <LogView enabled={project !== null} />
            </div>
          ) : view === 'editor' ? (
            <div className="page page--workspace">
              <div className={`workspace${document?.kind === 'article' && resourceSidebarOpen ? ' workspace--resources' : ''}`}>
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
                            <button type="button" onClick={() => setView('preview')}>打开站点预览</button>
                            {document.kind === 'article' ? <button type="button" aria-controls="document-resources" aria-expanded={resourceSidebarOpen} onClick={() => setResourceSidebarOpen(opened => !opened)}>{resourceSidebarOpen ? '收起资源栏' : '展开资源栏'}</button> : <button type="button" onClick={() => setView('attachments')}>附件管理</button>}
                            <button type="button" onClick={() => { setSettingsSection('network-images'); setView('settings'); }}>网络图片</button>
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
                          key={`${project?.path}:${document.kind}:${document.id}`}
                          ref={editorHandle}
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
                        description="选择内容后直接在这里所见即所得写作，也可切换 MD 源码；查看网站效果请打开站点预览。"
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
                {document?.kind === 'article' && <DocumentResourcePanel
                  key={`${project?.path}:${document.id}`}
                  documentId={document.id}
                  source={source}
                  hidden={!resourceSidebarOpen}
                  onInsertMarkdown={markdown => {
                    if (!editorHandle.current) throw new Error('编辑器尚未就绪，请稍后重试。');
                    editorHandle.current.insertMarkdown(markdown);
                  }}
                  onOpenManager={() => setView('attachments')}
                />}
              </div>
            </div>
          ) : null}
        </div>

        <footer className="statusbar">
          <span>{status}</span>
          <span className="statusbar-end muted">
            {project ? project.name : '未打开项目'}
          </span>
        </footer>
      </div>


    </div>
  );
}
