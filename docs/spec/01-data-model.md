# Veloct — Data model & on-disk contract

> Derived directly from `handoff.md` §3, §7, §10, §13, §14, §20, §22. Where this file and
> `handoff.md` disagree, `handoff.md` wins.

## 1. Project directory layout (truth on disk)

```text
<project>/
├── .veloct/
│   ├── project.json                 # project identity + site template version
│   ├── veloct.db                    # single global SQLite DB (handoff §2.4)
│   ├── releases/<release-id>/
│   │   ├── release.json             # manifest — MUST NOT contain its own commit sha
│   │   ├── build-inputs.json        # frozen non-Secret build inputs
│   │   ├── target-snapshot.json     # frozen logical/main/works targets
│   │   ├── site-template/           # frozen Astro template files for reproducible rebuild
│   │   └── dist-manifest.json       # normalised route/asset manifest of the build
│   └── logs/veloct-YYYY-MM-DD.log   # rotated log files (handoff §18)
├── content/
│   ├── articles/<stable-id>.md
│   ├── pages/<stable-id>.md
│   └── knowledge/<stable-id>.md
├── works/<stable-id>/
│   ├── work.json
│   ├── preview/{cover.webp,screenshot.webp}
│   └── site/**                      # work body truth
├── site/                            # the Astro project (copied from app template)
│   ├── astro.config.mjs
│   ├── package.json
│   ├── package-lock.json
│   ├── src/{content.config.ts,components,layouts,pages,styles}
│   └── public/
├── data/
│   ├── site.json                    # site config (single website) — config truth is SQLite,
│   │                                # this file is the frozen export for building
│   └── links.json                   # friend links
├── .attachments-cache/<sha256>/<original-name-with-suffix>
├── dist/
│   ├── site/
│   └── works/
└── .git/
```

Markdown body files are addressed by **stable id**, not slug: renaming a slug never moves a
file and never changes `id` (handoff §3.2).

## 2. Markdown frontmatter contract

```yaml
---
id: 01J8Z5Q0K3W7M9V2B4N6P8R1TY      # ULID, permanent, never derived from slug
type: article                       # article | page | knowledge
slug: hello-world                   # mutable, public
title: Hello World
summary: ""                         # optional; §11.11 can fill it
wip: false                          # article only
wip_publish: false                  # article only; §7.1
wip_ever_published: false           # §7.2 — records "this WIP URL was public once"
created_at: 2026-01-01T00:00:00Z
updated_at: 2026-01-01T00:00:00Z
published_at: null
tags: []
category: null
cover: null
# page-creator only:
blocks: []                          # {{blockRef}} list, §8 — still Markdown-expressible
---
```

Additional rule from handoff §7.2: `wip_ever_published` is set to `true` the first time a
build actually emits `/wip/<id>` into `dist/site` (i.e. `wip=true && wip_publish=true`). It is
never reset by the user. It is what makes the permanent redirect possible later.

### 2.1 Paragraph identity (handoff §9.2)

Knowledge bodies carry hidden, stable paragraph ids as HTML comments on their own line:

```markdown
<!--p:01J8Z5Q0K3W7M9V2B4N6P8R1TZ-->

First paragraph text.

<!--p:01J8Z5Q0K3W7M9V2B4N6P8R2AA-->

Second paragraph text.
```

- Rendered HTML never shows these (Astro/HTML strips comments or emits them harmlessly).
- The editor maintains them; they are ULIDs, so they never collide.
- Ids do not depend on paragraph text, so editing text preserves the id.
- Ledger of observed ids lives in SQLite `knowledge_paragraphs`, keyed by
  `(content_id, paragraph_id)` and carrying `paragraph_order` + `content_hash`
  (handoff §22.5).

### 2.2 Reference conflict rules (handoff §9.3)

A reference is a directive in the *referencing* article:

```markdown
:::knowledgeref{content="01J8Z..." paragraph="01J8Z...P"}
:::
```

Build resolution outcomes — never guess:

| Situation | Outcome |
|---|---|
| id + paragraph resolve | card with title, excerpt, ellipsis |
| `paragraph` missing in DB | `BROKEN_REFERENCE` diagnostic, card renders broken state |
| duplicate paragraph id in one knowledge file | `DUPLICATE_PARAGRAPH_ID` diagnostic (error) |
| unknown content id | `UNKNOWN_CONTENT_ID` diagnostic |
| knowledge file has paragraphs with no ids at all | `UNIDENTIFIED_PARAGRAPH` diagnostic (warning) |

Diagnostics are surfaced in the release build report and in the editor, and an unresolved
reference keeps its broken state until the user fixes it (handoff §9.3).

