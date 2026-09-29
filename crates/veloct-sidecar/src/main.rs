//! The Astro preview sidecar.
//!
//! Tauri's `externalBin` runs this with the arguments the shell supervisor supplies (`dev --port <p>
//! --host 127.0.0.1`). Its whole job is to find the bundled Node runtime and the site's Astro entry point,
//! then hand the process over to Node.
//!
//! Two decisions worth stating:
//!
//! * **`exec`-style handover on Unix, a child on Windows.** On Unix `exec` replaces this process, so there
//!   is no wrapper left to leak and the pid the supervisor recorded is still the server. Windows has no
//!   `exec`, so the child is spawned and this process waits and forwards the exit code — and the supervisor
//!   kills the whole tree, which is why the wrapper being in the tree is safe.
//! * **Diagnostics go to stderr and the exit code is non-zero on failure.** The supervisor polls
//!   readiness and a failure to launch must not look like a slow start: a sidecar that printed a warning
//!   and exited 0 would leave the preview "starting" forever.

use std::process::{Command, Stdio};

use veloct_sidecar::{command_line, is_preview_command, resolve_node, resolve_site};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(code) => code,
        Err(message) => {
            // stderr, because stdout is not read by the supervisor and a user launching the sidecar by hand
            // should see the problem.
            eprintln!("veloct-astro-sidecar: {message}");
            2
        }
    };
    std::process::exit(code);
}

fn run(args: &[String]) -> Result<i32, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not determine my own location: {error}"))?;
    let directory = executable
        .parent()
        .ok_or_else(|| "my own location has no parent directory".to_string())?
        .to_path_buf();

    let node = resolve_node(&executable).map_err(|error| {
        // The hint carries where it looked, which is the actionable part.
        match error.hint() {
            Some(hint) => format!("{error}\n  {hint}"),
            None => error.to_string(),
        }
    })?;

    let project_root = std::env::var_os("VELOCT_PROJECT_ROOT").map(std::path::PathBuf::from);
    let (site_dir, astro_entry) = resolve_site(project_root.as_deref(), &directory).map_err(|error| {
        match error.hint() {
            Some(hint) => format!("{error}\n  {hint}"),
            None => error.to_string(),
        }
    })?;

    // A subcommand this launcher does not serve is refused rather than passed through: passing `build`
    // through would run a build in a process the supervisor expects to be a long-lived server, and the
    // build belongs to the release pipeline where its inputs are frozen.
    if !is_preview_command(args) {
        return Err(format!(
            "expected `dev` or `preview`, got {:?}; the site build belongs to the release pipeline",
            args.first().map(String::as_str).unwrap_or("(nothing)")
        ));
    }

    let (program, full_args) = command_line(&node, &astro_entry, args);

    let mut command = Command::new(&program);
    command
        .args(&full_args)
        .current_dir(&site_dir)
        // Inherited, not piped: the supervisor reads the child's output for its log, and piping here would
        // require this wrapper to pump it.
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        // The Astro CLI runs from the site directory and resolves `node_modules` from there; this is what
        // makes the site's own dependency tree authoritative rather than whatever is above it.
        .env("NODE_ENV", "development");

    // `VELOCT_PROJECT_ROOT` is already in the environment and is forwarded by inheritance, which is what
    // lets the site template read the same project this launcher resolved.

    let status = command
        .status()
        .map_err(|error| format!("could not start {}: {error}", program.display()))?;

    Ok(status.code().unwrap_or(1))
}
