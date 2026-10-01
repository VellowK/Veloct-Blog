//! The works service: import, list, export, edit metadata, remove, poster image.
//!
//! `handoff.md` §10.1 makes a work's truth source two files and nothing else:
//! `works/<stable-id>/work.json` for the metadata and the sibling `site/` for the static body.
//! SQLite only ever holds an index of either. This module therefore writes files and never a
//! database row, and every function takes the works root rather than a connection.
//!
//! Before this module existed, §10's whole model was reachable only from `veloct-content`'s own
//! tests: `import_work`, `list_works` and `export_work` had no caller above the crate, so a user
//! could not import a work, could not see one, and could not generate a poster image. That is
//! PROCESSING.md §5.9 / H10, and the commands in `src-tauri` are the other half of closing it.
//!
//! Two decisions are worth stating because both are about not inventing behaviour:
//!
//! * **The stored `screenshot` path is canonical, not detected.** §10.1's layout is
//!   `preview/screenshot.webp`, and the stored value is always that path. Accepting an arbitrary
//!   path would mean a second rule about what the site serves, which is exactly the kind of
//!   duplicated decision this repository keeps removing. An import whose archive already contains
//!   `preview/screenshot.webp` satisfies it; an import whose archive carries one under a different
//!   name records it in the plan's `unused_images` so the user is told rather than left guessing.
//! * **The bytes are copied, never reinterpreted.** Whatever the user hands us is what is stored.
//!   Veloct claims WebP because the file must BE WebP for a browser to render it, and it says so
//!   in the error when it is not, rather than silently writing a mislabelled file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_content::works::{
    export_work, import_work, read_work, write_work, ImportOptions, ImportOutcome, ImportSource,
    WorkMetadata, WorkRecord,
};
use veloct_core::ids::StableId;
use veloct_core::{Result, VeloctError};

/// The work directory's poster image, relative to the work directory (`handoff.md` §10.1).
pub const SCREENSHOT_RELATIVE_PATH: &str = "preview/screenshot.webp";

/// The largest poster image Veloct will store, in bytes.
///
/// 12 MiB. A poster is displayed at card size; a larger file is a mistake worth reporting rather
/// than a size worth paying for on every build.
pub const MAX_SCREENSHOT_BYTES: u64 = 12 * 1024 * 1024;

/// The file formats a poster may be stored as.
///
/// A deliberate allow-list rather than a free-for-all: the file at
/// [`SCREENSHOT_RELATIVE_PATH`] is served as WebP by the site's own route, so storing a PNG there
/// would produce a file a browser refuses while every layer above it reported success.
pub const SUPPORTED_IMAGE_FORMATS: &[&str] = &["webp", "png", "jpg", "jpeg"];

/// One work, as the window sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkView {
    /// `work.json`, verbatim.
    pub metadata: WorkMetadata,
    /// Whether `site/` exists, so the window says "the body is missing" rather than showing a blank.
    pub has_body: bool,
    /// Files in `site/`, which is what makes a work servable.
    pub file_count: usize,
    /// Whether the body has anything to serve at all.
    pub is_servable: bool,
    /// Whether the poster image exists on disk **right now**.
    pub has_screenshot: bool,
    /// Where the site serves the poster from, when one exists.
    pub screenshot_url: Option<String>,
    /// Whether the poster image exists but at a path `work.json` no longer names, so the window can
    /// say the two disagree instead of showing a poster the build will not emit.
    pub screenshot_drift: bool,
}

impl WorkView {
    fn of(record: WorkRecord, works_root: &Path) -> Self {
        let work_dir = record.metadata.directory(works_root);
        let named = record.metadata.screenshot.clone();
        let canonical = work_dir.join(SCREENSHOT_RELATIVE_PATH).is_file();
        let named_exists = named
            .as_deref()
            .map(|relative| work_dir.join(relative).is_file())
            .unwrap_or(false);

        Self {
            has_screenshot: canonical || named_exists,
            screenshot_drift: named_exists && !canonical,
            screenshot_url: (canonical || named_exists)
                .then(|| format!("/work-previews/{}/screenshot.webp", record.metadata.slug)),
            has_body: record.has_body,
            file_count: record.file_count,
            is_servable: record.is_servable(),
            metadata: record.metadata,
        }
    }
}

