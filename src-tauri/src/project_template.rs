//! Resolve the bundled scaffold on the backend for installed and development applications.
use std::path::{Path, PathBuf};

pub fn resolve(resource_dir: &Path) -> veloct_core::Result<PathBuf> {
    let bundled = resource_dir.join("project-template");
    if complete(&bundled) {
        return Ok(bundled);
    }
    // Tauri dev resource_dir is beside the executable, not necessarily the manifest directory.
    // Release builds must never depend on the build machine's repository.
    #[cfg(debug_assertions)]
    {
        let staged = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/project-template");
        if complete(&staged) {
            return Ok(staged);
        }
    }
    Err(veloct_core::VeloctError::not_configured(
        "the bundled project template is missing or incomplete",
        "repair the Veloct installation; development builds must run the template preparation step",
    ))
}

fn complete(path: &Path) -> bool {
    ["site/package.json", "site/astro.config.mjs", "data/site.json", "data/links.json"]
        .iter().all(|relative| path.join(relative).is_file())
}
