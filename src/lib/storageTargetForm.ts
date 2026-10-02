/**
 * The storage-target FORM's decisions, kept out of the component.
 *
 * `PROCESSING.md` §5.5 is the gap this module exists for: `api.saveStorageTarget`, `api.deleteStorageTarget` and
 * `api.uploadEligibility` all existed and no component called them, so §13's storage targets could be configured
 * through the command surface and NOT through the window. Building the form inside `AttachmentPanel.tsx` would
 * put the interesting parts — which fields a kind needs, what a blank credential means, when an upload may be
 * attempted — where no test can reach them, and this repository has no React test renderer (deliberately).
 *
 * So the component renders and this module decides. Every rule below is a rule the Rust side also holds, and
 * the two are kept deliberately parallel:
 *
 *  * **A blank credential is OMITTED, not sent as an empty string.** `save_storage_target` treats an omitted or
 *    empty credential as "leave the stored one alone" (§12.2), and the honest request is one that does not
 *    mention the credential at all. This is what makes editing a target safe: the view never carries a
 *    credential VALUE (asserted on the Rust side), so the form starts empty and an untouched field must not be
 *    read as "clear the stored secret".
 *  * **Only the fields the chosen kind uses are submitted.** Switching a kind therefore cannot smuggle a
 *    leftover bucket into a GitHub target; the fields that will not be sent are reported instead, so the switch
 *    is never silent.
 *  * **The kind's adapter status comes from the BUILD, never from a hard-coded list.** `storage_kinds` reports
 *    every kind this build knows, from the crate's canonical `StorageKind::ALL`, with the crate's own
 *    `is_implemented` / `unimplemented_reason`; a saved target's own answer still wins for its kind, because it
 *    is the more specific statement. A copy here would be a second, stale answer to §5.3/§5.4's question — and
 *    a form that answered it from a list of its own would disagree with the build the moment either changed.
 *  * **An upload's eligibility is read before the attempt.** §13.2.2's reason exists so the window can say WHY
 *    the button is disabled instead of offering an action that will fail.
 */

import type {
  AttachmentView,
  ObjectView,
  StorageKindView,
  StorageTargetView,
  UploadEligibility,
} from './api';

/** The controls a kind can ask for. `id`, `name` and `kind` are always shown and are not in this list. */
export type StorageFieldKey =
  | 'pathPrefix'
  | 'publicUrlPrefix'
  | 'repository'
  | 'branch'
  | 'endpoint'
  | 'region'
  | 'bucket'
  | 'accessKey'
  | 'instance'
  | 'credential';

export interface StorageField {
  key: StorageFieldKey;
  label: string;
  /** Guidance shown under the control. Never a credential. */
  hint: string;
  /** A password input, and a value that is never read back from a view (§12.2). */
  secret: boolean;
  placeholder: string;
}

