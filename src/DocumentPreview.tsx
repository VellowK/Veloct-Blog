import { useEffect, useState } from 'react';
import { api, describeError, type ContentKind } from './lib/api';
import { waitForPreviewDocument } from './lib/previewReadiness';
import { StateBlock } from './components/ui';

// Parent keys by project/server/route/revision, so old readiness can never expose a new iframe.
export default function DocumentPreview({ siteUrl, kind, id, route }: {
  siteUrl: string; kind: ContentKind; id: string; route: string;
}) {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setReady(false); setError(null);
    void waitForPreviewDocument(() => api.previewDocumentReady(kind, id, siteUrl), controller.signal)
      .then(() => { if (!controller.signal.aborted) setReady(true); })
      .catch((caught: unknown) => { if (!controller.signal.aborted) setError(describeError(caught)); });
    return () => controller.abort();
  }, [siteUrl, kind, id, route, attempt]);
  if (ready) return <iframe title="站点预览" src={new URL(route, siteUrl).href} sandbox="allow-same-origin allow-scripts" />;
  return <div className="pane-body--center" role="status">
    <StateBlock tone={error ? 'error' : 'loading'} title={error ? '此页面预览尚未就绪' : '正在生成页面预览…'}
      description={error ?? '等待 Astro 识别新文件，页面就绪后自动显示。'}
      actions={error ? <button type="button" onClick={() => { setReady(false); setError(null); setAttempt(value => value + 1); }}>重试预览</button> : undefined} />
  </div>;
}
