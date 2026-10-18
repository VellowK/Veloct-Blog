import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { Feedback } from './components/Notifications';
import { api, describeError, type AttachmentView, type AttachmentSyncSettings, type StorageTargetView, type UploadEligibility } from './lib/api';
import { isImageAttachment, syncUploadControl } from './lib/attachmentSync';
import { eligibilityFallback } from './lib/storageTargetForm';
import { attachmentResourceUrls, belongsToDocument, documentResourceNamespace, readDocumentResources, remoteImageMarkdown, resourceMatchesTab, resourceUrl, thumbnailUrl, type ResourceTab } from './lib/documentResources';
import './document-resources.css';
import { uploadTargets, storageThumbnail } from './lib/s3Form';

interface Props {
  documentId: string;
  source: string;
  hidden: boolean;
  onInsertMarkdown: (markdown: string) => void;
  onOpenManager: () => void;
}

function ResourceThumbnail({ url, label }: { url: string | null; label: string }) {
  const [failed, setFailed] = useState<string | null>(null);
  const safe = thumbnailUrl(url);
  return <div className="document-resource__thumbnail">{safe && failed !== safe
    ? <img src={safe} alt={label} loading="lazy" referrerPolicy="no-referrer" onError={() => setFailed(safe)} />
    : <span>{safe ? '图片暂不可预览' : url ? '站点相对图片' : '本地图片'}<br /><small>{safe ? '地址可能失效或不允许外链' : url ? '请在站点预览中查看' : '上传后可显示缩略图'}</small></span>}</div>;
}

