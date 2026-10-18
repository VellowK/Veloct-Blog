import type { S3Policy, StorageTargetView } from './api';
export type Protocol = 'https' | 'http';
export interface S3FormDraft extends S3Policy { endpointProtocol: Protocol; cdnProtocol: Protocol }
export function splitAddress(value: string): { protocol: Protocol; host: string } {
  const match = /^(https?):\/\/(.*)$/i.exec(value.trim());
  return match ? { protocol: match[1]!.toLowerCase() as Protocol, host: match[2]! } : { protocol: 'https', host: value.trim() };
}
export function joinAddress(protocol: Protocol, value: string): string {
  const host = splitAddress(value).host;
  return host ? `${protocol}://${host}` : '';
}
export function s3Defaults(legacy = false, endpoint = '', cdn = ''): S3FormDraft {
  return { access_style: legacy ? 'auto' : 'path', rename: legacy ? 'legacy' : 'uuid',
    duplicate: legacy ? 'legacy_reuse' : 'random_alphanumeric', random_length: 8, filename_template: '',
    url_suffixes: [], thumbnail: '', endpointProtocol: splitAddress(endpoint).protocol, cdnProtocol: splitAddress(cdn).protocol };
}
export function policyFromForm(form: S3FormDraft): S3Policy {
  const { endpointProtocol: _endpoint, cdnProtocol: _cdn, ...policy } = form;
  return { ...policy, url_suffixes: policy.url_suffixes.map(rule => ({ extensions: rule.extensions.trim(), suffix: rule.suffix.trim() })), thumbnail: policy.thumbnail.trim() };
}
/** Presentation only: hiding never disables background synchronization or deletes the strategy. */
export function uploadTargets(targets: StorageTargetView[]): StorageTargetView[] {
  return targets.filter(target => !target.hidden_from_upload).sort((a, b) => (b.upload_priority ?? 0) - (a.upload_priority ?? 0));
}
export function storageThumbnail(url: string | null, target: StorageTargetView | undefined, width = 400): string | null {
  if (!url) return null;
  const template = target?.s3_policy?.thumbnail;
  if (!template) return url;
  // Never turn an unsafe URL into an image; the caller's HTTPS whitelist remains authoritative.
  let suffix = template.replaceAll('{width}', String(Math.max(1, Math.min(4096, Math.round(width)))));
  if (url.includes('?') && suffix.startsWith('?')) suffix = `&${suffix.slice(1)}`;
  return `${url}${suffix}`;
}
