# Upstream facts (verified live, not from memory)

> Every version and API shape in this file was read from crates.io, registry.npmjs.org or the
> official docs during project setup. It exists so no team member has to guess, and so a wrong
> assumption is caught here rather than in a build failure. Anything that could **not** be
> verified is marked `UNVERIFIED` and must be treated as unknown, never invented.

## 1. Host environment (this machine)

| Fact | Value |
|---|---|
| rustc / cargo | 1.97.1 — **not on `PATH`**; use `%USERPROFILE%\.cargo\bin` or `scripts/cargo.ps1` |
| active toolchain | `stable-x86_64-pc-windows-msvc` (only the msvc target is installed) |
| MSVC build tools | Visual Studio **18** BuildTools, `Hostx64\x64\link.exe` |
| Windows SDK | 10.0.26100.0 |
| WebView2 runtime | 153.0.4234.48 (installed) |
| node / npm | v24.18.0 / 11.19.0 (npm must be invoked as `npm.cmd` or via `cmd /c`; `npm.ps1` is blocked by execution policy) |
| PowerShell | Windows PowerShell 5.1; **`pwsh` is not installed**; `.ps1` execution is disabled unless invoked with `-ExecutionPolicy Bypass` |
| package managers | npm and pnpm present; bun absent |
| git | `D:\Git\cmd\git.exe` |

### 1.1 Cargo registry access (important)

`%USERPROFILE%\.cargo\config.toml` replaces crates.io with a USTC mirror, and Windows has an
HTTP proxy configured (`127.0.0.1:10818`, an xray client) that Cargo's libcurl picks up but the
mirror is unreachable through. Two things make Cargo work here:

1. An isolated `CARGO_HOME` at `.cargo-home/` pinning the canonical sparse registry
   (`sparse+https://index.crates.io/`).
