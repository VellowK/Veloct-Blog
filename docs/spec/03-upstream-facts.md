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
