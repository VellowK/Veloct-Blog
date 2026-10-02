/**
 * The storage-target form's decisions (`PROCESSING.md` §5.5, and §5.10.1 for what left this file).
 *
 * `handoff.md` §13 is a locked section, and this repository has no React test renderer — deliberately. So the
 * form's decisions live in `src/lib/storageTargetForm.ts` and are asserted here, the way `frontmatter.test.ts`
 * asserts a pure function: which fields a kind asks for, what a blank credential means to the command surface,
 * why an upload button is disabled, and what the window does with the crate's ruling on a draft.
 *
 * **What is deliberately NOT here any more is a second copy of any rule.** Two transcription tables used to
 * live in this file — nine public-URL shapes and nine `{bucket}` endpoint shapes, each row carrying a
 * hand-written copy of the crate's verdict — and they could only ever DETECT that the window's copy of
 * `StorageConfig::validate` had drifted from the crate. The copy is gone and so are they: the RULES are
 * asserted in Rust, against the authority (`src-tauri/src/attachments.rs` drives the `validate_storage_target`
 * command's own body), and this file asserts that the window SENDS the save path's request and SURFACES the
 * crate's message unchanged.
 */

import { describe, expect, it } from 'vitest';

import type {
  AttachmentView,
  ObjectView,
  StorageKindView,
  StorageTargetView,
  UploadEligibility,
} from '../src/lib/api';
import {
  buildSaveRequest,
  changeKind,
  deletionWarning,
  draftFromTarget,
  eligibilityFallback,
  emptyDraft,
  fieldKeysForKind,
  fieldsForKind,
  hiddenFieldsNote,
  kindGapSummary,
  kindNotice,
  normaliseKind,
  remoteCleanup,
  storageKindOptions,
  unimplementedKinds,
  uploadControl,
  validateDraft,
  type StorageTargetDraft,
  type StorageTargetSaveRequest,
} from '../src/lib/storageTargetForm';

/** One target view, with every field `StorageTargetView` carries and only the interesting ones overridden. */
function view(overrides: Partial<StorageTargetView>): StorageTargetView {
  return {
    id: 'main',
    name: '主目标',
    kind: 'github',
    implemented: true,
    unimplemented_reason: null,
    path_prefix: null,
    public_url_prefix: '',
    repository: null,
    branch: null,
    endpoint: null,
    region: null,
    bucket: null,
    access_key: null,
    instance: null,
    credential_ref: null,
    has_credential: false,
    ...overrides,
  };
}

function attachment(overrides: Partial<AttachmentView>): AttachmentView {
  return {
    id: 'att-1',
    original_name: 'photo.png',
    content_hash: 'hash',
    size_bytes: 10,
    cache_path: 'D:/project/.veloct/cache/att-1.png',
    storage_target_id: 'local',
    storage_namespace: 'local',
    remote_url: null,
    remote_key: null,
    upload_status: 'PENDING',
    reference_status: 'NOT_APPLIED',
    cache_status: 'PRESENT',
    last_error: null,
    created_at: '2026-01-01T00:00:00Z',
    reference_count: 0,
    ...overrides,
  };
}

function object(overrides: Partial<ObjectView>): ObjectView {
  return {
    dedup_key: 'k',
    key: 'blog/photo.png',
    url: 'https://cdn.example.com/blog/photo.png',
    bytes: 10,
    storage_target_id: 'local',
    ...overrides,
  };
}

/** A complete S3 draft: every §13.11 field filled in. */
function s3Draft() {
  return {
    ...emptyDraft('s3'),
    id: 'assets',
    name: 'S3 附件',
    pathPrefix: 'blog',
    publicUrlPrefix: 'https://cdn.example.com',
    endpoint: 'http://127.0.0.1:9000',
    region: 'us-east-1',
    bucket: 'veloct-assets',
    accessKey: 'AKIAEXAMPLE',
    credential: 's3-secret',
  };
}

/**
 * One kind as `storage_kinds` reports it: a fact about the BUILD, with no target and no credential handle.
 */
function kind(overrides: Partial<StorageKindView> & Pick<StorageKindView, 'kind'>): StorageKindView {
  return { implemented: true, unimplemented_reason: null, ...overrides };
}

/** The crate's own words for the two reported gaps (`PROCESSING.md` §5.3, §5.4). */
const SFTP_REASON =
  'SFTP 需要 SSH 实现,本构建出于依赖原因没有引入;请改用 GitHub、S3 或服务器目录目标。';