2. `CARGO_HTTP_PROXY=http://127.0.0.1:10818` (the machine's proxy).

Both are handled by `scripts/cargo.ps1`. **Always run Cargo through that script from this
repository**; a bare `cargo` will fail with `failed to load source for dependency` errors.

## 2. Rust crates (crates.io `max_stable_version`)

Read `max_stable_version`, **not** `newest_version` — Tauri 3.0.0-alpha.3 is published and
`newest_version` returns it.

| Crate | Pin | Notes |
|---|---|---|
| `tauri` | 2.12 | MSRV 1.90 |
| `tauri-build` | 2.7 | |
| `tauri-plugin-shell` | 2.4 | the only supported sidecar path in Tauri 2 |
| `tauri-plugin-opener` | 2.6 | |
| `tauri-plugin-dialog` | 2.8 | |
| `keyring` | 4.2, feature `v1` | Windows support is automatic under `v1`; `Entry::new` returns `Result`; the delete method is **`delete_credential`**, not `delete_password` |
| `git2` | 0.21 | `default-features = false, features = ["vendored-libgit2"]`. **Never enable `https`/`ssh`** — they pull `openssl-sys`, which has no system provider on MSVC |
| `rusqlite` | 0.40, features `bundled`+`backup`+`functions` | `bundled` compiles SQLite from source; no system DLL |
| `rusqlite_migration` | 2.6 | tracks version in SQLite's `user_version`, no CLI needed, `const`-constructible via `from_slice` |
| `zip` | 8 | default features include `deflate-zopfli` and a pure-Rust zlib — no C zlib needed |
| `reqwest` | 0.13 | already a Tauri 2.12 dependency, so versions align; use `rustls-tls`, never native-tls |
| `gix` (alternative to git2) | 0.88 | pure Rust; feature names beyond `max-performance-safe` are `UNVERIFIED` |

### 2.1 Tauri 2 sidecar facts (these are easy to get wrong)

- `bundle.externalBin` entries are paths **without** the triple and **without** `.exe`,
  resolved against `src-tauri/`. For `"binaries/veloct-sidecar"` the file on disk must be
  `src-tauri/binaries/veloct-sidecar-x86_64-pc-windows-msvc.exe`.
- At runtime the sidecar is resolved **relative to the running executable's directory**, so it
  must sit next to `veloct.exe`.
- `app.shell().sidecar("name")` from **Rust** takes the **bare** file name and is **not**
  ACL-gated. The JS path takes the full configured string and *is* gated. Veloct spawns from
  Rust, so no `shell:allow-*` capability is needed.
- `CommandChild::kill()` terminates **only that one process**. A Node sidecar spawns
  esbuild/Vite children, so teardown must be `taskkill /PID <pid> /T /F` on Windows.
- `CommandEvent` is `#[non_exhaustive]`; stdout/stderr arrive as `Vec<u8>` already split on
  newlines, so the `_ => {}` arm is mandatory.
- There is **no** spawning API in `tauri::process`; `tauri-plugin-process` only offers
  `exit`/`restart`.
- Kill the sidecar from `app.run(|_, event| ...)` on `RunEvent::Exit` — **not** from
  `on_window_event`, which fires per window.

### 2.2 Capabilities

Do all filesystem, HTTP, git, SQLite and upload work in Rust `#[tauri::command]`s and grant the
webview almost nothing. That avoids the `http:default` trap (it enables the fetch verbs but
allows **no origins**, so every request is denied until an explicit URL scope is added).

## 3. Node packages (registry.npmjs.org `dist-tags.latest`)

| Package | Pin | Notes |
|---|---|---|
| `astro` | **7.3.5** | engines `node >=22.12.0`; deps include `vite ^8`, `zod ^4.5.4` |
| `tailwindcss` + `@tailwindcss/vite` | 4.3.3 | CSS-first; **no `tailwind.config.*`, no PostCSS config** |
| `@astrojs/tailwind` | **do not use** | peer range stops at `astro ^5`; incompatible with Astro 7 |
| `@astrojs/sitemap` | 3.7.4 | `filter(page => ...)` receives the full URL |
| `@astrojs/rss` | 4.0.19 | |
| `@astrojs/markdown-remark` | 7.3.1 | **not installed by default on Astro 7**; needed for the `unified()` processor |
| `remark-directive` | 4.0.0 | ESM-only |
| `@tauri-apps/api` / `@tauri-apps/cli` | 2.12.0 | |
| `@tauri-apps/plugin-opener` | 2.6.0 | |

### 3.1 Astro 7 API differences that break most generated code

- Config file is `src/content.config.ts`; `src/content/config.ts` is legacy and removed.
- Zod is **v4**: import `{ z } from 'astro/zod'`. `z.string().url()` → `z.url()`; error message
  option is `{ error: ... }`, not `{ message: ... }`.
- `entry.slug` **does not exist**. `CollectionEntry` has `id`, `collection`, `data`, `body`,
  `rendered`, `filePath`.
- `render(entry)` is a free function from `astro:content`; `entry.render()` is legacy.
- Custom components go on the **component**: `<Content components={{ ... }} />`. There is no
  `components` option on `render()`.
- `astro:build:done` receives `{ pages, dir, assets, logger }` — it is `pages`, **not** `routes`,
  and `dir` is a **`URL`**.
- `compressHTML` defaults to `'jsx'` in v7 (was `true`), which changes emitted whitespace.
- `getCollection()` is typed as non-Promise; `getStaticPaths()` must not use `Astro`.

### 3.2 UNVERIFIED (must not be assumed)

1. `this.emitFile` inside an Astro integration hook — treat as unavailable; write files with
   `node:fs` from `astro:build:done`, or use `createCodegenDir()`.
2. Whether a **new** content-collection entry written by an external process appears in a
   running `astro dev` without a content-layer refresh. Editing an existing entry is fine.
   The escape hatch is the `astro:server:setup` hook's `refreshContent()`.
3. Custom (capitalised) tag names in the `<Content components>` map — the docs only sanction
   HTML element names. Veloct therefore emits HTML element names (`aside`, `figure`, ...) with
   `data-veloct-*` attributes.
4. `rusqlite`'s `serde_json` feature.
5. Whether `@tauri-apps/plugin-http` 2.7.0 interoperates with the Rust `tauri-plugin-http`
   2.8.0 (a real upstream version skew; Veloct avoids the JS HTTP plugin entirely).
6. ~~EdgeOne's upload API.~~ **Protocol/source verification closed 2026-10-17**, see the dated
   EdgeOne supplement below and ADR-0012 follow-up. Both regions now have direct-upload adapters;
   real-account deployment remains unverified and requires separate user confirmation.
7. ~~Unsplash's and Pexels's image-search protocols.~~ **CLOSED by section 4 below: both were read
   live on 2026-10-01, and Pixabay's was read too — which is why Pixabay is NOT one of the two sources
   implemented.** Reading the vendors first changed the design; the entry is kept so that the order of
   events is visible.

### EdgeOne direct-upload protocol verified (2026-10-17)

Sources read, not guessed:
- [Official API Token guide](<https://pages.edgeone.ai/document/api-token>): create a Pages/Makers API Token
  in the selected region's console; it authorizes direct uploads. Do not ask for Tencent Cloud account keys.
- [Makers SDK 0.1.0 client](<https://unpkg.com/@edgeone/makers-sdk@0.1.0/dist/client.js>) and package contract
  0.1.39: both regional API bases, Bearer header, JSON `Action`, `Code=0` and `Data.Response`; project
  lookup, `DescribePagesCosTempToken`, `CreatePagesDeployment`, `DescribePagesDeployments`.
- [Official COS signing reference](<https://cloud.tencent.com/document/product/436/7778>) and
  [COS Node SDK 2.15.4 signing implementation](<https://unpkg.com/cos-nodejs-sdk-v5@2.15.4/sdk/util.js>):
  HMAC-SHA1 key-time / HTTP-string / string-to-sign protocol. A fixed SDK-generated signature for Unicode
  object paths and temporary tokens is checked verbatim by the Rust test.

The China endpoint is `https://pages-api.cloud.tencent.com/v1`; international is
`https://pages-api.edgeone.ai/v1`. A project must be an existing `Provider=Upload` project. The service
returns short-lived Bucket/Region/TargetPath and Credentials (TmpSecretId/TmpSecretKey/Token); only these
are used for COS ZIP upload, never the Pages bearer token. Creation uses `Env=Production`, `DistType=Zip`
and `TempBucketPath`, then polls the returned deployment ID until `Success`, failure or bounded timeout.
Cloud response text and untrusted URLs are not surfaced as executable instructions or credential-bearing
links. No CLI install or live-account write was performed to establish these protocol facts.

**Evidence boundary:** this is official-source verification plus offline HTTP/ZIP/signing tests and real
browser UI fixtures. It is not a user's token/project permission, quota, domain or successful-live-deploy
check. A real cloud deployment still needs explicit permission and a configured region/project/Token.

## 4. Image providers (read live, verified — and why there are two and not three)

Read on **2026-10-01** from each provider's own documentation, plus Unsplash's published OpenAPI
specification (`https://unsplash.com/spec/v1.json`), which is what makes their `required` fields a fact
rather than an inference.

`handoff.md` §13.1's fifth target is "free network image sources (such as Pixabay)" and §13.14
prescribes `搜索 → 用户选择 → 直接写入远程 URL → Markdown` with **不进入附件缓存池,不下载,不再上传**.
Two of the three sources below permit or REQUIRE that flow. **Pixabay forbids it**, which is why Veloct
offers Unsplash and Pexels and reports Pixabay's refusal in the provider's own words.

### 4.1 Unsplash — hotlinking is REQUIRED

| Fact | Value |
|---|---|
| Host / path | `https://api.unsplash.com` + `/search/photos` (OpenAPI `servers[0].url`, `paths./search/photos`) |
| Auth | `Authorization: Client-ID <access_key>` — OpenAPI `securitySchemes.Auth`. (**Not** a bare key.) |
| Version header | `Accept-Version: v1` — requested by the documentation's "Version" section |
| Query | `query` (required), `page` (≥1, default 1), `per_page` (≥1, **default 10, max 30**), `order_by` (`latest`/`editorial`/`relevant`), `orientation` (`landscape`/`portrait`/`squarish`), `collections`, `content_filter` (`low`/`high`), `color`, `lang` (incl. `zh-Hans`/`zh-Hant`) |
| 200 body | `{ total, total_pages, results }` — **all three `required`** (OpenAPI `responses.SearchAssetFeed`), so "is there another page" is the document's answer and not a guess |
| A result | `Asset.Basic` = `Asset.VeryBasic` (`id`, `created_at`, `updated_at`, `blur_hash`, `urls`) + `promoted_at`, `links`, `width`, `height`, `description`, `alt_description`, `color`, `user`, `current_user_collections` |
| `urls` | `raw`, `full`, `regular`, `small`, `thumb` — all required. **`regular` is 1080 px wide and is the documented size for display**; `full` is explicitly discouraged for performance |
| `links` | `html`, `download`, `self`, **`download_location`** — all required |
| Error body | `{ errors: [ … ] }`, minItems 1 (OpenAPI `schemas.ErrorResponse`); 410 means the query was flagged as spam |
| Rate limit | **50 requests/hour in demo mode**, 1000/hour after production approval; state in `X-Ratelimit-Limit` / `X-Ratelimit-Remaining` |

**The hotlinking rule, in their own words** (documentation, "Hotlinking"):

> Unlike most APIs, we **require** the image URLs returned by the API to be **directly used or embedded
> in your applications** (generally referred to as *hotlinking*). By using our CDN and embedding the
> photo URLs in your application, we can better track photo views and pass those stats on to the
> photographer …

And their API Guidelines, technical rules 1–3:

1. All API uses **must** use the hotlinked image URLs under `photo.urls` — "this applies to all uses of
   the image and not just search results".
2. When the application does something similar to a download — "**like when a user chooses the image to
   include in a blog post**" — it **must** send a request to `photo.links.download_location`.
3. It **must** attribute Unsplash AND the photographer, link back to their Unsplash profile, and use
   `?utm_source=<app>&utm_medium=referral` on every link back to Unsplash.

**So §13.14 is not merely permitted for this source — it is what Unsplash asks for**, with two extra
obligations Veloct implements: the credit line cannot be omitted, and choosing an image sends the
download report.

Two implementation-critical details: the `ixid` parameter in every image URL **must be kept** (their
guidelines say so, and it is what makes the view count work), and the URLs are dynamically resizable
Imgix URLs, so `w`/`h`/`fm`/`q`/`fit`/`dpr` can be appended if a size is ever needed.

### 4.2 Pexels — hotlinking is permitted, and attribution is not optional

| Fact | Value |
|---|---|
| Host / path | `https://api.pexels.com` + `/v1/search` (documentation, "Search for Photos") |
| Auth | `Authorization: <api_key>` — **the bare key, with no prefix**. Copying Unsplash's `Client-ID ` prefix here produces a 401 that looks like an invalid key |
| Query | `query` (required), `orientation` (`landscape`/`portrait`/`square`), `size` (`large`/`medium`/`small`), `color`, `locale` (incl. **`zh-CN`**), `page`, `per_page` (**default 15, max 80**) |
| 200 body | `{ total_results, page, per_page, photos, next_page?, prev_page? }` |
| A photo | `id`, `width`, `height`, `url` (the Pexels page), `photographer`, `photographer_url`, `photographer_id`, `avg_color`, `src{…}`, `liked`, `alt` |
| `src` | `original`, `large2x`, `large` (940×650), `medium` (h 350), `small` (h 130), `portrait`, `landscape`, `tiny` |
| Rate limit | 200 requests/hour and 20,000/month; `X-Ratelimit-Limit`/`-Remaining`/`-Reset` **only on 2xx** — a 429 carries no headers, so a caller must track them itself |

**Their stated obligations** (documentation, "Guidelines"): show a "prominent link to Pexels" on every
API request's UI, and "always credit our photographers when possible (e.g. *Photo by John Doe on
Pexels* with a link to the photo page on Pexels)". Content "is available free of charge, and you are
welcome to use Pexels content for anything you'd like, as long as it is within our [Guidelines]". Also
prohibited: replicating Pexels' core functionality (a wallpaper app), and working around the rate limit.

**No download-reporting step is documented**, so Veloct makes no request beyond the search.

**Availability, verified again on 2026-10-02 — and NOT implied by the contract above: Pexels has paused
new API key issuance.** The page a new user actually requests a key on, `https://www.pexels.com/api/`,
says so **twice**: "New API key issuance is currently paused." **The Authorization section of THIS
documentation page still says the opposite** — "Anyone with a Pexels account can request an API key,
which you will receive instantly" — so a reader who checks the documentation rather than the request
page reaches the wrong conclusion. The endpoints remain up (an unauthenticated
`GET https://api.pexels.com/v1/search` returns **401**) and existing keys still work, which is why
Veloct's implementation is kept rather than deleted. `PROCESSING.md` §5.12 records the decision to leave
the source as it is, matrix row **D165** records the fact, and the consequence a user meets is that
`ImageSource::Pexels` is still selectable while **nothing in the UI says a new user cannot obtain a key**.

### 4.3 Pixabay — the source §13.1 names, and the reason it is NOT implemented

Read from Pixabay's own API documentation. Its contract is perfectly clear (`GET https://pixabay.com/api/`,
`key` required, `q`/`lang`/`image_type`/`orientation`/`category`/`min_width`/`min_height`/`colors`/
`editors_choice`/`safesearch`/`order`/`page`/`per_page` (3–200, default 20), returning
`{ total, totalHits, hits: […] }`; 100 requests/60 s; a 24-hour cache requirement) — and so is the clause
that rules it out:

> Returned image URLs may be used for temporarily displaying search results. However, permanent
> hotlinking of images (using Pixabay URLs in your app) is not allowed. If you intend to use the images,
> please download them to your server first.

§13.14 prescribes exactly the prohibited use. Two further facts would bite even if the terms allowed it:
a `webformatURL` is **valid for 24 hours**, so a saved Markdown file would break within a day; and the
24-hour cache requirement means a per-keystroke search box would violate the API's own usage rule.

**Consequence:** Veloct offers Unsplash and Pexels and refuses Pixabay with this reason, in the
provider's own words, from `veloct_attach::network::providers::unimplemented_reason`. The source is
listed in the window rather than silently missing, because a source the contract names that is absent is
something a user should be told about. **A future implementer must not "just wire it up"**: that would
publish pages violating the provider's terms, which §20's honesty rules forbid. PROCESSING.md §5.11
records the decision and matrix rows D155/D156 carry the evidence.

### 4.4 The generalisable rule from this section

**Read the vendor before writing the code that talks to it** — and be prepared for the answer to be
"the locked clause is what is wrong", which is far cheaper to discover before the code exists than
after it has tests. The same habit had already produced §5.3's and §5.4's gaps; this round it produced
a design change instead.

## 5. Lsky Pro (§13.12) — read from the PRODUCT, because the vendor publishes no API reference

Read on **2026-10-02**. The first thing verification established is WHERE this contract actually lives, and it
is not a documentation site.

**There is no public API reference.** The documentation site (`docs.lsky.pro`, migrated to
`docs.huohuastudio.com/lsky-pro`) documents installation, storage drivers, upgrades and the paid tiers, and
publishes no endpoint list. The Halo app-store page's note — that the definitions are visible in the Lsky
backend — is literally true: **the contract ships inside the product**, as the page `Common\ApiController`
renders at `{instance}/api`.

**So the authority is the vendor's own artifacts**, read at `lsky-org/lsky-pro` on `master` — the FREE line,
whose releases run to **2.1** (2022-08-17) and whose Laravel 9 / PHP 8 code is what `master` carries:

| Fact | Value, from the vendor's own shipped page and code |
|---|---|
| Base | `{instance}/api/v1` — `resources/views/common/api.blade.php`: 接口URL `{scheme}://{host}/api/v1` |
| Auth | `Authorization: Bearer <token>`, a Sanctum plain-text token (the page's own example is `1\|1bJb…`). **A request with no such header is treated as 游客上传 (guest upload)**, which the page states outright |
| Upload | `POST /api/v1/upload`, `multipart/form-data`, part `file` (required, File), optional `strategy_id` (Integer) — `routes/api.php`; `ImageService::store` reads `$request->file('file')` and `$request->input('strategy_id')` |
| Envelope | `{status: Boolean, message: String, data: Object}` — `app/Http/Result.php`. **HTTP is 200 even when `status` is false**, and `data` is `{}` when empty |
| Upload data | `key`, `name`, `pathname`, `origin_name`, `size` (**kilobytes**, a float), `mimetype`, `extension`, `md5`, `sha1`, `links{url, html, bbcode, markdown, markdown_with_link, thumbnail_url}` — `Api/V1/ImageController::upload`, `Image::links`/`url`/`pathname` |
| Disabled API | `CheckIsEnableApi` answers JSON **403** with `{"status":false,"message":"管理员未启用 API"}`, and a 404 for a request that does not ask for JSON |
| Token | `POST /api/v1/tokens` with `email`+`password` → `data.token`, throttled `3,1` (three per minute); `DELETE /api/v1/tokens` clears them — `Api/V1/TokenController` |
| Connection | `GET /api/v1/profile`, behind `auth:sanctum`, answers with the account — which is what makes it a credential test rather than a ping |
| Delete | `DELETE /api/v1/images/{key}` behind `auth:sanctum` → `删除成功`. `Image` declares **no `SoftDeletes`**, so the row goes; the physical file is removed only when no OTHER row on the same strategy shares its `md5`+`sha1`, and **a failure to remove it is caught and merely logged** |
| Image list | `GET /api/v1/images`, query `page`, `order`, `permission`, `album_id`, `keyword` — and `keyword` filters `origin_name`/`alias_name`, **never `key`** (`Image::scopeFilter`), so there is NO lookup by key |

**Three obligations this puts on any caller, each of which Veloct's adapter encodes:**

1. **`Accept: application/json` on every request.** `CheckIsEnableApi` chooses between the JSON 403 and an HTML
   404 on `expectsJson()`, and Laravel answers an unauthenticated `auth:sanctum` request with a REDIRECT rather
   than a 401 unless the client asks for JSON — so a rejected token would look like a login page.
2. **`is_success()` is not the success signal.** A refusal arrives as HTTP 200 carrying `status:false`, so a
   client that trusts the status reports every failure as a success.
3. **Never send an upload without the token.** The instance ACCEPTS an anonymous upload when its admin allows
   guest uploads, so a missing credential must be refused locally instead of falling back to one.

**What is verified, and what is NOT.** The table is the **free line** (2.0–2.1 / `master`). The PAID line
("Lsky Pro+", 2.x, and enterprise 1.x) is the same architecture *by the vendor's own documentation*, but **from
version 2.3.0 the vendor encrypts the backend source** and no public API reference exists, so Veloct does not
claim it: `crates/veloct-attach/src/lsky.rs` states that limit, and `StorageKind::is_verified` covers the free
line only.

**The generalisable rule, and it is the mirror of §4.4's:** a vendor publishing no reference is not the same as
a protocol that cannot be verified — sometimes the reference is INSIDE the product, in the page the vendor tells
its own users to read. Look for that artifact before recording a gap as permanent.


## 6. Alibaba Cloud OSS (§13.11 / §17.4) — measured against a real bucket, 2026-10-02

Not a vendor whose documentation was read: these four facts came from REQUESTS to a live bucket
(`vellow-chengdu-archieve`, region `cn-chengdu`), which is why they are worth more than a summary of a manual.
They are recorded here because an S3-compatible provider is exactly the kind of thing this file exists for, and
because two of them changed code.

**Endpoints.** `oss-<region>.aliyuncs.com` is the public one. `oss-<region>-internal.aliyuncs.com` is reachable
only from inside Alibaba's own network — a workstation gets no connection at all, so a target configured with
the internal endpoint fails for a reason that looks like a network fault rather than a configuration mistake.
The region for SigV4 is the bare form (`cn-chengdu`), not the endpoint's `oss-cn-chengdu`.

**Addressing: the bucket MUST be in the hostname.** A path-style bucket request is refused in the vendor's own
words:

```
GET https://oss-cn-chengdu.aliyuncs.com/vellow-chengdu-archieve
403 SecondLevelDomainForbidden
"The bucket you are attempting to access must be addressed using OSS third level domain."
```

The same LIST as `https://vellow-chengdu-archieve.oss-cn-chengdu.aliyuncs.com/` answers **200** with a
`ListBucketResult`. This is the opposite of what ADR-0016 assumed about a user-supplied endpoint ("a self-hosted
service almost always wants path style"), which is why an endpoint may now say `{bucket}` — see ADR-0018.

**Signature: SigV4 with the service scope `s3`, NOT `oss`.** Measured with identical URLs and credentials:

| scope | result |
| --- | --- |
| `aws:amz:cn-chengdu:oss` | **400 InvalidArgument** |
| `aws:amz:cn-chengdu:s3` | **200**, a real listing |

So a hand-rolled AWS SigV4 client works against OSS unchanged — `veroct-s3` signs `s3` and needed no signing
change. (This is also why the second-order signing details matter: OSS refused the FIRST live request with
`403 AccessDenied: OSS authentication requires a valid Date`, because `x-amz-date` carried sub-second digits.
The vendor message named the field, not the bug — D174 and ADR-0018 have that story.)

**Permissions.** A repository-style `permissions.admin` analogue is not needed here, but the bucket's own
policy is what protects objects: OSS serves an object over `https://<bucket>.oss-<region>.aliyuncs.com/<key>`
only when the bucket or object grants public read. `veroct-attach`'s public URL is BUILT from the configured
prefix (§13.6) and never fetched, so a target can report a URL that a private bucket would refuse — which is
the bucket's policy to set, not the client's to assume.

**The generalisable rule, and it is §4.4's and §5's in a third form:** probe the vendor before writing to it,
and read the ERROR rather than the manual. This one sentence — "must be addressed using OSS third level
domain" — saved guessing at addressing, and the `valid Date` refusal is what found a defect that 1400+ tests
could not see.

## 7. Unsplash's keyless routes are DEAD, and the plugin cited as proof is not keyless either

Read this before replacing a working, terms-compliant integration because a remembered workaround sounds
cheaper. Measured on 2026-10-02, because the belief keeps circulating. Four requests, four answers:

| What people remember as keyless | Measured |
| --- | --- |
| `https://unsplash.com/napi/search/photos?query=…` (the site's own internal API) | **307** to `https://unsplash.com/.within.website?redir=…` — an **Anubis proof-of-work bot gate** ("Making sure you're not a bot!") — and the challenge page is what a plain HTTP client gets. Not usable without running JavaScript. |
| `https://source.unsplash.com/random` (the endpoint that WAS keyless) | **503 Application Error**, with `-L` and a browser User-Agent. Retired. |
| `https://api.unsplash.com/search/photos?…` with no credential | **401** `{"errors":["OAuth error: The access token is invalid"]}` — the documented API wanting a key, exactly as §4.1 says. |
| `halo-sigs/plugin-unsplash` (cited as proof that a key is unnecessary) | **It requires one.** `settings.yaml` declares an `accessKey` field labelled "Access Key" whose help points at `unsplash.com/developers`; the console does `const unsplash = createApi({ accessKey: accessKey.value })` — the official `unsplash-js` SDK — and every query is `enabled: computed(() => !!accessKey.value)`. Nothing in it fetches without a key. |

**So there is nothing here to adopt, and no availability problem to solve.** Unsplash's key is free and issued
immediately, which is the opposite of Pexels's pause (§4.2) — the reason this project needs a key is not a gate,
and the integration that uses one also honours the two obligations §4.1 verified (hotlinking the returned URL
and reporting the download). A keyless route, if one existed, would break the second of those, because the
download report is an API call.

**The generalisable rule, and it is the third form of §4.4's and §5's:** a remembered workaround is a fact to
MEASURE, not to inherit. The endpoint that made the belief true was retired; the page that replaced it is a bot
check; and the project cited as proof asks for a key in its own settings file.

### 7.1 The plugin as a REFERENCE: what it implements, and the one thing it has that Veloct does not

Read on 2026-10-02 from its own source (`halo-sigs/plugin-unsplash`, `main`), because "look at how they do it" is
the right instinct even when the premise that brought you there turns out to be wrong. **It is a competent
implementation of the same documented API**, and comparing it with §13.14's is useful in both directions.

| | Halo's plugin | Veloct (§13.14) |
| --- | --- | --- |
| API | the official `unsplash-js` SDK, `createApi({ accessKey })` | hand-rolled HTTP against the same documented endpoints — no vendor SDK, the style ADR-0016 set for S3 |
| Where the key lives | a `$formkit: password` field in the plugin's settings, read back by the browser console and used there | the OS credential store; it never leaves the process and the window never receives it (§12.2, §18.1) |
| Browsing | **topics** (`topics.list`, 100 of them) AND keyword search (48 per page, paginated), so a user can browse a topic or search | keyword search. **Topic browsing does not exist** |
| Rendition | the user picks `raw` / `full` / `regular` / `small` by name in the settings | one rendition, the 1080 px `regular`, fixed and live-verified (D164) |
| Where the image ends up | hotlink by default; with 转存 ("transfer") enabled it fetches the chosen URL, uploads it into Halo's OWN attachment storage under a policy and group, binds photo→attachment, and links the local permalink | hotlinks the returned URL — which §4.1's quote from Unsplash's guidelines **requires** ("we **require** the image URLs returned by the API to be directly used or embedded") |
| Download reporting | **not sent anywhere in the component** — no `links.download_location`, no `/download` call | **sent**, and D164's live run includes the accepted report. It is an obligation, not a courtesy |

**So Veloct is the stricter of the two on the two things that are obligations** (the key never reaches a UI; the
download report is sent), and **the plugin is ahead on exactly one feature: browsing by topic.**

**Three candidates, and the first was ADOPTED the same day on the owner's instruction (D181):**

1. **Topic browsing — ADOPTED on 2026-10-02.** It is a genuine browse mode (Unsplash curates the topics), the
   provider calls are two endpoints, and its absence was the only gap the comparison found. **It is an
   ENHANCEMENT BEYOND §13.14's letter** — that clause's flow is 搜索 → 用户选择 → 直接写入远程 URL → Markdown —
   and it is recorded as the owner's decision rather than as contract compliance. **What was followed, and what
   was not:** the topic list, the "a keyword wins over a selected topic" precedence, and the single browse
   action come from the plugin; its `per_page` values (100 for topics, 48 for photos) are **above** Unsplash's
   documented ceiling of 30 and are NOT copied, `has_more` comes from the provider's own
   `Link: …; rel="next"` header instead of being inferred from array length, the automatic selection of the
   first topic is dropped because it sends a request the user did not ask for, there is no rendition choice,
   and **its 转存 mode is absent entirely** — see (3).
2. **A rendition choice is NOT worth copying.** Veloct's 1080 px `regular` is the size §13.14's flow needs and
   D164 verified live; letting a user pick `raw` invites multi-megabyte pages for no benefit the clause asks for.
3. **The 转存 (re-host) mode is the one to be careful about, and NOT to copy as it stands.** Two reasons, and the
   first is the plugin's own: it re-hosts an image the API terms expect to be hotlinked, and it does so WITHOUT
   the download report that a download is supposed to trigger. Veloct already has a place a user can put a file
   they want to own — §13.5's attachment pool — and it documents the consequence of re-hosting (a remote cleanup
   leaves the article pointing at a dead URL, §5.10.3).

**The generalisable rule:** a reference implementation is evidence about the API, not a specification for your
product. Read what it calls, then check each of those calls against the obligations YOU verified — the plugin is
right about the endpoints and silent about one of the two obligations §4.1 records.

## 8. S3/R2 policy reference and conditional creates (2026-10-17)

- User reference: [Halo S3 configuration guide](<https://github.com/halo-dev/plugin-s3#上传目录>), read from the same project's [README mirror](<https://cdn.jsdelivr.net/gh/halo-dev/plugin-s3@main/README.md>) because local GitHub resolution was unavailable. Confirms endpoint without protocol/bucket, explicit path/virtual styles, lowercase R2 `auto`, slash-trimmed directory, UUID/time/date/random placeholders and filename modes. Veloct uses persisted attachment UTC time/stable random seeds to preserve retries; it is not a wholesale port of Halo settings/plugins.
- [Cloudflare's S3 compatibility table](<https://developers.cloudflare.com/r2/api/s3/api/>) confirms account endpoint, region `auto` (empty/us-east-1 aliases), and atomic `PutObject` conditional headers including `If-None-Match`; with `*`, at most one concurrent create succeeds and others receive 412. This is the basis of non-overwriting collision handling, not a HEAD-then-unconditional-PUT assumption.
- [AWS PutObject](<https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObject.html>) documents `If-None-Match: *`, 412 existing-object and 409 concurrent-conflict responses. Veloct signs that header, renames under a bounded loop or reports conflict; existing legacy reuse and backup writes are unchanged.
- A public custom domain is separate from the authenticated bucket endpoint; processing query/thumbnail parameters are service-specific. R2 storage alone is not an image-processing service. Veloct supports none/custom thumbnail parameters and ordered extension suffixes, without claiming a specific CDN transformation works.
- Evidence boundary: signed request/409/412/URL/path fixtures, UI tests and production-bundle Edge checks; no current real bucket read/write or CDN-account verification. The explicit form verification is one signed read-only ListObjectsV2 request, not an upload/delete probe.