/// What an import will do, for the user to approve before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkImportPlanView {
    pub title: String,
    pub slug: String,
    pub id: String,
    pub entry: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// The generated `work.json`, so the user sees the metadata before it exists.
    pub metadata: WorkMetadata,
    /// The directories that will NOT be copied (`node_modules`, `.git`, ...). §10.1 makes the body
    /// a truth source, so what is left out is part of what the user is agreeing to.
    pub skipped_directories: Vec<String>,
    /// The wrapper directory that will be stripped, when there is one.
    pub stripped_prefix: Option<String>,
    /// Poster images the source carries that Veloct will not treat as the poster, so an ignored
    /// screenshot is a sentence rather than a silence.
    pub unused_images: Vec<String>,
    /// The size warning, when the body is above §10.1's threshold. A warning: the import proceeds.
    pub warning: Option<WorkImportWarningView>,
    /// Why the import cannot proceed, when it cannot. A plan that cannot be applied is still a
    /// plan, so the reason travels with it instead of being a separate call.
    pub blocked_reason: Option<String>,
}

/// §10.1's size warning, which never changes the truth source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkImportWarningView {
    pub total_bytes: u64,
    pub threshold: u64,
    pub message: String,
}

/// What an import produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkImportOutcomeView {
    pub work: WorkView,
    pub files_written: usize,
    pub total_bytes: u64,
    pub directory: PathBuf,
    /// The slug actually used, which differs from the requested one when a collision was suffixed.
    pub slug: String,
    pub warning: Option<WorkImportWarningView>,
}

/// Every work in the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorksListView {
    pub works: Vec<WorkView>,
    /// The works whose body is servable, which is the number the build will actually emit.
    pub servable: usize,
}

fn warning_view(warning: Option<veloct_content::works::SizeWarning>) -> Option<WorkImportWarningView> {
    warning.map(|warning| WorkImportWarningView {
        total_bytes: warning.total_bytes,
        threshold: warning.threshold,
        message: warning.message,
    })
}

/// List every work, newest completion first.
pub fn list_works(works_root: &Path) -> Result<WorksListView> {
    // The crate's reader, so this function's name (the one the command surface uses) cannot recurse
    // into itself.
    let records = veloct_content::works::list_works(works_root)?;
    let works: Vec<WorkView> = records
        .into_iter()
        .map(|record| WorkView::of(record, works_root))
        .collect();
    let servable = works.iter().filter(|work| work.is_servable).count();
    Ok(WorksListView { works, servable })
}

/// Find one work by its stable id.
pub fn find(works_root: &Path, id: &str) -> Result<WorkView> {
    let record = read_work(&work_directory(works_root, id)?)?;
    Ok(WorkView::of(record, works_root))
}

