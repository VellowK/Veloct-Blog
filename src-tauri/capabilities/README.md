# Tauri capabilities: why the webview is granted almost nothing

`default.json` grants only core, event, window, webview, opener and dialog permissions. There is
deliberately **no** `fs:*`, **no** `http:*` and **no** `shell:*` scope.

Veloct performs every filesystem, HTTP, git, SQLite, upload and child-process operation inside
Rust `#[tauri::command]` handlers. Commands registered through `invoke_handler` are permitted for
all windows by default, so the frontend keeps full functionality while the webview holds no
ambient authority over the machine.

Two concrete traps this avoids:

* `http:default` enables the fetch verbs but permits **no origins**. Every request is denied
  until an explicit URL scope is added, which is a confusing failure to debug. Veloct does not
  use the JS HTTP plugin at all, which also sidesteps the known skew between
  `@tauri-apps/plugin-http` 2.7.0 and the Rust `tauri-plugin-http` 2.8.0.
* Shell scope entries are deny-by-default for arguments and deserialize with
  `deny_unknown_fields`, so a single typo becomes a hard startup error.

The Astro preview sidecar is spawned and torn down from Rust. Rust-side `Shell::sidecar` is not
ACL-gated, so no shell permission is needed. If a future change moves sidecar control into the
webview, it must add a narrowly scoped `shell:allow-spawn` entry with per-argument validators —
never an unrestricted execute permission, and never `shell:allow-execute` for arbitrary binaries.

Note also that `CommandChild::kill()` only terminates one process. The Node sidecar spawns Vite
and esbuild workers, so the Rust teardown uses `taskkill /PID <pid> /T /F` on Windows.
