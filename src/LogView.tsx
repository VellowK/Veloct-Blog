/**
 * The log page.
 *
 * `handoff.md` §18 asks for an independent log page, and §18.1 constrains what may be in it. Both are
 * honoured here:
 *
 *  * The page shows the level counts, a category filter built from what actually happened, a search, and
 *    the entries as a timeline. Filtering happens on the Rust side, so the page does not receive the
 *    whole log to display part of it.
 *  * Nothing here can widen what was logged. A field value arriving on this boundary is already
 *    redacted, and the page renders whatever it is given rather than reaching for anything else.
 *
 * The counts describe the WHOLE page rather than the filtered view, which is why the summary stays
 * meaningful when a filter is applied: a filtered view showing "1 error" next to an unfiltered "3 errors"
 * would make the reader think an error had been fixed.
 */

import { useCallback, useEffect, useState } from 'react';
import { Feedback } from './components/Notifications';

import { api, describeError, type LogPage } from './lib/api';

const LEVEL_CLASS: Record<string, string> = {
  info: 'level-info',
  warning: 'level-warning',
  error: 'level-error',
};

const LEVEL_LABEL: Record<string, string> = {
  info: '信息',
  warning: '警告',
  error: '错误',
};

export default function LogView({ enabled }: { enabled: boolean }) {
  const [page, setPage] = useState<LogPage | null>(null);
  const [level, setLevel] = useState<'info' | 'warning' | 'error' | ''>('');
  const [category, setCategory] = useState('');
  const [search, setSearch] = useState('');
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!enabled) return;
    try {
      const result = await api.readLog({
        ...(level ? { level } : {}),
        ...(category ? { category } : {}),
        ...(search.trim() ? { search: search.trim() } : {}),
      });
      setPage(result);
      setError(null);
    } catch (caught) {
      setError(describeError(caught));
    }
  }, [enabled, level, category, search]);

  useEffect(() => {
    void load();
  }, [load]);

  if (!enabled) {
    return (
      <div className="log-page">
        <p className="muted">打开一个项目后可以查看它的日志。</p>
      </div>
    );
  }

  return (
    <div className="log-page">
      <header className="log-header">
        <strong>日志</strong>
        {page && (
          <span className="log-counts">
            <span className="level-info">{page.info} 信息</span>
            <span className="level-warning">{page.warning} 警告</span>
            <span className="level-error">{page.error} 错误</span>
          </span>
        )}
        <button type="button" onClick={() => void load()}>
          刷新
        </button>
      </header>

      <div className="log-filters">
        <select
          value={level}
          onChange={(event) => setLevel(event.target.value as typeof level)}
          aria-label="按等级过滤"
        >
          <option value="">全部等级</option>
          <option value="info">信息</option>
          <option value="warning">警告</option>
          <option value="error">错误</option>
        </select>

        <select
          value={category}
          onChange={(event) => setCategory(event.target.value)}
          aria-label="按类别过滤"
        >
          <option value="">全部类别</option>
          {/* Built from what actually happened, rather than a hard-coded list that could drift. */}
          {(page?.categories ?? []).map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>

        <input
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder="搜索消息或字段"
          spellCheck={false}
        />
      </div>

      <Feedback message={error} tone="error" />

      <ul className="log-entries">
        {(page?.entries ?? []).map((entry, index) => (
          <li key={`${entry.timestamp}-${index}`} className={LEVEL_CLASS[entry.level] ?? ''}>
            <span className="timestamp">{entry.timestamp}</span>
            <span className="level">{LEVEL_LABEL[entry.level] ?? entry.level}</span>
            <span className="category">{entry.category_label}</span>
            <span className="message">{entry.message}</span>
            {Object.keys(entry.fields).length > 0 && (
              <span className="fields">
                {Object.entries(entry.fields).map(([key, value]) => (
                  <span key={key} className="field">
                    {key}={value}
                  </span>
                ))}
              </span>
            )}
          </li>
        ))}
        {page && page.entries.length === 0 && (
          <li className="muted">没有匹配的日志条目。</li>
        )}
      </ul>
    </div>
  );
}
