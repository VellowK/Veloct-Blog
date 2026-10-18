import type { AttachmentMessages, AttachmentSyncSettings, AttachmentView, UploadEligibility } from './api';
import { uploadControl, type UploadControl } from './storageTargetForm';

export function isImageAttachment(name: string): boolean {
  return /\.(avif|gif|jpe?g|png|svg|webp|ico|bmp|tiff?)$/i.test(name);
}

export function moveSyncTarget(ids: string[], id: string, delta: -1 | 1): string[] {
  const from = ids.indexOf(id);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= ids.length) return [...ids];
  const result = [...ids];
  [result[from], result[to]] = [result[to]!, result[from]!];
  return result;
}

export function attachmentIssueCount(messages: AttachmentMessages): number {
  return new Set([...messages.upload_errors, ...messages.health_errors].map((m) => m.attachment_id)).size;
}

export function syncUploadControl(attachment: AttachmentView, settings: AttachmentSyncSettings, fallbackTarget: string, eligibility: UploadEligibility | null): UploadControl {
  const multiple = isImageAttachment(attachment.original_name) && settings.target_ids.length > 0;
  if (!multiple) return uploadControl({ storageTargetId: fallbackTarget, eligibility });
  if (attachment.upload_status === 'UPLOADING') return { disabled: true, reason: '正在上传,请等待本次上传结束。' };
  if (attachment.remote_url) return { disabled: false, reason: null };
  return uploadControl({ storageTargetId: settings.target_ids[0]!, eligibility });
}
