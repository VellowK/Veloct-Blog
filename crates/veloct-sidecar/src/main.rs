//! The Veloct preview sidecar.
//!
//! Two modes, and what separates them is who serves the bytes:
//!
//! * **`dev --port <p> --host 127.0.0.1` / `preview --port <p> --host 127.0.0.1`**: the site's own Astro
//!   CLI. This launcher's job is to find the bundled Node runtime and the site's Astro entry point and
//!   hand the process over to Node.
//! * **`serve <directory> --port <p> --host 127.0.0.1`**: an already-built directory of static files,
//!   answered by THIS process with no Node runtime involved. That is the works preview, and it is
//!   deliberately not an Astro command: `dist/works` is build output, so an Astro `dev` would compile
//!   and serve the wrong thing and an Astro `preview` would serve the site's own `dist/site`. See
//!   `veloct_sidecar::serve`.
//!
//! Three decisions carry across both modes, and they are worth stating:
//!
//! * **`exec`-style handover on Unix, a child on Windows.** On Unix `exec` replaces this process, so there
//!   is no wrapper left to leak and the pid the supervisor recorded is still the server. Windows has no
//!   `exec`, so the child is spawned and this process waits and forwards the exit code — and the supervisor
//!   kills the whole tree, which is why the wrapper being in the tree is safe.
//! * **Diagnostics go to stderr and the exit code is non-zero on failure.** The supervisor polls
//!   readiness and a failure to launch must not look like a slow start: a sidecar that printed a warning
//!   and exited 0 would leave the preview "starting" forever.
//! * **The subcommand decides what has to be resolved.** `serve` resolves nothing: it needs neither the
//!   Node runtime nor the site's `node_modules`, and requiring them would make a works preview fail on a
//!   machine that could serve the built files perfectly well.

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use veloct_sidecar::serve::StaticServer;
use veloct_sidecar::{
    command_line, is_preview_command, is_serve_command, parse_serve, resolve_node, resolve_site,
};

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
    // Dispatched BEFORE Node and the Astro template are resolved, because `serve` needs neither. This is
    // not an optimisation: the works preview serves build output, and making it depend on the bundled
    // runtime or on `site/node_modules` would reintroduce "a preview that works only where the site's
    // dependencies happen to be installed".
    if is_serve_command(args) {
        return serve_built_directory(args.get(1..).unwrap_or_default());
    }

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
            "expected `dev`, `preview` or `serve`, got {:?}; the site build belongs to the release pipeline",
            args.first().map(String::as_str).unwrap_or("(nothing)")
        ));
    }

    veloct_sidecar::preview_preferences::disable_dev_toolbar(&site_dir)
        .map_err(|error| format!("could not disable Astro's preview toolbar: {error}"))?;
    let (program, full_args) = command_line(&node, &astro_entry, args);

    let mut command = Command::new(&program);
    // The launcher's flag does not apply to a new Node process: hide its console as well, while
    // preserving inherited output handles and the parent/child tree the supervisor terminates.
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
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

/// Serve an already-built directory as static files until the process is killed.
///
/// The directory comes from the `serve` arguments and is resolved against this process's working
/// directory, which the spawner sets to the project root. A directory that is not there is reported by
/// `StaticServer::bind` together with what produces it: a preview that answered 404 for everything
/// because the build had not run would be reported as a running preview, which is the pretending
/// `handoff.md` §10.4 forbids.
fn serve_built_directory(args: &[String]) -> Result<i32, String> {
    let request = parse_serve(args).map_err(|error| error.to_string())?;
    let server = StaticServer::bind(&request.directory, &request.host, request.port)?;

    // Blocks forever: the supervisor stops a preview with a process-tree kill (`handoff.md` §5.3), which
    // is the only shutdown this process has.
    server.serve()?;
    Ok(0)
}
