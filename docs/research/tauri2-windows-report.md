# Veloct — Tauri 2 Windows Research Report

**Research date:** 2026-09-28 (host clock, `Asia/Shanghai`)
**Host verified:** `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `host: x86_64-pc-windows-msvc`, `LLVM 22.1.6`
**Toolchains installed:** `stable-x86_64-pc-windows-gnu`, `stable-x86_64-pc-windows-msvc` (active/default)
**Method:** every version below was read live from `crates.io` / `registry.npmjs.org` on 2026-09-28, cross-checked against `docs.rs` and the actual crate tarballs where the question was about build behaviour. Anything I could not verify is marked **UNVERIFIED**.

> Note on `rustc` in this shell: `rustc`/`cargo`/`rustup` are **not on `PATH`** here; they live at `$env:USERPROFILE\.cargo\bin\`. Tauri builds invoked from a bare shell will fail with "not recognized". Use the absolute path or add `%USERPROFILE%\.cargo\bin` to `PATH`.

---

## 1. Tauri 2 current state

### 1.1 Core crates (crates.io, `max_stable_version`)

| Crate | Latest stable | Newest (incl. prerelease) | MSRV (`rust_version`) | Released |
|---|---|---|---|---|
| `tauri` | **2.12.0** | `3.0.0-alpha.3` | 1.90 | 2026-09-26 |
| `tauri-build` | **2.7.0** | `3.0.0-alpha.2` | 1.90 | 2026-09-26 |

**Tauri 3 has entered alpha.** `tauri 3.0.0-alpha.0` shipped 2026-09-13, and `3.0.0-alpha.3` on 2026-09-26. `max_version`/`newest_version` now report `3.0.0-alpha.3`, so **never read `newest_version` if you want stable** — read `max_stable_version`. There is only one `tauri` npm dist-tag for 3.x (`next`); `latest` still points at 2.12.0.

### 1.2 Plugin crates — Rust side (crates.io) vs JS side (npm)

| Plugin | Rust crate | npm package | Match? |
|---|---|---|---|
| shell | `tauri-plugin-shell` **2.4.0** | `@tauri-apps/plugin-shell` **2.4.0** | ✅ |
| http | `tauri-plugin-http` **2.8.0** | `@tauri-apps/plugin-http` **2.7.0** | ⚠️ **out of sync by one minor** |
| fs | `tauri-plugin-fs` **2.6.0** | `@tauri-apps/plugin-fs` **2.6.0** | ✅ |
| dialog | `tauri-plugin-dialog` **2.8.0** | `@tauri-apps/plugin-dialog` **2.8.0** | ✅ |
| opener | `tauri-plugin-opener` **2.6.0** | `@tauri-apps/plugin-opener` **2.6.0** | ✅ |
| process | `tauri-plugin-process` **2.4.0** | `@tauri-apps/plugin-process` **2.4.0** | ✅ |
| store | `tauri-plugin-store` **2.5.0** | `@tauri-apps/plugin-store` **2.5.0** | ✅ |
| sql | `tauri-plugin-sql` **2.5.0** | `@tauri-apps/plugin-sql` **2.5.0** | ✅ |
| updater | `tauri-plugin-updater` **2.13.0** | `@tauri-apps/plugin-updater` **2.13.0** | ✅ |
| stronghold | `tauri-plugin-stronghold` **2.4.0** | `@tauri-apps/plugin-stronghold` **2.4.0** | ✅ |
| upload | `tauri-plugin-upload` **2.5.0** | `@tauri-apps/plugin-upload` **2.5.0** | ✅ |
| websocket | `tauri-plugin-websocket` **2.5.0** | `@tauri-apps/plugin-websocket` **2.5.0** | ✅ |
| log | `tauri-plugin-log` **2.10.0** | `@tauri-apps/plugin-log` **2.10.0** | ✅ |
| os | `tauri-plugin-os` **2.4.0** | `@tauri-apps/plugin-os` **2.4.0** | ✅ |
| cli | `tauri-plugin-cli` **2.5.0** | `@tauri-apps/plugin-cli` **2.5.0** | ✅ |
| notification | `tauri-plugin-notification` **2.5.0** | `@tauri-apps/plugin-notification` **2.5.0** | ✅ |
| global-shortcut | `tauri-plugin-global-shortcut` **2.4.0** | `@tauri-apps/plugin-global-shortcut` **2.4.0** | ✅ |
| autostart | `tauri-plugin-autostart` **2.6.0** | `@tauri-apps/plugin-autostart` **2.6.0** | ✅ |
| single-instance | `tauri-plugin-single-instance` **2.5.0** | not published (Rust-only) | n/a |
| window-state | `tauri-plugin-window-state` **2.5.0** | `@tauri-apps/plugin-window-state` **2.5.0** | ✅ |
| clipboard-manager | `tauri-plugin-clipboard-manager` **2.4.0** | `@tauri-apps/plugin-clipboard-manager` **2.4.0** | ✅ |
| deep-link | `tauri-plugin-deep-link` **2.5.0** | `@tauri-apps/plugin-deep-link` **2.5.0** | ✅ |
| localhost | `tauri-plugin-localhost` **2.4.0** | not published (Rust-only) | n/a |
| positioner | — | `@tauri-apps/plugin-positioner` **2.4.0** | (Rust crate `tauri-plugin-positioner 2.4.0`) |

Every `tauri-plugin-*` 2.x above declares **MSRV 1.90** — comfortably satisfied by rustc 1.97.1.

### 1.3 Frontend / CLI (npm `latest`)

| Package | Latest |
|---|---|
| `@tauri-apps/api` | **2.12.0** (published 2026-09-26T16:05:49Z) |
| `@tauri-apps/cli` | **2.12.0** (published 2026-09-26T17:09:29Z) |
| `vite` | **8.3.1** |
| `typescript` | **7.0.2** |
| `astro` | **7.3.5** |

`@tauri-apps/api` and `@tauri-apps/cli` track the `tauri` crate version 1:1, so all three should be pinned to `2.12.0` together.

### 1.4 How to verify the exact latest version

**crates.io JSON API** (no auth required — I confirmed it responds with no custom `User-Agent`):

```text
# Full crate metadata. Read .crate.max_stable_version (NOT .crate.newest_version, which may be a prerelease)
https://crates.io/api/v1/crates/{crate}

# Reverse-dependency count plus all versions (paginated)
https://crates.io/api/v1/crates/{crate}/versions?limit=10

# Pinned version metadata, incl. .version.rust_version (the MSRV) and .version.features
https://crates.io/api/v1/crates/{crate}/{version}

# Resolved dependency list for one version
https://crates.io/api/v1/crates/{crate}/{version}/dependencies

# Owners (for supply-chain sanity: Tauri crates show "tauri-bot")
https://crates.io/api/v1/crates/{crate}/owners
```

Concrete check (works offline-free, no key):

```powershell
$r = Invoke-RestMethod 'https://crates.io/api/v1/crates/tauri'
"{0} stable / {1} newest" -f $r.crate.max_stable_version, $r.crate.newest_version
# -> 2.12.0 stable / 3.0.0-alpha.3 newest
```

**registry.npmjs.org URL patterns** (also no auth):

```text
# Full packument. Read .["dist-tags"].latest
https://registry.npmjs.org/{package}
# Scoped packages: the "/" must be percent-encoded
https://registry.npmjs.org/@tauri-apps%2Fcli

# Abbreviated single-version document (smallest response)
https://registry.npmjs.org/{package}/latest