## 3. SQLite schema (v1 — migrations must be re-runnable and versioned, handoff §20.1)

```sql
CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);

-- ---------- content index (rebuildable from Markdown; NEVER the body truth) ----------
CREATE TABLE contents (
  id TEXT PRIMARY KEY,                 -- stable ULID
  type TEXT NOT NULL,                  -- article | page | knowledge
  file_path TEXT NOT NULL UNIQUE,      -- project-relative POSIX path
  slug TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT,
  category TEXT,
  cover TEXT,
  wip INTEGER NOT NULL DEFAULT 0,
  wip_publish INTEGER NOT NULL DEFAULT 0,
  wip_ever_published INTEGER NOT NULL DEFAULT 0,
  tags TEXT NOT NULL DEFAULT '[]',     -- JSON array
  blocks TEXT NOT NULL DEFAULT '[]',   -- JSON array (page creator)
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  published_at TEXT,
  content_hash TEXT NOT NULL,          -- sha256 of the markdown file body+frontmatter
  word_count INTEGER NOT NULL DEFAULT 0,
  reading_minutes INTEGER NOT NULL DEFAULT 0,
  indexed_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_contents_type_slug ON contents(type, slug);
CREATE INDEX idx_contents_type_wip ON contents(type, wip);

CREATE TABLE content_paragraphs (
  content_id TEXT NOT NULL REFERENCES contents(id) ON DELETE CASCADE,
  paragraph_id TEXT NOT NULL,
  paragraph_order INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  PRIMARY KEY (content_id, paragraph_id)
);

CREATE TABLE content_links (          -- graph of knowledge<->article references
  from_content_id TEXT NOT NULL REFERENCES contents(id) ON DELETE CASCADE,
  to_content_id TEXT NOT NULL REFERENCES contents(id) ON DELETE CASCADE,
  to_paragraph_id TEXT,
  reference_location TEXT NOT NULL,   -- e.g. "body:block:3"
  status TEXT NOT NULL,               -- OK | BROKEN
  diagnostic TEXT,
  PRIMARY KEY (from_content_id, to_content_id, to_paragraph_id, reference_location)
);

-- ---------- site / app config (config truth = SQLite) ----------
CREATE TABLE site_config  (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE friend_links (id TEXT PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL,
                           avatar TEXT, description TEXT, sort INTEGER NOT NULL DEFAULT 0);
CREATE TABLE nav_items    (id TEXT PRIMARY KEY, label TEXT NOT NULL, href TEXT NOT NULL,
                           sort INTEGER NOT NULL DEFAULT 0, parent_id TEXT);
CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL);

-- ---------- AI (no secrets: credential_ref only) ----------
CREATE TABLE ai_providers (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, protocol TEXT NOT NULL,  -- openai|anthropic|ollama
  base_url TEXT NOT NULL, credential_ref TEXT, enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL);
CREATE TABLE ai_models (
  id TEXT PRIMARY KEY, provider_id TEXT NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
  model_id TEXT NOT NULL, display_name TEXT, source TEXT NOT NULL, -- fetched|manual
  context_window INTEGER, last_seen_at TEXT, UNIQUE(provider_id, model_id));
CREATE TABLE ai_features (
  id TEXT PRIMARY KEY,           -- builtin feature id, e.g. article.summary
  label TEXT NOT NULL,
  provider_id TEXT REFERENCES ai_providers(id), model_id TEXT, enabled INTEGER NOT NULL DEFAULT 0);
CREATE TABLE ai_fallback (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  provider_id TEXT REFERENCES ai_providers(id), model_id TEXT);
CREATE TABLE ai_requests (       -- metadata only; no chat history (handoff §11.6, §22.6)
  id TEXT PRIMARY KEY, feature_id TEXT NOT NULL, provider_id TEXT, model TEXT,
  status TEXT NOT NULL,          -- SUCCEEDED|FAILED|CANCELLED|FALLBACK_SUCCEEDED|FALLBACK_FAILED
  used_fallback INTEGER NOT NULL DEFAULT 0, error_code TEXT, error_message TEXT,
  created_at TEXT NOT NULL, completed_at TEXT);

-- ---------- deployment targets (no secrets) ----------
CREATE TABLE storage_defs (      -- attachment storage targets, handoff §13
  id TEXT PRIMARY KEY, kind TEXT NOT NULL,       -- sftp|s3|lsky|github|remote_image
  name TEXT NOT NULL, config TEXT NOT NULL,      -- JSON, secret-free
  credential_ref TEXT, enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL);
CREATE TABLE storage_namespaces (-- §13.5 namespace identity
  id TEXT PRIMARY KEY, storage_def_id TEXT NOT NULL REFERENCES storage_defs(id) ON DELETE CASCADE,
  namespace TEXT NOT NULL,       -- canonical string: s3:bucket+prefix / github:repo+branch+path ...
  public_url_prefix TEXT, created_at TEXT NOT NULL, UNIQUE(storage_def_id, namespace));

CREATE TABLE deploy_targets (    -- one row per logical target
  id TEXT PRIMARY KEY, kind TEXT NOT NULL,       -- cloudflare|edgeone_cn|edgeone_intl|manual_1panel
  name TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL);
CREATE TABLE deploy_subtargets ( -- handoff §10.5/§14.7: main | works
  id TEXT PRIMARY KEY, target_id TEXT NOT NULL REFERENCES deploy_targets(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('main','works')),
  config TEXT NOT NULL,          -- JSON, secret-free: project name, origin url, branch ...
  credential_ref TEXT,
  is_degraded INTEGER NOT NULL DEFAULT 0,   -- true when no independent Works Origin exists
  UNIQUE(target_id, role));

-- ---------- release index (query index only; truth = release.json + git tag) ----------
CREATE TABLE releases (
  release_id TEXT PRIMARY KEY, commit_sha TEXT NOT NULL, tag_name TEXT NOT NULL UNIQUE,
  build_fingerprint TEXT NOT NULL, build_inputs_hash TEXT NOT NULL,
  created_at TEXT NOT NULL, created_from_release_id TEXT);
CREATE TABLE deployment_runs (   -- handoff §22.3; never written back into release.json
  id TEXT PRIMARY KEY, release_id TEXT NOT NULL, logical_target_id TEXT NOT NULL,
  sub_target TEXT NOT NULL CHECK (sub_target IN ('main','works')),
  status TEXT NOT NULL,          -- QUEUED|RUNNING|SUCCEEDED|FAILED|SKIPPED
  build_fingerprint TEXT, reused_dist INTEGER NOT NULL DEFAULT 0,
  error TEXT, started_at TEXT, finished_at TEXT);
CREATE TABLE seo_pushes (        -- handoff §15.2: independent post-publish task
  id TEXT PRIMARY KEY, release_id TEXT NOT NULL, engine TEXT NOT NULL,
  status TEXT NOT NULL, error TEXT, created_at TEXT, finished_at TEXT, manual_retry INTEGER NOT NULL DEFAULT 0);

-- ---------- attachments (handoff §13, §22.4) ----------
CREATE TABLE attachments (
  id TEXT PRIMARY KEY, original_name TEXT NOT NULL, stored_name TEXT, content_hash TEXT NOT NULL,
  mime_type TEXT, size INTEGER NOT NULL,
  storage_def_id TEXT REFERENCES storage_defs(id), storage_namespace_id TEXT REFERENCES storage_namespaces(id),
  remote_key TEXT, public_url TEXT,
  upload_status TEXT NOT NULL,     -- PENDING|UPLOADING|SUCCEEDED|FAILED
  reference_status TEXT NOT NULL,  -- NOT_APPLIED|APPLYING|APPLIED|FAILED
  cache_status TEXT NOT NULL,      -- PRESENT|CLEANED
  cache_path TEXT, remote_object_id TEXT,
  last_error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX idx_attachments_dedup ON attachments(storage_def_id, storage_namespace_id, content_hash);
CREATE INDEX idx_attachments_status ON attachments(upload_status, reference_status, cache_status);

CREATE TABLE attachment_references (   -- a real set, not a boolean (handoff §13.2.3)
  attachment_id TEXT NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  content_id TEXT NOT NULL REFERENCES contents(id) ON DELETE CASCADE,
  reference_location TEXT NOT NULL,    -- mdast node path
  created_at TEXT NOT NULL,
  PRIMARY KEY (attachment_id, content_id, reference_location));
CREATE INDEX idx_attachment_refs_content ON attachment_references(content_id);

CREATE TABLE attachment_tasks (        -- resumable operations; never lose a file (§13.3, §20.4)
  id TEXT PRIMARY KEY, attachment_id TEXT NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,                  -- UPLOAD|APPLY_REFERENCE|REWRITE_URL
  status TEXT NOT NULL,                -- QUEUED|RUNNING|SUCCEEDED|FAILED|INTERRUPTED
  attempts INTEGER NOT NULL DEFAULT 0, payload TEXT, error TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE upload_locks (            -- §13.4 concurrency guard
  dedup_key TEXT PRIMARY KEY, owner_task_id TEXT NOT NULL, acquired_at TEXT NOT NULL);

-- ---------- backups (handoff §17, §22.7) ----------
CREATE TABLE backup_targets (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL,   -- github|s3
  name TEXT NOT NULL, config TEXT NOT NULL, credential_ref TEXT,
  retention INTEGER NOT NULL DEFAULT 3,     -- §17.6 default 3
  enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL);
CREATE TABLE backups (
  id TEXT PRIMARY KEY, target_id TEXT, kind TEXT NOT NULL,      -- auto|manual|export
  created_at TEXT NOT NULL, status TEXT NOT NULL,
  artifact_identifier TEXT, size INTEGER, snapshot_ref TEXT,
  secret_scan_status TEXT, secret_scan_hits INTEGER NOT NULL DEFAULT 0,
  error TEXT);

-- ---------- logs / diagnostics index (handoff §18) ----------
CREATE TABLE log_index (id INTEGER PRIMARY KEY AUTOINCREMENT, ts TEXT NOT NULL, level TEXT NOT NULL,
                        area TEXT NOT NULL, message TEXT NOT NULL, detail TEXT);
CREATE TABLE diagnostics (id TEXT PRIMARY KEY, area TEXT NOT NULL, code TEXT NOT NULL,
                          severity TEXT NOT NULL, message TEXT NOT NULL, detail TEXT,
                          content_id TEXT, created_at TEXT NOT NULL, resolved INTEGER NOT NULL DEFAULT 0);
CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, root_path TEXT NOT NULL UNIQUE,
                       site_template_version TEXT NOT NULL, created_at TEXT NOT NULL, last_opened_at TEXT);
```