const LSKY_REASON =
  'Lsky Pro 需要其专用上传令牌协议,本构建尚未实现;请先使用 GitHub、S3 或服务器目录目标。';

/** What this build reports, in the crate's canonical `StorageKind::ALL` order. */
function reportedKinds(): StorageKindView[] {
  return [
    kind({ kind: 'local' }),
    kind({ kind: 's3' }),
    kind({ kind: 'github' }),
    kind({ kind: 'sftp', implemented: false, unimplemented_reason: SFTP_REASON }),
    kind({ kind: 'lsky', implemented: false, unimplemented_reason: LSKY_REASON }),
  ];
}

describe('which fields a kind shows', () => {
  it('shows S3 its endpoint, region, bucket, access key ID and its SECRET key', () => {
    const keys = fieldKeysForKind('s3');
    for (const key of ['endpoint', 'region', 'bucket', 'accessKey', 'credential']) {
      expect(keys).toContain(key);
    }
    const secret = fieldsForKind('s3').find((field) => field.key === 'credential');
    // §13.11 names both keys, and only the SECRET one is a credential: the ID is configuration.
    expect(secret?.secret).toBe(true);
    expect(secret?.label).toContain('Secret');
  });

  it('shows GitHub a repository and a branch, and not the S3 fields', () => {
    const keys = fieldKeysForKind('github');
    expect(keys).toContain('repository');
    expect(keys).toContain('branch');
    expect(keys).not.toContain('bucket');
    expect(keys).not.toContain('endpoint');
    const secret = fieldsForKind('github').find((field) => field.key === 'credential');
    expect(secret?.label).toContain('Token');
  });

  it('shows the local directory kind exactly the path prefix and the public URL prefix', () => {
    expect(fieldKeysForKind('local')).toEqual(['pathPrefix', 'publicUrlPrefix']);
    expect(fieldKeysForKind('local')).not.toContain('bucket');
    expect(fieldKeysForKind('local')).not.toContain('repository');
    // A local target reads no credential at all, so there is no SECRET field to fill in.
    expect(fieldKeysForKind('local')).not.toContain('credential');
  });

  it('asks SFTP for no field, because this build has no adapter for it', () => {
    // SFTP is the LAST kind in that state. Lsky Pro left it when §5.4's protocol was read from the vendor's own
    // artifacts and an adapter landed, and asking for fields the command surface cannot use would produce
    // exactly the half-configured target the gap report exists to avoid.
    expect(fieldKeysForKind('sftp')).toEqual([]);
  });

  it('shows Lsky Pro its instance address and its API token, and no path prefix', () => {
    // §13.12 names 实例地址 / API 地址 and an independent Token, which is the credential.
    expect(fieldKeysForKind('lsky')).toEqual(['instance', 'credential']);
    const secret = fieldsForKind('lsky').find((field) => field.key === 'credential');
    expect(secret?.secret).toBe(true);
    expect(secret?.label).toContain('Token');
    const instance = fieldsForKind('lsky').find((field) => field.key === 'instance');
    expect(instance?.secret).toBe(false);
    expect(instance?.label).toContain('实例地址');
    // The object's KEY and public URL belong to the instance, so a path prefix here is a field nothing reads.
    expect(fieldKeysForKind('lsky')).not.toContain('pathPrefix');
    expect(fieldKeysForKind('lskypro')).toEqual(['instance', 'credential']);
  });

  it('recognises the kind spellings the command surface also accepts', () => {
    expect(normaliseKind(' dir ')).toBe('local');
    expect(normaliseKind('directory')).toBe('local');
    expect(normaliseKind('GITHUB')).toBe('github');
    expect(normaliseKind('s3-compatible')).toBe('s3');
    expect(normaliseKind('lskypro')).toBe('lsky');
    // `ssh` and `server` are NOT aliases any more. Both were spellings of §13.10's SSH target, which was
    // REMOVED (ADR-0017): an alias that mapped a retired spelling onto a live kind would be a second door into
    // a kind that cannot work, so the form now knows nothing about them — and the crate refuses them at the
    // command surface, which is asserted on the Rust side.
    expect(normaliseKind('ssh')).not.toBe('sftp');
    expect(normaliseKind('server')).not.toBe('sftp');
    expect(fieldKeysForKind('ssh')).toEqual([]);
    expect(fieldKeysForKind('dir')).toEqual(fieldKeysForKind('local'));
  });

  it('reports SFTP and Lsky Pro as having no adapter by reading their views, not a hard-coded list', () => {
    const options = storageKindOptions([
      view({
        id: 'server',
        kind: 'sftp',
        implemented: false,
        unimplemented_reason: 'SFTP 需要 SSH 实现,本构建出于依赖原因没有引入。',
      }),
      view({
        id: 'lsky-main',
        kind: 'lsky',
        implemented: false,
        unimplemented_reason: 'Lsky Pro 需要其专用上传令牌协议,本构建尚未实现。',
      }),
    ]);

    const sftp = options?.find((option) => option.kind === 'sftp');
    expect(sftp?.implemented).toBe(false);
    expect(sftp?.unimplemented_reason).toContain('SSH');
    const notice = kindNotice(sftp);
    expect(notice?.level).toBe('warn');
    expect(notice?.text).toContain('SSH');

    const lsky = options.find((option) => option.kind === 'lsky');
    expect(lsky?.implemented).toBe(false);
    expect(kindNotice(lsky)?.text).toContain('上传令牌');
  });

  it('says nothing about a kind whose view reports an adapter', () => {
    const options = storageKindOptions([view({ kind: 's3', implemented: true })]);
    const s3 = options.find((option) => option.kind === 's3');
    expect(s3?.implemented).toBe(true);
    expect(kindNotice(s3)).toBeNull();
  });

  it('calls a kind with no instance unknown rather than assuming it works', () => {
    const options = storageKindOptions([]);
    expect(options).toHaveLength(5);
    const sftp = options.find((option) => option.kind === 'sftp');
    expect(sftp?.implemented).toBeNull();
    expect(kindNotice(sftp)?.level).toBe('info');
  });

  it('keeps offering a kind a project recorded under a name of its own, so a target stays editable', () => {
    const options = storageKindOptions([view({ kind: 'webdav', implemented: false })]);
    const webdav = options.find((option) => option.kind === 'webdav');
    expect(webdav?.label).toBe('webdav');
    expect(webdav?.implemented).toBe(false);
  });
});