# dist-tags only
https://registry.npmjs.org/-/package/{package}/dist-tags
```

```powershell
# Latest stable
(Invoke-RestMethod 'https://registry.npmjs.org/-/package/@tauri-apps/cli/dist-tags') | ConvertTo-Json -Compress
# -> {"next":"3.0.0-alpha.3","latest":"2.12.0"}
```

**Rule for CI / scripted checks:** assert on `max_stable_version` (crates.io) and `dist-tags.latest` (npm). Both exclude prereleases. Comparing against `newest_version`/`dist-tags.next` is what will silently pull you onto the Tauri 3 alpha line.

**Sources:** [crates.io API](https://crates.io/api/v1/crates/tauri) · [registry.npmjs.org](https://registry.npmjs.org/@tauri-apps%2Fcli) · [Tauri config reference](https://v2.tauri.app/reference/config/)

---

## 2. Tauri 2 sidecar (`bundle.externalBin`)

### 2.1 `tauri.conf.json` schema

From the authoritative generated schema (`config.schema.json`, `definitions.BundleConfig.properties.externalBin`):

```json
{
  "description": "A list of—either absolute or relative—paths to binaries to embed with your application.\n\nNote that Tauri will look for system-specific binaries following the pattern \"binary-name{-target-triple}{.system-extension}\".\n\nE.g. for the external binary \"my-binary\", Tauri looks for:\n\n- \"my-binary-x86_64-pc-windows-msvc.exe\" for Windows\n- \"my-binary-x86_64-apple-darwin\" for macOS\n- \"my-binary-x86_64-unknown-linux-gnu\" for Linux\n\nso don't forget to provide binaries for all targeted platforms.",
  "type": ["array", "null"],
  "items": { "type": "string" }
}
```

So: `bundle.externalBin` is `string[] | null`. Each entry is a path **without** the target-triple suffix and **without** `.exe`.

```jsonc
// src-tauri/tauri.conf.json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Veloct",
  "version": "0.1.0",
  "identifier": "com.veloct.app",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:5173",
    "beforeDevCommand": "pnpm dev",
    "beforeBuildCommand": "pnpm build"
  },
  "app": {
    "windows": [{ "title": "Veloct", "label": "main", "width": 1280, "height": 800 }],
    "security": {
      "csp": null,
      "capabilities": ["default"]
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/icon.ico"],
    "externalBin": ["binaries/veloct-astro-sidecar"],
    "windows": {
      // Only needed if a sidecar links the VC++ runtime dynamically.
      // Tauri's own binaries statically link it by default (build.windows.staticVCRuntime = true).
      "bundleVCRuntime": true
    }
  }
}
```

Relative paths are resolved against `src-tauri/` (the directory containing `tauri.conf.json`), so `binaries/veloct-astro-sidecar` means `<PROJECT ROOT>/src-tauri/binaries/veloct-astro-sidecar`.

### 2.2 Required target-triple file naming on Windows/MSVC

For `"externalBin": ["binaries/veloct-astro-sidecar"]` you must have exactly:

```text
src-tauri/binaries/veloct-astro-sidecar-x86_64-pc-windows-msvc.exe
```

The file name is `{bare-name}-{target-triple}.exe`, and the triple for this host is confirmed by:

```powershell
rustc --print host-tuple     # -> x86_64-pc-windows-msvc
# Rust < 1.84 has no --print host-tuple:
rustc -Vv | Select-String "host: " | ForEach-Object { $_.Line.Split(" ")[1] }
```

I verified on this host that `rustc 1.97.1` prints `host: x86_64-pc-windows-msvc`, and that the **only** installed rustup target is `x86_64-pc-windows-msvc` (the GNU toolchain is installed but has no target added). A GNU build would require `veloct-astro-sidecar-x86_64-pc-windows-gnu.exe` instead — plan for MSVC.

Rename script (adapts the Tauri-documented one):

```powershell
$triple = (rustc --print host-tuple).Trim()
Move-Item `
  "$PSScriptRoot/../src-tauri/binaries/veloct-astro-sidecar.exe" `
  "$PSScriptRoot/../src-tauri/binaries/veloct-astro-sidecar-$triple.exe" -Force
```

### 2.3 Runtime resolution — this is the part people get wrong

I read the actual `tauri-plugin-shell 2.4.0` source (`src/process/mod.rs`, `src/lib.rs`, `src/scope.rs`, `src/commands.rs`). Findings:

1. `Shell::sidecar()` → `Command::new_sidecar()` → `relative_command_path()` (`process/mod.rs:120-153`), which resolves the program **relative to the directory containing the running executable** and **appends `.exe` on Windows if not already present**. So at runtime the sidecar must sit next to `veloct.exe`.

2. `Shell::command()` and `Shell::sidecar()` are **plain Rust constructors — they are not gated by the ACL.** The capability/permission check lives in the **JS** command path only (`commands.rs:104-148`, via `CommandScope`/`GlobalScope`). **Consequence: a sidecar spawned entirely from Rust needs no `shell:allow-*` permission.** Capabilities are only required if the webview reaches the sidecar through `@tauri-apps/plugin-shell`.

3. The JS path is stricter: `commands.rs:115-138` looks the program up by exact string equality against `config.bundle.external_bin` and returns `SidecarNotAllowed` if it does not match. So JS must pass the **full configured string** (`"binaries/veloct-astro-sidecar"`), while Rust must pass the **bare file name** (`"veloct-astro-sidecar"`). These are different conventions on purpose.

4. `CommandChild` (`process/mod.rs:63-87`) is a thin wrapper over `Arc<SharedChild>` and exposes **only** `write`, `kill`, and `pid`.

### 2.4 `tauri_plugin_shell` vs `tauri::process`

**There is no `tauri::process` spawning API in Tauri 2.** Tauri 1's `tauri::api::process::Command` was removed; all child-process work moved to the shell plugin. Tauri 2 does ship `tauri-plugin-process` 2.4.0, but it is *not* a process-spawning API — its whole surface is exit/restart:

```text
process:allow-exit, process:allow-restart   (that is the entire permission table)
```

**Use `tauri-plugin-shell` 2.4.0.** That is the only supported sidecar path.

### 2.5 Child pid and killing the process tree

- `child.pid() -> u32` returns the immediate child pid.
- `child.kill()` consumes `self` and calls `SharedChild::kill()` → on Windows `TerminateProcess` on **that one process only**. It does **not** kill the tree.
- For a Node/Astro sidecar this matters: `astro dev` spawns children (esbuild, Vite workers, etc.) that will be orphaned.

**Windows-correct teardown: capture the pid, then `taskkill /PID <pid> /T /F`.** I verified `taskkill.exe` resolves to `C:\WINDOWS\system32\taskkill.exe` on this host.

### 2.6 Minimal, correct example

This is a complete, compilable-shaped example: OS-assigned free port, spawn with args + env, stream stdout lines, kill the tree on exit.

```rust
// src-tauri/src/sidecar.rs
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

/// Marker is only needed so we can `remove_file` the "used" flag; the pid is what matters.
#[derive(Default)]
pub struct SidecarState(pub Mutex<Option<u32>>);

/// Ask the OS for a free loopback port.
///
/// We bind port 0, read back the assigned port, then drop the listener. This is a
/// TOCTOU-shaped race (another process could take the port in between), but it is the
/// standard technique and is what `portpicker` does internally. Ports handed out by the
/// ephemeral range are also unlikely to be re-taken immediately.
pub fn pick_free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Kill the whole tree. `CommandChild::kill()` only terminates the immediate child,
/// so on Windows we shell out to taskkill with /T (tree) and /F (force).
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(not(windows))]
    {
        // Process-group kill: spawn the sidecar with setsid/process_group(0) and signal -pid.
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .status();
    }
}

