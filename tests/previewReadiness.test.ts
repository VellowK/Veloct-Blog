import { afterEach, describe, expect, it, vi } from 'vitest';
import { waitForPreviewDocument } from '../src/lib/previewReadiness';

afterEach(() => vi.useRealTimers());
describe('new document preview readiness', () => {
  it('does not accept a server-home-page-ready result; waits until the document probe succeeds', async () => {
    vi.useFakeTimers();
    const check = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(false).mockResolvedValue(true);
    const done = vi.fn();
    const task = waitForPreviewDocument(check, new AbortController().signal).then(done);
    await vi.advanceTimersByTimeAsync(149); expect(done).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(151); await task;
    expect(check).toHaveBeenCalledTimes(3); expect(done).toHaveBeenCalledOnce();
  });
  it('accepts an already generated route without delay', async () => {
    const check = vi.fn().mockResolvedValue(true);
    await waitForPreviewDocument(check, new AbortController().signal); expect(check).toHaveBeenCalledOnce();
  });
  it('has a finite timeout so a real route error cannot spin forever', async () => {
    vi.useFakeTimers();
    const task = waitForPreviewDocument(async () => false, new AbortController().signal, 300);
    const rejection = expect(task).rejects.toThrow('预览尚未生成此页面');
    await vi.advanceTimersByTimeAsync(300); await rejection;
    expect(vi.getTimerCount()).toBe(0);
  });
  it('cancels retries on selection/project change', async () => {
    vi.useFakeTimers(); const controller = new AbortController();
    const check = vi.fn().mockResolvedValue(false);
    const task = waitForPreviewDocument(check, controller.signal);
    const rejection = expect(task).rejects.toMatchObject({ name: 'AbortError' });
    await vi.advanceTimersByTimeAsync(0); controller.abort(); await rejection;
    expect(vi.getTimerCount()).toBe(0); expect(check).toHaveBeenCalledOnce();
  });
  it('ignores success from an old in-flight request after selection changed', async () => {
    const controller = new AbortController(); let complete!: (value: boolean) => void;
    const task = waitForPreviewDocument(() => new Promise<boolean>(resolve => { complete = resolve; }), controller.signal);
    const rejection = expect(task).rejects.toMatchObject({ name: 'AbortError' });
    controller.abort(); complete(true); await rejection;
  });
  it('does not swallow command failures or leak a retry timer', async () => {
    await expect(waitForPreviewDocument(async () => { throw new Error('server stopped'); }, new AbortController().signal))
      .rejects.toThrow('server stopped');
  });
});