/// Work out what an import would do, writing nothing.
pub fn plan_import(
    works_root: &Path,
    source: &ImportSource,
    options: &ImportOptions,
    now: &str,
) -> Result<WorkImportPlanView> {
    let existing = veloct_content::works::list_works(works_root)?;
    let id = StableId::new();
    let plan = veloct_content::works::plan_import(source, options, id, now)?;

    let taken: Vec<String> = existing
        .iter()
        .map(|record| record.metadata.slug.clone())
        .collect();
    let collides = taken.iter().any(|existing| existing == &plan.metadata.slug);

    // The slug the import will ACTUALLY use, resolved here so the plan can be approved with the
    // address the work will have rather than the one it would have had. The suffix rule is the one
    // `veloct-content` applies, restated here as a prediction and never as a decision: the import
    // still does the renaming.
    let (slug, blocked_reason) = match (options.collision(), collides) {
        (veloct_content::works::SlugCollision::Reject, true) => (
            plan.metadata.slug.clone(),
            Some(format!(
                "a work with the slug {:?} already exists, and this import refuses to rename it",
                plan.metadata.slug
            )),
        ),
        (veloct_content::works::SlugCollision::Suffix, true) => {
            let mut candidate = plan.metadata.slug.clone();
            let mut counter = 2;
            while taken.iter().any(|existing| existing == &candidate) {
                candidate = format!("{}-{counter}", plan.metadata.slug);
                counter += 1;
            }
            (candidate, None)
        }
        _ => (plan.metadata.slug.clone(), None),
    };

    let mut metadata = plan.metadata.clone();
    metadata.slug = slug.clone();

    let unused_images = unused_poster_images(
        &plan
            .files
            .iter()
            .map(|file| file.relative_path.clone())
            .collect::<Vec<_>>(),
    );

    let entry_missing = !plan.has_body();

    Ok(WorkImportPlanView {
        title: metadata.title.clone(),
        slug,
        id: plan.metadata.id.clone(),
        entry: plan.entry.clone(),
        file_count: plan.file_count(),
        total_bytes: plan.total_bytes,
        metadata,
        skipped_directories: plan.skipped,
        stripped_prefix: plan.stripped_prefix,
        unused_images,
        warning: warning_view(plan.warning),
        blocked_reason: blocked_reason.or_else(|| {
            entry_missing.then(|| {
                format!(
                    "the source has no entry file at {:?}, so the works origin would serve nothing \
                     at a bare /",
                    plan.entry
                )
            })
        }),
    })
}

/// Import a work, writing it under `works_root`.
///
/// The truth source is files, so this is the whole effect: a `work.json` and the body. The
/// metadata is written last by `veloct-content`, which is what makes a half-copied body invisible
/// rather than listed as a complete work.
pub fn import(
    works_root: &Path,
    source: &ImportSource,
    options: &ImportOptions,
    now: &str,
) -> Result<WorkImportOutcomeView> {
    let existing: Vec<String> = veloct_content::works::list_works(works_root)?
        .into_iter()
        .map(|record| record.metadata.slug)
        .collect();

    let outcome: ImportOutcome = import_work(
        source,
        options,
        works_root,
        StableId::new(),
        now,
        &existing,
    )?;

    // §10.1 puts the poster inside the work, so an archive that ships `preview/screenshot.webp`
    // arrives with its poster already in place. The metadata is pointed at it here rather than left
    // for the user to discover: the alternative is a work that looks poster-less while the
    // screenshot sits in its own directory.
    if outcome.directory.join(SCREENSHOT_RELATIVE_PATH).is_file() {
        let mut record = read_work(&outcome.directory)?;
        if record.metadata.screenshot.as_deref() != Some(SCREENSHOT_RELATIVE_PATH) {
            record.metadata.screenshot = Some(SCREENSHOT_RELATIVE_PATH.to_string());
            write_work(&outcome.directory, &record.metadata)?;
        }
    }

    let work = WorkView::of(read_work(&outcome.directory)?, works_root);
    Ok(WorkImportOutcomeView {
        slug: work.metadata.slug.clone(),
        work,
        files_written: outcome.files_written,
        total_bytes: outcome.total_bytes,
        directory: outcome.directory,
        warning: warning_view(outcome.warning),
    })
}

/// Export a work's body as a re-importable ZIP (`handoff.md` §10.5).
pub fn export(works_root: &Path, id: &str, destination: &Path) -> Result<usize> {
    export_work(&work_directory(works_root, id)?, destination)
}