/// Returns the pid so the caller can record it.
pub async fn start_astro_sidecar(app: &AppHandle, project_root: &str) -> Result<u32, String> {
    let port = pick_free_port().map_err(|e| format!("no free port: {e}"))?;

    let cmd = app
        .shell()
        .sidecar("veloct-astro-sidecar") // BARE file name; NOT "binaries/veloct-astro-sidecar"
        .map_err(|e| format!("sidecar not found: {e}"))?
        .args([
            "dev",
            "--host", "127.0.0.1",
            "--port", &port.to_string(),
            "--root", project_root,
        ])
        .env("NODE_ENV", "development")
        .env("VELOCT_PORT", port.to_string())
        // Astro/Vite honour this; keeps the child from trying to open a browser.
        .env("BROWSER", "none")
        .current_dir(project_root);

    let (mut rx, child) = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
    let pid = child.pid();

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // NOTE: stdout/stderr arrive as Vec<u8> already split on \n or \r.
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(bytes) => {
                    let line = String::from_utf8_lossy(&bytes);
                    let line = line.trim_end();
                    // Astro prints the dev URL once ready — surface it to the UI.
                    if line.contains("localhost:") || line.contains("127.0.0.1:") {
                        let _ = app_handle.emit("astro-ready", line.to_string());
                    }
                    let _ = app_handle.emit("astro-stdout", line.to_string());
                    log::debug!("[astro] {line}");
                }
                CommandEvent::Stderr(bytes) => {
                    let line = String::from_utf8_lossy(&bytes);
                    let _ = app_handle.emit("astro-stderr", line.trim_end().to_string());
                    log::warn!("[astro] {}", line.trim_end());
                }
                CommandEvent::Error(err) => {
                    let _ = app_handle.emit("astro-error", err.clone());
                    log::error!("[astro] io error: {err}");
                }
                CommandEvent::Terminated(payload) => {
                    let _ = app_handle.emit("astro-exited", payload.code);
                    log::info!("[astro] terminated code={:?}", payload.code);
                    break;
                }
                _ => {}
            }
        }
    });

    // Stash the pid so the exit hook can tear the tree down. We deliberately let
    // `CommandChild` drop: kill() takes `self` by value, and we want the taskkill
    // tree-kill on exit anyway. If you prefer graceful shutdown, store the
    // CommandChild itself in the state and .take() it in the exit hook.
    app.state::<SidecarState>().0.lock().unwrap().replace(pid);
    drop(child);

    Ok(pid)
}
```

Wiring it into `lib.rs` — this is the code that actually owns the app lifecycle:

```rust
// src-tauri/src/lib.rs
mod sidecar;

use sidecar::{kill_tree, SidecarState};

#[tauri::command]
async fn start_astro(app: tauri::AppHandle, project_root: String) -> Result<u32, String> {
    sidecar::start_astro_sidecar(&app, &project_root).await
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(SidecarState::default())
        .invoke_handler(tauri::generate_handler![start_astro])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // RunEvent::Exit is the correct teardown hook. Do NOT use on_window_event:
    // it fires per window and would kill the sidecar while other windows are live.
    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let state = app_handle.state::<SidecarState>();
            let pid = state.0.lock().unwrap().take();
            if let Some(pid) = pid {
                kill_tree(pid);
            }
        }
    });
}
```

`kill_tree` and `pick_free_port` must be `pub` for this to compile from `lib.rs`. `State<T>` derefs to `T`, so `state.0` reaches the tuple field. `RunEvent` is `#[non_exhaustive]`, so the `if let` (rather than an exhaustive `match`) is the right shape — you may add more arms later without a wildcard panic.


Two corrections to be aware of when you wire this in:

- If you keep `child` alive in a `Mutex<Option<CommandChild>>` you get graceful `child.kill()` for free, but `CommandChild: !Sync`-adjacent concerns and the `kill(self)` signature mean you must `.take()` it out of the mutex. `taskkill /T /F` from the stored pid is the simpler, more reliable Windows path — Astro's teardown is not important enough to warrant graceful signalling.
- `RunEvent::Exit` is the right hook (verified variant list in `tauri 2.12.0`: `Exit`, `ExitRequested { code, api }`, `WindowEvent`, `WebviewEvent`, `Ready`, `Resumed`, `MainEventsCleared`, `MenuEvent`, `TrayIconEvent`). Do **not** use `on_window_event` for cleanup — it fires per window and will run while other windows are still alive.

`CommandEvent` is `#[non_exhaustive]` with variants `Stderr(Vec<u8>)`, `Stdout(Vec<u8>)`, `Error(String)`, `Terminated(TerminatedPayload)` — hence the `_ => {}` arm is mandatory, not optional.