describe('the kind list the build reports (§5.3, §5.4)', () => {
  it('offers every kind the build reports, in the order it reports them', () => {
    // The list is fed by `storage_kinds`, which reads the crate's canonical `StorageKind::ALL` — the form keeps
    // no catalog of its own that could disagree with the build about which kinds exist.
    const options = storageKindOptions([], reportedKinds());
    expect(options.map((option) => option.kind)).toEqual(['local', 's3', 'github', 'sftp', 'lsky']);
    expect(options.find((option) => option.kind === 's3')?.label).toContain('S3');
    expect(options.find((option) => option.kind === 'local')?.label).toContain('目录');
  });

  it('reports the build gap for SFTP and Lsky Pro with the build’s own reason, before anything is saved', () => {
    // The edge ui-engineer reported: a kind with no adapter cannot usefully be saved, so a target list can say
    // nothing about it. With the report and NO target at all, the window already knows.
    const options = storageKindOptions([], reportedKinds());

    const sftp = options.find((option) => option.kind === 'sftp');
    expect(sftp?.implemented).toBe(false);
    expect(sftp?.unimplemented_reason).toBe(SFTP_REASON);
    const sftpNotice = kindNotice(sftp);
    expect(sftpNotice?.level).toBe('warn');
    expect(sftpNotice?.text).toContain(SFTP_REASON);

    const lsky = options.find((option) => option.kind === 'lsky');
    expect(lsky?.implemented).toBe(false);
    expect(lsky?.unimplemented_reason).toBe(LSKY_REASON);
    expect(kindNotice(lsky)?.text).toContain(LSKY_REASON);

    // And the kinds that work say nothing, so "no notice" stays meaningful.
    for (const spelling of ['local', 's3', 'github']) {
      const option = options.find((candidate) => candidate.kind === spelling);
      expect(option?.implemented).toBe(true);
      expect(kindNotice(option)).toBeNull();
    }
  });

  it('states the build’s gaps in one sentence the form can show up front', () => {
    const summary = kindGapSummary(storageKindOptions([], reportedKinds()));
    expect(summary).toContain(SFTP_REASON);
    expect(summary).toContain(LSKY_REASON);
    expect(summary).toContain('保存前');
    // And there is nothing to warn about when every reported kind has an adapter.
    const allWorking = reportedKinds().map((entry) => ({ ...entry, implemented: true, unimplemented_reason: null }));
    expect(kindGapSummary(storageKindOptions([], allWorking))).toBeNull();
    expect(unimplementedKinds(storageKindOptions([], allWorking))).toEqual([]);
    expect(unimplementedKinds(storageKindOptions([], reportedKinds()))).toHaveLength(2);
  });

  it('lets a saved target’s own answer win for its kind, because it is the more specific statement', () => {
    // The build says the kind works; this project's own target says otherwise, and the target is the one the
    // user is about to upload to.
    const options = storageKindOptions(
      [
        view({
          id: 'server',
          kind: 'sftp',
          implemented: false,
          unimplemented_reason: '这一台服务器还没有配置主机。',
        }),
        view({ id: 'legacy', kind: 's3', implemented: false, unimplemented_reason: '这个 S3 目标缺少凭据。' }),
      ],
      reportedKinds(),
    );

    const sftp = options.find((option) => option.kind === 'sftp');
    expect(sftp?.unimplemented_reason).toBe('这一台服务器还没有配置主机。');
    const s3 = options.find((option) => option.kind === 's3');
    expect(s3?.implemented).toBe(false);
    expect(s3?.unimplemented_reason).toBe('这个 S3 目标缺少凭据。');
    // Exactly one option per kind still: the override refines an entry, it does not add one.
    expect(options.filter((option) => option.kind === 's3')).toHaveLength(1);
  });

  it('still offers a kind a project recorded that the build does not report', () => {
    // A target saved under a spelling this build no longer knows must stay editable, and it is offered with its
    // own view's answer rather than being assumed to work.
    const options = storageKindOptions(
      [view({ id: 'old', kind: 'webdav', implemented: false, unimplemented_reason: '这台构建没有 WebDAV。' })],
      reportedKinds(),
    );

    const webdav = options.find((option) => option.kind === 'webdav');
    expect(webdav?.label).toBe('webdav');
    expect(webdav?.implemented).toBe(false);
    expect(webdav?.unimplemented_reason).toBe('这台构建没有 WebDAV。');
    // The reported kinds are all still there.
    expect(options.map((option) => option.kind)).toEqual([
      'local',
      's3',
      'github',
      'sftp',
      'lsky',
      'webdav',
    ]);
  });

  it('claims nothing about a kind when the build’s list has not been read', () => {
    // A failed read must not be reported as "this works": every kind is unknown, and the form says why.
    const options = storageKindOptions([], null);
    expect(options.length).toBeGreaterThan(0);
    for (const option of options) {
      expect(option.implemented).toBeNull();
      expect(option.unimplemented_reason).toBeNull();
    }
    expect(kindGapSummary(options)).toBeNull();
    const notice = kindNotice(options.find((option) => option.kind === 'sftp'));
    expect(notice?.level).toBe('info');
    expect(notice?.text).toContain('还没有读到本构建的类型列表');
  });
});

