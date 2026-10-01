// The network image browser (`handoff.md` §13.14).
//
// §13.14's flow is *搜索 → 用户选择 → 直接写入远程 URL → Markdown*, and the promise it makes is as
// important as the feature: **不进入附件缓存池,不下载,不再上传** — the image is a URL in the Markdown
// and nothing else. So this panel never caches, never uploads, and never adds an attachment; it searches,
// shows, and writes a line of Markdown.
//
// **Two sources are offered and a third is explained.** Pixabay's own terms forbid permanent hotlinking
// and require images to be downloaded to the caller's own server, which is the opposite of what §13.14
// prescribes, so the source is refused in the provider's own words rather than silently missing. See
// PROCESSING.md §5.11.
//
// What the user must do is the provider's business, not this component's: whether the credit line is
// optional is `attribution_optional`, which `veloct-attach` derives from the provider's terms, and
// whether a download must be reported is `download_reported`. The panel renders both rather than
// deciding either — every rule it applies lives in `src/lib/networkImageForm.ts`, because this
// repository has no React test renderer and a rule in JSX is a rule nothing checks.

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  api,
  describeError,
  type ImageResultView,
  type ImageSearchView,
  type ImageSourceName,
  type NetworkImageChoiceResultView,
  type NetworkImageSourceView,
} from './lib/api';
import {
  attributionNotice,
  attributionToggle,
  choiceNotice,
  hasKey,
  insertReadiness,
  moreLabel,
  pageSizeFor,
  refusedSummary,
  resultMeta,
  searchNotice,
  searchReadiness,
  sourceOptions,
  suggestedAlt,
  validateKeyInput,
} from './lib/networkImageForm';

interface Props {
  /** A network image lands in a document, so the panel needs an open project like its neighbours. */
  enabled: boolean;
  /** Called with the Markdown for a chosen image. The caller decides what to do with it. */
  onInsertMarkdown?: (markdown: string) => void;
}

