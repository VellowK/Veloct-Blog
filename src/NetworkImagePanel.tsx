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
  type NetworkImageTopicView,
} from './lib/api';
import {
  attributionNotice,
  attributionToggle,
  browseAction,
  browseIntent,
  choiceNotice,
  hasKey,
  insertReadiness,
  moreLabel,
  pageSizeFor,
  refusedSummary,
  resultMeta,
  resultsEmptyText,
  resultsHeading,
  searchReadiness,
  sourceOptions,
  suggestedAlt,
  topicLabel,
  topicsReadiness,
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
  // Topic browsing: the strip's list, the selected topic, and the title to name it in the header.
  const [topics, setTopics] = useState<NetworkImageTopicView[]>([]);
  const [selectedTopic, setSelectedTopic] = useState<string | null>(null);
  const [topicTitle, setTopicTitle] = useState('');

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
  // ceiling the crate reports. A source change also drops the topic strip: topics are Unsplash's, so a
  // list fetched for one source must not be shown as another source's.
  useEffect(() => {
    setPageSize(current ? current.default_per_page : 0);
    setFound(null);
    setPage(1);
    setTopics([]);
    setSelectedTopic(null);
    setTopicTitle('');
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
  // Topic browsing is Unsplash's alone, so its readiness is its own sentence rather than a second
  // condition inside `searchReadiness`.
  const topicReadiness = topicsReadiness({ source: current, busy });

  const search = useCallback(
    (nextPage: number) =>
      run(async () => {
        if (!current) return;
        // ONE decision about which endpoint a click sends, in the pure module: a non-empty keyword
        // wins over the selected topic, and the per-page is the clamped one. This is the reference
        // implementation's own precedence (docs/spec/03-upstream-facts.md §7.1).
        const intent = browseIntent({
          topic: selectedTopic,
          query,
          page: nextPage,
          perPage: pageSizeFor(current, pageSize),
        });
        if (intent.mode === 'idle') throw new Error(intent.reason);

        const result =
          intent.mode === 'topic'
            ? await api.browseNetworkTopicPhotos({
                source: current.source,
                topic: intent.topic,
                page: intent.page,
                perPage: intent.perPage,
              })
            : await api.searchNetworkImages({
                source: current.source,
                query: intent.query,
                page: intent.page,
                perPage: intent.perPage,
              });
        // A page-2 request continues the list rather than replacing it, so "load more" appends. The
        // mode is part of the identity, so switching between a topic and a keyword starts fresh.
        setFound((previous) =>
          nextPage > 1 &&
          previous &&
          previous.source === result.source &&
          previous.query === result.query
            ? { ...result, results: [...previous.results, ...result.results] }
            : result,
        );
        setPage(nextPage);
      }, null),
    [current, pageSize, query, run, selectedTopic],
  );

  /** Open a topic's photos, which is the browse this panel adds beside the keyword search. */
  const openTopic = useCallback(
    async (topic: NetworkImageTopicView) => {
      setSelectedTopic(topic.id);
      setQuery('');
      await run(async () => {
        if (!current) return;
        const result = await api.browseNetworkTopicPhotos({
          source: current.source,
          topic: topic.id,
          page: 1,
          perPage: pageSizeFor(current, pageSize),
        });
        setFound(result);
        setTopicTitle(topic.title);
        setPage(1);
      }, null);
    },
    [current, pageSize, run],
  );

  const loadTopics = () =>
    run(async () => {
      if (!current) return;
      const listed = await api.networkImageTopics(current.source);
      setTopics(listed);
      // Nothing is auto-selected: a strip that opens a topic on its own would send a request the user
      // did not ask for, and the plugin's own auto-select of the first topic is not copied here.
    }, null);

  /**
   * What the one button under the search box does, and why it is disabled.
   *
   * `browseAction` is the decision, in the pure module: the reference implementation's rule (a keyword
   * wins over a selected topic) folded together with the search's readiness, so the button's label and
   * its reason come from ONE place rather than from two conditions that could disagree.
   */
  const searchAction = browseAction({
    source: current,
    query,
    topic: selectedTopic,
    perPage: current ? pageSizeFor(current, pageSize) : 0,
    busy,
  });

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
            disabled={!searchAction.ready}
            title={searchAction.reason ?? searchAction.label}
          >
            {searchAction.label}
          </button>
        </div>
        {/* A disabled button with no explanation is what §13.2.2's reason exists to prevent. */}
        {searchAction.reason && <p className="muted hint">{searchAction.reason}</p>}
      </section>

      {/* Topic browsing, ADDED BESIDE the keyword search rather than instead of it: the clause
          §13.14 states is met by the search above, and this is the browse mode the owner asked for.
          A keyword in the box wins over the selected topic, which is the reference implementation's
          own precedence. */}
      <section>
        <h3>主题</h3>
        <div className="row">
          <button
            type="button"
            onClick={() => void loadTopics()}
            disabled={!topicReadiness.ready}
            title={topicReadiness.reason ?? '读取主题'}
          >
            {topics.length > 0 ? '刷新主题' : '读取主题'}
          </button>
          {selectedTopic && (
            <button
              type="button"
              onClick={() => {
                setSelectedTopic(null);
                setTopicTitle('');
                setFound(null);
              }}
              disabled={busy}
            >
              取消选择主题
            </button>
          )}
        </div>
        {/* The reason is a sentence, not a silent disabled control. */}
        {topicReadiness.reason && <p className="muted hint">{topicReadiness.reason}</p>}
        {topics.length > 0 ? (
          <div className="row network-image-topics">
            {topics.map((topic) => (
              <button
                key={topic.id}
                type="button"
                className={topic.id === selectedTopic ? 'active' : ''}
                onClick={() => void openTopic(topic)}
                disabled={busy}
                title={topic.description ?? topic.slug}
              >
                {topicLabel(topic)}
              </button>
            ))}
          </div>
        ) : (
          <p className="muted">
            主题由 Unsplash 自己维护。读取后选一个主题即可浏览它的照片;关键词框里有内容时,
            搜索会按关键词走(与参考实现一致)。
          </p>
        )}
      </section>

      {found && (
        <section>
          {/* ONE decision about which mode the results came from, from the pure module, so the header
              and the empty-state message cannot disagree. A keyword in the box wins over the selected
              topic, which is the rule `browseIntent` applies to the request too. */}
          <h3>{resultsHeading(found, { topic: selectedTopic, topicTitle, query })}</h3>
          {found.results.length === 0 && (
            <p className="muted">{resultsEmptyText({ topic: selectedTopic, query })}</p>
          )}

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