describe('building the save request', () => {
  it('sends the id, the name, the kind and the fields the chosen kind needs', () => {
    const request = buildSaveRequest(s3Draft());
    expect(request.id).toBe('assets');
    expect(request.name).toBe('S3 附件');
    expect(request.kind).toBe('s3');
    expect(request.endpoint).toBe('http://127.0.0.1:9000');
    expect(request.region).toBe('us-east-1');
    expect(request.bucket).toBe('veloct-assets');
    expect(request.accessKey).toBe('AKIAEXAMPLE');
    expect(request.credential).toBe('s3-secret');
    expect(request.pathPrefix).toBe('blog');
    expect(request.publicUrlPrefix).toBe('https://cdn.example.com');
  });

  it('sends the GitHub repository and branch and nothing from another kind', () => {
    const request = buildSaveRequest({
      ...emptyDraft('github'),
      id: 'assets-gh',
      name: 'GitHub 附件',
      repository: 'vellow/assets',
      branch: 'main',
      credential: 'ghp_example',
    });
    expect(request.repository).toBe('vellow/assets');
    expect(request.branch).toBe('main');
    expect('bucket' in request).toBe(false);
    expect('endpoint' in request).toBe(false);
    expect('accessKey' in request).toBe(false);
  });

  it('omits a blank credential instead of sending it as an empty string', () => {
    const request = buildSaveRequest({ ...s3Draft(), credential: '' });
    // §12.2: an omitted credential means "leave the stored one alone". Sending `''` would say the same thing to
    // the command surface today, but the request must not mention a credential the user did not type.
    expect('credential' in request).toBe(false);
    expect(Object.values(request)).not.toContain('');
  });

  it("trims a typed credential rather than storing the user's stray whitespace", () => {
    const request = buildSaveRequest({ ...s3Draft(), credential: '  s3-secret  ' });
    expect(request.credential).toBe('s3-secret');
  });

  it('does not submit values typed for another kind', () => {
    // The user filled the S3 fields and then switched to GitHub. The values stay in the form (see the kind-switch
    // tests), but a bucket must not be written into a GitHub target.
    const request = buildSaveRequest({ ...s3Draft(), kind: 'github', repository: 'vellow/assets' });
    expect(request.kind).toBe('github');
    expect(request.repository).toBe('vellow/assets');
    expect('bucket' in request).toBe(false);
    expect('accessKey' in request).toBe(false);
  });

  it('sends no credential for the local directory kind, which reads none', () => {
    const request = buildSaveRequest({
      ...emptyDraft('local'),
      id: 'on-this-machine',
      name: '本机目录',
      pathPrefix: 'images',
      credential: 'left-over-from-another-kind',
    });
    expect('credential' in request).toBe(false);
    expect(request.pathPrefix).toBe('images');
  });
});