/** Every field the form can carry, one entry per control, so a key switch can report what it hides. */
const FIELDS: Record<StorageFieldKey, StorageField> = {
  pathPrefix: {
    key: 'pathPrefix',
    label: '路径前缀',
    hint: '相对路径,留空表示目标根目录。改它会形成新的命名空间,同一个文件会被当成新对象(§13.5)。',
    secret: false,
    placeholder: 'blog/images',
  },
  publicUrlPrefix: {
    key: 'publicUrlPrefix',
    label: '公开 URL 前缀',
    hint: '生成 Markdown 地址用的前缀(§13.6);改它不会移动已有的远程对象。',
    secret: false,
    placeholder: 'https://cdn.example.com/assets',
  },
  repository: {
    key: 'repository',
    label: '仓库',
    hint: 'owner/repo,也可以粘贴完整的 GitHub 地址(§13.13)。',
    secret: false,
    placeholder: 'vellow/assets',
  },
  branch: {
    key: 'branch',
    label: '分支',
    hint: '留空使用仓库的默认分支。',
    secret: false,
    placeholder: 'main',
  },
  endpoint: {
    key: 'endpoint',
    label: 'Endpoint',
    hint: '留空表示使用 AWS 官方端点(§13.11)。',
    secret: false,
    placeholder: 'http://127.0.0.1:9000',
  },
  region: {
    key: 'region',
    label: 'Region',
    hint: 'S3-compatible 服务多数需要一个 region,例如 us-east-1。',
    secret: false,
    placeholder: 'us-east-1',
  },
  bucket: {
    key: 'bucket',
    label: 'Bucket',
    hint: 'S3 的 bucket 名称;§13.11 要求它,没有它目标无法使用。',
    secret: false,
    placeholder: 'veloct-assets',
  },
  accessKey: {
    key: 'accessKey',
    label: 'Access Key ID',
    hint: '这是标识符而不是密钥,所以可以保存在项目配置里(§13.11)。',
    secret: false,
    placeholder: 'AKIAEXAMPLE',
  },
  instance: {
    key: 'instance',
    label: '实例地址 / API 地址',
    hint: '兰空图床的站点地址(§13.12),例如 https://img.example.com;上传地址是它加上 /api/v1。也可以直接粘贴完整的 API 地址。',
    secret: false,
    placeholder: 'https://img.example.com',
  },
  credential: {
    key: 'credential',
    label: '凭据',
    hint: '只写入操作系统凭据库,不进入项目文件、日志或备份(§12.2)。留空表示保留已保存的凭据。',
    secret: true,
    placeholder: '留空则保留已保存的凭据',
  },
};

/** The field list per kind. A kind absent from this table asks for no kind-specific field. */
const KIND_FIELDS: Record<string, StorageFieldKey[]> = {
  // §13.11 names endpoint, region, bucket, path prefix, public URL prefix, access key and secret key.
  s3: ['endpoint', 'region', 'bucket', 'accessKey', 'credential', 'pathPrefix', 'publicUrlPrefix'],
  // §13.13 names repository, path prefix, public URL prefix and an independent credential.
  github: ['repository', 'branch', 'credential', 'pathPrefix', 'publicUrlPrefix'],
  // §13.10's "server directory": the directory comes from the project and the target id, so only the prefix
  // and the public URL are configuration.
  local: ['pathPrefix', 'publicUrlPrefix'],
  // §13.12 names "实例地址 / API 地址" and an independent Token, which is the credential; there is no path
  // prefix, because the KEY and the public URL are the INSTANCE's rather than ours.
  lsky: ['instance', 'credential'],
  // The RETIRED §13.10 kind (ADR-0017). It is NOT offered — the kind list comes from the crate's canonical
  // `ALL`, which no longer contains it — but a target saved before the removal still exists in a user's list,
  // and this entry is what lets the form render that row with the build's own reason instead of a blank.
  sftp: [],
};

/**
 * The labels the form renders for each kind, keyed by the crate's own spelling.
 *
 * This is PRESENTATION, not a source of truth about which kinds exist or which of them work. The kinds the
 * form OFFERS come from the `storage_kinds` command (`StorageKindView[]`), which reports them from the crate's
 * canonical `StorageKind::ALL` and answers each one with `is_implemented` / `unimplemented_reason`.
 *
 * The map is also what the form can still render when that command has not been read, and in that state every
 * option is reported as UNKNOWN (`implemented: null`) unless a saved target of that kind says otherwise — so
 * nothing here ever claims that a kind works.
 */
const KIND_LABELS: Record<string, string> = {
  local: '本机 / 服务器目录(§13.10)',
  s3: 'S3 / S3-compatible(§13.11)',
  github: 'GitHub 仓库(§13.13)',
  sftp: 'SFTP(已从 §13.10 移除)',
  lsky: 'Lsky Pro 兰空图床(§13.12)',
};

/**
 * The spellings the command surface also accepts, mapped to the crate's own name.
 *
 * `StorageKind::parse` and `parse_storage_kind` accept these (`dir`, `directory`, `s3-compatible`, `gh`, `ssh`,
 * `lskypro`, `server`), so a value read from somewhere other than a view still lands on the right field set.
 */