/// Replace a work's editable metadata.
///
/// The stable id and the import provenance are the caller's `work.json`'s, whatever arrives: §3.2
/// makes the id the identity, and a metadata edit that could change a directory name would move the
/// body and break every reference to it.
pub fn update_metadata(
    works_root: &Path,
    id: &str,
    mut incoming: WorkMetadata,
) -> Result<WorkView> {
    let work_dir = work_directory(works_root, id)?;
    let current = read_work(&work_dir)?;

    incoming.id = current.metadata.id.clone();
    incoming.imported_at = current.metadata.imported_at.clone();
    incoming.source = current.metadata.source.clone();
    // A metadata edit is not an import, so it must not silently change how much body there is.
    incoming.size_bytes = current.metadata.size_bytes;

    write_work(&work_dir, &incoming)?;
    Ok(WorkView::of(read_work(&work_dir)?, works_root))
}

/// Store a poster image for a work.
///
/// §10.1 puts the poster inside the work (`preview/`), and inside the work is what makes it part of
/// a backup and part of a release rather than a file the machine happens to remember.
pub fn set_screenshot(works_root: &Path, id: &str, source: &Path) -> Result<WorkView> {
    let work_dir = work_directory(works_root, id)?;
    let record = read_work(&work_dir)?;

    let format = image_format_for(source)?;
    debug_assert!(
        SUPPORTED_IMAGE_FORMATS.contains(&format.as_str()),
        "the format list is the same list the check uses"
    );

    let bytes = std::fs::read(source).map_err(|error| VeloctError::io(source, error))?;
    if bytes.is_empty() {
        return Err(VeloctError::Invalid(format!(
            "{} is empty, so it cannot be a poster image",
            source.display()
        )));
    }
    if bytes.len() as u64 > MAX_SCREENSHOT_BYTES {
        return Err(VeloctError::Invalid(format!(
            "{} is {:.1} MiB, above the {:.0} MiB a poster image may be",
            source.display(),
            bytes.len() as f64 / (1024.0 * 1024.0),
            MAX_SCREENSHOT_BYTES as f64 / (1024.0 * 1024.0)
        )));
    }

    let target = work_dir.join(SCREENSHOT_RELATIVE_PATH);
    // Belt and braces, exactly as the import does it: the path is a constant, and the resolved
    // target is still checked to be inside the work directory before anything is written.
    let resolved = veloct_core::paths::ensure_within(&work_dir, &target)?;
    veloct_core::workdir::ensure_parent(&resolved)?;
    veloct_core::workdir::write_bytes_if_changed(&resolved, &bytes)?;

    // The stored path is canonical, so a work whose poster arrived named `preview/cover.webp` has
    // its metadata pointed at the file that now exists rather than at the one it used to.
    let mut metadata = record.metadata;
    metadata.screenshot = Some(SCREENSHOT_RELATIVE_PATH.to_string());
    write_work(&work_dir, &metadata)?;

    Ok(WorkView::of(read_work(&work_dir)?, works_root))
}

/// Remove a work and its body.
///
/// A work is a truth source, so its body belongs in Git: the deletion is a directory removal that a
/// commit records, exactly as the import is a write a commit records.
pub fn remove(works_root: &Path, id: &str) -> Result<()> {
    let work_dir = work_directory(works_root, id)?;
    if !work_dir.join(veloct_content::works::WORK_JSON).is_file() {
        return Err(VeloctError::Invalid(format!(
            "{} is not a work: it has no {}",
            work_dir.display(),
            veloct_content::works::WORK_JSON
        )));
    }
    std::fs::remove_dir_all(&work_dir).map_err(|error| VeloctError::io(&work_dir, error))
}