Notes:

- The `contents`, `content_paragraphs`, `content_links` tables are **derived**: dropping and
  rebuilding them from Markdown must produce an identical index (handoff §2.3, §19.1, §20.1).
  A test asserts this property.
- No table has a column that can hold a secret value. `credential_ref` is an opaque handle
  resolved through the OS credential store (handoff §12).

## 4. `work.json` contract (handoff §10.1, §22)

```json
{
  "id": "01J8Z5Q0K3W7M9V2B4N6P8R1TY",
  "slug": "my-demo",
  "title": "My Demo",
  "author": "vellow",
  "summary": "",
  "completed_at": "2026-01-01T00:00:00Z",
  "imported_at": "2026-01-01T00:00:00Z",
  "source": { "kind": "zip", "original_name": "my-demo.zip" },
  "entry": "index.html",
  "tags": [],
  "cover": "preview/cover.webp",
  "screenshot": "preview/screenshot.webp",
  "size_bytes": 12345678,
  "notes": ""
}
```

`completed_at` defaults to the import timestamp and stays editable (handoff §10.2).

## 5. Release artifacts (handoff §14.4–§14.9)

`.veloct/releases/<id>/release.json` — **contains no commit sha**:

```json
{
  "release_id": "20260101T120000Z-3f9a1c",
  "schema": 1,
  "build_inputs_hash": "<sha256>",
  "build_toolchain_snapshot": { "node": "...", "npm": "...", "astro": "...", "os": "..." },
  "deployment_target_snapshot_ref": "target-snapshot.json",
  "site_template_version": "...",
  "release_build_time": "2026-01-01T12:00:00Z",
  "created_at": "2026-01-01T12:00:00Z"
}
```

