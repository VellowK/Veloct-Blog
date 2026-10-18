import { useCallback, useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { isTauri } from '@tauri-apps/api/core';
import { api, describeError } from './lib/api';
import { notify, temporaryUiId } from './components/Notifications';
import './attachment-picker.css';

export type PendingFile = { id: string; name: string; size: number; preview: string | null; path?: string; file?: File };
export const MAX_FILE_BYTES = 64 * 1024 * 1024;
export function validateFiles(files: { name: string; size: number }[]): string | null {
  if (files.length > 20) return '一次最多选择20个文件';
  if (files.some(file => file.size > MAX_FILE_BYTES)) return '单个文件不能超过64MiB';
  return null;
}
export function fileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error('无法读取文件，请重新选择'));
    reader.readAsDataURL(file);
  });
}
async function rasterPreview(file: File): Promise<string | null> {
  if (file.size > 8 * 1024 * 1024) return null;
  const bytes = new Uint8Array(await file.slice(0, 12).arrayBuffer());
  const starts = (...prefix: number[]) => prefix.every((value, index) => bytes[index] === value);
  const mime = starts(137, 80, 78, 71, 13, 10, 26, 10) ? 'image/png'
    : starts(255, 216, 255) ? 'image/jpeg' : starts(71, 73, 70, 56) ? 'image/gif'
    : starts(82, 73, 70, 70) && String.fromCharCode(...bytes.slice(8, 12)) === 'WEBP' ? 'image/webp'
    : starts(66, 77) ? 'image/bmp' : null;
  return mime ? fileAsDataUrl(new File([file], file.name, { type: mime })) : null;
}
export default function AttachmentFilePicker({ files, onChange, disabled, onReadingChange }: { files: PendingFile[]; onChange: (files: PendingFile[]) => void; disabled: boolean; onReadingChange: (reading: boolean) => void }) {
  const box = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const live = useRef(false);
  const generation = useRef(0);
  const locked = useRef(disabled);
  locked.current = disabled;
  const callback = useRef(onChange); callback.current = onChange;
  const hovered = useRef(false);
  const [dragging, setDragging] = useState(false);
  const [reading, setReading] = useState(false);
  useEffect(() => { onReadingChange(reading); }, [reading, onReadingChange]);
  const [failedPreviews, setFailedPreviews] = useState<string[]>([]);
  const load = useCallback(async (get: () => Promise<PendingFile[]>) => {
    if (locked.current || !live.current) return;
    const version = ++generation.current;
    setReading(true);
    try {
      const next = await get();
      if (version !== generation.current || !live.current || locked.current) return;
      const reason = validateFiles(next);
      if (reason) throw new Error(reason);
      if (next.length) { callback.current(next); setFailedPreviews([]); }
    } catch (error) { if (live.current && version === generation.current) notify(describeError(error), 'error'); }
    finally { if (live.current && version === generation.current) setReading(false); }
  }, []);
  const fromPaths = useCallback((paths: string[]) => load(async () => {
    if (paths.length > 20) throw new Error('一次最多选择20个文件');
    return Promise.all(paths.map(async path => { const result = await api.previewAttachment(path); return { id: temporaryUiId(), name: result.name, size: result.size, preview: result.data_url, path }; }));
  }), [load]);
  const fromFiles = useCallback((selection: File[]) => load(async () => {
    const reason = validateFiles(selection); if (reason) throw new Error(reason);
    return Promise.all(selection.map(async file => ({ id: temporaryUiId(), name: file.name, size: file.size, preview: await rasterPreview(file), file })));
  }), [load]);
  useEffect(() => {
    live.current = true;
    let active = true;
    const paste = (event: ClipboardEvent) => {
      if (locked.current || !(hovered.current || box.current === document.activeElement)) return;
      const selected = Array.from(event.clipboardData?.files ?? []);
      if (selected.length) { event.preventDefault(); void fromFiles(selected); }
    };
    document.addEventListener('paste', paste);
    let unlisten: (() => void) | undefined;
    if (isTauri()) void getCurrentWebview().onDragDropEvent(event => {
      if (!active || !live.current || locked.current) return;
      if (event.payload.type === 'leave') { setDragging(false); return; }
      const rect = box.current?.getBoundingClientRect();
      const x = event.payload.position.x / window.devicePixelRatio;
      const y = event.payload.position.y / window.devicePixelRatio;
      const inside = !!rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
      setDragging(inside && event.payload.type !== 'drop');
      if (inside && event.payload.type === 'drop') void fromPaths(event.payload.paths);
    }).then(stop => { if (active) unlisten = stop; else stop(); }).catch(() => undefined);
    return () => { active = false; live.current = false; generation.current++; unlisten?.(); document.removeEventListener('paste', paste); };
  }, [fromFiles, fromPaths]);
  const choose = () => {
    if (disabled || reading) return;
    if (!isTauri()) { input.current?.click(); return; }
    void load(async () => {
      const selected = await open({ multiple: true, directory: false, title: '选择要加入附件池的文件' });
      const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      if (paths.length > 20) throw new Error('一次最多选择20个文件');
      return Promise.all(paths.map(async path => { const result = await api.previewAttachment(path); return { id: temporaryUiId(), name: result.name, size: result.size, preview: result.data_url, path }; }));
    });
  };
  return <div className="attachment-picker">
    <input ref={input} type="file" multiple hidden onChange={event => { void fromFiles(Array.from(event.target.files ?? [])); event.target.value = ''; }} />
    <button ref={box} type="button" className={`attachment-picker__drop${dragging ? ' is-dragging' : ''}`} disabled={disabled || reading} aria-label="选择或拖拽文件" onClick={choose} onMouseEnter={() => { hovered.current = true; }} onMouseLeave={() => { hovered.current = false; }} onDragOver={event => { event.preventDefault(); setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={event => { event.preventDefault(); setDragging(false); const selection = Array.from(event.dataTransfer.files); if (selection.length) void fromFiles(selection); }}>
      <span className="attachment-picker__symbol" aria-hidden="true">＋</span><strong>{reading ? '正在读取预览…' : '点击选择文件，或将文件拖到这里'}</strong>
      <span>悬停或聚焦后可 Ctrl+V 粘贴图片/文件 · 单个文件不超过64MiB</span><small>先预览，再点击「加入附件池」；不会自动上传到云端</small>
    </button>
    {files.length > 0 && <div className="attachment-picker__previews" aria-label="待加入附件预览">{files.map(file => <figure key={file.id}>
      {file.preview && !failedPreviews.includes(file.id) ? <img src={file.preview} alt={`待加入：${file.name}`} onError={() => setFailedPreviews(current => [...current, file.id])} /> : <div className="attachment-picker__placeholder">{file.preview ? '图片预览失败' : '文件预览'}<small>非位图或超过8MiB时仅显示文件信息</small></div>}
      <figcaption><strong>{file.name}</strong><small>{(file.size / 1024).toFixed(1)} KB · 尚未加入附件池</small><button type="button" disabled={disabled || reading} onClick={() => { onChange(files.filter(candidate => candidate.id !== file.id)); }}>移除</button></figcaption>
    </figure>)}</div>}
  </div>;
}
