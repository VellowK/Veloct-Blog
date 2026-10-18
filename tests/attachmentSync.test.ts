import { describe, expect, it } from 'vitest';
import { attachmentIssueCount, isImageAttachment, moveSyncTarget, syncUploadControl } from '../src/lib/attachmentSync';
import { remoteCleanup } from '../src/lib/storageTargetForm';
import type { AttachmentView, ObjectView } from '../src/lib/api';

const attachment: AttachmentView = {
  id: 'photo', original_name: 'photo.PNG', content_hash: 'hash', size_bytes: 10,
  cache_path: 'hash/photo.png', storage_target_id: 'local', storage_namespace: 'default',
  remote_url: 'https://primary.test/photo.png', remote_key: 'photo.png',
  upload_status: 'SUCCEEDED', reference_status: 'APPLIED', cache_status: 'PRESENT',
  last_error: null, created_at: '', reference_count: 1,
};

describe('ordered image replication', () => {
  it('moves a target without mutating the saved order', () => {
    const ids = ['a', 'b', 'c'];
    expect(moveSyncTarget(ids, 'b', -1)).toEqual(['b', 'a', 'c']);
    expect(moveSyncTarget(ids, 'b', 1)).toEqual(['a', 'c', 'b']);
    expect(ids).toEqual(['a', 'b', 'c']);
  });
  it('does not wrap at the ends or invent unknown targets', () => {
    expect(moveSyncTarget(['a', 'b'], 'a', -1)).toEqual(['a', 'b']);
    expect(moveSyncTarget(['a', 'b'], 'b', 1)).toEqual(['a', 'b']);
    expect(moveSyncTarget(['a'], 'missing', 1)).toEqual(['a']);
  });
  it('recognizes images but keeps regular attachments as files', () => {
    expect(isImageAttachment('photo.PNG')).toBe(true);
    expect(isImageAttachment('report.pdf')).toBe(false);
  });
  it('lets a selected primary retry backups even when ordinary uploading is forbidden', () => {
    expect(syncUploadControl(attachment, { target_ids: ['a', 'b'] }, '', { can_upload: false, upload_status: 'SUCCEEDED', reason: 'already uploaded' }).disabled).toBe(false);
  });
  it('does not bypass an upload already in flight', () => {
    expect(syncUploadControl({ ...attachment, upload_status: 'UPLOADING' }, { target_ids: ['a'] }, '', null).disabled).toBe(true);
  });
  it('uses the sorted list rather than the single-file selector for initial image uploads', () => {
    expect(syncUploadControl({ ...attachment, remote_url: null, upload_status: 'PENDING' }, { target_ids: ['a'] }, '', { can_upload: true, upload_status: 'PENDING', reason: null }).disabled).toBe(false);
  });
  it('keeps single-target behavior when sync is disabled or the attachment is a file', () => {
    expect(syncUploadControl(attachment, { target_ids: [] }, '', null).disabled).toBe(true);
    expect(syncUploadControl({ ...attachment, original_name: 'report.pdf' }, { target_ids: ['a'] }, '', null).disabled).toBe(true);
  });
  it('counts affected attachments, not the number of failing hosts', () => {
    const message = { attachment_id: 'one', target_id: 'a', url: null, message: 'failed' };
    expect(attachmentIssueCount({ upload_errors: [message], health_errors: [message, { ...message, target_id: 'b' }], checked_at: null })).toBe(1);
  });
  it('never selects a same-key backup for the primary-delete action', () => {
    const object = (id: string, url: string): ObjectView => ({ content_hash: 'hash', storage_namespace: 'default', dedup_key: id, key: 'photo.png', url, bytes: 10, storage_target_id: id });
    const objects = [object('a-backup', 'https://backup.test/photo.png'), object('z-primary', attachment.remote_url!)];
    expect(remoteCleanup(attachment, objects, []).storageTargetId).toBe('z-primary');
  });
});