**Sources:** [Embedding External Binaries](https://v2.tauri.app/develop/sidecar/) · [Tauri config schema `externalBin`](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-schema-generator/schemas/config.schema.json) · [tauri-plugin-shell 2.4.0 `Shell`](https://docs.rs/tauri-plugin-shell/2.4.0/tauri_plugin_shell/struct.Shell.html) · [tauri 2.12.0 `RunEvent`](https://docs.rs/tauri/2.12.0/tauri/enum.RunEvent.html) · [Node.js as a sidecar](https://v2.tauri.app/learn/sidecar-nodejs/)

---

## 3. Tauri 2 capabilities / permissions

### 3.1 How it works

- Capability files live in `src-tauri/capabilities/*.json` (or `.toml`). **All files in that directory are auto-enabled.** If you list any capability in `tauri.conf.json` under `app.security.capabilities`, *only* those listed are used.
- A capability = `identifier` + `windows`/`webviews` (or `"*"`) + optional `platforms` + optional `remote.urls` + a `permissions: []` array.
- Permissions are strings (`"core:default"`, `"shell:allow-spawn"`) or **scoped objects** (`{ "identifier": "shell:allow-execute", "allow": [ ... ] }`).
- Capabilities **merge** across everything matching a window. Overlapping scopes widen, they do not narrow.
- `$schema` should point at the generated schema: `"../gen/schemas/desktop-schema.json"` (generated by `tauri-build`).

### 3.2 What Veloct needs

| Concern | Permission | Notes |
|---|---|---|
| shell — spawn a sidecar from **Rust** | **none** | Not ACL-gated. `Shell` commands are Rust-side constructors. |
| shell — spawn a sidecar from **JS** | `shell:allow-spawn` (+ scope entry with `sidecar: true`), or `shell:allow-execute` if using `.execute()` | Scoped entry **must** use the exact `externalBin` string. `cmd` is not allowed when `sidecar: true`. |
| shell — kill from JS | `shell:allow-kill` | You do not need this if Rust owns the lifecycle. |
| shell — stdin from JS | `shell:allow-stdin-write` | |
| http | `http:default` (enables `fetch`, `fetch-cancel`, `fetch-send`, `fetch-read-body`, `fetch-cancel-body`) **plus explicit `"http:allow-fetch"` scope with URLs** | `http:default` enables the verbs but allows **no origins**. Without an origin scope every request is denied. |
| fs | `fs:default` (read/write only inside the app-specific dirs: AppConfig, AppData, AppLocalData, AppCache, AppLog) plus `fs:allow-*-read-recursive`/`-write-recursive` and `fs:scope-*` for anywhere else | `fs:default` also installs `deny-default`, which blocks `$APPLOCALDATA/EBWebView` (the WebView2 profile) on Windows. |

**Recommended posture for Veloct:** do all fs/http/sql/git/upload work in Rust `#[tauri::command]`s and grant the webview almost nothing. Rust commands registered with `invoke_handler` are allowed for all windows by default; to constrain them, use `tauri_build::AppManifest::new().commands(&[...])` in `build.rs`. This keeps the capability file small and avoids the http-origin-scope footgun entirely.

### 3.3 Correct example capability file

```jsonc
// src-tauri/capabilities/default.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Veloct main-window capability: core APIs, the Astro sidecar, and a narrowly scoped HTTP allowlist.",
  "platforms": ["windows", "macOS", "linux"],
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:event:default",
    "core:window:default",
    "core:webview:default",
    "core:app:default",
    "core:path:default",
    "core:resources:default",
    "core:window:allow-set-title",
    "core:window:allow-start-dragging",

    // Local store (non-secret prefs only — secrets go to the OS keychain via Rust).
    "store:default",

    // Opener for external links (replaces the deprecated shell.open).
    "opener:default",
    {
      "identifier": "opener:allow-open-url",
      "allow": [{ "url": "https://*" }]
    },

    // --- Sidecar: only if the WEBVIEW spawns/kills it. Omit entirely if Rust owns it. ---
    {
      "identifier": "shell:allow-spawn",
      "allow": [
        {
          "name": "binaries/veloct-astro-sidecar",
          "sidecar": true,
          "args": [
            "dev",
            "--host",
            { "validator": "^127\\.0\\.0\\.1$" },
            "--port",
            { "validator": "^[0-9]{4,5}$" },
            "--root",
            { "validator": "^.+$" }
          ]
        }
      ]
    },
    "shell:allow-kill",

    // --- HTTP: verbs + explicit origins. http:default alone allows NO origins. ---
    {
      "identifier": "http:default",
      "allow": [
        { "url": "http://127.0.0.1:*" },
        { "url": "http://localhost:*" }
      ]
    },

    // --- FS: app dirs are covered by fs:default; add explicit scopes only if needed. ---
    "fs:default",
    {
      "identifier": "fs:allow-read-text-file",
      "allow": [{ "path": "$APPDATA/**" }]
    }
  ]
}
```

Field rules verified against `tauri-plugin-shell 2.4.0/src/scope_entry.rs`:

- `name` is **required** on every shell scope entry.
- `cmd` is **required unless** `sidecar: true`.
- `args` is `boolean | array`; `true` allows any args, `false` (the default) denies all. An array entry is either a fixed string or `{ "validator": "<regex>", "raw": false }`, matched **in order**.
- `sidecar` defaults to `false`.
- The deserializer uses `deny_unknown_fields` — a typo'd key is a hard error, not a warning.

**Sources:** [Capabilities](https://v2.tauri.app/security/capabilities/) · [Shell plugin permissions](https://v2.tauri.app/plugin/shell/) · [HTTP client plugin](https://v2.tauri.app/plugin/http-client/) · [File system plugin](https://v2.tauri.app/plugin/file-system/) · [Process plugin](https://v2.tauri.app/plugin/process/)

---

## 4. Keychain crates

### 4.1 `keyring` — latest 4.2.0, and a **breaking architecture change you must know about**

`keyring 4.2.0` is not a drop-in for the `keyring 2.x`/`3.x` API you will find in most blog posts and LLM output.

**The 4.x model:**

- The **engine** moved to a new crate, `keyring-core` **1.0.0** (published 2026-09-23). Credential stores implement its `CredentialStore` trait.
- `keyring 4.2.0` is now a *facade* with two mutually-informative modes selected by Cargo features:
  - **`v1`** (in `default`) — "behaves essentially the same as the v1 version of Keyring behaved": platform-independent `Entry` with automatic store selection. Sets `apple-native-keyring-store/keychain`, `windows-native-keyring-store`, `zbus-secret-service-keyring-store`.
  - **`cli`** — the glue for the demo CLI; pulls in *every* store (Android, Apple, db-keystore, dbus-secret-service, linux-keyutils, zbus, Windows, keyring-core/sample). Don't enable this in Veloct.
- Windows support is **unconditional under the default `v1` feature** — `windows-native-keyring-store ^1` is a target-gated optional dep under `cfg(target_os = "windows")` and `v1` activates it. Latest `windows-native-keyring-store` is **1.1.0** ("Windows native credential store for keyring").
- The docs explicitly say: apps that want to choose their own stores should depend on `keyring-core` + a specific store, not on `keyring`. For Veloct (Windows-first, wants Windows Credential Manager) the simple `v1` mode is the right trade.

**`v1::Entry` API (exact, from docs.rs for 4.2.0):**

```rust
pub fn new(service: &str, username: &str) -> Result<Self>
pub fn store_status() -> &'static Result<()>
pub fn set_password(&self, password: &str) -> Result<()>
pub fn set_secret(&self, secret: &[u8]) -> Result<()>
pub fn get_password(&self) -> Result<String>
pub fn get_secret(&self) -> Result<Vec<u8>>
pub fn delete_credential(&self) -> Result<()>
```

Note the name: **`delete_credential`**, not `delete_password` (that was the 2.x/3.x name). And `Entry::new` returns a `Result`, not a bare `Entry`.

**Cargo.toml:**

```toml
[dependencies]
# v1 is the default feature; spelling it out documents intent and survives a future
# default-feature change.
keyring = { version = "4.2.0", features = ["v1"] }
```

No `[target.'cfg(windows)'.dependencies]` block is needed — the Windows store is pulled in automatically by `v1`.

### 4.2 Tauri official plugin: Stronghold, not a keychain plugin

There is **no official Tauri keychain/credential-manager plugin**. What exists:

| Crate | Latest | Verdict |
|---|---|---|
| `tauri-plugin-stronghold` | **2.4.0** | Official. IOTA Stronghold encrypted vault (Argon2-derived key + in-memory snapshot, optionally persisted). Encrypted-at-rest **file**, not the OS credential store. Requires managing a password/salt, which is the exact problem a keychain solves. Overkill for "store an S3 secret key". |
| `tauri-plugin-keyring` | **0.1.0** | **Not official.** A third-party wrapper around the `keyring` crate. At 0.1.0 it has no stability promise and will lag the `keyring` 4.x `CredentialStore` rework. Do not depend on it. |

### 4.3 Recommendation

**Use `keyring = { version = "4.2.0", features = ["v1"] }` in a dedicated Rust `credentials` crate, exposed to the frontend only through typed `#[tauri::command]`s.** Rationale:

1. On Windows it writes to **Windows Credential Manager**, the OS-supported secret store, keyed by your service name. Nothing lands on disk in plaintext, and it survives app upgrades.
2. It is a pure-Rust dependency (`windows-native-keyring-store` uses the Windows credential APIs) — no C toolchain, no vendored native lib, no build.rs surprises in the workspace.
3. Stronghold gives you a portable encrypted vault but makes *you* the key manager, which is strictly worse for this use case. A compromised machine with the vault password is compromised either way; Credential Manager at least gets DPAPI protection tied to the user profile.
4. **Do not** put secrets in `tauri-plugin-store` (2.5.0). That is plaintext JSON in `$APPDATA`. Reference secrets by an opaque id; never round-trip the secret value to the webview if you can avoid it.

### 4.4 Minimal set/get/delete example

```rust
// crates/veloct-credentials/src/lib.rs
use keyring::Entry;

const SERVICE: &str = "com.veloct.app";

/// Store a secret. The service+username pair is the primary key in Windows
/// Credential Manager; the value is written via CredWriteW (DPAPI-protected).
pub fn set_secret(username: &str, secret: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, username)?;
    entry.set_password(secret)
}

pub fn get_secret(username: &str) -> Result<String, keyring::Error> {
    let entry = Entry::new(SERVICE, username)?;
    entry.get_password()
}

/// NOTE: in keyring 4.x this is `delete_credential`, not `delete_password`.
pub fn delete_secret(username: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, username)?;
    entry.delete_credential()
}

/// Call once at startup to fail fast and get a usable error instead of a
/// confusing NoDefaultStore on first use.
pub fn check_available() -> Result<(), String> {
    match Entry::store_status() {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("OS credential store unavailable: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let user = "test/round-trip";
        set_secret(user, "s3cr3t-value").unwrap();
        assert_eq!(get_secret(user).unwrap(), "s3cr3t-value");
        delete_secret(user).unwrap();
        assert!(get_secret(user).is_err());
    }
}
```

Binary secrets (`set_secret`/`get_secret` with `&[u8]`) are available if you need to store raw key material; use `zeroize` on the buffer afterwards.

**Sources:** [keyring 4.2.0](https://docs.rs/keyring/4.2.0/keyring/) · [keyring::v1::Entry](https://docs.rs/keyring/4.2.0/keyring/v1/struct.Entry.html) · [keyring-core 1.0.0](https://docs.rs/keyring-core/latest/keyring_core/) · [`tauri-plugin-stronghold` on crates.io](https://crates.io/crates/tauri-plugin-stronghold)

---

## 5. `git2` crate

### 5.1 Versions

| Crate | Latest stable | Notes |
|---|---|---|
| `git2` | **0.21.0** | Wraps `libgit2-sys 0.18.8+1.9.7` (libgit2 **1.9.7**) |
| `libgit2-sys` | **0.18.8+1.9.7** | |
| `gix` | **0.88.0** | Pure Rust. MSRV **1.88**, released 2026-09-25 |
| `gitoxide` (CLI) | **0.59.0** | The binary; not what you depend on |

### 5.2 Does `git2` need vendored libgit2 on Windows?

**It builds without any extra system dependency on MSVC, and it does not use CMake at all.** I verified this by reading `libgit2-sys 0.18.8+1.9.7/build.rs` and `Cargo.toml`:

- The vendored path compiles libgit2 with the **`cc` crate** (`[build-dependencies] cc = { version = "1.0.43", features = ["parallel"] }`) by enumerating `libgit2/src/**/*.c`. `pkg-config` is only consulted when a system libgit2 is attempted.
- Vendoring is attempted by default (`let try_to_use_system_libgit2 = !vendored && !zlib_ng_compat;`) and you can force system-only with `LIBGIT2_NO_VENDOR`.
- MSVC-specific defines are handled in-tree (`build.rs:191-206`): `_CRT_SECURE_NO_DEPRECATE`, `_CRT_SECURE_NO_WARNINGS`, `STRSAFE_NO_DEPRECATE`, `WIN32`, `_WIN32_WINNT=0x0600`.
- Windows system libs are linked automatically (`build.rs:312-319`): `winhttp`, `rpcrt4`, `ole32`, `crypt32`, `secur32`, `advapi32`. All ship with the Windows SDK — nothing to install.
- libgit2's **built-in** SHA-256 is used when the `https` feature is off; the `ssh` path uses `libssh2-sys` only if you ask for it.

So: **no `vendored-libgit2` is strictly required on MSVC**, but pinning it is the right call for reproducibility — it prevents a hostile `pkg-config` on a dev machine or CI image from silently switching you to a system libgit2 of unknown version.

### 5.3 The one real trap: `https` pulls in OpenSSL

`git2`'s `https` feature expands to `["libgit2-sys/https", "openssl-sys", "openssl-probe", "cred"]`. On Windows/MSVC, `openssl-sys` has **no system OpenSSL to find**, so it fails unless you either:

- add `vendored-openssl` (which needs **Perl** and **NASM** on the machine and adds a long C build), or
- avoid the `https` feature.

**Recommendation for Veloct:** do **not** enable `git2`'s `https`/`ssh` features. Run local repository operations through `git2` and do all *network* git traffic through your existing Rust HTTP client (`reqwest`) or through `gitoxide`, which has pure-Rust transport. If you must have libgit2-driven fetch/push over HTTPS, that is a deliberate decision to take on the vendored-OpenSSL toolchain requirement.

### 5.4 Exact Cargo.toml lines

Veloct, git via libgit2, local operations only:

```toml
[dependencies]
# Wraps libgit2 1.9.7. Vendoring compiles libgit2 with `cc` (no CMake, no system libgit2).
# Do NOT enable `https` or `ssh`: `https` drags in openssl-sys, which has no system
# provider on MSVC and would force `vendored-openssl` (+ Perl + NASM).
git2 = { version = "0.21.0", default-features = false, features = ["vendored-libgit2"] }
```

If you decide to use `gitoxide` instead (pure Rust, no C toolchain, builds everywhere):

```toml
[dependencies]
# Pure-Rust git. MSRV 1.88, satisfied by rustc 1.97.1.
gix = { version = "0.88.0", default-features = false, features = ["max-performance-safe", "blocking-http-transport-reqwest-rust-tls", "worktree-mutation", "status", "dirwalk"] }
```

Feature names above for `gix` beyond `default-features`/`max-performance-safe` are version-sensitive — **UNVERIFIED** for 0.88.0 in particular; resolve them with `cargo add gix` and read the resulting `Cargo.toml`, or check the feature list at `https://crates.io/api/v1/crates/gix/0.88.0`.

**Trade-off summary:** `git2` gives you the mature, complete libgit2 API surface with a heavier-but-automatic C build. `gix` avoids the C toolchain entirely and is what you want if cross-compiling or building on a machine without MSVC Build Tools, but its API is lower-level and still evolving at 0.x.

**Sources:** [libgit2-sys 0.18.8+1.9.7 build.rs](https://docs.rs/libgit2-sys/0.18.8+1.9.7/libgit2_sys/) · [git2 0.21.0 on crates.io](https://crates.io/crates/git2) · [gix 0.88.0 on crates.io](https://crates.io/crates/gix) · [How to Build libgit2](https://libgit2.org/docs/guides/build-and-link/)

---

## 6. SQLite crates

### 6.1 Versions

| Crate | Latest stable | Released |
|---|---|---|
| `rusqlite` | **0.40.2** | — |
| `libsqlite3-sys` (pulled by `bundled`) | **0.38.2** | — |
| `sqlx` | **0.9.0** | 2026-07-20 |
| `rusqlite_migration` | **2.6.0** | — (depends on `rusqlite ^0.40.0`) |
| `refinery` | **0.9.2** | — |

`rusqlite 0.40.2` features relevant here (from the crates.io feature map):

```text
bundled            = ["libsqlite3-sys?/bundled", "modern_sqlite"]
bundled-full       = ["modern-full", "bundled"]
bundled-windows    = ["libsqlite3-sys?/bundled-windows"]
modern_sqlite      = ["libsqlite3-sys?/bundled_bindings"]
backup, blob, functions, hooks, limits, load_extension, serialize, trace, vtab
```

`sqlx 0.9.0` relevant features:

```text
sqlite                  = ["sqlite-bundled", "sqlite-deserialize", "sqlite-load-extension", "sqlite-unlock-notify"]
sqlite-bundled          = ["_sqlite", "sqlx-sqlite/bundled", "sqlx-macros?/sqlite"]
sqlite-unbundled        = ["_sqlite", "sqlx-sqlite/unbundled", "sqlx-macros?/sqlite-unbundled"]
runtime-tokio, tls-rustls-ring, macros, migrate, derive, json, chrono, uuid
```

### 6.2 Which is right for Veloct?

**`rusqlite 0.40.2` with `bundled`, plus `rusqlite_migration 2.6.0`.**

Reasons specific to this project:

- Veloct's DB is a single-user, single-file embedded store. `sqlx`'s value is compile-time-checked queries and a uniform async driver across Postgres/MySQL/SQLite. You have no second backend, and compile-time query checking requires a live DB or a checked-in `.sqlx` offline cache at build time — real friction in a Cargo-workspace + Tauri build.
- `rusqlite` is synchronous. Tauri commands can be `async fn` but blocking DB work belongs on `tauri::async_runtime::spawn_blocking` anyway. `sqlx` would force a Tokio runtime into your DB crate and make every call site `async` for no concurrency benefit at this scale.
- `bundled` compiles SQLite from source via the `cc` crate. On MSVC this is automatic and gives you a known SQLite version (`libsqlite3-sys 0.38.2`) with no DLL to ship and no "user has an ancient sqlite3.dll" support class. This is the single biggest argument for `bundled` and it is not optional — **always** use `bundled` on Windows.
- The migration story is a compiled-in, ordered list of SQL strings versioned via SQLite's `user_version` integer — no external CLI, no `migrations/` directory to ship and locate at runtime, no async.

**`rusqlite_migration 2.6.0` details** (verified on docs.rs):

- Uses `user_version` rather than a bookkeeping table → one integer at a fixed file offset, no extra parsing on open. Its own docs advertise this as the performance win.
- API: `Migrations::new(Vec<M>)`, `Migrations::from_slice(&[M])` (const-capable), `Migrations::from_directory(&Dir)` (feature `from-directory`, requires `include_dir`), `current_version(&Connection) -> SchemaVersion`, `pending_migrations(&Connection) -> i32`, `to_latest(&mut Connection)`, `to_version`, `validate()`.
- `pending_migrations` is the hook for "back up before migrating": if it's non-zero, copy the DB file first.
- Optional feature `from-directory` (adds `include_dir`) lets you keep `.sql` files and `include_dir!` them into the binary — nice if you want migrations readable in the repo. Requires directories named `{id}-{name}` containing `up.sql` (and optional `down.sql`).

### 6.3 Exact Cargo.toml lines

```toml
[dependencies]
# Bundled SQLite: compiled from source with `cc`, statically linked. No DLL to ship,
# no dependency on a system sqlite3.dll. REQUIRED on Windows.
rusqlite = { version = "0.40.2", features = ["bundled", "backup", "functions", "serde_json"] }
# NOTE: verify "serde_json" is a real feature name for 0.40.2 before committing;
# the verified feature list is available at
# https://crates.io/api/v1/crates/rusqlite/0.40.2  (.version.features)
# Confirmed-present features: bundled, bundled-full, bundled-windows, modern_sqlite,
# backup, blob, functions, hooks, limits, load_extension, serialize, trace, vtab.

# Untracked migrations in Rust source. Depends on rusqlite ^0.40.0 — matches.
rusqlite_migration = "2.6.0"

# If you prefer migration .sql files embedded from a directory:
# rusqlite_migration = { version = "2.6.0", features = ["from-directory"] }
# include_dir = "0.7"
```

If you instead choose `sqlx` (not recommended here), the pinned form is:

```toml
sqlx = { version = "0.9.0", default-features = false, features = [
  "runtime-tokio", "tls-rustls-ring", "sqlite", "migrate", "macros", "json", "chrono", "uuid",
] }
```

### 6.4 Minimal example

```rust
// crates/veloct-store/src/db.rs
use rusqlite::{Connection, Result};
use rusqlite_migration::{Migrations, M};

/// Ordered, append-only. NEVER edit or reorder an existing entry — only append.
/// Index i corresponds to `user_version = i + 1`.
const MIGRATIONS_SLICE: &[M<'_>] = &[
    M::up(
        "CREATE TABLE project (
             id           INTEGER PRIMARY KEY,
             name         TEXT NOT NULL UNIQUE,
             root_path    TEXT NOT NULL,
             created_at   TEXT NOT NULL DEFAULT (datetime('now')),
             updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE credential_ref (
             id           INTEGER PRIMARY KEY,
             project_id   INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
             keychain_key TEXT NOT NULL,
             kind         TEXT NOT NULL CHECK (kind IN ('s3','sftp','git')),
             UNIQUE (project_id, kind)
         );
         CREATE TABLE deployment (
             id          INTEGER PRIMARY KEY,
             project_id  INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
             target      TEXT NOT NULL,
             status      TEXT NOT NULL,
             started_at  TEXT NOT NULL DEFAULT (datetime('now')),
             finished_at TEXT
         );",
    ),
    // M::up("ALTER TABLE project ADD COLUMN description TEXT;"),
];

const MIGRATIONS: Migrations<'_> = Migrations::from_slice(MIGRATIONS_SLICE);

/// Open (creating if needed), apply pragmas, then migrate. Idempotent.
pub fn open(db_path: &std::path::Path) -> Result<Connection> {
    let mut conn = Connection::open(db_path)?;

    // Pragmas belong OUTSIDE the migrations. WAL is the big one for a desktop app
    // that may be read while the sidecar writes.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;

    // Back up before a schema change lands (cheap insurance against a bad migration).
    if MIGRATIONS.pending_migrations(&conn).unwrap_or(0) != 0 {
        let mut backup_path = db_path.to_path_buf();
        backup_path.set_extension("premigration.bak");
        let _ = std::fs::copy(db_path, &backup_path);
    }

    MIGRATIONS
        .to_latest(&mut conn)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        // catches duplicate/conflicting versions and malformed definitions at test time
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn migrates_from_scratch() {
        let mut conn = Connection::open_in_memory().unwrap();
        MIGRATIONS.to_latest(&mut conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM project", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
```

`MIGRATIONS.to_latest()` returns `rusqlite_migration::Error`, not `rusqlite::Error`; the `map_err` above is a placeholder for whatever error type your crate actually exposes — use `thiserror` and return your own error enum in real code rather than contorting `rusqlite::Error`.

**Sources:** [rusqlite 0.40.2](https://docs.rs/rusqlite/0.40.2/rusqlite/) · [rusqlite_migration 2.6.0](https://docs.rs/rusqlite_migration/2.6.0/rusqlite_migration/) · [`Migrations` API](https://docs.rs/rusqlite_migration/2.6.0/rusqlite_migration/struct.Migrations.html) · [sqlx 0.9.0 migrate](https://docs.rs/sqlx/0.9.0/sqlx/migrate/index.html)

---

## 7. Windows build prerequisites check

### 7.1 What a Tauri 2 Windows MSVC build actually requires

1. **Microsoft C++ Build Tools** with the **"Desktop development with C++"** workload — provides `link.exe`, the Windows SDK, and the MSVC headers. Required. Tauri's own docs name this explicitly.
2. **Microsoft Edge WebView2 Runtime** — required at runtime for development *and* production. Preinstalled on Windows 10 1803+ and Windows 11. Tauri can also auto-deploy it via the installer: `bundle.windows.webviewInstallMode` defaults to `{ "type": "downloadBootstrapper", "silent": true }`.
3. **Rust** with the `x86_64-pc-windows-msvc` target (this host: rustc 1.97.1, target installed and active).
4. **VBSCRIPT optional Windows feature** — only if you build MSI (`bundle.targets` includes `"msi"`). Failure mode is `failed to run light.exe`. **Not needed if you use `"nsis"`**, which is the better default for a modern app anyway.
5. **Node.js** for the frontend build and for producing the sidecar. Required for Veloct regardless.
6. Optional: **Perl + NASM** if any dependency needs `vendored-openssl`. Avoid by not enabling `git2`'s `https` feature (§5.3).

Two schema facts worth pinning in `tauri.conf.json`:

- `build.windows.staticVCRuntime` — **defaults to `true`**. Tauri's own binaries statically link the VC++ runtime. Good.
- `bundle.windows.bundleVCRuntime` — **defaults to `false`**. Set it to `true` **only if your sidecar or a bundled DLL links `vcruntime140.dll` / `msvcp140.dll` dynamically**; otherwise end users without the VC++ Redistributable get a silent launch failure. If your Node sidecar was produced by `@yao-pkg/pkg` it is self-contained, so this should stay `false` — verify with `dumpbin /dependents` on the produced `.exe`.

### 7.2 Programmatic check: is `link.exe` available?

`link.exe` is normally **not on `PATH`** — it only appears after sourcing `vcvarsall.bat` / `Microsoft.VC.`. I verified this on the host: `Get-Command link.exe` returns nothing, but `vswhere` locates the Build Tools install and `link.exe` exists underneath it.

Verified on this host:

```text
vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  -> C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools
link.exe found at:
  C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC\14.51.36231\bin\Hostx64\x64\link.exe
```

**Check A — is the MSVC toolchain installed at all (recommended gate):**

```powershell
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"

if (-not (Test-Path $vswhere)) {
    Write-Error "vswhere.exe not found: no Visual Studio / Build Tools installer on this machine."
    exit 1
}

$msvcRoot = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath

if (-not $msvcRoot) {
    Write-Error "MSVC C++ build tools not installed. Install 'Desktop development with C++'."
    exit 1
}
Write-Host "MSVC root: $msvcRoot"

$link = Get-ChildItem -Path (Join-Path $msvcRoot 'VC\Tools\MSVC') `
    -Filter 'link.exe' -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match 'Hostx64\\x64' } |
    Select-Object -First 1

if (-not $link) {
    Write-Error "link.exe not found under $msvcRoot\VC\Tools\MSVC"
    exit 1
}
Write-Host "link.exe: $($link.FullName)"
```

**Check B — is `link.exe` on the current `PATH` (i.e. is this shell already a dev shell):**

```powershell
$onPath = (Get-Command link.exe -ErrorAction SilentlyContinue).Source
if ($onPath) { "link.exe on PATH: $onPath" } else { "link.exe NOT on PATH — run from a VS Developer PowerShell, or cargo will fail to link" }
```

**Check C — resolve the Dev Shell environment for a non-dev terminal (what CI needs).** Prefer this over scraping `PATH`:

```powershell
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsRoot  = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath

# Import the x64 developer environment into this PowerShell session.
$devShell = Join-Path $vsRoot 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll'
if (-not (Test-Path $devShell)) { $devShell = Join-Path $vsRoot 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll' }

Import-Module $devShell
Enter-VsDevShell -VsInstallPath $vsRoot -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'

# Now link.exe is on PATH:
(Get-Command link.exe).Source
```

**Check D — the definitive proof: does a real MSVC link succeed?**

```powershell
# Fastest true end-to-end test. Uses --print host-tuple (Rust 1.84+).
$triple = (rustc --print host-tuple).Trim()
if ($triple -ne 'x86_64-pc-windows-msvc') {
    Write-Warning "Active host tuple is '$triple', expected x86_64-pc-windows-msvc."
}

$probe = Join-Path $env:TEMP "veloct-linkprobe-$(Get-Random)"
cargo new --bin $probe --quiet
Push-Location $probe
cargo build --quiet    # fails loudly if link.exe cannot be found
$ok = $LASTEXITCODE -eq 0
Pop-Location
Remove-Item $probe -Recurse -Force -ErrorAction SilentlyContinue
if ($ok) { 'MSVC link OK' } else { 'MSVC link FAILED' }
```

Also confirm `rustc`/`cargo` are reachable — **on this host they are not on `PATH`**:

```powershell
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoExe = "$env:USERPROFILE\.cargo\bin\cargo.exe"
    if (Test-Path $cargoExe) {
        Write-Warning "cargo not on PATH; found at $cargoExe — prepend $env:USERPROFILE\.cargo\bin"
        $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    } else {
        Write-Error "cargo not found. Install Rust via rustup."
    }
}
rustc -Vv   # want: host: x86_64-pc-windows-msvc
```

### 7.3 Programmatic check: is WebView2 installed?

Microsoft documents exactly two registry locations on 64-bit Windows and two on 32-bit Windows. The check is: key present **and** `pv (REG_SZ)` present and greater than `0.0.0.0`.

I verified this on the host — the 64-bit per-machine key exists and reports `pv = 153.0.4234.48` (Microsoft Edge WebView2 Runtime), while the 32-bit-path key and the HKCU key are absent:

```text
HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}
  -> pv=153.0.4234.48  name=Microsoft Edge WebView2 Runtime
HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{...}     -> NOT PRESENT
HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{...}     -> NOT PRESENT
${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application\153.0.4234.48   (install dir present)
```

**Check A — registry probe (exact PowerShell):**

```powershell
$wv2Guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'

# The documented set: (HKLM+CU) x (WOW6432Node for 64-bit OS / plain for 32-bit OS).
# On a 64-bit OS, 32-bit PowerShell sees HKLM\SOFTWARE as WOW6432Node, so list both
# spellings and let the missing ones fail silently.
$candidateKeys = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$wv2Guid"
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$wv2Guid"
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$wv2Guid"
)

$webview2Version = $null
foreach ($key in $candidateKeys) {
    $pv = (Get-ItemProperty -Path $key -Name 'pv' -ErrorAction SilentlyContinue).pv
    if ($pv -and [version]$pv -gt [version]'0.0.0.0') {
        $webview2Version = $pv
        break
    }
}

if ($webview2Version) {
    Write-Host "WebView2 Runtime installed: $webview2Version"
} else {
    Write-Warning "WebView2 Runtime NOT installed. Install the Evergreen Bootstrapper, or rely on the Tauri installer's webviewInstallMode."
}
```

**Check B — file-system corroboration (belt and braces):**

```powershell
$wv2Dir = "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application"
if (Test-Path $wv2Dir) {
    $versions = Get-ChildItem $wv2Dir -Directory |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending
    if ($versions) { "WebView2 on disk: $($versions[0].Name) at $($versions[0].FullName)" }
} else {
    "No WebView2 Application folder at $wv2Dir"
}
```

**Check C — the runtime-independent API path (Microsoft's "Approach 2").** `GetAvailableCoreWebView2BrowserVersionString` returns information for the Runtime *or* any preview Edge channel, and a `nullptr` version means the Runtime is missing. It requires loading `WebView2Loader.dll`, so it is only practical from the app itself (via P/Invoke) — **UNVERIFIED** here as I did not exercise the P/Invoke path. The registry probe in Check A is the documented, dependency-free equivalent and is what a preflight script should use.

**Note:** `msedgewebview2.exe` is **not** on `PATH` (confirmed on this host) — do not use `Get-Command msedgewebview2.exe` as a detection method.

**Sources:** [Tauri prerequisites (Windows)](https://v2.tauri.app/start/prerequisites/) · [Distribute your app and the WebView2 Runtime](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) · [GetAvailableCoreWebView2BrowserVersionString](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/webview2-idl#getavailablecorewebview2browserdversionstring) · [WebView2Feedback #925](https://github.com/MicrosoftEdge/WebView2Feedback/issues/925)

---

## Appendix A — Supporting crate versions (queried 2026-09-28)

These were not the subject of a numbered section but are needed to fill out the stack. All are `max_stable_version` from crates.io.

| Purpose | Crate | Latest stable | MSRV | Note |
|---|---|---|---|---|
| HTTP client, streaming | `reqwest` | **0.13.5** | 1.85.0 | Tauri 2.12.0 itself depends on `reqwest ^0.13`, so this aligns. |
| HTTP (alt) | — | — | — | `ureq` — not queried |
| S3 (pure Rust) | `rust-s3` | **0.37.2** | — | no MSRV declared |
| S3 (generic object store) | `object_store` | **0.14.2** | 1.85 | Apache Arrow project; good S3+Azure+GCS abstraction |
| S3 (official AWS) | `aws-sdk-s3` | **1.150.0** | **1.94.1** | large dependency tree; MSRV 1.94.1 is fine on 1.97.1 |
| S3 (config) | `aws-config` | **1.12.0** | — | |
| SFTP | `ssh2` | **0.9.6** | — | libssh2 bindings; needs a C build (vendored) |
| SFTP | `russh` | **0.63.3** | — | pure Rust SSH |
| SFTP | `russh-sftp` | **3.0.0** | — | pairs with `russh` |
| SFTP | `openssh-sftp-client` | **0.15.9** | 1.64 | higher-level; newer, smaller ecosystem |
| Zip | `zip` | **8.6.0** | 1.88 | newest is `9.0.0-pre3` — use 8.6.0 stable. `default` = `aes-crypto,bzip2,deflate64,deflate,lzma,ppmd,time,zstd,xz`; `deflate` alone = `deflate-zopfli,deflate-flate2-zlib-rs` (pure-Rust zlib) |
| Tar | `tar` | **0.4.46** | 1.63 | |
| Compression | `flate2` | **1.1.10** | — | |
| Async runtime | `tokio` | **1.53.1** | 1.71 | |
| Free port | `portpicker` | **0.1.1** | — | **last published 2021-06-18** — effectively unmaintained; prefer the ~4-line `TcpListener::bind(("127.0.0.1", 0))` helper in §2.6 |
| Process spawning (non-Tauri) | `duct` | **1.1.2** | — | |
| Process listing / tree kill | `sysinfo` | **0.39.6** | — | better than shelling to `taskkill` if you need to enumerate children |
| Windows APIs | `windows` | **0.62.2** | — | same major as `tauri 2.12.0`'s own dependency, so no duplicate |
| Windows APIs (raw) | `windows-sys` | **0.61.2** | — | |
| Temp files | `tempfile` | **3.27.0** | — | |
| Serde | `serde` | **1.0.229** | — | |
| JSON | `serde_json` | **1.0.151** | — | |
| Errors | `thiserror` | **2.0.21** | — | same major as `tauri-plugin-shell 2.4.0` |
| Errors | `anyhow` | **1.0.104** | — | |
| Directory walk | `walkdir` | **2.5.0** | — | |

### Cargo.toml snippet for the archiving / network / SFTP domain crates

```toml
[dependencies]
reqwest = { version = "0.13.5", default-features = false, features = [
  "rustls-tls", "stream", "json", "gzip", "brotli", "http2",
] }
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "fs", "io-util", "process", "net"] }
zip = { version = "8.6.0", default-features = false, features = ["deflate", "time", "aes-crypto"] }
tar = "0.4.46"
flate2 = "1.1.10"
object_store = { version = "0.14.2", features = ["aws"] }
russh = "0.63.3"
russh-sftp = "3.0.0"
tempfile = "3.27.0"
```

Prefer the pure-Rust paths on Windows: `russh`/`russh-sftp` over `ssh2` (no libssh2 C build, no vendored OpenSSL), and `rustls` over native-tls in `reqwest` (no Schannel edge cases, no OpenSSL).

⚠️ **`aws-sdk-s3` declares MSRV 1.94.1.** Satisfied by rustc 1.97.1, but it locks the workspace's minimum to a very recent Rust. If you want a lower MSRV, `object_store` (1.85) or `rust-s3` (none declared) are the alternatives.

---

## Appendix B — Consolidated version index for `Cargo.toml` / `package.json`

```toml
# src-tauri/Cargo.toml  (workspace root [workspace.dependencies])
[workspace.dependencies]
tauri = { version = "2.12.0", features = [] }
tauri-build = { version = "2.7.0", features = [] }
tauri-plugin-shell = "2.4.0"
tauri-plugin-http = "2.8.0"
tauri-plugin-fs = "2.6.0"
tauri-plugin-dialog = "2.8.0"
tauri-plugin-opener = "2.6.0"
tauri-plugin-store = "2.5.0"
tauri-plugin-single-instance = "2.5.0"
tauri-plugin-log = "2.10.0"
tauri-plugin-updater = "2.13.0"
tauri-plugin-window-state = "2.5.0"
keyring = { version = "4.2.0", features = ["v1"] }
git2 = { version = "0.21.0", default-features = false, features = ["vendored-libgit2"] }
rusqlite = { version = "0.40.2", features = ["bundled", "backup", "functions"] }
rusqlite_migration = "2.6.0"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
thiserror = "2.0.21"
anyhow = "1.0.104"
```

```jsonc
// package.json
{
  "dependencies": {
    "@tauri-apps/api": "2.12.0",
    "@tauri-apps/plugin-shell": "2.4.0",
    "@tauri-apps/plugin-http": "2.7.0",
    "@tauri-apps/plugin-fs": "2.6.0",
    "@tauri-apps/plugin-dialog": "2.8.0",
    "@tauri-apps/plugin-opener": "2.6.0",
    "@tauri-apps/plugin-store": "2.5.0",
    "@tauri-apps/plugin-updater": "2.13.0",
    "@tauri-apps/plugin-log": "2.10.0",
    "@tauri-apps/plugin-window-state": "2.5.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "2.12.0",
    "vite": "8.3.1",
    "typescript": "7.0.2",
    "astro": "7.3.5"
  }
}
```

Notice `@tauri-apps/plugin-http` is pinned to **2.7.0** (npm `latest`) while the Rust `tauri-plugin-http` is **2.8.0**. This is a genuine upstream skew, not a typo — pin each side to its own registry's latest and re-check before release.

---

## Appendix C — Explicitly UNVERIFIED items

Stated plainly rather than guessed:

1. **`gix 0.88.0` feature names** beyond `max-performance-safe` — **UNVERIFIED**. Resolve via `cargo add gix` or the crates.io feature map for that exact version.
2. **`rusqlite 0.40.2`'s `serde_json` feature** — I confirmed the feature list and `serde_json` was **not** in the subset I filtered and printed. Treat `serde_json` in the `rusqlite` feature list as **UNVERIFIED**; check `https://crates.io/api/v1/crates/rusqlite/0.40.2` `.version.features` before using it. `bundled`, `backup`, `functions`, `blob`, `hooks`, `limits`, `load_extension`, `serialize`, `trace`, `vtab`, `modern_sqlite`, `bundled-full`, `bundled-windows` **are** confirmed present.
3. **WebView2 `GetAvailableCoreWebView2BrowserVersionString` P/Invoke from PowerShell** — **UNVERIFIED** (not exercised). The registry probe is verified and is the documented equivalent.
4. **Exact behaviour of `bundle.windows.bundleVCRuntime: true`** with a `@yao-pkg/pkg`-produced Node sidecar — **UNVERIFIED**. Determine empirically with `dumpbin /dependents` on the produced sidecar `.exe`.
5. **Whether `tauri-plugin-keyring 0.1.0` builds against `keyring` 4.x** — **UNVERIFIED** and not recommended; the crate predates the 4.x `CredentialStore` rework and may not compile.
6. **The `@tauri-apps/plugin-http` 2.7.0 ↔ `tauri-plugin-http` 2.8.0 skew** — I verified both numbers independently on 2026-09-28, but I did **not** verify that 2.7.0 JS interoperates with 2.8.0 Rust. Test the pair before shipping.
7. **`portpicker 0.1.1`'s correctness on modern Windows** — **UNVERIFIED**; the crate has not been published since 2021-06-18. The `TcpListener` helper in §2.6 is verified working on this host.