`build-inputs.json` freezes content hashes, site config, nav, WIP policy, comment config, SEO
config, work metadata, toolchain, lockfile hash, allow-listed build env, and
`credential_ref`s only (handoff §14.6).

`build_fingerprint = sha256(release_commit_sha ‖ build_inputs_hash ‖ lockfile_hash ‖
toolchain_version ‖ build_env_hash)` (handoff §14.8).

## 6. Status machines (single definition, used by code and tests)

```text
Attachment.upload_status    : PENDING → UPLOADING → SUCCEEDED | FAILED
Attachment.reference_status : NOT_APPLIED → APPLYING → APPLIED | FAILED
Attachment.cache_status     : PRESENT → CLEANED

Orphan-safe-to-clean ⇔ reference_status == NOT_APPLIED
                     ∧ no queued APPLY task
                     ∧ upload_status ∉ {PENDING, UPLOADING}
                     ∧ no resumable tasks                       (handoff §13.2.4)

DeploymentRun.status        : QUEUED → RUNNING → SUCCEEDED | FAILED | SKIPPED
Logical target roll-up      : SUCCEEDED | PARTIAL (main/works differ) | FAILED
Release life cycle          : DRAFT → COMMITTED → TAGGED → BUILT → DISTRIBUTED
```

`SUCCEEDED + FAILED + PRESENT` enables "retry reference application" **without** re-uploading
(handoff §13.2.2, §24.9).
