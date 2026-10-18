import { useMemo, useState } from 'react';
import { Icon, PageHeader, type IconName } from './components/ui';
import type { RibbonEntry } from './lib/api';
import './dashboard.css';

export interface DashboardProps {
  documents: RibbonEntry[];
  onSelect: (entry: RibbonEntry) => void;
}

const KIND_LABELS = { article: '文章', page: '页面', knowledge: '知识库' } as const;
const WEEKDAYS = ['一', '二', '三', '四', '五', '六', '日'];
const number = new Intl.NumberFormat('zh-CN');

/** Keep the author's calendar date; never substitute updated_at for a missing creation date. */
function creationDay(entry: RibbonEntry): string | null {
  const value = entry.created_at;
  if (typeof value !== 'string') return null;
  const match = /^(\d{4})-(\d{2})-(\d{2})(?:$|T|\s)/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1) return null;
  const date = new Date(0);
  date.setFullYear(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) return null;
  return `${match[1]}-${match[2]}-${match[3]}`;
}

function updateTime(entry: RibbonEntry): number {
  const timestamp = Date.parse(entry.updated_at);
  return Number.isFinite(timestamp) ? timestamp : 0;
}

function updateLabel(entry: RibbonEntry): string {
  const timestamp = Date.parse(entry.updated_at);
  if (!Number.isFinite(timestamp)) return '更新时间未知';
  return new Intl.DateTimeFormat('zh-CN', { month: 'short', day: 'numeric' }).format(timestamp);
}

function StatCard({ label, value, detail, icon, tone }: {
  label: string; value: number; detail: string; icon: IconName;
  tone: 'primary' | 'teal' | 'amber' | 'neutral';
}) {
  return <section className={`dashboard-stat dashboard-stat--${tone}`} aria-label={`${label} ${value}`}>
    <div className="dashboard-stat-top"><span>{label}</span><span className="dashboard-stat-icon"><Icon name={icon} /></span></div>
    <strong className="dashboard-stat-value">{number.format(value)}</strong><p>{detail}</p>
  </section>;
}