describe('editing an existing target', () => {
  it('pre-fills exactly the fields the view carries', () => {
    const draft = draftFromTarget(
      view({
        id: 'assets',
        name: 'S3 附件',
        kind: 's3',
        path_prefix: 'blog',
        public_url_prefix: 'https://cdn.example.com',
        endpoint: 'http://127.0.0.1:9000',
        region: 'us-east-1',
        bucket: 'veloct-assets',
        access_key: 'AKIAEXAMPLE',
        has_credential: true,
      }),
    );
    expect(draft.id).toBe('assets');
    expect(draft.name).toBe('S3 附件');
    expect(draft.kind).toBe('s3');
    expect(draft.pathPrefix).toBe('blog');
    expect(draft.publicUrlPrefix).toBe('https://cdn.example.com');
    expect(draft.endpoint).toBe('http://127.0.0.1:9000');
    expect(draft.region).toBe('us-east-1');
    expect(draft.bucket).toBe('veloct-assets');
    expect(draft.accessKey).toBe('AKIAEXAMPLE');
  });

  it('shows an unset view field as an empty control rather than as the text "null"', () => {
    const draft = draftFromTarget(view({ path_prefix: null, branch: null, bucket: null }));
    expect(draft.pathPrefix).toBe('');
    expect(draft.branch).toBe('');
    expect(draft.bucket).toBe('');
  });

  it('starts the credential empty, because the view never carries a value', () => {
    // The Rust side asserts `StorageTargetView`'s exact field set for this reason: there is no field a credential
    // VALUE could arrive in, so an edit can never render one back into the form.
    const draft = draftFromTarget(view({ has_credential: true, credential_ref: 'storage:assets' }));
    expect(draft.credential).toBe('');
  });

  it('keeps the credential of an untouched existing target', () => {
    const draft = { ...draftFromTarget(view({ id: 'assets', name: '旧名字' })), name: '新名字' };
    const request = buildSaveRequest(draft);
    expect(request.name).toBe('新名字');
    expect(request.id).toBe('assets');
    // The field was left alone, so the request does not mention a credential and the stored one survives.
    expect('credential' in request).toBe(false);
  });

  it('replaces the credential when the user types a new one', () => {
    const draft = { ...draftFromTarget(view({ id: 'assets', kind: 'github' })), credential: 'ghp_new' };
    expect(buildSaveRequest(draft).credential).toBe('ghp_new');
  });
});

describe('switching a kind', () => {
  it('keeps the values already typed and names the ones the new kind will not save', () => {
    const change = changeKind(s3Draft(), 'github');
    expect(change.draft.bucket).toBe('veloct-assets');
    expect(change.draft.endpoint).toBe('http://127.0.0.1:9000');
    expect(change.hiddenFilled).toEqual(['endpoint', 'region', 'bucket', 'accessKey']);
    // GitHub takes a credential of its own, so the secret field is not among the ones the switch hides.
    expect(change.hiddenFilled).not.toContain('credential');
    const note = hiddenFieldsNote(change.hiddenFilled, change.draft.kind);
    expect(note).toContain('Bucket');
    expect(note).toContain('不会随这次保存提交');
  });

  it('reports nothing hidden when the typed values fit the new kind', () => {
    const change = changeKind({ ...emptyDraft('github'), repository: 'vellow/assets' }, 'github');
    expect(change.hiddenFilled).toEqual([]);
    expect(hiddenFieldsNote(change.hiddenFilled, 'github')).toBeNull();
  });

  it('keeps a value that the new kind does not use, so switching back restores it', () => {
    const there = changeKind(s3Draft(), 'local');
    const back = changeKind(there.draft, 's3');
    expect(back.draft.bucket).toBe('veloct-assets');
    expect(buildSaveRequest(back.draft).bucket).toBe('veloct-assets');
  });
});

