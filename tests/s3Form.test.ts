import { describe, expect, it } from 'vitest';
import type { StorageTargetView } from '../src/lib/api';
import { buildSaveRequest, draftFromTarget, emptyDraft } from '../src/lib/storageTargetForm';
import { joinAddress, policyFromForm, s3Defaults, splitAddress, storageThumbnail, uploadTargets } from '../src/lib/s3Form';
function view(overrides: Partial<StorageTargetView> = {}): StorageTargetView {
  return { id: 'r2', name: 'R2', kind: 's3', implemented: true, unimplemented_reason: null, path_prefix: null,
    public_url_prefix: '', repository: null, branch: null, endpoint: null, region: null, bucket: null,
    access_key: null, instance: null, credential_ref: null, has_credential: false, ...overrides };
}
describe('S3 presentation and policy', () => {
  it('splits existing protocols and composes exactly one user-selected protocol', () => {
    expect(splitAddress(' HTTP://localhost:9000/base ')).toEqual({ protocol: 'http', host: 'localhost:9000/base' });
    expect(joinAddress('https', 'https://account.r2.cloudflarestorage.com')).toBe('https://account.r2.cloudflarestorage.com');
    expect(joinAddress('http', 'https://localhost:9000/base')).toBe('http://localhost:9000/base');
    expect(joinAddress('https', '')).toBe('');
  });
  it('new policies use requested R2-safe defaults without inserting any real account', () => {
    expect(s3Defaults()).toMatchObject({ access_style: 'path', rename: 'uuid', duplicate: 'random_alphanumeric', thumbnail: '' });
    expect(JSON.stringify(s3Defaults())).not.toContain('r2.cloudflarestorage');
  });
  it('old targets stay in legacy naming/addressing/reuse until explicitly changed', () => {
    const old = view({ endpoint: 'http://localhost:9000', public_url_prefix: 'http://cdn.example/base', path_prefix: 'uploads' });
    const draft = draftFromTarget(old);
    expect(draft.s3).toBeUndefined();
    expect(s3Defaults(true, draft.endpoint, draft.publicUrlPrefix)).toMatchObject({ rename: 'legacy', duplicate: 'legacy_reuse', access_style: 'auto', endpointProtocol: 'http', cdnProtocol: 'http' });
    expect(buildSaveRequest(draft)).not.toHaveProperty('s3Policy');
    expect(buildSaveRequest(draft)).toMatchObject({ endpoint: old.endpoint, publicUrlPrefix: old.public_url_prefix, pathPrefix: 'uploads' });
  });
  it('roundtrips the new policy, preserves lowercase auto and leaves saved secret empty', () => {
    const policy = policyFromForm({ ...s3Defaults(), thumbnail: '?width={width}', url_suffixes: [{ extensions: ' png ', suffix: ' !webp ' }] });
    const draft = draftFromTarget(view({ bucket: 'picbed', endpoint: 'https://account.r2.cloudflarestorage.com', region: 'auto', access_key: 'access-id', s3_policy: policy, hidden_from_upload: true, upload_priority: 42, has_credential: true }));
    expect(draft.credential).toBe('');
    expect(buildSaveRequest(draft)).toMatchObject({ region: 'auto', accessKey: 'access-id', hiddenFromUpload: true, uploadPriority: 42, s3Policy: policy });
  });
  it('never sends form-only protocol fields in persisted policy', () => {
    const policy = policyFromForm(s3Defaults());
    expect(policy).not.toHaveProperty('endpointProtocol'); expect(policy).not.toHaveProperty('cdnProtocol');
  });
  it('non-S3 saves cannot leak a dormant S3 policy or S3 fields', () => {
    const request = buildSaveRequest({ ...emptyDraft(), id: 'local', name: 'Local', kind: 'local', s3: s3Defaults(), bucket: 'private-bucket', credential: '' });
    expect(request).not.toHaveProperty('s3Policy'); expect(request).not.toHaveProperty('bucket');
  });
  it('upload choices hide and stably prioritize without mutating manager rows', () => {
    const targets = [view({ id: 'a' }), view({ id: 'hidden', hidden_from_upload: true, upload_priority: 999 }), view({ id: 'b', upload_priority: 8 }), view({ id: 'c', upload_priority: 8 })];
    expect(uploadTargets(targets).map(v => v.id)).toEqual(['b', 'c', 'a']);
    expect(targets.map(v => v.id)).toEqual(['a', 'hidden', 'b', 'c']);
  });
  it('preview-only widths are clamped and never mutate canonical URLs or policies', () => {
    const target = view({ s3_policy: { ...policyFromForm(s3Defaults()), thumbnail: '?width={width}&fit=cover' } });
    expect(storageThumbnail('https://cdn.example/a.png', target)).toBe('https://cdn.example/a.png?width=400&fit=cover');
    expect(storageThumbnail('https://cdn.example/a.png?format=webp', target, 9999)).toBe('https://cdn.example/a.png?format=webp&width=4096&fit=cover');
    expect(storageThumbnail('https://cdn.example/a.png', target, 0)).toContain('width=1');
    expect(target.s3_policy?.thumbnail).toBe('?width={width}&fit=cover');
    expect(storageThumbnail(null, target)).toBeNull();
    expect(storageThumbnail('https://cdn.example/a.png', view())).toBe('https://cdn.example/a.png');
  });
});