export default function Dashboard({ documents, onSelect }: DashboardProps) {
  const [month, setMonth] = useState(() => {
    const today = new Date();
    return { year: today.getFullYear(), index: today.getMonth() };
  });
  const [pickerOpen, setPickerOpen] = useState(false);
  const today = new Date();
  const todayKey = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
  const stats = useMemo(() => {
    const kinds = (['article', 'page', 'knowledge'] as const).map(kind => {
      const entries = documents.filter(entry => entry.kind === kind);
      return {
        kind,
        total: entries.length,
        published: entries.filter(entry => entry.wip_state !== 'draft').length,
        publishedWip: entries.filter(entry => entry.wip_state === 'published_wip').length,
      };
    });
    const articles = documents.filter(entry => entry.kind === 'article').length;
    const drafts = documents.filter(entry => entry.wip_state === 'draft');
    const days = new Map<string, number>();
    let missingDates = 0;
    for (const entry of documents) {
      const day = creationDay(entry);
      if (day) days.set(day, (days.get(day) ?? 0) + 1);
      else missingDates += 1;
    }
    return { articles, kinds, drafts: [...drafts].sort((a, b) => updateTime(b) - updateTime(a)), days, missingDates };
  }, [documents]);

  const monthKey = `${String(month.year).padStart(4, '0')}-${String(month.index + 1).padStart(2, '0')}`;
  const firstDay = new Date(0);
  firstDay.setFullYear(month.year, month.index, 1);
  const lastDay = new Date(0);
  lastDay.setFullYear(month.year, month.index + 1, 0);
  const daysInMonth = lastDay.getDate();
  const offset = (firstDay.getDay() + 6) % 7;
  const cells = Math.ceil((offset + daysInMonth) / 7) * 7;
  const monthCounts = Array.from({ length: daysInMonth }, (_, index) => stats.days.get(`${monthKey}-${String(index + 1).padStart(2, '0')}`) ?? 0);
  const monthlyTotal = monthCounts.reduce((sum, count) => sum + count, 0);
  const activeDays = monthCounts.filter(count => count > 0).length;
  const hasCreationData = documents.length > stats.missingDates;

  function moveMonth(delta: number) {
    const next = month.year * 12 + month.index + delta;
    const year = Math.floor(next / 12);
    if (year >= 1 && year <= 9999) setMonth({ year, index: ((next % 12) + 12) % 12 });
    setPickerOpen(false);
  }

  return <div className="dashboard">
    <PageHeader title="仪表盘" description="记录灵感，沉淀知识。这里是你的创作概览。" actions={<span className="dashboard-date">{new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'long', day: 'numeric', weekday: 'long' }).format(today)}</span>} />
    <div className="dashboard-stats">
      <StatCard label="文章总数" value={stats.articles} detail="每一篇，都是思考的积累" icon="edit" tone="primary" />
      {stats.kinds.map(({ kind, published, publishedWip }) => <StatCard
        key={kind}
        label={`已发布${KIND_LABELS[kind]}`}
        value={published}
        detail={publishedWip > 0 ? `含 ${publishedWip} 篇已发布初稿` : `已进入${KIND_LABELS[kind]}发布内容`}
        icon={kind === 'article' ? 'deploy' : kind === 'page' ? 'grid' : 'archive'}
        tone={kind === 'article' ? 'teal' : kind === 'page' ? 'neutral' : 'amber'}
      />)}
      <StatCard label="草稿" value={stats.drafts.length} detail="尚未发布，等待继续打磨" icon="archive" tone="amber" />
    </div>
    <div className="dashboard-grid">
      <section className="dashboard-panel dashboard-activity" aria-labelledby="dashboard-activity-title">
        <header className="dashboard-panel-head"><div><h2 id="dashboard-activity-title">创作热力图</h2><p>从每一次落笔，看见积累的力量</p></div><span className="dashboard-panel-tag">按创建日期</span></header>
        <div className="dashboard-calendar-toolbar">
          <div className="dashboard-month-nav">
            <button type="button" className="dashboard-arrow" aria-label="上个月" title="上个月" disabled={month.year === 1 && month.index === 0} onClick={() => moveMonth(-1)}><Icon name="collapse" /></button>
            <div className="dashboard-month-control">
              <button type="button" className="dashboard-month-button" aria-expanded={pickerOpen} aria-controls="dashboard-month-picker" onClick={() => setPickerOpen(open => !open)}>{month.year} 年 {month.index + 1} 月 <span aria-hidden="true">⌄</span></button>
              {pickerOpen && <div className="dashboard-month-picker" id="dashboard-month-picker">
                <label>年份<input aria-label="选择年份" type="number" min={1} max={9999} value={month.year} onChange={event => {
                  const year = Number(event.target.value);
                  if (Number.isInteger(year) && year >= 1 && year <= 9999) setMonth(current => ({ ...current, year }));
                }} /></label>
                <label>月份<select aria-label="选择月份" value={month.index} onChange={event => { setMonth(current => ({ ...current, index: Number(event.target.value) })); setPickerOpen(false); }}>
                  {Array.from({ length: 12 }, (_, index) => <option key={index} value={index}>{index + 1} 月</option>)}
                </select></label>
                <button type="button" className="dashboard-picker-done" onClick={() => setPickerOpen(false)}>完成</button>
              </div>}
            </div>
            <button type="button" className="dashboard-arrow" aria-label="下个月" title="下个月" disabled={month.year === 9999 && month.index === 11} onClick={() => moveMonth(1)}><Icon name="expand" /></button>
          </div>
          <button type="button" className="dashboard-today-button" onClick={() => { setMonth({ year: today.getFullYear(), index: today.getMonth() }); setPickerOpen(false); }}>回到本月</button>
        </div>
        <div className="dashboard-calendar" aria-label={`${month.year} 年 ${month.index + 1} 月创作记录`}>
          {WEEKDAYS.map(day => <span className="dashboard-weekday" key={day}>{day}</span>)}
          {Array.from({ length: cells }, (_, index) => {
            const day = index - offset + 1;
            if (day < 1 || day > daysInMonth) return <span key={`blank-${index}`} className="dashboard-day-blank" aria-hidden="true" />;
            const key = `${monthKey}-${String(day).padStart(2, '0')}`;
            const count = stats.days.get(key) ?? 0;
            const tooltip = `${month.year} 年 ${month.index + 1} 月 ${day} 日 · ${hasCreationData || documents.length === 0 ? `创建 ${count} 篇内容` : '暂无创建日期数据'}`;
            return <div key={key} tabIndex={0} className={`dashboard-day dashboard-day--${Math.min(count, 4)}${key === todayKey ? ' dashboard-day--today' : ''}`} aria-label={tooltip}>
              <span className="dashboard-day-number">{day}</span>{count > 0 && <span className="dashboard-day-count">{count} 篇</span>}
              <span role="tooltip" className="dashboard-day-tooltip">{tooltip}</span>
            </div>;
          })}
        </div>
        <div className="dashboard-calendar-footer">
          <p>{hasCreationData || documents.length === 0 ? <>本月新建 <strong>{monthlyTotal}</strong> 篇 · 活跃 <strong>{activeDays}</strong> 天</> : '暂无可用的创建日期'}</p>
          <div className="dashboard-legend" aria-label="热度从少到多"><span>少</span>{[0, 1, 2, 3, 4].map(level => <i key={level} className={`dashboard-day--${level}`} />)}<span>多</span></div>
        </div>
        {(documents.length === 0 || stats.missingDates > 0 || monthlyTotal === 0) && <p className="dashboard-data-note">
          {documents.length === 0 ? '还没有内容。写下第一篇文章，让创作日历亮起来。' : stats.missingDates > 0 ? `${stats.missingDates} 篇内容缺少有效创建日期，未计入热力图。` : '这个月还没有新建内容，已有内容的编辑不会计入创作日历。'}
        </p>}
      </section>
      <section className="dashboard-panel dashboard-drafts" aria-labelledby="dashboard-drafts-title">
        <header className="dashboard-panel-head"><div><h2 id="dashboard-drafts-title">草稿箱</h2><p>把未完成的灵感，继续写下去</p></div><span className="dashboard-draft-badge">{stats.drafts.length}</span></header>
        {stats.drafts.length > 0 ? <div className="dashboard-draft-list">
          {stats.drafts.slice(0, 6).map(entry => <button type="button" className="dashboard-draft-row" key={`${entry.kind}:${entry.id}`} onClick={() => onSelect(entry)} aria-label={`编辑草稿：${entry.title || '未命名内容'}`}>
            <span className="dashboard-draft-icon"><Icon name={entry.kind === 'article' ? 'edit' : entry.kind === 'page' ? 'grid' : 'archive'} /></span>
            <span className="dashboard-draft-text"><strong>{entry.title || '未命名内容'}</strong><span>{KIND_LABELS[entry.kind]}<span aria-hidden="true"> · </span>{entry.category || '未分类'}</span></span>
            <span className="dashboard-draft-time" title={Number.isFinite(Date.parse(entry.updated_at)) ? `最后更新：${new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(entry.updated_at))}` : '更新时间未知'}>{updateLabel(entry)}</span>
            <span className="dashboard-draft-chevron"><Icon name="expand" /></span>
          </button>)}
        </div> : <div className="dashboard-empty"><span className="dashboard-empty-icon"><Icon name="archive" /></span><strong>{documents.length === 0 ? '灵感，从这里开始' : '草稿箱清空了'}</strong><p>{documents.length === 0 ? '新建内容后，尚未发布的草稿会出现在这里。' : '所有内容都已发布。下一次灵感，值得被记录。'}</p></div>}
        <footer className="dashboard-draft-footer">{stats.drafts.length > 6 ? `展示最近更新的 6 篇，共 ${stats.drafts.length} 篇草稿` : stats.drafts.length > 0 ? '按最近更新时间排列 · 点击继续编辑' : '每一点积累，都有意义'}</footer>
      </section>
    </div>
  </div>;
}