/**
 * The form's ruling is the CRATE's, and this is where that is asserted.
 *
 * `PROCESSING.md` §5.10.1: the window used to restate `StorageConfig::validate` — the S3 bucket, §13.11's
 * endpoint, §13.12's instance address, §13.10's prefix rules, §13.6's public URL — and two transcription
 * tables here compared the two hand-written copies. There is no copy left, so what these tests prove is the
 * SHAPE of what replaced it: the exact request the save path would send goes out, and whatever the crate
 * answers comes back unchanged. The RULES themselves are asserted in Rust, where the authority is
 * (`src-tauri/src/attachments.rs`).
 */
describe('asking the crate about a draft', () => {
  /** A validator that refuses exactly what the fake "crate" is told to refuse. */
  function crate(says: (request: StorageTargetSaveRequest) => string | null) {
    return async (request: StorageTargetSaveRequest) => {
      const problem = says(request);
      if (problem !== null) {
        // The shape a rejected `invoke` produces, so the window's own error handling is what is exercised.
        throw { code: 'INVALID', message: problem, actionable: false, hint: null };
      }
    };
  }

  /** A crate that accepts everything, which is what makes the "no rule text here" assertion meaningful. */
  const acceptsEverything = crate(() => null);

  it('accepts a draft the crate accepts, and holds no opinion of its own', async () => {
    expect(await validateDraft(s3Draft(), acceptsEverything)).toBeNull();
    // A draft the CRATE would refuse is accepted here, because the window has no rule to refuse it with. This
    // is the assertion that a copy of the rules cannot come back without a test noticing.
    const wouldBeRefused = { ...emptyDraft('s3'), id: 'x', name: 'X' };
    expect(await validateDraft(wouldBeRefused, acceptsEverything)).toBeNull();
  });

  it("shows the crate's own message, word for word", async () => {
    // An English arm and a Chinese one, because the crate's `validate` speaks both and the window must not
    // reword either: the message IS the authority's answer.
    const english = 'S3 needs a bucket';
    expect(await validateDraft({ ...emptyDraft('s3'), id: 'x', name: 'X' }, crate(() => english))).toBe(
      english,
    );

    const chinese =
      'SFTP 已从 §13.10 移除(ADR-0017):这个目标无法再使用,请删除它并改用本机 / 服务器目录、S3、兰空图床或 GitHub 目标。';
    expect(await validateDraft({ ...emptyDraft('sftp'), id: 's', name: 'S' }, crate(() => chinese))).toBe(
      chinese,
    );
  });

  it("carries the crate's hint when there is one, and invents none when there is not", async () => {
    const withHint = await validateDraft(s3Draft(), async () => {
      throw {
        code: 'INVALID',
        message: 'the path prefix ".." must be a relative path without ..',
        actionable: true,
        hint: '见 §13.5:改它会形成新的命名空间。',
      };
    });
    expect(withHint).toContain('must be a relative path without ..');
    expect(withHint).toContain('见 §13.5');

    const withoutHint = await validateDraft(s3Draft(), crate(() => 'GitHub needs a repository'));
    expect(withoutHint).toBe('GitHub needs a repository');
  });

  it('reports a failed call rather than treating it as an acceptance', async () => {
    // A transport failure must never read as "the draft is fine": the form would then offer a save whose
    // refusal the user never saw.
    const reason = await validateDraft(s3Draft(), async () => {
      throw new Error('窗口与后端之间的连接已断开。');
    });
    expect(reason).toBe('窗口与后端之间的连接已断开。');
  });

  it('asks about exactly the request the save path would send', async () => {
    // The other half of one authority: the object judged and the object written are one expression. A field
    // the kind does not render is NOT in the request, which is what keeps §5.10.1's D152 over-refusal fixed
    // without a kind-aware copy of the rule in the window.
    const seen: StorageTargetSaveRequest[] = [];
    const record = async (request: StorageTargetSaveRequest) => {
      seen.push(request);
    };

    // An S3 draft switched to Lsky Pro: the S3 fields are still in the FORM (a switch never discards what was
    // typed) and must not be submitted, and the credential field is untouched.
    const switched: StorageTargetDraft = {
      ...s3Draft(),
      kind: 'lsky',
      instance: 'https://img.example.com',
      credential: '',
    };
    expect(await validateDraft(switched, record)).toBeNull();

    expect(seen).toHaveLength(1);
    const request = seen[0];
    if (request === undefined) throw new Error('the validator was not asked about the draft');
    expect(request).toEqual(buildSaveRequest(switched));
    expect('pathPrefix' in request).toBe(false);
    expect('bucket' in request).toBe(false);
    expect('credential' in request).toBe(false);
    expect(request.instance).toBe('https://img.example.com');
  });

  it('is asked on every call, so the answer cannot come from a cache of an older draft', async () => {
    let calls = 0;
    const counting = async () => {
      calls += 1;
    };
    await validateDraft(s3Draft(), counting);
    await validateDraft({ ...s3Draft(), bucket: '' }, counting);
    expect(calls).toBe(2);
  });
});