export default function NetworkImagePanel({ enabled, onInsertMarkdown }: Props) {
  const [sources, setSources] = useState<NetworkImageSourceView[]>([]);
  const [active, setActive] = useState<ImageSourceName>('unsplash');
  const [query, setQuery] = useState('');
  const [pageSize, setPageSize] = useState(0);
  const [page, setPage] = useState(1);
  const [found, setFound] = useState<ImageSearchView | null>(null);

  const [keyInput, setKeyInput] = useState('');
  const [alt, setAlt] = useState('');
  const [includeAttribution, setIncludeAttribution] = useState(true);
  const [lastChoice, setLastChoice] = useState<NetworkImageChoiceResultView | null>(null);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      setSources(await api.networkImageSources());
    } catch (caught) {
      setError(describeError(caught));
    }
  }, [enabled]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const options = useMemo(() => sourceOptions(sources), [sources]);
  const current = options.offered.find((entry) => entry.source === active) ?? options.offered[0];
  const refusal = useMemo(() => refusedSummary(options.refused), [options.refused]);

  // The page size follows the source's own default until the user picks one, and is clamped to the
  // ceiling the crate reports.
  useEffect(() => {
    setPageSize(current ? current.default_per_page : 0);
    setFound(null);
    setPage(1);
  }, [current?.source, current?.default_per_page]);

  const run = useCallback(async (action: () => Promise<void>, done: string | null) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      if (done) setNotice(done);
    } catch (caught) {
      setError(describeError(caught));
    } finally {
      setBusy(false);
    }
  }, []);

  // ---- Keys (§12.2) ---------------------------------------------------------------------------

  const saveKey = () =>
    run(async () => {
      if (!current) return;
      const problem = validateKeyInput(keyInput);
      if (problem) throw new Error(problem);
      await api.saveImageSourceKey(current.source, keyInput.trim());
      setKeyInput('');
      setSources(await api.networkImageSources());
    }, '密钥已保存到操作系统凭据库。§12.1:保存后不会再显示它。');

  const deleteKey = () =>
    run(async () => {
      if (!current) return;
      await api.deleteImageSourceKey(current.source);
      setKeyInput('');
      setSources(await api.networkImageSources());
    }, '密钥已从操作系统凭据库移除。');

  // ---- Search ---------------------------------------------------------------------------------

  const readiness = searchReadiness({ source: current, query, busy });

  const search = useCallback(
    (nextPage: number) =>
      run(async () => {
        if (!current) return;
        const result = await api.searchNetworkImages({
          source: current.source,
          query: query.trim(),
          page: nextPage,
          perPage: pageSizeFor(current, pageSize),
        });
        // A page-2 request continues the list rather than replacing it, so "load more" appends.
        setFound((previous) =>
          nextPage > 1 && previous && previous.source === result.source && previous.query === result.query
            ? { ...result, results: [...previous.results, ...result.results] }
            : result,
        );
        setPage(nextPage);
      }, null),
    [current, pageSize, query, run],
  );

  // ---- Choosing -------------------------------------------------------------------------------

  const choose = (result: ImageResultView) =>
    run(async () => {
      if (!current) return;
      const chosen = await api.chooseNetworkImage({
        source: current.source,
        result,
        alt: alt.trim() === '' ? undefined : alt.trim(),
        includeAttribution,
      });
      setLastChoice(chosen);
      // The Markdown goes to the editor's buffer. The panel STAYS OPEN, unlike the attachment panel,
      // because a browser is used for several picks in a row.
      if (onInsertMarkdown) onInsertMarkdown(chosen.markdown);
      setNotice(choiceNotice(chosen).text);
    }, null);

  const insertState = insertReadiness({ markdown: lastChoice?.markdown ?? '', busy });
  const credit = current ? attributionNotice(current) : null;
  const toggle = found ? attributionToggle(found) : null;

  if (!enabled) {
    return (
      <div className="network-image-panel">
        <p className="muted">打开一个项目后可以浏览网络图片。</p>
      </div>
    );
  }

  return (
    <div className="network-image-panel">
      <header className="network-image-header">
        <strong>网络图片浏览</strong>
        <span className="muted">§13.14:只写入远程地址,不下载、不进缓存池</span>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新来源
        </button>
      </header>

      {error && (
        <p className="network-image-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="network-image-notice">{notice}</p>}

      {/* A source the contract names but this build does not offer is a sentence, not a silence. */}
      {refusal && <p className="network-image-warning">{refusal}</p>}

      <section>
        <h3>来源</h3>
        {options.offered.length === 0 && <p className="muted">当前构建没有可用的图片来源。</p>}
        <div className="row">
          {options.offered.map((entry) => (
            <button
              key={entry.source}
              type="button"
              className={entry.source === current?.source ? 'active' : ''}
              onClick={() => setActive(entry.source)}
              title={entry.attribution_rule}
            >
              {entry.label}
              {hasKey(entry) ? '' : '(未配置密钥)'}
            </button>
          ))}
        </div>

        {current && (
          <>
            <p className="muted hint">{current.notice}</p>
            {credit && (
              <p className={credit.required ? 'network-image-warning' : 'muted hint'}>
                {credit.text}
              </p>
            )}

            {/* §12.1: the window says whether a key is saved and never shows it. */}
            {current.needs_key && (
              <div className="row">
                <label className="grow">
                  <span>
                    {current.label} 访问密钥{current.has_key ? '(已保存,可覆盖)' : ''}
                  </span>
                  <input
                    type="password"
                    value={keyInput}
                    onChange={(event) => setKeyInput(event.target.value)}
                    placeholder={current.has_key ? '已保存;留空不会改动' : '粘贴 API Key'}
                    spellCheck={false}
                    autoComplete="off"
                  />
                </label>
                <button
                  type="button"
                  onClick={() => void saveKey()}
                  disabled={busy || keyInput.trim() === ''}
                >
                  保存密钥
                </button>
                <button
                  type="button"
                  className="danger"
                  onClick={() => void deleteKey()}
                  disabled={busy || !current.has_key}
                  title="从操作系统凭据库移除这个来源的密钥"
                >
                  删除密钥
                </button>
              </div>
            )}
          </>
        )}
      </section>

      <section>
        <h3>搜索</h3>
        <div className="row">
          <label className="grow">
            <span>关键词</span>
            <input
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setFound(null);
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && readiness.ready) void search(1);
              }}
              placeholder="例如:咖啡 桌面"
              spellCheck={false}
            />
          </label>
          {current && (
            <label>
              <span>每页(上限 {current.max_per_page})</span>
              <input
                type="number"
                min={1}
                max={current.max_per_page}
                value={pageSize}
                onChange={(event) => setPageSize(Number(event.target.value))}
              />
            </label>
          )}
          <button
            type="button"
            onClick={() => void search(1)}
            disabled={!readiness.ready}
            title={readiness.reason ?? '搜索'}
          >
            搜索
          </button>
        </div>
        {/* A disabled button with no explanation is what §13.2.2's reason exists to prevent. */}
        {readiness.reason && <p className="muted hint">{readiness.reason}</p>}
      </section>

      {found && (
        <section>
          <h3>{searchNotice(found)}</h3>
          {found.results.length === 0 && <p className="muted">没有结果,换一个关键词试试。</p>}

          <div className="row">
            <label className="grow">
              <span>Alt 文本(留空则用来源自己的描述)</span>
              <input
                value={alt}
                onChange={(event) => setAlt(event.target.value)}
                placeholder={found.results[0] ? suggestedAlt(found.results[0]) : ''}
              />
            </label>
            {/* Rendered only where the provider leaves the credit to the user: a switch that the crate
                would ignore would be a lie. */}
            {toggle?.shown ? (
              <label className="network-image-toggle">
                <span>{toggle.label}</span>
                <input
                  type="checkbox"
                  checked={includeAttribution}
                  onChange={(event) => setIncludeAttribution(event.target.checked)}
                />
              </label>
            ) : (
              <p className="muted hint">{toggle?.label}</p>
            )}
          </div>

          <ul className="network-image-grid">
            {found.results.map((result) => {
              const usable = result.url.trim() !== '' && /^https?:\/\//i.test(result.url.trim());
              return (
                <li key={`${result.source}:${result.source_id}`}>
                  <span className="network-image-thumb">
                    {result.thumbnail_url ? (
                      <img
                        src={result.thumbnail_url}
                        alt={suggestedAlt(result)}
                        loading="lazy"
                        decoding="async"
                      />
                    ) : (
                      <span className="muted">无缩略图</span>
                    )}
                  </span>
                  <span className="network-image-meta">
                    <span className="muted">{resultMeta(result)}</span>
                    {result.source_page && (
                      <a href={result.source_page} target="_blank" rel="noreferrer noopener">
                        来源页 ↗
                      </a>
                    )}
                    <code title={result.url}>{result.url.slice(0, 48)}…</code>
                  </span>
                  <button type="button" onClick={() => void choose(result)} disabled={busy || !usable}>
                    {usable ? '插入这张' : '地址不可用'}
                  </button>
                </li>
              );
            })}
          </ul>

          {moreLabel(found, page) && (
            <button type="button" onClick={() => void search(page + 1)} disabled={busy}>
              {moreLabel(found, page)}
            </button>
          )}
        </section>
      )}

      {lastChoice && (
        <section className="network-image-last">
          <h3>最近插入</h3>
          {lastChoice.download_note && (
            <p className="network-image-warning">{lastChoice.download_note}</p>
          )}
          <pre>{lastChoice.markdown}</pre>
          {lastChoice.attribution && (
            <p className="muted hint">署名:{lastChoice.attribution}</p>
          )}
          {!insertState.ready && <p className="muted hint">{insertState.reason}</p>}
        </section>
      )}
    </div>
  );
}
