import { useEffect, useState } from 'react';
import { Feedback } from './components/Notifications';
import { api, type LinkedKnowledge, type LinkedKnowledgeView, type KnowledgeOutlineNode, type KnowledgeProvider } from './lib/api';

const label = (provider: KnowledgeProvider) => provider === 'yuque' ? '语雀' : '飞书';
const message = (e: unknown) => e && typeof e === 'object' && 'message' in e ? String(e.message) : String(e);
function Outline({ nodes, open }: { nodes: KnowledgeOutlineNode[]; open: (id: string) => void }) {
  return <ul className="linked-outline">{nodes.map((node) => <li key={node.id}>
    {node.children.length ? <details open><summary>{node.url ? <button type="button" onClick={(event) => { event.preventDefault(); open(node.id); }} title="在默认浏览器打开源文档 ↗">{node.title || '未命名'} ↗</button> : <span>{node.title || '分组'}</span>}</summary><Outline nodes={node.children} open={open} /></details>
      : node.url ? <button type="button" onClick={() => open(node.id)} title="在默认浏览器打开源文档">{node.title || '未命名'} ↗</button> : <span>{node.title || '分组'}</span>}
  </li>)}</ul>;
}
export default function LinkedKnowledgePanel({ enabled }: { enabled: boolean }) {
  const [data, setData] = useState<LinkedKnowledgeView>({ items: [], revision: null });
  const [form, setForm] = useState(false);
  const [editing, setEditing] = useState(false);
  const [provider, setProvider] = useState<KnowledgeProvider>('yuque');
  const [name, setName] = useState('');
  const [source, setSource] = useState('');
  const [token, setToken] = useState('');
  const [appId, setAppId] = useState('');
  const [appSecret, setAppSecret] = useState('');
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState('');
  const [pending, setPending] = useState<string | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    api.linkedKnowledge().then((value) => { if (active) { setData(value); setReady(true); } }).catch((e) => { if (active) setError(message(e)); });
    return () => { active = false; };
  }, [enabled]);
  const clearSecrets = () => { setToken(''); setAppId(''); setAppSecret(''); };
  const edit = (item?: LinkedKnowledge) => {
    clearSecrets(); setError(null); setNotice(''); setEditing(!!item); setForm(true);
    setProvider(item?.provider ?? 'yuque'); setName(item?.name ?? ''); setSource(item?.source_url ?? '');
  };
  const run = async (operation: () => Promise<LinkedKnowledgeView>, done: string) => {
    setBusy(true); setError(null); setNotice('');
    try { setData(await operation()); setReady(true); setNotice(done); setPending(null); return true; }
    catch (e) { setError(message(e)); return false; }
    finally { setBusy(false); clearSecrets(); }
  };
  const open = (id: string, nodeId: string | null = null) => { void api.openLinkedKnowledge(id, nodeId).catch((e) => setError(message(e))); };
  return <section className="linked-knowledge" aria-label="链接知识库">
    <header className="linked-knowledge__heading"><div><h2>链接知识库</h2><p className="muted">链接语雀或飞书，只同步目录，不复制正文。点击条目会在浏览器打开源文档。</p></div><div className="linked-knowledge__actions"><button type="button" disabled={!enabled || busy} onClick={() => void run(() => api.linkedKnowledge(), '列表已重新读取，可重试操作')}>重新读取列表</button><button type="button" disabled={!enabled || !ready || busy} onClick={() => edit()}>链接知识库</button></div></header>
    <p className="muted">目录是手动同步的快照；构建发布后，目录标题和源链接会显示在网站「知识库」中。私有文档仍需访问者在源站有权限。</p>
    <Feedback message={error} tone="error" actions={<button type="button" disabled={busy || !enabled} onClick={() => void run(() => api.linkedKnowledge(), '列表已重新读取，可重试操作')}>重新读取列表</button>} />
    <Feedback message={notice} tone="success" />
    {form && <form className="linked-knowledge__form" onSubmit={(event) => {
      event.preventDefault();
      void run(() => api.linkKnowledge({ name, provider, source_url: source, ...(token ? { token } : {}), ...(appId ? { app_id: appId } : {}), ...(appSecret ? { app_secret: appSecret } : {}) }, data.revision), '已链接并获取最新目录').then((ok) => { if (ok) setForm(false); });
    }}>
      <label>平台<select value={provider} disabled={busy || editing} onChange={(e) => { setProvider(e.target.value as KnowledgeProvider); clearSecrets(); }}><option value="yuque">语雀</option><option value="feishu">飞书</option></select></label>
      <label>显示名称<input value={name} required maxLength={80} disabled={busy} onChange={(e) => setName(e.target.value)} /></label>
      <label>知识库链接<input type="url" required value={source} disabled={busy || editing} placeholder={provider === 'yuque' ? 'https://www.yuque.com/团队/知识库' : 'https://租户.feishu.cn/wiki/根节点token'} onChange={(e) => setSource(e.target.value)} /></label>
      {provider === 'yuque' ? <><label>语雀个人 Token<input type="password" autoComplete="off" value={token} disabled={busy} placeholder={editing ? '留空沿用已保存授权' : '需具备读取知识库目录的权限'} onChange={(e) => setToken(e.target.value)} /></label><p className="muted">在语雀个人设置中创建 Token，并确保账号可访问该知识库。填写知识库地址，不是单篇文档地址。</p></>
        : <><label>飞书自建应用 App ID<input autoComplete="off" value={appId} disabled={busy} placeholder={editing ? '与 Secret 一起留空可沿用授权' : 'cli_…'} onChange={(e) => setAppId(e.target.value)} /></label><label>App Secret<input type="password" autoComplete="off" value={appSecret} disabled={busy} onChange={(e) => setAppSecret(e.target.value)} /></label><p className="muted">先在飞书开放平台创建并发布企业自建应用，开通知识库只读权限（wiki:wiki:readonly），再把应用加入目标知识库并赋予查看权限。只能同步应用有权查看的目录。</p></>}
      <p className="muted">授权只保存在系统凭据库，不进入项目文件、网站或备份。同步失败会保留旧目录。</p>
      <div className="linked-knowledge__actions"><button type="submit" disabled={busy || !ready}>{busy ? '正在获取目录…' : editing ? '保存并更新目录' : '链接并获取目录'}</button><button type="button" disabled={busy} onClick={() => { setForm(false); clearSecrets(); }}>取消</button></div>
    </form>}
    {!data.items.length && !form && <p className="muted">还没有链接外部知识库；本地知识库仍在上方独立管理。</p>}
    {data.items.map((item) => <article className="linked-knowledge__card" key={item.id}>
      <header className="linked-knowledge__heading"><div><h3><button type="button" className="linked-knowledge__source" onClick={() => open(item.id)}>{item.name} ↗</button></h3><p className="muted">{label(item.provider)} · {item.has_credential ? '已授权' : '需重新授权'} · 同步于 {item.synced_at}</p></div>
        <div className="linked-knowledge__actions"><button type="button" disabled={busy || !item.has_credential} onClick={() => void run(() => api.refreshLinkedKnowledge(item.id, data.revision), '目录已更新')}>更新目录</button><button type="button" disabled={busy} onClick={() => edit(item)}>名称与授权</button>
          {pending === item.id ? <><button type="button" className="danger" disabled={busy} onClick={() => void run(() => api.unlinkKnowledge(item.id, data.revision), '已解除链接；源站内容未修改')}>确认解除</button><button type="button" disabled={busy} onClick={() => setPending(null)}>取消</button></> : <button type="button" disabled={busy} onClick={() => setPending(item.id)}>解除链接</button>}
        </div>
      </header>
      {item.nodes.length ? <Outline nodes={item.nodes} open={(nodeId) => open(item.id, nodeId)} /> : <p className="muted">目录为空；请确认源站内容及应用可见范围，再更新目录。</p>}
    </article>)}
  </section>;
}
