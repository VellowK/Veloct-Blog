import { describe, expect, it } from 'vitest';
import { MAX_FILE_BYTES, validateFiles } from '../src/AttachmentFilePicker';
import { pushRoute } from '../src/lib/navigationHistory';

describe('pending attachment validation', () => {
  it('accepts ordinary and empty files up to the actual backend limit', () => {
    expect(validateFiles([{ name: 'empty.txt', size: 0 }, { name: 'photo.png', size: MAX_FILE_BYTES }])).toBeNull();
    expect(validateFiles([{ name: 'too-large', size: MAX_FILE_BYTES + 1 }])).toContain('64MiB');
  });
  it('bounds one selection before reading previews', () => {
    expect(validateFiles(Array.from({ length: 20 }, () => ({ name: 'x', size: 1 })))).toBeNull();
    expect(validateFiles(Array.from({ length: 21 }, () => ({ name: 'x', size: 1 })))).toContain('20');
  });
});
describe('in-app route history', () => {
  it('deduplicates rerenders but records section and document changes', () => {
    const start = [{ view: 'settings', section: 'site' }];
    expect(pushRoute(start, { view: 'settings', section: 'site' })).toBe(start);
    expect(pushRoute(start, { view: 'settings', section: 'attachments' })).toHaveLength(2);
    expect(pushRoute([{ view: 'editor', id: 'a' }], { view: 'editor', id: 'b' })).toHaveLength(2);
  });
  it('retains a bounded history and never mutates the previous stack', () => {
    const history = Array.from({ length: 50 }, (_, id) => ({ id }));
    const next = pushRoute(history, { id: 50 });
    expect(next).toHaveLength(50); expect(next[0]).toEqual({ id: 1 }); expect(history[0]).toEqual({ id: 0 });
  });
});
