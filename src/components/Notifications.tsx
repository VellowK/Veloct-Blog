import { isValidElement, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import './notifications.css';

export type NoticeTone = 'success' | 'error' | 'warning' | 'info';
type Notice = { id: string; tone: NoticeTone; title?: string | undefined; message: ReactNode; actions?: ReactNode; created: number };
const notices = new Map<string, Notice>();
const listeners = new Set<() => void>();
const publish = () => listeners.forEach(listener => listener());
export const NOTIFICATION_DURATION = 5000;
let temporaryId = 0;
/** UI keys only: not an attachment identity, credential or cryptographic nonce. */
export function temporaryUiId() { return `ui-${Date.now().toString(36)}-${++temporaryId}`; }
export function dismissNotification(id: string) { notices.delete(id); publish(); }
export function notify(message: ReactNode, tone: NoticeTone = 'info', id: string = temporaryUiId(), title?: string | undefined, actions?: ReactNode) {
  notices.set(id, { id, tone, message, title, actions, created: Date.now() });
  while (notices.size > 5) notices.delete(notices.keys().next().value!);
  publish();
  return id;
}
function messageText(node: ReactNode): string {
  if (node == null || typeof node === 'boolean') return '';
  if (typeof node === 'string' || typeof node === 'number' || typeof node === 'bigint') return String(node);
  if (Array.isArray(node)) return node.map(messageText).join('\n');
  return isValidElement<{ children?: ReactNode }>(node) ? messageText(node.props.children) : '';
}
/** The message remains in the caller's state; dismissing a toast does not turn a failed read into loading. */
export function Feedback({ message, tone = 'info', title, actions }: { message?: ReactNode; tone?: NoticeTone; title?: string | undefined; actions?: ReactNode }) {
  const id = useId();
  const latest = useRef({ message, actions });
  latest.current = { message, actions };
  const text = messageText(message);
  useEffect(() => {
    if (text) notify(latest.current.message, tone, id, title, latest.current.actions);
    else dismissNotification(id);
  }, [text, tone, title, id]);
  return null;
}
const symbols: Record<NoticeTone, string> = { success: '✓', error: '×', warning: '!', info: 'i' };
function Toast({ notice, onDetails }: { notice: Notice; onDetails: () => void }) {
  useEffect(() => {
    const timer = window.setTimeout(() => dismissNotification(notice.id), Math.max(0, NOTIFICATION_DURATION - (Date.now() - notice.created)));
    return () => window.clearTimeout(timer);
  }, [notice.id, notice.created]);
  return <article className={`toast toast--${notice.tone}`} role={notice.tone === 'error' || notice.tone === 'warning' ? 'alert' : 'status'}>
    <span className="toast__icon" aria-hidden="true">{symbols[notice.tone]}</span>
    <button type="button" className="toast__summary" onClick={onDetails} title="点击查看消息详情">
      <strong>{notice.title ?? ({ success: '操作成功', error: '操作失败', warning: '请注意', info: '提示' })[notice.tone]}</strong>
      <span className="toast__excerpt">{notice.message}</span>
    </button>
    <button type="button" className="toast__close" aria-label="关闭通知" onClick={() => dismissNotification(notice.id)}>×</button>
    <span className="toast__progress" key={notice.created} style={{ animationDuration: `${NOTIFICATION_DURATION}ms` }} aria-hidden="true" />
  </article>;
}
export function NotificationHost() {
  const [items, setItems] = useState(() => [...notices.values()]);
  const [details, setDetails] = useState<Notice | null>(null);
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const update = () => setItems([...notices.values()]);
    listeners.add(update); update();
    return () => { listeners.delete(update); };
  }, []);
  useEffect(() => {
    if (!details) return;
    const previous = document.activeElement as HTMLElement | null;
    close.current?.focus();
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') setDetails(null); };
    window.addEventListener('keydown', key);
    return () => { window.removeEventListener('keydown', key); if (previous?.isConnected) previous.focus(); };
  }, [details]);
  return createPortal(<>
    <section className="toast-stack" aria-label="通知消息">{items.map(notice => <Toast key={notice.id} notice={notice} onDetails={() => { setDetails(notice); dismissNotification(notice.id); }} />)}</section>
    {details && <div className="notification-backdrop" onClick={() => setDetails(null)}><section className="notification-details" role="dialog" aria-modal="true" aria-label="消息详情" onClick={event => event.stopPropagation()} onKeyDown={event => { if (event.key === 'Tab') { const buttons = event.currentTarget.querySelectorAll<HTMLElement>('button:not([disabled]),a[href],input,select,textarea'); const first = buttons[0], last = buttons[buttons.length - 1]; if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); } } }}>
      <header><h2>{details.title ?? '消息详情'}</h2><button ref={close} type="button" onClick={() => setDetails(null)} aria-label="关闭消息详情">关闭</button></header>
      <div className="notification-details__body">{details.message}</div>
      {details.actions && <div className="notification-details__actions">{details.actions}</div>}
    </section></div>}
  </>, document.body);
}
