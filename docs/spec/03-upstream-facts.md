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
6. EdgeOne's upload API. Veloct implements EdgeOne as a manual/export target (ADR-0012) rather
   than inventing endpoints.
7. ~~Unsplash's and Pexels's image-search protocols.~~ **CLOSED by section 4 below: both were read
   live on 2026-10-01, and Pixabay's was read too — which is why Pixabay is NOT one of the two sources
   implemented.** Reading the vendors first changed the design; the entry is kept so that the order of
   events is visible.

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