describe('the upload eligibility reason (§13.2.2)', () => {
  const blocked: UploadEligibility = {
    can_upload: false,
    upload_status: 'SUCCEEDED',
    reason: '该附件处于 SUCCEEDED 状态,按第 13.2.2 节不允许再次上传。',
  };
  const allowed: UploadEligibility = {
    can_upload: true,
    upload_status: 'FAILED',
    reason: null,
  };

  it('shows the reason and disables the button when can_upload is false', () => {
    const control = uploadControl({ eligibility: blocked, storageTargetId: 'assets' });
    expect(control.disabled).toBe(true);
    expect(control.reason).toBe(blocked.reason);
  });

  it('has a reason of its own when the service sends none', () => {
    const control = uploadControl({
      eligibility: { can_upload: false, upload_status: 'UPLOADING', reason: null },
      storageTargetId: 'assets',
    });
    expect(control.disabled).toBe(true);
    expect(control.reason).toContain('13.2.2');
  });

  it('enables the button once can_upload is true, with nothing to explain', () => {
    const control = uploadControl({ eligibility: allowed, storageTargetId: 'assets' });
    expect(control.disabled).toBe(false);
    expect(control.reason).toBeNull();
  });

  it('does not offer an upload before the eligibility has been read', () => {
    const control = uploadControl({ eligibility: null, storageTargetId: 'assets' });
    expect(control.disabled).toBe(true);
    expect(control.reason).toContain('13.2.2');
  });

  it('explains a missing target instead of disabling the button silently', () => {
    const control = uploadControl({ eligibility: allowed, storageTargetId: '' });
    expect(control.disabled).toBe(true);
    expect(control.reason).toContain('存储目标');
  });

  it('falls back to the attachment status, with a reason, when the eligibility read fails', () => {
    // A failed read must not remove the action the window had before the reason existed, and it must not be
    // silent either: the fallback says it could not read §13.2.2's condition.
    const pending = eligibilityFallback('PENDING');
    expect(uploadControl({ eligibility: pending, storageTargetId: 'assets' }).disabled).toBe(false);

    const succeeded = eligibilityFallback('SUCCEEDED');
    const control = uploadControl({ eligibility: succeeded, storageTargetId: 'assets' });
    expect(control.disabled).toBe(true);
    expect(control.reason).toContain('13.2.2');
    expect(control.reason).toContain('SUCCEEDED');
  });
});

describe('the remote cleanup (§13.8)', () => {
  it('names the target the object is actually on, read from the object record', () => {
    const cleanup = remoteCleanup(
      attachment({ storage_target_id: 'local', remote_key: 'blog/photo.png', remote_url: 'https://x.test/p' }),
      [object({ key: 'blog/photo.png', storage_target_id: 's3-assets' })],
      [view({ id: 'local' }), view({ id: 's3-assets', kind: 's3' })],
    );
    expect(cleanup.storageTargetId).toBe('s3-assets');
    expect(cleanup.disabled).toBe(false);
  });

  it("falls back to the attachment's own target for the ordinary add-and-upload flow", () => {
    const cleanup = remoteCleanup(
      attachment({
        storage_target_id: 's3-assets',
        remote_key: 'blog/photo.png',
        remote_url: 'https://x.test/p',
      }),
      [],
      [view({ id: 's3-assets', kind: 's3' })],
    );
    expect(cleanup.storageTargetId).toBe('s3-assets');
  });

  it('is not offered at all when there is no recorded remote object', () => {
    const cleanup = remoteCleanup(attachment({}), [], [view({ id: 'local' })]);
    expect(cleanup.storageTargetId).toBeNull();
  });

  it('says it removes only the remote object and never the article or the cached file', () => {
    const cleanup = remoteCleanup(
      attachment({ remote_key: 'k', remote_url: 'https://x.test/p' }),
      [object({ key: 'k' })],
      [view({ id: 'local' })],
    );
    expect(cleanup.wording).toContain('只删除远程对象');
    expect(cleanup.wording).toContain('Markdown 引用');
    expect(cleanup.wording).toContain('缓存文件');
    expect(cleanup.wording).toContain('13.8');
    // PROCESSING.md §5.10.3's decision, taken rather than left open: §13.8 removes the remote object and does
    // NOT rewrite the article, so the confirmation has to say what that means for the page — a broken image
    // where the article still points — instead of only saying what it leaves untouched.
    expect(cleanup.wording).toContain('断链');
    expect(cleanup.wording).toContain('不会替你改写文章');
  });

  it('is disabled with the view reason when the target has no adapter', () => {
    const cleanup = remoteCleanup(
      attachment({ storage_target_id: 'server', remote_key: 'k', remote_url: 'https://x.test/p' }),
      [object({ key: 'k', storage_target_id: 'server' })],
      [view({ id: 'server', kind: 'sftp', implemented: false, unimplemented_reason: 'SFTP 需要 SSH 实现。' })],
    );
    expect(cleanup.disabled).toBe(true);
    expect(cleanup.disabledReason).toContain('SSH');
  });
});

