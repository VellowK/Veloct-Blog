import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { RibbonEntry } from './lib/api';
import { menuPageOptions } from './lib/menuPages';

function PageLinkDialog({ pages, onSelect, onClose }: {
  pages: readonly RibbonEntry[];
  onSelect: (page: RibbonEntry) => void;
  onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState('');
  const options = menuPageOptions(pages, query);
  useEffect(() => {
    const dialog = dialogRef.current;
    const returnFocus = document.activeElement;
    dialog?.showModal();
    dialog?.querySelector<HTMLInputElement>('input[type="search"]')?.focus();
    return () => {
      dialog?.close();
      if (returnFocus instanceof HTMLElement && returnFocus.isConnected) returnFocus.focus();
    };
  }, []);

  // A native modal traps focus, makes the workbench inert and restores focus on close.
  // Portalling also keeps the modal outside the content area's entrance transform.
  return createPortal(
    <dialog ref={dialogRef} className="navigation-page-dialog" aria-labelledby="navigation-page-title"
      onCancel={(event) => { event.preventDefault(); onClose(); }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); return; }
        if (event.key !== 'Tab') return;
        const controls = event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)');
        const first = controls[0];
        const last = controls[controls.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }}>
      <header className="navigation-page-dialog__header">
        <h2 id="navigation-page-title">选择页面链接</h2>
        <button type="button" aria-label="关闭页面选择" onClick={onClose}>×</button>
      </header>
      <div className="navigation-page-dialog__body">
        <label htmlFor="navigation-page-search">搜索页面</label>
        <input id="navigation-page-search" type="search" placeholder="输入页面名称或链接" value={query} onChange={(event) => setQuery(event.target.value)} />
        <p className="navigation-hint">选择后填入链接地址，点击“新增菜单”才会添加。仅显示可生成的页面。</p>
        <ul className="navigation-page-options" aria-label="可选页面">
          {options.map((page) => <li key={page.id}>
            <button type="button" onClick={() => onSelect(page)}>
              <strong>{page.title}</strong><small>{page.route}</small>
            </button>
          </li>)}
        </ul>
        {options.length === 0 && <p role="status" className="navigation-empty">{query.trim() ? '没有匹配的页面。' : '暂无可选页面，请先在页面管理中创建页面。'}</p>}
      </div>
      <footer className="navigation-page-dialog__footer"><button type="button" onClick={onClose}>取消</button></footer>
    </dialog>, document.body,
  );
}

export type CustomMenu = { id: string; label: string; url: string; visible: boolean; builtin: boolean };

export const BUILTIN_MENU_IDS = new Set(['home', 'articles', 'pages', 'archive', 'links']);

export default function NavigationPanel({ menus, pages, onChange }: { menus: CustomMenu[]; pages: readonly RibbonEntry[]; onChange: (menus: CustomMenu[]) => void }) {
  const [label, setLabel] = useState('');
  const [url, setUrl] = useState('/');
  const [choosingPage, setChoosingPage] = useState(false);
  const add = () => {
    const text = label.trim();
    if (!text) return;
    onChange([...menus, { id: `custom-${Date.now()}`, label: text, url: url.trim() || '/', visible: true, builtin: false }]);
    setLabel(''); setUrl('/');
  };

  return (
    <section className="navigation-panel settings-panel page" aria-labelledby="navigation-title">
      <header className="navigation-header">
        <div>
          <h1 id="navigation-title">菜单管理</h1>
          <p>管理网站左侧菜单。预置菜单可隐藏，自定义菜单可删除。</p>
        </div>
        <span className="navigation-count">{menus.filter((menu) => menu.visible).length} / {menus.length} 项显示</span>
      </header>

      <div className="navigation-list">
        <div className="navigation-columns" aria-hidden="true"><span>菜单 / 链接</span><span>显示</span><span>操作</span></div>
        <ul aria-label="网站菜单">
          {menus.map((menu) => (
            <li className={`navigation-row${menu.visible ? '' : ' is-hidden'}`} key={menu.id}>
              <div className="navigation-item">
                <div className="navigation-item-title"><strong>{menu.label}</strong><span className="navigation-kind">{menu.builtin ? '预置' : '自定义'}</span></div>
                <small title={menu.url}>{menu.url}</small>
              </div>
              <label className="navigation-visibility">
                <input type="checkbox" checked={menu.visible} aria-label={`显示${menu.label}菜单`} onChange={(event) => onChange(menus.map((item) => item.id === menu.id ? { ...item, visible: event.target.checked } : item))} />
                <span>{menu.visible ? '显示' : '隐藏'}</span>
              </label>
              <div className="navigation-actions">
                {menu.builtin ? <span className="navigation-protected" title="预置菜单不能删除">保留</span> : <button type="button" className="navigation-delete" aria-label={`删除${menu.label}菜单`} onClick={() => onChange(menus.filter((item) => item.id !== menu.id))}>删除</button>}
              </div>
            </li>
          ))}
        </ul>
        {menus.length === 0 && <p className="navigation-empty">暂无菜单，可在下方新增。</p>}
      </div>

      <form className="navigation-add" onSubmit={(event) => { event.preventDefault(); add(); }}>
        <h2>新增菜单</h2>
        <div className="navigation-add-fields">
          <div className="settings-field"><label htmlFor="navigation-label">菜单名称</label><input id="navigation-label" value={label} placeholder="例如：关于" onChange={(event) => setLabel(event.target.value)} /></div>
          <div className="settings-field"><label htmlFor="navigation-url">链接地址/页面</label><div className="navigation-link-input"><input id="navigation-url" value={url} placeholder="/about/ 或 https://…" onChange={(event) => setUrl(event.target.value)} /><button type="button" aria-haspopup="dialog" onClick={() => setChoosingPage(true)}>选择页面</button></div></div>
          <button type="submit" className="primary" disabled={!label.trim()}>新增菜单</button>
        </div>
        <p className="navigation-hint">支持站内路径或外部链接，留空使用 /。</p>
      </form>
      {choosingPage && <PageLinkDialog pages={pages} onClose={() => setChoosingPage(false)} onSelect={(page) => {
        setUrl(page.route);
        if (!label.trim()) setLabel(page.title);
        setChoosingPage(false);
      }} />}
    </section>
  );
}