const KIND_ALIASES: Record<string, string> = {
  dir: 'local',
  directory: 'local',
  's3-compatible': 's3',
  s3_compatible: 's3',
  gh: 'github',
  // `ssh` and `server` are GONE: both were spellings of §13.10's SSH target, which was REMOVED (ADR-0017), and
  // an alias that maps a retired spelling onto a live kind would be a second door into a kind that cannot work.
  lskypro: 'lsky',
  lsky_pro: 'lsky',
};

/** The crate's name for a kind, so a view's `kind` and a typed value agree before anything is compared. */
export function normaliseKind(kind: string): string {
  const trimmed = kind.trim().toLowerCase();
  return KIND_ALIASES[trimmed] ?? trimmed;
}

/** The fields a kind asks for, in the order the form should show them. */
export function fieldKeysForKind(kind: string): StorageFieldKey[] {
  return KIND_FIELDS[normaliseKind(kind)] ?? [];
}

/** The controls a kind shows, with the credential's label naming the secret that kind actually needs. */
export function fieldsForKind(kind: string): StorageField[] {
  const normalised = normaliseKind(kind);
  return fieldKeysForKind(normalised).map((key) => {
    if (key !== 'credential') return FIELDS[key];
    const label =
      normalised === 's3'
        ? 'Secret Access Key'
        : normalised === 'github'
          ? 'GitHub Token'
          : normalised === 'lsky'
            ? 'Lsky Pro API Token'
            : '凭据';
    return { ...FIELDS[key], label };
  });
}

/** A field's label, for a message about a value that a kind switch will not save. */
export function fieldLabel(key: StorageFieldKey): string {
  return FIELDS[key].label;
}

/** One kind as the window may offer it, with its adapter status read from the build, or from the project. */
export interface StorageKindOption {
  kind: string;
  label: string;
  /** The build's answer (`storage_kinds`) or a recorded target's own answer; `null` when neither has been read. */
  implemented: boolean | null;
  /** The §5.3/§5.4 gap, verbatim from the build (or from the saved target that carries it). */
  unimplemented_reason: string | null;
}

/**
 * The kind options, fed by the `storage_kinds` command and refined by the project's own targets.
 *
 * `kinds` is the command's answer: the BUILD's list, which is the authority on which kinds exist and which have
 * an adapter. This is what makes §5.3/§5.4's gap visible on a project with NO target of that kind — an SFTP or
 * Lsky target cannot usefully be saved, so `storageTargets` alone could never report it.
 *
 * It is optional because the read can fail. With no report the form falls back to the labels it can render and
 * reports every one of them UNKNOWN (`implemented: null`) rather than assuming it works or assuming it fails.
 *
 * A saved target's own `implemented` / `unimplemented_reason` OVERRIDES the kind-level answer for that kind,
 * because a recorded target is the more specific statement and `StorageTargetView` is what carries it. A kind a
 * project recorded under a spelling the build does not report is still offered, so an existing target cannot
 * become uneditable.
 */
export function storageKindOptions(
  views: StorageTargetView[],
  kinds?: StorageKindView[] | null,
): StorageKindOption[] {
  const base: StorageKindOption[] =
    kinds && kinds.length > 0
      ? kinds.map((entry) => {
          const kind = normaliseKind(entry.kind);
          return {
            kind,
            label: KIND_LABELS[kind] ?? entry.kind,
            implemented: entry.implemented,
            unimplemented_reason: entry.unimplemented_reason,
          };
        })
      : Object.entries(KIND_LABELS).map(([kind, label]) => ({
          kind,
          label,
          implemented: null,
          unimplemented_reason: null,
        }));

  const options: StorageKindOption[] = base.map((option) => {
    const seen = views.find((view) => normaliseKind(view.kind) === option.kind);
    if (!seen) return option;
    return {
      ...option,
      implemented: seen.implemented,
      unimplemented_reason: seen.unimplemented_reason,
    };
  });

  for (const view of views) {
    const kind = normaliseKind(view.kind);
    if (kind && !options.some((option) => option.kind === kind)) {
      options.push({
        kind,
        label: view.kind,
        implemented: view.implemented,
        unimplemented_reason: view.unimplemented_reason,
      });
    }
  }

  return options;
}