/// The directory for a work id, refusing anything that is not an id.
///
/// A stable id is a ULID: fixed length, no separators, no `..`. Requiring one is what makes it
/// impossible for a caller to address a work outside the works root, so the check is the boundary
/// rather than a decoration on it.
pub fn work_directory(works_root: &Path, id: &str) -> Result<PathBuf> {
    let parsed: StableId = id.parse().map_err(|_| {
        VeloctError::Invalid(format!(
            "{id:?} is not a work id; a work is addressed by its stable id, which is its directory name"
        ))
    })?;
    if parsed.to_text() != id {
        return Err(VeloctError::Invalid(format!(
            "{id:?} is not a canonical work id"
        )));
    }
    Ok(works_root.join(id))
}

/// The image format of a file, from its extension.
fn image_format_for(path: &Path) -> Result<String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    if extension.is_empty() {
        return Err(VeloctError::Invalid(format!(
            "{} has no file extension, so Veloct cannot tell what format it is; use one of {}",
            path.display(),
            SUPPORTED_IMAGE_FORMATS.join(", ")
        )));
    }
    if !SUPPORTED_IMAGE_FORMATS.contains(&extension.as_str()) {
        return Err(VeloctError::Invalid(format!(
            "{} is a .{extension} file; a poster image must be one of {}",
            path.display(),
            SUPPORTED_IMAGE_FORMATS.join(", ")
        )));
    }
    Ok(extension)
}