describe('deleting a target', () => {
  it('says the stored credential goes with it and that the remote files stay', () => {
    const warning = deletionWarning({ id: 'assets', name: 'S3 附件', has_credential: true });
    // `delete_storage_target` removes the credential from the operating system store, so recreating the target
    // means entering it again — and §13.8 means the files already uploaded are untouched.
    expect(warning).toContain('操作系统凭据库');
    expect(warning).toContain('S3 附件');
    expect(warning).toContain('assets');
    expect(warning).toContain('远程文件不会被删除');
    expect(warning).toContain('13.8');
  });
});

describe('the Lsky Pro target (§13.12)', () => {
  it('submits the instance and the token, and nothing else', () => {
    const draft: StorageTargetDraft = {
      ...emptyDraft('lsky'),
      id: 'img',
      name: '兰空图床',
      instance: 'https://img.example.com',
      credential: '1|secret',
    };
    const request = buildSaveRequest(draft);
    expect(request.instance).toBe('https://img.example.com');
    expect(request.credential).toBe('1|secret');
    // A leftover value from another kind must not ride along on a Lsky Pro target (§13.5).
    expect(request.bucket).toBeUndefined();
    expect(request.repository).toBeUndefined();
    expect(request.pathPrefix).toBeUndefined();
  });

  it('keeps a saved target’s instance when it is edited, because the view carries it', () => {
    // The §5.5 defect this guards: a form that cannot SEE the instance submits nothing for it, the command
    // stores `None`, and saving an edit silently breaks a target that was working.
    const saved = view({
      id: 'img',
      name: '兰空图床',
      kind: 'lsky',
      implemented: true,
      instance: 'https://img.example.com',
    });
    const draft = draftFromTarget(saved);
    expect(draft.instance).toBe('https://img.example.com');

    const request = buildSaveRequest(draft);
    expect(request.instance).toBe('https://img.example.com');
    // The credential is never read back, so an untouched field must mean "keep the stored one".
    expect(request.credential).toBeUndefined();
  });

  it('asks the crate about the instance the view carried, and keeps the token out of it', async () => {
    // §13.12's instance rule is the CRATE's now (D168 moved it in, and `StorageConfig::validate` enforces it on
    // this path). What the window must still prove is that the instance survives the edit and reaches the
    // validator, and that the credential does not: a form that could not SEE the instance would submit none,
    // store `None`, and silently break a working target — the §5.5 defect.
    const saved = view({
      id: 'img',
      name: '兰空图床',
      kind: 'lsky',
      instance: 'https://img.example.com',
      has_credential: true,
    });
    const draft = { ...draftFromTarget(saved), credential: '1|a-secret-token' };

    const seen: StorageTargetSaveRequest[] = [];
    const reason = await validateDraft(draft, async (request) => {
      seen.push(request);
    });

    expect(reason).toBeNull();
    expect(seen).toHaveLength(1);
    const request = seen[0];
    if (request === undefined) throw new Error('the validator was not asked about the draft');
    expect(request.instance).toBe('https://img.example.com');
    // §12.2: a credential the user TYPED is sent (so it can be stored), and it is the only field that carries
    // a secret — the view's own `has_credential` boolean never becomes a value.
    expect(request.credential).toBe('1|a-secret-token');
    expect(JSON.stringify(request)).not.toContain('credential_ref');
  });
});