/** The kinds this build reports as having no adapter, for a statement made BEFORE anything is saved. */
export function unimplementedKinds(options: StorageKindOption[]): StorageKindOption[] {
  return options.filter((option) => option.implemented === false);
}

/**
 * The sentence the form shows for the build's reported gaps, or `null` when there is nothing to report.
 *
 * This is the edge the command exists for. With no target of that kind saved, a target list has nothing to
 * say, and the form used to answer "save one and the target list will report it" — which asks a user to
 * configure a capability this build does not have. The reason is stated up front instead.
 */
export function kindGapSummary(options: StorageKindOption[]): string | null {
  const gaps = unimplementedKinds(options);
  if (gaps.length === 0) return null;
  const details = gaps
    .map((option) => `${option.label}——${option.unimplemented_reason ?? '本构建没有适配器'}`)
    .join(';');
  return `本构建对这些类型没有适配器,保存前就能看到:${details}。可以保存配置,但现在无法上传到这些类型。`;
}

/** What the form says about the chosen kind's adapter, before anything is saved. */
export interface KindNotice {
  level: 'warn' | 'info';
  text: string;
}

export function kindNotice(option: StorageKindOption | undefined): KindNotice | null {
  if (!option) return null;
  if (option.implemented === false) {
    return {
      level: 'warn',
      text: `${option.label}在本构建没有适配器:${option.unimplemented_reason ?? '上游协议未验证'}可以保存配置,但现在无法上传到这个类型。`,
    };
  }
  if (option.implemented === null) {
    return {
      level: 'info',
      text: `${option.label}:还没有读到本构建的类型列表,所以这里不替它判断能不能用;刷新可以重试。`,
    };
  }
  return null;
}

/** The form's state: every field it can carry, as text, plus the kind it is for. */
export interface StorageTargetDraft {
  id: string;
  name: string;
  kind: string;
  pathPrefix: string;
  publicUrlPrefix: string;
  repository: string;
  branch: string;
  endpoint: string;
  region: string;
  bucket: string;
  accessKey: string;
  /** Lsky Pro: the instance address (§13.12). Its API token is the credential. */
  instance: string;
  /** The credential VALUE while it is being typed. Never read back from a view, never shown again. */
  credential: string;
}

/** What `save_storage_target` is asked for. A property that is absent was not asked for at all. */
export interface StorageTargetSaveRequest {
  id: string;
  name: string;
  kind: string;
  pathPrefix?: string | undefined;
  publicUrlPrefix?: string | undefined;
  repository?: string | undefined;
  branch?: string | undefined;
  endpoint?: string | undefined;
  region?: string | undefined;
  bucket?: string | undefined;
  accessKey?: string | undefined;
  instance?: string | undefined;
  credential?: string | undefined;
}

/** A blank form. `local` needs no credential and no network, so it is the safe starting kind. */
export function emptyDraft(kind = 'local'): StorageTargetDraft {
  return {
    id: '',
    name: '',
    kind: normaliseKind(kind),
    pathPrefix: '',
    publicUrlPrefix: '',
    repository: '',
    branch: '',
    endpoint: '',
    region: '',
    bucket: '',
    accessKey: '',
    instance: '',
    credential: '',
  };
}

/**
 * Pre-fill the form from an existing target.
 *
 * Exactly the fields the view carries, and the credential starts EMPTY: the view never carries a credential
 * value (§12.2 asserts the field set for that reason), so an edit cannot render one back, and leaving the field
 * untouched must mean "keep the stored one".
 */
export function draftFromTarget(view: StorageTargetView): StorageTargetDraft {
  return {
    id: view.id,
    name: view.name,
    kind: normaliseKind(view.kind),
    pathPrefix: view.path_prefix ?? '',
    publicUrlPrefix: view.public_url_prefix,
    repository: view.repository ?? '',
    branch: view.branch ?? '',
    endpoint: view.endpoint ?? '',
    region: view.region ?? '',
    bucket: view.bucket ?? '',
    accessKey: view.access_key ?? '',
    instance: view.instance ?? '',
    credential: '',
  };
}