/** Current-article resources only. Destructive/global cleanup stays in the existing manager. */
export default function DocumentResourcePanel({ documentId, source, hidden, onInsertMarkdown, onOpenManager }: Props) {
  const namespace = documentResourceNamespace('article', documentId);
  const [tab, setTab] = useState<ResourceTab>('attachments');
  const [pool, setPool] = useState<AttachmentView[]>([]);
  const [targets, setTargets] = useState<StorageTargetView[]>([]);
  const [sync, setSync] = useState<AttachmentSyncSettings>({ target_ids: [] });
  const [target, setTarget] = useState('');
  const [eligibility, setEligibility] = useState<Record<string, UploadEligibility>>({});
  const [reading, setReading] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [imageUrl, setImageUrl] = useState('');
  const [imageAlt, setImageAlt] = useState('');
  const live = useRef(false);
  const actionLock = useRef(false);
  const readVersion = useRef(0);
  const id = useId();
  const resources = useMemo(() => readDocumentResources(source), [source]);
  const current = useMemo(() => pool.filter(item => belongsToDocument(item, namespace, resources)), [pool, namespace, resources]);
  const currentIds = current.map(item => item.id).join('\0');
  const matchedImageUrls = new Set(pool.filter(item => isImageAttachment(item.original_name)).flatMap(attachmentResourceUrls));
  const externalImages = resources.images.filter(image => !matchedImageUrls.has(resourceUrl(image.url)));
  const matches = (value: string) => value.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
  const visible = current.filter(item => resourceMatchesTab(item, tab) && matches(item.original_name));
  const available = pool.filter(item => !current.some(own => own.id === item.id) && resourceMatchesTab(item, tab) && matches(item.original_name));
  const images = externalImages.filter(image => matches(image.alt));
  const counts = { attachments: current.filter(item => !isImageAttachment(item.original_name)).length, images: current.filter(item => isImageAttachment(item.original_name)).length + externalImages.length };

  const reload = useCallback(async () => {
    const version = ++readVersion.current;
    if (live.current) setReading(true);
    try {
      const [attachments, storage, settings] = await Promise.all([api.listAttachments(), api.storageTargets(), api.attachmentSyncSettings()]);
      if (!live.current || version !== readVersion.current) return;
      setPool(attachments);
      setTargets(storage);
      setSync(settings);
      setTarget(previous => uploadTargets(storage).some(item => item.id === previous && item.implemented) ? previous : '');
      setLoaded(true);
      setError(null);
    } catch (caught) {
      if (live.current && version === readVersion.current) setError(describeError(caught));
    } finally {
      if (live.current && version === readVersion.current) setReading(false);
    }
  }, []);

  useEffect(() => {
    live.current = true;
    void reload();
    const refresh = () => { void reload(); };
    window.addEventListener('veloct-attachment-messages', refresh);
    return () => { live.current = false; ++readVersion.current; window.removeEventListener('veloct-attachment-messages', refresh); };
  }, [reload]);

  useEffect(() => {
    let active = true;
    setEligibility({});
    const ids = currentIds ? currentIds.split('\0') : [];
    void Promise.all(ids.map(async attachmentId => {
      try { return [attachmentId, await api.uploadEligibility(attachmentId)] as const; }
      catch { return [attachmentId, eligibilityFallback(pool.find(item => item.id === attachmentId)?.upload_status ?? 'UNKNOWN')] as const; }
    })).then(entries => { if (active && live.current) setEligibility(Object.fromEntries(entries)); });
    return () => { active = false; };
  }, [currentIds, pool]);

  const run = async (action: () => Promise<void | boolean>, done: string) => {
    if (actionLock.current || !live.current) return;
    actionLock.current = true; setBusy(true); setError(null); setNotice(null);
    try {
      const changed = await action();
      if (!live.current || changed === false) return;
      await reload();
      if (!live.current) return;
      window.dispatchEvent(new Event('veloct-attachment-messages'));
      setNotice(done);
    } catch (caught) {
      if (!live.current) return;
      await reload();
      if (live.current) setError(describeError(caught));
    } finally {
      actionLock.current = false;
      if (live.current) setBusy(false);
    }
  };

  const insert = async (attachment: AttachmentView) => {
    const markdown = await api.attachmentMarkdown(attachment.id);
    if (live.current) onInsertMarkdown(markdown);
  };
  const addFiles = () => run(async () => {
    const selection = await open({ multiple: true, directory: false, title: tab === 'images' ? '加入本文图片池' : '加入本文附件池', ...(tab === 'images' ? { filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'avif', 'svg', 'bmp', 'ico', 'tif', 'tiff'] }] } : {}) });
    const paths = selection === null ? [] : typeof selection === 'string' ? [selection] : selection;
    if (!paths.length) return false;
    for (const sourcePath of paths) {
      if (!live.current) return;
      await api.addAttachment({ sourcePath, storageTargetId: target || 'local', namespace });
    }
  }, '文件已加入本文资源池，尚未插入正文，也未自动上传。');

  const upload = (attachment: AttachmentView) => run(async () => {
    const results = await api.syncUploadAttachment(attachment.id, target);
    const failures = results.filter(result => result.error);
    if (failures.length) throw new Error(`${results.length - failures.length}/${results.length} 个目标成功。${failures.map(result => `${result.target_id}: ${result.error}`).join('；')}。可重试，主地址不会自动切换。`);
  }, '上传完成，正文未改动。需要新地址时请点击「插入」。');

  return <aside className="document-resources" id="document-resources" aria-label="本文资源" hidden={hidden}>
    <header className="document-resources__head"><div><strong>本文资源</strong><span>仅当前文章 · 添加不改正文</span></div><button type="button" disabled={busy || reading} onClick={() => void reload()} aria-label="刷新本文资源">刷新</button></header>
    <div className="document-resources__tabs" role="tablist" aria-label="本文资源类型">
      {(['attachments', 'images'] as const).map((value, index, tabs) => <button key={value} type="button" role="tab" id={`${id}-${value}`} aria-controls={`${id}-panel`} aria-selected={tab === value} tabIndex={tab === value ? 0 : -1} onClick={() => setTab(value)} onKeyDown={event => {
        const next = event.key === 'Home' ? tabs[0] : event.key === 'End' ? tabs[1] : event.key === 'ArrowRight' || event.key === 'ArrowLeft' ? tabs[1 - index] : null;
        if (next) { event.preventDefault(); setTab(next); event.currentTarget.parentElement?.querySelector<HTMLButtonElement>(`[id="${id}-${next}"]`)?.focus(); }
      }}>{value === 'attachments' ? '附件池' : '图片池'}<span>{counts[value]}</span></button>)}
    </div>
    <div className="document-resources__body" role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${tab}`}>
      <div className="document-resources__tools">
        <button type="button" className="primary" disabled={busy || !loaded} onClick={() => void addFiles()}>{busy ? '处理中…' : tab === 'images' ? '＋ 添加本地图片' : '＋ 添加附件'}</button>
        <input type="search" aria-label="查找本文资源" placeholder="查找本文资源…" value={query} onChange={event => setQuery(event.target.value)} />
        <label className="document-resources__target">上传到<select aria-label="本文资源上传目标" value={target} disabled={busy} onChange={event => setTarget(event.target.value)}><option value="">仅本地缓存</option>{uploadTargets(targets).filter(item => item.implemented).map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
      </div>
      <Feedback message={error} tone="error" actions={<button type="button" disabled={reading || busy} onClick={() => void reload()}>重试读取</button>} />
      <Feedback message={notice} tone="success" />
      {!loaded && reading && <p className="document-resources__empty" role="status">正在读取本文资源…</p>}
      {loaded && visible.length === 0 && (tab !== 'images' || images.length === 0) && <p className="document-resources__empty">{query.trim() ? '没有匹配的本文资源。' : tab === 'images' ? '本文还没有图片。添加本地图片，或插入网络图片。' : '本文还没有附件。添加文件后，可在这里上传和插入。'}</p>}
      <ul className={`document-resources__list${tab === 'images' ? ' is-images' : ''}`}>
        {visible.map(attachment => {
          const control = syncUploadControl(attachment, sync, target, eligibility[attachment.id] ?? null);
          const inBody = attachmentResourceUrls(attachment).some(url => resources.urls.has(url));
          return <li className="document-resource" key={attachment.id}>
            {tab === 'images' && <ResourceThumbnail url={storageThumbnail(attachment.remote_url, targets.find(t => t.id === attachment.storage_target_id))} label={attachment.original_name} />}
            <strong title={attachment.original_name}>{attachment.original_name}</strong>
            <span className="document-resource__meta">{(attachment.size_bytes / 1024).toFixed(1)} KB · {inBody ? '正文已引用' : '待插入'} · {attachment.remote_url ? '已上传' : '本地缓存'}</span>
            <div className="document-resource__actions"><button type="button" disabled={busy} onMouseDown={event => event.preventDefault()} onClick={() => void run(() => insert(attachment), '已插入编辑位置，沿用文章自动保存。')}>插入</button><button type="button" disabled={busy || control.disabled} title={control.reason ?? undefined} onClick={() => void upload(attachment)}>{isImageAttachment(attachment.original_name) && sync.target_ids.length > 0 ? '同步上传' : '上传'}</button></div>
            {control.reason && <small className="document-resource__reason">{control.reason}</small>}
            {attachment.last_error && <small className="document-resource__error" role="alert">{attachment.last_error}</small>}
          </li>;
        })}
        {tab === 'images' && images.map(image => <li className="document-resource" key={image.url}>
          <ResourceThumbnail url={image.url} label={image.alt} />
          <strong title={image.url}>{image.alt}</strong><span className="document-resource__meta">正文已引用 · 非附件池图片</span><small className="document-resource__reason">不下载、不重新上传，不删除远程文件。</small>
        </li>)}
      </ul>
      {tab === 'images' && <details className="document-resources__details"><summary>插入网络图片</summary><form onSubmit={event => { event.preventDefault(); try { const markdown = remoteImageMarkdown(imageUrl, imageAlt); onInsertMarkdown(markdown); setImageUrl(''); setImageAlt(''); setError(null); setNotice('已插入远程地址，不下载、不进入附件缓存。'); } catch (caught) { setError(describeError(caught)); } }}><label>图片地址<input aria-label="网络图片地址" type="url" placeholder="https://…" required value={imageUrl} onChange={event => setImageUrl(event.target.value)} /></label><label>图片描述<input aria-label="网络图片描述" value={imageAlt} onChange={event => setImageAlt(event.target.value)} /></label><button type="submit" disabled={busy}>插入网络图片</button></form></details>}
      <details className="document-resources__details"><summary>从项目资源池选用 ({available.length})</summary><p>这里是其他资源，选用前不会计入本文。删除和远程清理请到全局管理。</p>{available.map(attachment => <div className="document-resources__available" key={attachment.id}><span title={attachment.original_name}>{attachment.original_name}</span><button type="button" disabled={busy} onMouseDown={event => event.preventDefault()} onClick={() => void run(() => insert(attachment), '已选用并插入本文，其他文章的引用保持不变。')}>选用</button></div>)}</details>
    </div>
    <footer className="document-resources__footer"><button type="button" onClick={onOpenManager}>全局附件管理</button><span>删除 / 远程清理在全局管理中操作</span></footer>
  </aside>;
}