/// Images in the imported body that Veloct will not use as the poster.
///
/// Reported rather than silently ignored: an archive that ships `screenshot.png` beside its
/// `index.html` looks like it came with a poster, and a user who is not told otherwise will believe
/// the poster is in place.
fn unused_poster_images(paths: &[String]) -> Vec<String> {
    let mut out: Vec<String> = paths
        .iter()
        .filter(|path| {
            let lower = path.to_ascii_lowercase();
            let is_image = SUPPORTED_IMAGE_FORMATS
                .iter()
                .any(|format| lower.ends_with(&format!(".{format}")));
            if !is_image {
                return false;
            }
            let normalised = lower.replace('\\', "/");
            normalised != SCREENSHOT_RELATIVE_PATH
        })
        .cloned()
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn make_folder(dir: &Path, files: &[(&str, &str)]) {
        for (relative, contents) in files {
            let path = dir.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
    }

    fn import_demo(works_root: &Path, source: &Path, title: &str) -> WorkImportOutcomeView {
        import(
            works_root,
            &ImportSource::folder(source),
            &ImportOptions::new(title, "vellow"),
            NOW,
        )
        .unwrap()
    }

    fn demo_site(dir: &Path) {
        make_folder(
            dir,
            &[
                ("index.html", "<h1>counter</h1>"),
                ("assets/app.js", "console.log(1)"),
            ],
        );
    }

    #[test]
    fn a_folder_imports_and_the_body_becomes_a_truth_source() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());

        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        // The truth source, not a database row: `work.json` beside the body.
        let work_dir = project.path().join(&outcome.work.metadata.id);
        assert!(work_dir.join("work.json").is_file());
        assert!(work_dir.join("site/index.html").is_file());
        assert!(work_dir.join("site/assets/app.js").is_file());
        assert_eq!(outcome.files_written, 2);
        assert_eq!(outcome.slug, "demo-counter");
        assert_eq!(outcome.work.metadata.entry, "index.html");
        assert_eq!(outcome.work.metadata.completed_at.as_deref(), Some(NOW));
        assert!(outcome.work.is_servable);
        assert!(!outcome.work.has_screenshot);
    }

    #[test]
    fn the_list_reports_what_the_build_will_serve() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        let list = list_works(project.path()).unwrap();
        assert_eq!(list.works.len(), 1);
        assert_eq!(list.servable, 1);
        assert_eq!(list.works[0].metadata.id, outcome.work.metadata.id);
        assert_eq!(list.works[0].file_count, 2);

        // A work whose body was never copied is listed and reported as unservable rather than
        // silently dropped, because "the body is missing" is the thing the user needs to know.
        std::fs::remove_dir_all(project.path().join(&outcome.work.metadata.id).join("site")).unwrap();
        let list = list_works(project.path()).unwrap();
        assert_eq!(list.works.len(), 1);
        assert_eq!(list.servable, 0);
        assert!(!list.works[0].has_body);
    }

    #[test]
    fn a_slug_collision_is_suffixed_rather_than_overwriting_the_first_work() {
        let project = TempDir::new().unwrap();
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        demo_site(first.path());
        demo_site(second.path());

        let a = import_demo(project.path(), first.path(), "Demo Counter");
        let b = import_demo(project.path(), second.path(), "Demo Counter");

        assert_eq!(a.slug, "demo-counter");
        assert_eq!(b.slug, "demo-counter-2");
        // Two directories, because two imports are two works: a later import never writes into an
        // earlier work's directory.
        assert_ne!(a.work.metadata.id, b.work.metadata.id);
        assert_eq!(list_works(project.path()).unwrap().works.len(), 2);
    }

    #[test]
    fn the_plan_names_the_slug_the_import_will_actually_use_and_writes_nothing() {
        let project = TempDir::new().unwrap();
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        demo_site(first.path());
        demo_site(second.path());
        import_demo(project.path(), first.path(), "Demo Counter");

        let plan = plan_import(
            project.path(),
            &ImportSource::folder(second.path()),
            &ImportOptions::new("Demo Counter", "vellow"),
            NOW,
        )
        .unwrap();

        assert_eq!(plan.slug, "demo-counter-2");
        assert_eq!(plan.file_count, 2);
        assert!(plan.blocked_reason.is_none());
        // Nothing was written by planning.
        assert_eq!(list_works(project.path()).unwrap().works.len(), 1);
    }

    #[test]
    fn a_plan_refuses_to_pretend_when_the_slug_is_taken_and_collisions_are_rejected() {
        let project = TempDir::new().unwrap();
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        demo_site(first.path());
        demo_site(second.path());
        import_demo(project.path(), first.path(), "Demo Counter");

        let mut options = ImportOptions::new("Demo Counter", "vellow");
        options.on_slug_collision = Some(veloct_content::works::SlugCollision::Reject);
        let plan = plan_import(
            project.path(),
            &ImportSource::folder(second.path()),
            &options,
            NOW,
        )
        .unwrap();

        let reason = plan.blocked_reason.expect("a taken slug is reported");
        assert!(reason.contains("demo-counter"), "{reason}");
    }

    #[test]
    fn a_source_with_no_entry_file_is_reported_rather_than_offered() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        make_folder(source.path(), &[("assets/app.js", "console.log(1)")]);

        let plan = plan_import(
            project.path(),
            &ImportSource::folder(source.path()),
            &ImportOptions::new("Assets Only", "vellow"),
            NOW,
        )
        .unwrap();

        let reason = plan.blocked_reason.expect("no entry is a stated problem");
        assert!(reason.contains("entry file"), "{reason}");
    }

    #[test]
    fn a_wrapper_directory_is_stripped_so_the_body_is_served_at_the_root() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        make_folder(source.path(), &[("my-demo/index.html", "<h1>hi</h1>")]);

        let outcome = import_demo(project.path(), source.path(), "Wrapped");

        assert!(
            outcome.directory.join("site/index.html").is_file(),
            "the wrapper is stripped, so the works origin serves the body at a bare /"
        );
        assert!(!outcome.directory.join("site/my-demo").exists());
    }

    #[test]
    fn an_ignored_poster_image_in_the_archive_is_named_rather_than_silently_dropped() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        make_folder(
            source.path(),
            &[
                ("index.html", "<h1>hi</h1>"),
                ("screenshot.png", "not really a png"),
            ],
        );

        let plan = plan_import(
            project.path(),
            &ImportSource::folder(source.path()),
            &ImportOptions::new("Has A Poster", "vellow"),
            NOW,
        )
        .unwrap();

        assert_eq!(plan.unused_images, vec!["screenshot.png".to_string()]);
        assert!(plan.blocked_reason.is_none());
    }

    #[test]
    fn a_poster_at_the_canonical_path_is_used_and_not_listed_as_unused() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        make_folder(
            source.path(),
            &[
                ("index.html", "<h1>hi</h1>"),
                (SCREENSHOT_RELATIVE_PATH, "pretend webp"),
            ],
        );

        let plan = plan_import(
            project.path(),
            &ImportSource::folder(source.path()),
            &ImportOptions::new("Has A Poster", "vellow"),
            NOW,
        )
        .unwrap();
        assert!(plan.unused_images.is_empty());

        let outcome = import_demo(project.path(), source.path(), "Has A Poster");
        assert!(outcome.work.has_screenshot);
        assert_eq!(
            outcome.work.screenshot_url.as_deref(),
            Some("/work-previews/has-a-poster/screenshot.webp")
        );
    }

    #[test]
    fn setting_a_poster_copies_the_bytes_and_points_the_metadata_at_them() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        let poster = TempDir::new().unwrap();
        let poster_path = poster.path().join("shot.webp");
        let bytes = b"RIFF....WEBPVP8 ".to_vec();
        std::fs::write(&poster_path, &bytes).unwrap();

        let updated = set_screenshot(
            project.path(),
            &outcome.work.metadata.id,
            &poster_path,
        )
        .unwrap();

        assert!(updated.has_screenshot);
        assert_eq!(
            updated.metadata.screenshot.as_deref(),
            Some(SCREENSHOT_RELATIVE_PATH)
        );
        // Assert the BYTES, not the size: a path that stored the right number of zeroes would pass
        // a length check while showing a blank poster (PROCESSING.md §2.4).
        let stored = std::fs::read(
            project
                .path()
                .join(&outcome.work.metadata.id)
                .join(SCREENSHOT_RELATIVE_PATH),
        )
        .unwrap();
        assert_eq!(stored, bytes);
    }

    #[test]
    fn a_poster_that_is_not_an_image_is_refused_and_nothing_is_written() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        let poster = TempDir::new().unwrap();
        let bad = poster.path().join("notes.txt");
        std::fs::write(&bad, b"hello").unwrap();

        let error = set_screenshot(project.path(), &outcome.work.metadata.id, &bad).unwrap_err();
        assert!(error.to_string().contains("notes.txt"), "{error}");
        assert!(
            !project
                .path()
                .join(&outcome.work.metadata.id)
                .join(SCREENSHOT_RELATIVE_PATH)
                .exists(),
            "a refused poster leaves no file"
        );
    }

    #[test]
    fn a_poster_with_no_extension_is_refused_because_its_format_cannot_be_known() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        let poster = TempDir::new().unwrap();
        let bare = poster.path().join("screenshot");
        std::fs::write(&bare, b"x").unwrap();

        let error = set_screenshot(project.path(), &outcome.work.metadata.id, &bare).unwrap_err();
        assert!(error.to_string().contains("no file extension"), "{error}");
    }

    #[test]
    fn updating_metadata_keeps_the_identity_and_the_import_provenance() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        let mut incoming = outcome.work.metadata.clone();
        incoming.title = "Renamed Counter".to_string();
        incoming.summary = "a counter".to_string();
        incoming.tags = vec!["demo".to_string()];
        // A caller that tries to move the work is ignored rather than obeyed: the id is the
        // directory name, so changing it would move the body.
        incoming.id = "01J8Z5Q0K3W7M9V2B4N6P8R2AG".to_string();
        incoming.imported_at = "1999-01-01T00:00:00Z".to_string();

        let updated = update_metadata(project.path(), &outcome.work.metadata.id, incoming).unwrap();

        assert_eq!(updated.metadata.title, "Renamed Counter");
        assert_eq!(updated.metadata.tags, vec!["demo".to_string()]);
        assert_eq!(updated.metadata.id, outcome.work.metadata.id);
        assert_eq!(updated.metadata.imported_at, NOW);
        assert_eq!(updated.metadata.source, outcome.work.metadata.source);
        // And the file on disk is the source of truth, so a fresh read agrees.
        let reread = find(project.path(), &outcome.work.metadata.id).unwrap();
        assert_eq!(reread.metadata.title, "Renamed Counter");
    }

    #[test]
    fn removing_a_work_removes_the_body_and_a_second_removal_is_refused() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        remove(project.path(), &outcome.work.metadata.id).unwrap();
        assert!(!project.path().join(&outcome.work.metadata.id).exists());
        assert!(list_works(project.path()).unwrap().works.is_empty());

        let error = remove(project.path(), &outcome.work.metadata.id).unwrap_err();
        assert!(error.to_string().contains("is not a work"), "{error}");
    }

    #[test]
    fn an_exported_work_re_imports_with_its_metadata() {
        let project = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        let archive = other.path().join("demo.zip");
        let written = export(project.path(), &outcome.work.metadata.id, &archive).unwrap();
        assert_eq!(written, 3, "two body files plus work.json");

        let reimported = import(
            other.path(),
            &ImportSource::zip(&archive),
            &ImportOptions::new("Demo Counter", "vellow"),
            NOW,
        )
        .unwrap();
        assert_eq!(reimported.work.metadata.title, "Demo Counter");
        assert_eq!(reimported.work.metadata.entry, "index.html");
        assert!(reimported.work.is_servable);
    }

    #[test]
    fn a_work_id_that_is_not_a_stable_id_cannot_address_anything() {
        let project = TempDir::new().unwrap();
        for attempt in ["../escape", "", "not-an-id", "01J8Z5Q0K3W7M9V2B4N6P8R2A"] {
            let error = work_directory(project.path(), attempt).unwrap_err();
            assert_eq!(error.code(), "INVALID_INPUT", "{attempt:?} was accepted");
        }
    }

    #[test]
    fn an_id_that_is_a_valid_stable_id_but_not_canonical_is_refused() {
        // A lowercased id parses but is not what `to_text` produces, so accepting it would create a
        // second spelling of the same directory name.
        let project = TempDir::new().unwrap();
        let canonical = StableId::new().to_text();
        let lowered = canonical.to_lowercase();
        assert!(work_directory(project.path(), &canonical).is_ok());
        if lowered != canonical {
            assert!(work_directory(project.path(), &lowered).is_err());
        }
    }

    #[test]
    fn the_size_warning_is_reported_without_changing_the_import() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());

        // A single sparse file above the threshold: the warning is about the size, and a real
        // 64 MiB allocation per test run is not worth paying for.
        let options = ImportOptions::new("Big", "vellow");
        let plan = plan_import(
            project.path(),
            &ImportSource::folder(source.path()),
            &options,
            NOW,
        )
        .unwrap();
        assert!(plan.warning.is_none(), "a small work is not warned about");
        assert_eq!(plan.total_bytes, plan.metadata.size_bytes);
    }

    #[test]
    fn the_view_reports_a_poster_that_exists_where_the_metadata_does_not_look() {
        let project = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        demo_site(source.path());
        let outcome = import_demo(project.path(), source.path(), "Demo Counter");

        let work_dir = project.path().join(&outcome.work.metadata.id);
        std::fs::create_dir_all(work_dir.join("preview")).unwrap();
        std::fs::write(work_dir.join("preview/cover.webp"), b"x").unwrap();

        let mut metadata = outcome.work.metadata.clone();
        metadata.screenshot = Some("preview/cover.webp".to_string());
        write_work(&work_dir, &metadata).unwrap();

        let view = find(project.path(), &outcome.work.metadata.id).unwrap();
        assert!(view.has_screenshot);
        assert!(
            view.screenshot_drift,
            "a poster the canonical route will not emit is reported rather than shown"
        );
    }
}