/**
 * The request to save the draft.
 *
 * Only the chosen kind's fields are considered, and a blank value is OMITTED rather than sent: for the
 * credential that is the difference between "leave the stored one alone" and "clear it" (the command surface
 * treats an omitted or empty credential as the former), and for the rest it keeps a leftover value from a
 * previous kind out of the saved configuration.
 */
export function buildSaveRequest(draft: StorageTargetDraft): StorageTargetSaveRequest {
  const request: StorageTargetSaveRequest = {
    id: draft.id.trim(),
    name: draft.name.trim(),
    kind: normaliseKind(draft.kind),
  };

  for (const key of fieldKeysForKind(draft.kind)) {
    const value = draft[key].trim();
    if (value === '') continue;
    request[key] = value;
  }

  return request;
}

/**
 * Why the draft cannot be saved, in the user's terms, or `null` when it can.
 *
 * Mirrors `veloct_attach::StorageConfig::validate` for the fields the window owns, so a configuration mistake
 * is reported as one here, in the field that was typed, instead of as a confusing failure at the first upload.
 *
 * Deliberately NOT a copy of the whole ruleset: the crate's SFTP arm demands a host and a username, while this
 * form asks SFTP for no field at all. That divergence stays open because SFTP is the one kind still exempt from
 * `validate()` on the save path — it is the only kind with no adapter — so demanding those fields here would
 * block configuring a target this build cannot upload to anyway. EVERY other arm is mirrored, Lsky Pro's
 * included: that kind has an adapter now, so the crate refuses a target with no instance at the save.
 */
export function validateDraft(draft: StorageTargetDraft): string | null {
  const kind = normaliseKind(draft.kind);

  if (draft.id.trim() === '') {
    return '请填写目标标识:它会写进附件的去重键,保存后再改会让同一个文件看起来像新的(§13.5)。';
  }
  if (draft.name.trim() === '') {
    return '请填写一个便于识别的名称。';
  }

  if (kind === 's3' && draft.bucket.trim() === '') {
    return 'S3 目标必须填写 Bucket(§13.11);Endpoint 留空表示使用 AWS。';
  }
  if (kind === 'github' && draft.repository.trim() === '') {
    return 'GitHub 目标必须填写仓库,写成 owner/repo(§13.13)。';
  }
  if (kind === 'lsky') {
    const instance = draft.instance.trim();
    if (instance === '') {
      return 'Lsky Pro 目标必须填写实例地址(§13.12),例如 https://img.example.com;API 令牌填在凭据里。';
    }
    if (!/^https?:\/\//i.test(instance)) {
      return `实例地址 ${instance} 必须以 http:// 或 https:// 开头:上传地址是在它后面拼 /api/v1 得到的。`;
    }
  }

  const prefix = draft.pathPrefix.trim();
  // Only a kind that SUBMITS this field is checked, mirroring `buildSaveRequest` and the public-URL rule
  // below. `fieldKeysForKind` renders `pathPrefix` for `s3`/`github`/`local` only, so a value left over
  // from a kind switch used to keep blocking a save for SFTP or Lsky Pro — a field those kinds do not
  // render, which the user could not clear without switching back to the kind that showed it. That
  // OVER-refused rather than under-refused, and it is the last member of the two-language pair that
  // PROCESSING.md §5.10.1 recorded.
  if (fieldKeysForKind(kind).includes('pathPrefix') && prefix !== '') {
    if (kind === 'local' && prefix.includes(':')) {
      return `目录前缀 ${prefix} 不能包含冒号:它会被当成盘符,把文件写到目标目录之外(§13.10)。`;
    }
    if (kind === 'local' && prefix.startsWith('\\')) {
      return `目录前缀 ${prefix} 不能以 \\ 开头:那是 UNC 路径,会写到目标目录之外(§13.10)。`;
    }
    if (prefix.startsWith('/')) {
      return `路径前缀 ${prefix} 必须以相对路径书写,不能以 / 开头。`;
    }
    if (prefix.includes('..')) {
      return `路径前缀 ${prefix} 不能包含 ..,否则会把文件写到目标目录之外。`;
    }
  }

  // §13.6 keeps the public URL prefix apart from the object's actual path, and the Markdown address is the two
  // put together. A relative prefix (or a protocol-relative one) therefore produces a URL no browser can
  // resolve. `StorageConfig::validate` refuses it; saying so HERE means the user reads the reason under the
  // field they typed instead of seeing a backend message after a save that could never have succeeded.
  //
  // Two boundaries, both deliberate:
  //  * Empty stays ALLOWED. The crate treats that as "no URL can be derived yet" and refuses later, at
  //    `public_url`, when an address is actually needed — a target the user is still configuring is a
  //    legitimate state, and requiring the field would block saving one before its URL is known.
  //  * Only a kind that SUBMITS this field is checked, mirroring `buildSaveRequest`. Otherwise a value hidden by
  //    a kind switch could block a save whose cause the user cannot even see.
  if (fieldKeysForKind(kind).includes('publicUrlPrefix')) {
    const publicUrl = draft.publicUrlPrefix.trim();
    if (publicUrl !== '' && !publicUrl.startsWith('http://') && !publicUrl.startsWith('https://')) {
      return `公开 URL 前缀 ${publicUrl} 必须是以 http:// 或 https:// 开头的完整地址:它要和对象路径拼接起来才是浏览器打开的地址(§13.6),相对地址拼出来的 URL 无法解析。`;
    }
  }

  return null;
}

