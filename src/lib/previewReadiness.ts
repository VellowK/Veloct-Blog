// Astro's content watcher is asynchronous. Never navigate a new document's iframe to a known 404.
export async function waitForPreviewDocument(
  check: () => Promise<boolean>,
  signal: AbortSignal,
  timeoutMs = 15_000,
  intervalMs = 150,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!signal.aborted) {
    if (await check()) {
      if (signal.aborted) throw new DOMException('Aborted', 'AbortError');
      return;
    }
    if (signal.aborted) break;
    const remaining = deadline - Date.now();
    if (remaining <= 0) throw new Error('预览尚未生成此页面，请稍后重试；若持续失败，请检查预览日志。');
    await new Promise<void>((resolve) => {
      const finish = () => { clearTimeout(timer); signal.removeEventListener('abort', finish); resolve(); };
      const timer = setTimeout(finish, Math.min(intervalMs, remaining));
      signal.addEventListener('abort', finish, { once: true });
    });
  }
  throw new DOMException('Aborted', 'AbortError');
}
