import { useEffect, useRef, useState, type CSSProperties } from 'react';
import type { PreviewView, SiteSettings } from './lib/api';
import { StateBlock } from './components/ui';
import './preview.css';

type Device = 'desktop' | 'mobile';
const ZOOM_LEVELS = [50, 75, 100, 125, 150] as const;

interface PreviewPanelProps {
  enabled: boolean;
  preview: PreviewView | null;
  busy: boolean;
  error: string | null;
  revision: number;
  onRetry: () => void;
  editorMode?: boolean;
  draftSettings?: SiteSettings | null;
}

/** One real site viewport, with controls floating above it rather than a second page header. */
export default function PreviewPanel({ enabled, preview, busy, error, revision, onRetry, editorMode = false, draftSettings = null }: PreviewPanelProps) {
  const [device, setDevice] = useState<Device>('desktop');
  const [zoom, setZoom] = useState(100);
  const [reload, setReload] = useState(0);
  const [frameError, setFrameError] = useState<string | null>(null);
  const [bounds, setBounds] = useState({ width: 0, height: 0 });
  const viewportRef = useRef<HTMLDivElement>(null);
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const state = preview?.site.state ?? 'stopped';
  const siteUrl = state === 'running' ? (editorMode ? `${preview?.site_url ?? ''}${preview?.site_url?.includes('?') ? '&' : '?'}veloct_editor=1` : preview?.site_url) : null;
  const sendEditorHandshake = () => {
    if (!iframeRef.current || !siteUrl) return;
    iframeRef.current.contentWindow?.postMessage({ type: 'veloct-site-editor-init' }, new URL(siteUrl).origin);
  };
  const sendDraft = () => {
    if (!iframeRef.current?.contentWindow || !draftSettings) return;
    iframeRef.current.contentWindow.postMessage({ type: 'veloct-site-draft', settings: draftSettings }, '*');
  };

  useEffect(() => { sendDraft(); }, [draftSettings]);


  const starting = enabled && (busy || state === 'starting');
  const failure = error ?? (preview?.site.state === 'crashed' ? preview.site.reason : null) ?? frameError;
  const retrying = preview?.site.state === 'crashed' && preview.site.will_retry;

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const measure = () => {
      const width = element.clientWidth;
      const height = element.clientHeight;
      setBounds((previous) => previous.width === width && previous.height === height ? previous : { width, height });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => setFrameError(null), [siteUrl, revision, reload]);

  const scale = zoom / 100;
  // At 100%, desktop fills the canvas. Zoom changes the actual CSS viewport and scales its rendering,
  // while mobile retains a true 390px viewport so the site's media queries behave like a phone.
  const width = device === 'mobile' ? 390 : Math.max(1, bounds.width / scale);
  const height = Math.max(1, bounds.height / scale);
  const frameStyle = {
    width,
    height,
    transform: `scale(${scale})`,
  } satisfies CSSProperties;
  const footprintStyle = {
    width: width * scale,
    height: height * scale,
  } satisfies CSSProperties;

  return (
    <section className="preview-panel" aria-label="页面预览">
      <div className="preview-panel__viewport" ref={viewportRef}>
        {siteUrl ? (
          <div className={`preview-panel__footprint preview-panel__footprint--${device}`} style={footprintStyle}>
            <iframe
              key={`${siteUrl}:${revision}:${reload}`}
              title={device === 'mobile' ? '站点移动端预览' : '站点桌面端预览'}
              src={siteUrl}
              style={frameStyle}
                             ref={iframeRef}
               onLoad={() => { sendEditorHandshake(); window.setTimeout(sendDraft, 0); }}
               sandbox="allow-same-origin allow-scripts allow-modals"
              onError={() => setFrameError('无法载入站点预览，请刷新重试或查看运行日志。')}
            />
          </div>
        ) : (
          <div className="preview-panel__state" aria-live="polite">
            <StateBlock
              tone={failure ? 'error' : starting || retrying ? 'loading' : 'empty'}
              title={!enabled ? '还没有打开项目' : failure ? '预览暂时不可用' : starting || retrying ? '正在启动站点预览…' : '正在准备预览'}
              description={!enabled ? '先打开一个项目，进入此页面后会自动启动站点。' : failure ?? '无需手动启动，站点就绪后会自动显示在这里。'}
              actions={enabled && failure && !starting && !retrying ? <button type="button" onClick={onRetry}>重试预览</button> : undefined}
            />
          </div>
        )}
      </div>

      {siteUrl && failure && (
        <div className="preview-panel__error" role="alert">
          <span>{failure}</span>
          <button type="button" onClick={() => { setFrameError(null); setReload((value) => value + 1); }}>刷新预览</button>
        </div>
      )}

      <div className="preview-toolbar" role="toolbar" aria-label="预览设备与缩放">
        <div className="preview-toolbar__devices" role="group" aria-label="预览设备">
          <button type="button" aria-pressed={device === 'desktop'} onClick={() => setDevice('desktop')} title="桌面端预览">
            <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"><rect x="3" y="4" width="18" height="13" rx="2" /><path d="M8 21h8M12 17v4" /></svg>
            <span>桌面</span>
          </button>
          <button type="button" aria-pressed={device === 'mobile'} onClick={() => setDevice('mobile')} title="移动端预览 · 390px">
            <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"><rect x="6" y="2" width="12" height="20" rx="2" /><path d="M10 18h4" /></svg>
            <span>移动端</span>
          </button>
        </div>
        <span className="preview-toolbar__divider" aria-hidden="true" />
        <label className="preview-toolbar__zoom">
          <span className="preview-toolbar__sr-only">预览缩放</span>
          <select aria-label="预览缩放" value={zoom} onChange={(event) => setZoom(Number(event.target.value))}>
            {ZOOM_LEVELS.map((value) => <option key={value} value={value}>{value}%</option>)}
          </select>
        </label>
        <button type="button" className="preview-toolbar__refresh" aria-label="刷新预览" title="刷新预览" disabled={!siteUrl} onClick={() => setReload((value) => value + 1)}>
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5" /><path d="M6 7a7 7 0 0 1 12-1l2 3M4 15l2 3a7 7 0 0 0 12-1" /></svg>
        </button>
        <span className={`preview-toolbar__status${state === 'running' ? ' is-running' : ''}`} role="status" title={state === 'running' ? '预览使用中，不会自动停止；离开后空闲 5 分钟自动停止' : starting ? '正在自动启动预览' : '预览尚未就绪'}>
          <span aria-hidden="true" />{state === 'running' ? '运行中' : starting || retrying ? '启动中' : '未就绪'}
        </span>
      </div>
    </section>
  );
}