/** The result of switching a kind: the new draft, and the filled values the new kind will NOT submit. */
export interface KindChange {
  draft: StorageTargetDraft;
  hiddenFilled: StorageFieldKey[];
}

/**
 * Switch a kind WITHOUT discarding what was typed.
 *
 * The values stay in the form, so switching back restores them, and the fields the new kind will not submit are
 * named — a kind switch that silently dropped a bucket or a repository would be worse than one that refuses.
 */
export function changeKind(draft: StorageTargetDraft, kind: string): KindChange {
  const next: StorageTargetDraft = { ...draft, kind: normaliseKind(kind) };
  const shown = new Set(fieldKeysForKind(next.kind));
  const hiddenFilled = (Object.keys(FIELDS) as StorageFieldKey[]).filter(
    (key) => !shown.has(key) && draft[key].trim() !== '',
  );
  return { draft: next, hiddenFilled };
}

/** A sentence for the fields a kind switch kept but will not save. */
export function hiddenFieldsNote(keys: StorageFieldKey[], kind: string): string | null {
  if (keys.length === 0) return null;
  const labels = keys.map((key) => fieldLabel(key)).join('、');
  return `你已填写的 ${labels} 不属于「${normaliseKind(kind)}」类型,不会随这次保存提交;切回原类型仍然保留。`;
}

/** The upload control's state: disabled or not, and the reason a user can read. */
export interface UploadControl {
  disabled: boolean;
  /** Shown next to the button. §13.2.2's reason, so a disabled button is never unexplained. */
  reason: string | null;
}

/**
 * Whether an attachment may be uploaded, and why not.
 *
 * The eligibility is consulted BEFORE the attempt. `null` means it has not been read yet, which is reported as
 * such rather than treated as "allowed": the one thing this must never do is let a button look available for
 * an attachment whose state §13.2.2 forbids uploading.
 */
export function uploadControl(options: {
  eligibility: UploadEligibility | null;
  storageTargetId: string;
}): UploadControl {
  if (options.storageTargetId === '') {
    return { disabled: true, reason: '请先选择一个存储目标,上传才有去向。' };
  }
  if (!options.eligibility) {
    return { disabled: true, reason: '正在读取该附件的上传条件(§13.2.2)。' };
  }
  if (!options.eligibility.can_upload) {
    return {
      disabled: true,
      reason:
        options.eligibility.reason ??
        `该附件的上传状态是 ${options.eligibility.upload_status},按第 13.2.2 节现在不能上传。`,
    };
  }
  return { disabled: false, reason: null };
}

/**
 * The eligibility to use when the read itself failed.
 *
 * `uploadEligibility` is the authority on whether §13.2.2 allows another attempt, and this does not pretend to
 * replace it: it is the attachment's own `upload_status` axis, which is what the window judged by before the
 * reason existed, plus a reason that says the read failed. Without it a transient failure would leave the upload
 * button disabled with nothing the user could do about it.
 */
export function eligibilityFallback(uploadStatus: string): UploadEligibility {
  const permitted = uploadStatus === 'PENDING' || uploadStatus === 'FAILED';
  return {
    can_upload: permitted,
    upload_status: uploadStatus,
    reason: permitted
      ? null
      : `没能读到第 13.2.2 节的上传条件,这里按附件状态 ${uploadStatus} 判断:现在不能上传;刷新可以重试。`,
  };
}

/** The §13.8 cleanup as the window offers it for one attachment. */
export interface RemoteCleanup {
  /** The target to name, or `null` when there is no recorded remote object to remove. */
  storageTargetId: string | null;
  disabled: boolean;
  disabledReason: string | null;
  /** What the action does, what it does NOT touch, and the consequence the user lives with. */
  wording: string;
}

const REMOTE_CLEANUP_WORDING =
  '只删除远程对象:不改动文章里的 Markdown 引用,不删除这条附件记录,也不删除缓存文件;删除后该附件可以重新上传(§13.8)。注意:文章里仍然指向这个地址,远程对象删掉后那里会变成断链——Veloct 不会替你改写文章(§13.8 不做自动重写)。';

/**
 * The remote cleanup for one attachment.
 *
 * `deleteRemoteObject` is refused when the target named is not the one the object's record names, so the target
 * comes from the recorded object (`attachmentObjects()`), and only falls back to the attachment's own target for
 * the ordinary flow where both are the same. Removing the record and the cached bytes is the OTHER action
 * (`removeAttachment`); §13.8 keeps them apart on purpose, and so does this.
 */
export function remoteCleanup(
  attachment: AttachmentView,
  objects: ObjectView[],
  targets: StorageTargetView[],
): RemoteCleanup {
  const wording = REMOTE_CLEANUP_WORDING;
  if (!attachment.remote_url && !attachment.remote_key) {
    return { storageTargetId: null, disabled: false, disabledReason: null, wording };
  }

  const recorded = attachment.remote_key
    ? objects.find((object) => object.key === attachment.remote_key)
    : undefined;
  const storageTargetId = recorded?.storage_target_id ?? attachment.storage_target_id;
  const target = targets.find((candidate) => candidate.id === storageTargetId);
  // A kind with no adapter cannot delete remotely either (§13.10/§13.12 report their gap), so the action is
  // offered with its reason rather than allowed to fail.
  if (target && !target.implemented) {
    return {
      storageTargetId,
      disabled: true,
      disabledReason: target.unimplemented_reason ?? `「${target.name}」在本构建没有适配器。`,
      wording,
    };
  }

  return { storageTargetId, disabled: false, disabledReason: null, wording };
}

/**
 * What the delete confirmation must say.
 *
 * `delete_storage_target` also removes that target's credential from the operating system store, so a user who
 * deletes a target has to enter the credential again — and §13.8 means the remote files are untouched, which is
 * the other half of what a user needs to know before confirming. This is destructive and irreversible, so the
 * caller shows it as a confirmation step that cannot be dismissed by accident.
 */
export function deletionWarning(target: Pick<StorageTargetView, 'id' | 'name' | 'has_credential'>): string {
  const credential = target.has_credential
    ? '会同时从操作系统凭据库中删除它保存的凭据,以后重新创建这个目标时必须重新输入凭据'
    : '会同时清掉它的凭据记录(操作系统凭据库里如果没有值,这一步没有额外影响)';
  return `删除存储目标「${target.name}」(${target.id}):${credential}。已经上传到该目标的远程文件不会被删除(§13.8);要删除远程文件,请对单个附件使用“仅删除远程对象”。`;
}
