//! Project lifecycle.
//!
//! A Veloct project is a directory on disk that Veloct manages; the application itself lives
//! elsewhere (ADR-0001). Creating a project materialises the scaffold described in
//! `docs/spec/01-data-model.md` §1, records its identity in `.veloct/project.json`, and takes a
//! shared project lock while it works.
//!
//! Ownership: `content-app`. The full service surface (content, WIP, works, settings,
//! diagnostics, logs) is built on top of this type.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::workdir::{write_text_if_changed, ProjectRoot, WriteOutcome};
use veloct_core::{layout, ProjectLock, Result, StableId, VeloctError};

/// The identity record stored at `.veloct/project.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    /// Stable id of this project, so a move or rename does not confuse the app.
    pub id: StableId,
    /// Display name.
    pub name: String,
    /// Which bundled site template version the scaffold was created from (ADR-0001).
    pub site_template_version: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp of the most recent open.
    pub last_opened_at: String,
}

/// Options accepted when creating a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectOptions {
    pub name: String,
    /// Default author for imported works; editable per work afterwards.
    pub default_author: String,
}

impl Default for CreateProjectOptions {
    fn default() -> Self {
        Self {
            name: "My Site".to_string(),
            default_author: "Veloct".to_string(),
        }
    }
}

/// Handle to an opened project.
#[derive(Debug)]
pub struct ProjectService {
    root: ProjectRoot,
    record: ProjectRecord,
    _lock: ProjectLock,
}

impl ProjectService {
    /// Absolute path of the project directory.
    pub fn root(&self) -> &ProjectRoot {
        &self.root
    }

    /// The project identity record.
    pub fn record(&self) -> &ProjectRecord {
        &self.record
    }

    /// Open an existing project. Fails when the directory is not a Veloct project, so the UI
    /// never silently treats an arbitrary folder as one.
    pub fn open(path: impl AsRef<Path>, clock: &dyn veloct_core::Clock) -> Result<Self> {
        let root = ProjectRoot::new(path.as_ref());
        if !root.as_path().is_dir() {
            return Err(VeloctError::ProjectNotFound(root.as_path().to_path_buf()));
        }
        let record_path = root.at(&[layout::VELOCT_DIR, layout::PROJECT_FILE]);
        let raw = veloct_core::workdir::read_text_optional(&record_path)?.ok_or_else(|| {
            VeloctError::ProjectNotFound(root.as_path().to_path_buf())
        })?;
        let mut record: ProjectRecord = serde_json::from_str(&raw)?;

        let lock = ProjectLock::shared(root.as_path(), "open-project")?;
        record.last_opened_at = veloct_core::clock::format_rfc3339(clock.now());
        let serialised = serde_json::to_string_pretty(&record)?;
        // Recording the open is a real write, so it goes through the atomic if-changed writer.
        let _ = write_text_if_changed(&record_path, &serialised)?;

        Ok(Self {
            root,
            record,
            _lock: lock,
        })
    }

    /// Create a new project scaffold in `path`.
    ///
    /// `site_template` is the directory of the bundled Astro template in the application. Its
    /// files are copied so the project is self-contained and reproducible.
    pub fn create(
        path: impl AsRef<Path>,
        options: CreateProjectOptions,
        site_template: &Path,
        site_template_version: &str,
        clock: &dyn veloct_core::Clock,
    ) -> Result<Self> {
        let root = ProjectRoot::new(path.as_ref());
        let veloct_dir = root.veloct_dir();

        if veloct_dir.join(layout::PROJECT_FILE).exists() {
            return Err(VeloctError::Invalid(format!(
                "{} is already a Veloct project",
                root.as_path().display()
            )));
        }

        // Exclusive while the scaffold is written: nothing else may touch a half-built project.
        let lock = ProjectLock::exclusive(root.as_path(), "create-project")?;

        for directory in [
            root.content_dir().join("articles"),
            root.content_dir().join("pages"),
            root.knowledge_dir(),
            root.works_dir(),
            root.dist_dir(),
            root.at(&[layout::DATA_DIR]),
            veloct_dir.join(layout::RELEASES_DIR),
            veloct_dir.join(layout::LOGS_DIR),
            root.at(&[layout::CACHE_DIR]),
        ] {
            std::fs::create_dir_all(&directory)
                .map_err(|e| VeloctError::io(&directory, e))?;
        }

        // The Astro project lives inside the managed project so it is versioned with it.
        if site_template.is_dir() {
            veloct_core::workdir::copy_dir_all(site_template, &root.site_dir(), |path, is_dir| {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                is_dir && matches!(name, "node_modules" | "dist" | ".astro" | ".git")
            })?;
        }

        let now = veloct_core::clock::format_rfc3339(clock.now());
        let record = ProjectRecord {
            id: StableId::new(),
            name: options.name.clone(),
            site_template_version: site_template_version.to_string(),
            created_at: now.clone(),
            last_opened_at: now,
        };
        let record_path = root.at(&[layout::VELOCT_DIR, layout::PROJECT_FILE]);
        std::fs::write(&record_path, serde_json::to_string_pretty(&record)?)
            .map_err(|e| VeloctError::io(&record_path, e))?;

        // Site configuration lives in SQLite; `data/site.json` is the build-time export.
        let site_json = serde_json::json!({
            "name": options.name,
            "default_author": options.default_author,
        });
        let _: WriteOutcome = write_text_if_changed(
            &root.at(&[layout::DATA_DIR, "site.json"]),
            &serde_json::to_string_pretty(&site_json)?,
        )?;

        let gitignore = root.as_path().join(".gitignore");
        write_text_if_changed(
            &gitignore,
            &format!(
                "# Build output is regenerated; the current dist is shipped with backups, not commits.\n{}/{}\n{}/{}\n{}/{}\n",
                layout::DIST_DIR, "",
                layout::CACHE_DIR, "",
                layout::VELOCT_DIR, layout::LOGS_DIR,
            ),
        )?;

        ensure_base_pages_unlocked(root.as_path(), &record.created_at)?;
        ensure_default_article_unlocked(root.as_path(), &record.created_at)?;
        Ok(Self {
            root,
            record,
            _lock: lock,
        })
    }

    /// Add the bundled scaffold to an existing directory, never replacing a file.
    /// Unlike `create`, this also repairs a project whose identity already exists.
    pub fn initialize(
        path: impl AsRef<Path>,
        options: CreateProjectOptions,
        template: &Path,
        template_version: &str,
        clock: &dyn veloct_core::Clock,
    ) -> Result<ProjectRecord> {
        let root = ProjectRoot::new(path.as_ref());
        if !root.as_path().is_dir() {
            return Err(VeloctError::ProjectNotFound(root.as_path().to_path_buf()));
        }
        for required in ["site/package.json", "site/astro.config.mjs", "data/site.json", "data/links.json"] {
            if !template.join(required).is_file() {
                return Err(VeloctError::Invalid(format!("bundled project template is incomplete: missing {required}")));
            }
        }
        let mut files = Vec::new();
        collect_template_files(template, template, &mut files)?;
        check_destination(root.as_path(), Path::new("veloct.lock"))?;
        let _lock = ProjectLock::exclusive(root.as_path(), "initialize-project")?;
        // Preflight the whole destination before writing. Never follow project links outside its root.
        for relative in &files {
            check_destination(root.as_path(), relative)?;
            if root.as_path().join(relative).is_dir() {
                return Err(VeloctError::Invalid(format!("expected a file at {}", relative.display())));
            }
        }
        for relative in [".veloct/project.json", ".gitignore", "content/articles", "content/pages", "content/knowledge", "works"] {
            check_destination(root.as_path(), Path::new(relative))?;
        }
        for relative in ["content/articles", "content/pages", "content/knowledge", "works", ".veloct"] {
            let directory = root.as_path().join(relative);
            if directory.exists() && !directory.is_dir() {
                return Err(VeloctError::Invalid(format!("expected a directory at {}", directory.display())));
            }
        }
        let mut config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(template.join("data/site.json"))
                .map_err(|e| VeloctError::io(template.join("data/site.json"), e))?
        )?;
        if !config.is_object() {
            return Err(VeloctError::Invalid("bundled site configuration must be an object".into()));
        }
        let record_path = root.at(&[layout::VELOCT_DIR, layout::PROJECT_FILE]);
        let existing_record = veloct_core::workdir::read_text_optional(&record_path)?;
        let now = veloct_core::clock::format_rfc3339(clock.now());
        let record: ProjectRecord = match existing_record {
            Some(raw) => serde_json::from_str(&raw)?,
            None => ProjectRecord {
                id: StableId::new(), name: options.name.clone(),
                site_template_version: template_version.to_string(),
                created_at: now.clone(), last_opened_at: now,
            },
        };
        config["name"] = serde_json::json!(record.name);
        config["author"] = serde_json::json!({ "name": options.default_author });
        for relative in &files {
            let source = template.join(relative);
            let destination = root.as_path().join(relative);
            let bytes = if relative == Path::new("data/site.json") {
                serde_json::to_vec_pretty(&config)?
            } else {
                std::fs::read(&source).map_err(|e| VeloctError::io(&source, e))?
            };
            write_missing(&destination, &bytes)?;
        }
        for relative in ["content/articles", "content/pages", "content/knowledge", "works", ".veloct"] {
            let directory = root.as_path().join(relative);
            std::fs::create_dir_all(&directory).map_err(|e| VeloctError::io(&directory, e))?;
        }
        write_missing(&record_path, &serde_json::to_vec_pretty(&record)?)?;
        write_missing(&root.as_path().join(".gitignore"), b"dist/\n.attachments-cache/\n.veloct/logs/\nsite/node_modules/\nsite/dist/\nsite/.astro/\nsite/.veloct-generated/\n")?;
        ensure_base_pages_unlocked(root.as_path(), &record.created_at)?;
        Ok(record)
    }

    /// Materialise the four editable built-in pages at runtime, without changing existing content.
    /// Listing documents may call this repeatedly; no identity, timestamp or body is rewritten.
    pub fn ensure_base_pages(path: impl AsRef<Path>, now: &str) -> Result<()> {
        // Serialise concurrent IPC list requests in this process. A shared project lock still allows
        // editing/preview and prevents a publish from replacing the tree while pages are created.
        static ENSURE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        let _guard = ENSURE_LOCK.lock();
        let root = path.as_ref();
        if !root.is_dir() {
            return Err(VeloctError::ProjectNotFound(root.to_path_buf()));
        }
        let _lock = ProjectLock::shared(root, "ensure-base-pages")?;
        ensure_base_pages_unlocked(root, now)
    }

    /// Upgrade only byte-identifiable official templates. Custom templates are left in place and
    /// reported to the caller. Every replaced file is backed up before the atomic write.
    pub fn upgrade_builtin_templates(path: impl AsRef<Path>, template: &Path) -> Result<Vec<String>> {
        let root = path.as_ref();
        // Opening a foreign/empty folder must not implicitly initialise an Astro project.
        if !root.join("site/package.json").is_file() {
            return Ok(Vec::new());
        }
        let _lock = ProjectLock::exclusive(root, "upgrade-builtin-templates")?;
        let mut warnings = Vec::new();
        for (relative, baseline) in TEMPLATE_BASELINES {
            let relative = Path::new(relative);
            check_destination(root, relative)?;
            let destination = root.join(relative);
            let source = template.join(relative);
            let Some(current) = veloct_core::workdir::read_text_optional(&source)? else {
                warnings.push(format!("内置模板缺少 {}，未升级。", relative.display()));
                continue;
            };
            let existing = veloct_core::workdir::read_text_optional(&destination)?;
            match existing {
                Some(ref old) if template_text(old) == template_text(&current) => continue,
                Some(ref old) if !known_template(relative, old, baseline) => {
                    warnings.push(format!("保留自定义模板 {}；此模板需手动合并更新，才能使用最新内置模板功能（包括基础页面、站点信息和网页转场）。", relative.display()));
                    continue;
                }
                Some(ref old) => {
                    let backup = Path::new(".veloct/template-backups").join(veloct_core::sha256_hex_str(old)).join(relative);
                    check_destination(root, &backup)?;
                    write_missing(&root.join(backup), old.as_bytes())?;
                }
                None => {}
            }
            write_text_if_changed(&destination, &current)?;
        }
        Ok(warnings)
    }

    /// Path of the SQLite database.
    pub fn db_path(&self) -> PathBuf {
        self.root.db_path()
    }
}

const TEMPLATE_BASELINES: &[(&str, &str)] = &[
    ("site/src/pages/index.astro", include_str!("../template-baselines/src/pages/index.astro")),
    ("site/src/pages/about/index.astro", include_str!("../template-baselines/src/pages/about/index.astro")),
    ("site/src/pages/links/index.astro", include_str!("../template-baselines/src/pages/links/index.astro")),
    ("site/src/pages/archive/index.astro", include_str!("../template-baselines/src/pages/archive/index.astro")),
    ("site/src/lib/site-config.mjs", include_str!("../template-baselines/src/lib/site-config.mjs")),
    ("site/src/components/Hero.astro", include_str!("../template-baselines/src/components/Hero.astro")),
    ("site/src/components/LeftSidebar.astro", include_str!("../template-baselines/src/components/LeftSidebar.astro")),
    ("site/src/components/SiteFooter.astro", include_str!("../template-baselines/src/components/SiteFooter.astro")),
    ("site/src/layouts/BaseLayout.astro", include_str!("../template-baselines/src/layouts/BaseLayout.previous.astro")),
    ("site/src/styles/global.css", include_str!("../template-baselines/src/styles/global.previous.css")),
    ("site/src/components/RightSidebar.astro", ""),
    ("site/src/components/LazyImage.astro", ""),
    ("site/src/pages/docs/[...path].astro", ""),
    ("site/src/scripts/knowledge-cards.js", ""),
    ("site/src/components/Comments.astro", ""),
    // New helper has no official old version: an existing custom helper must never be replaced.
    ("site/src/lib/pages.ts", ""),
    ("site/src/components/SocialIcon.astro", ""),
    ("site/src/lib/social-platforms.ts", ""),
    ("site/src/pages/docs/index.astro", include_str!("../template-baselines/src/pages/docs/index.astro")),
    ("site/src/pages/wip/[id].astro", ""),
    ("site/src/components/LinkedKnowledgeTree.astro", ""),
    ("site/src/lib/linked-knowledge.mjs", ""),
    ("site/src/components/ProfileHome.astro", include_str!("../template-baselines/src/components/ProfileHome.before-switch.astro")),
    ("site/src/components/ActivityHeatmap.astro", include_str!("../template-baselines/src/components/ActivityHeatmap.before-switch.astro")),
    ("site/src/lib/profile-home.mjs", include_str!("../template-baselines/src/lib/profile-home.before-switch.mjs")),
    ("site/src/styles/profile-home.css", include_str!("../template-baselines/src/styles/profile-home.before-switch.css")),
];

fn template_text(text: &str) -> String {
    // Git checkout line endings are not user customisations; all other text must match exactly.
    text.replace("\r\n", "\n")
}

// SHA-256 of complete official UTF-8 text with CRLF normalized to LF. These are immutable
// release baselines, not fingerprints of selected markers or user-editable fragments.
const TEMPLATE_BASELINE_HASHES: &[(&str, &str)] = &[
    // Complete official annual-chart templates before moving the shared toggle beside the title.
    ("site/src/components/ProfileHome.astro", "b55fc5ac787f2e849622ff67b5b99f8293eddb2548fb81b2723a1742ec87a7be"),
    ("site/src/components/ActivityHeatmap.astro", "6ca019c471fdb930c6cb314de8118eb639d345f9debbe4b37a99dad3d6007461"),
    ("site/src/styles/profile-home.css", "c8a31f20fb7ba01a57ff815f2db2dc8fe8adb2255f3380567fda022dfa19c930"),
    // Previous single-chart release used a rolling 90-day window; owner restored the original year.
    ("site/src/lib/profile-home.mjs", "aea8fd909402035a8c84c67a312262e2967b1bea396c4cfe7750066c5f7d3503"),
    ("site/src/components/ActivityHeatmap.astro", "9ba7770223f9d57dd9d3078577054fd7f64705bc5dd84478ebb2780d4fa8fd2e"),
    ("site/src/styles/profile-home.css", "1c03a7dcf96565c9f8fd5d93cf2afd9ae14b2400dfdabbce16f345545a731705"),
    // Earlier managed-home/client-router/empty-brand variants missed by previous release ledgers.
    // Exact complete text only; never infer official status from a few matching template markers.
    ("site/src/pages/index.astro", "dceb3bb8ca19ee135efffacd1ffb714669a50897c663678155c5c353d2734e1c"),
    ("site/src/layouts/BaseLayout.astro", "1a4d3d56ec4fea98dc76b0e54e5346fe40191fa61ed1e0b808e62cc4e113973b"),
    ("site/src/lib/site-config.mjs", "cc5b5cfe9e72f73ce6c359f9026be5470f0a0c724754618a2e2e66d01628b4ff"),
    // Complete official files shipped in the blank-home round, before the profile-home redesign.
    ("site/src/pages/index.astro", "fd9183efea2f17bc36e04cd1125aac35f2219885c152c87f3f5c400ea94d96c1"),
    ("site/src/layouts/BaseLayout.astro", "5eb8d7f06ef80b6897f1754737d17e1fa5ce5914bbc3a01a0fd962a214ec3393"),
    ("site/src/lib/site-config.mjs", "085641799285a9a431c1bb555b888057e58f26359530a51cf2b6e6c080396562"),
    // Last official home and layout before the owner requested an empty home canvas.
    ("site/src/pages/index.astro", "9e30e762bf85265c48d8a71de6a885e443398b70e52073ec487be6412066aa53"),
    ("site/src/layouts/BaseLayout.astro", "d1e912891e1dda225d4e6c55b9ac221dc22c46eb1d0eda227bf8059712c55dab"),
    // Official WIP route before preview-only drafts were included in getStaticPaths.
    ("site/src/pages/wip/[id].astro", "6c4b59ffefd250c2a2374dfed10117c4cea5d9a416e5615ad5f69edf79c75c78"),
    ("site/src/scripts/knowledge-cards.js", "eb0ac5ae0c154a5f6ea7ffc07d28904b46a7500455e92d438b8c07e2a5ba884a"),
    ("site/src/components/Comments.astro", "be607d939d8fe20aef50cb49762801020c848c88035e5c9b832de1952fb81c1c"),
    // Last release, captured before the footer/flex-layout and filing-link update.
    ("site/src/layouts/BaseLayout.astro", "210a4b878c5f668a862101673c8989c7db99d5dfb86b644191b036d548e0bf16"),
    ("site/src/lib/site-config.mjs", "37a2b344f9990f3f552da2d476d325352f52feb36b9089f9839a34cdc145e6ec"),
    ("site/src/components/SiteFooter.astro", "56c007d0c655e26c85540b2320e872f3fea5db7242364d777db14b51f3a9a158"),
    ("site/src/layouts/BaseLayout.astro", "759149f5bc82aab6822eb668c8d7f1549f93e2c0537db3ee5b44061474165d0f"),
    ("site/src/styles/global.css", "b65b637a1f7f2f02e62d45e8c60ba23583fc6eed356b72d58bbd8c6518b81cdf"),
    ("site/src/styles/global.css", "fd67ecd060e731bb7c75856333daac2f6a55d909ee9168fc15ac086132ce3d4b"),
    ("site/src/components/RightSidebar.astro", "27f7d44a7d2c32b941d022db4dd5a90a955466883585f23a03f9587435a5e183"),
    ("site/src/components/LazyImage.astro", "cf6700c00fd71f8f5d66f283b50f84b40f99c7e4d3f2cfcb06c268cc3a7680ce"),
    ("site/src/pages/docs/[...path].astro", "e27caed9f8c36d48c9b4866a26993611d80b537713226a73b8120cfccb68ddbe"),
    ("site/src/components/LeftSidebar.astro", "db163433478ee6ae2f8c11d647b98057fc9d053db0656a18197a04e2c3a110f5"),
];

fn known_template(path: &Path, text: &str, baseline: &str) -> bool {
    let text = template_text(text);
    (!baseline.is_empty() && text == template_text(baseline)) ||
        (path == Path::new("site/src/lib/site-config.mjs") && text == template_text(include_str!("../template-baselines/src/lib/site-config.previous.mjs"))) ||
        (path == Path::new("site/src/pages/archive/index.astro") && text == template_text(include_str!("../template-baselines/src/pages/archive.previous.astro"))) ||
        TEMPLATE_BASELINE_HASHES.iter().any(|(relative, hash)| {
            path == Path::new(relative) && veloct_core::sha256_hex_str(&text) == *hash
        })
}

fn ensure_base_pages_unlocked(root: &Path, now: &str) -> Result<()> {
    use veloct_content::{ContentKind, ContentService, CreateOptions};
    check_destination(root, Path::new("content/pages"))?;
    let pages = root.join("content/pages");
    let mut slugs = std::collections::BTreeSet::new();
    collect_page_slugs(root, &pages, &mut slugs)?;
    let content = ContentService::open(root);
    for (slug, title, summary, body) in [
        ("home", "首页", "欢迎来到我的博客。", "欢迎来到我的博客。这里记录我的思考、学习和生活。\n"),
        ("about", "关于", "关于我和这个网站。", "你好，欢迎来到我的个人网站。\n\n你可以在页面管理中编辑这段介绍，写下自己的经历、兴趣和联系方式。\n"),
        ("links", "友情链接", "我的朋友们与值得访问的网站。", "这里收录朋友们的网站。欢迎交换友情链接。\n"),
        ("archive", "归档", "按时间回顾已经发布的文章。", "这里按时间整理所有已发布的文章，方便回顾与查找。\n"),
    ] {
        if slugs.contains(slug) {
            continue;
        }
        let mut options = CreateOptions::new(ContentKind::Page, title)
            .with_slug(slug).with_body(body);
        options.summary = Some(summary.to_string());
        content.create(&options, now, StableId::new())?;
    }
    Ok(())
}

/// Add the harmless deployment smoke-test article to newly created projects only.
/// Existing projects are not modified by this helper; the article is keyed by its stable slug.
fn ensure_default_article_unlocked(root: &Path, now: &str) -> Result<()> {
    use veloct_content::{ContentKind, ContentService, CreateOptions};
    let content = ContentService::open(root);
    if content.list(ContentKind::Article)?.into_iter().any(|reference| {
        content.read(reference).map(|document| document.slug() == "test-article").unwrap_or(false)
    }) {
        return Ok(());
    }
    let options = CreateOptions::new(ContentKind::Article, "测试文章")
        .with_slug("test-article")
        .with_body("这是一篇测试文章，当你能看到这行文字时代表网站已成功部署。\n")
        .start_as_wip(false);
    content.create(&options, now, StableId::new())?;
    Ok(())
}

// Look at all Markdown page files, including hand-written/nested ones consumed by Astro. Never
// create a second slug just because a legacy file name is not a Veloct stable id. Broken documents
// are surfaced rather than overwritten or guessed around.
fn collect_page_slugs(root: &Path, directory: &Path, slugs: &mut std::collections::BTreeSet<String>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory).map_err(|e| VeloctError::io(directory, e))? {
        let entry = entry.map_err(|e| VeloctError::io(directory, e))?;
        let path = entry.path();
        check_destination(root, path.strip_prefix(root).expect("page stays in project"))?;
        let kind = entry.file_type().map_err(|e| VeloctError::io(&path, e))?;
        if kind.is_dir() {
            collect_page_slugs(root, &path, slugs)?;
        } else if kind.is_file() && path.extension().and_then(|v| v.to_str()) == Some("md") {
            let source = veloct_core::workdir::read_text(&path)?;
            let document = veloct_markdown::parse::parse(&source)?;
            slugs.insert(document.frontmatter.slug);
        }
    }
    Ok(())
}

fn collect_template_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory).map_err(|e| VeloctError::io(directory, e))? {
        let entry = entry.map_err(|e| VeloctError::io(directory, e))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| VeloctError::io(&path, e))?;
        if kind.is_symlink() {
            return Err(VeloctError::Invalid(format!("template contains a symbolic link: {}", path.display())));
        }
        if kind.is_dir() {
            collect_template_files(root, &path, files)?;
        } else if kind.is_file() {
            files.push(path.strip_prefix(root).expect("template traversal stays inside root").to_path_buf());
        }
    }
    Ok(())
}

fn check_destination(root: &Path, relative: &Path) -> Result<()> {
    let mut path = root.to_path_buf();
    // Checking the root too prevents an explicitly opened link from redirecting initialization.
    let root_meta = std::fs::symlink_metadata(root).map_err(|e| VeloctError::io(root, e))?;
    if root_meta.file_type().is_symlink() {
        return Err(VeloctError::Invalid("initialization cannot follow a project symbolic link".into()));
    }
    let count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(VeloctError::Invalid(format!("initialization cannot follow a symbolic link: {}", path.display())));
            }
            Ok(meta) if index + 1 < count && !meta.is_dir() => {
                return Err(VeloctError::Invalid(format!("expected a directory at {}", path.display())));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(VeloctError::io(&path, error)),
        }
    }
    Ok(())
}

fn write_missing(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VeloctError::io(parent, e))?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(bytes).map_err(|e| VeloctError::io(path, e)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(VeloctError::io(path, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::new(time::macros::datetime!(2026-01-01 12:00:00 UTC))
    }

    #[test]
    fn template_upgrade_backs_up_official_files_and_preserves_custom_files() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let template = dir.path().join("template");
        std::fs::create_dir_all(project.join("site")).unwrap();
        std::fs::write(project.join("site/package.json"), "{}").unwrap();
        for (relative, baseline) in TEMPLATE_BASELINES {
            let source = template.join(relative);
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(&source, format!("updated {relative}\n")).unwrap();
            if !baseline.is_empty() {
                let destination = project.join(relative);
                std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
                std::fs::write(destination, baseline).unwrap();
            }
        }
        let custom = project.join("site/src/pages/links/index.astro");
        std::fs::write(&custom, "My custom template\n").unwrap();
        let warnings = ProjectService::upgrade_builtin_templates(&project, &template).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("links"));
        assert_eq!(std::fs::read_to_string(&custom).unwrap(), "My custom template\n");
        let (relative, baseline) = TEMPLATE_BASELINES[0];
        let upgraded = project.join(relative);
        assert_eq!(std::fs::read_to_string(&upgraded).unwrap(), format!("updated {relative}\n"));
        let backup = project.join(".veloct/template-backups").join(veloct_core::sha256_hex_str(baseline)).join(relative);
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), baseline);
        let modified = std::fs::metadata(&upgraded).unwrap().modified().unwrap();
        let repeated = ProjectService::upgrade_builtin_templates(&project, &template).unwrap();
        assert_eq!(repeated, warnings);
        assert_eq!(std::fs::metadata(&upgraded).unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn transition_template_upgrade_preserves_customizations_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let template = dir.path().join("template");
        std::fs::create_dir_all(project.join("site")).unwrap();
        std::fs::write(project.join("site/package.json"), "{}").unwrap();
        for (relative, _) in TEMPLATE_BASELINES {
            let source = template.join(relative);
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(&source, format!("new official {relative}\n")).unwrap();
        }
        for relative in [
            "site/src/layouts/BaseLayout.astro", "site/src/styles/global.css",
            "site/src/components/LeftSidebar.astro", "site/src/components/RightSidebar.astro",
            "site/src/components/LazyImage.astro", "site/src/pages/docs/[...path].astro",
            "site/src/components/SocialIcon.astro", "site/src/lib/social-platforms.ts",
            "site/src/scripts/knowledge-cards.js", "site/src/components/Comments.astro",
            "site/src/pages/docs/index.astro", "site/src/components/LinkedKnowledgeTree.astro",
            "site/src/lib/linked-knowledge.mjs",
        ] {
            let destination = project.join(relative);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            let baseline = TEMPLATE_BASELINES.iter().find(|(path, _)| *path == relative).unwrap().1;
            let customized = format!("{baseline}\n/* user customization */\n");
            std::fs::write(&destination, &customized).unwrap();
            let warnings = ProjectService::upgrade_builtin_templates(&project, &template).unwrap();
            assert!(warnings.iter().any(|warning| warning.contains(relative)), "{relative}");
            assert_eq!(std::fs::read_to_string(&destination).unwrap(), customized);
            assert!(!project.join(".veloct/template-backups").join(veloct_core::sha256_hex_str(&customized)).join(relative).exists());
            std::fs::remove_file(&destination).unwrap();
            ProjectService::upgrade_builtin_templates(&project, &template).unwrap();
            assert_eq!(std::fs::read_to_string(&destination).unwrap(), format!("new official {relative}\n"));
            if !baseline.is_empty() {
                let old = baseline.replace('\n', "\r\n");
                std::fs::write(&destination, &old).unwrap();
                assert!(ProjectService::upgrade_builtin_templates(&project, &template).unwrap().is_empty());
                let backup = project.join(".veloct/template-backups").join(veloct_core::sha256_hex_str(&old)).join(relative);
                assert_eq!(std::fs::read_to_string(backup).unwrap(), old);
            }
            let modified = std::fs::metadata(&destination).unwrap().modified().unwrap();
            assert!(ProjectService::upgrade_builtin_templates(&project, &template).unwrap().is_empty());
            assert_eq!(std::fs::metadata(&destination).unwrap().modified().unwrap(), modified);
        }
    }

    #[test]
    fn transition_template_historical_hashes_are_exact_and_path_scoped() {
        let css = include_str!("../template-baselines/src/styles/global.previous.css");
        let old_css = css.split("\n/* ==================================================================================================").next().unwrap();
        assert_eq!(veloct_core::sha256_hex_str(old_css), "fd67ecd060e731bb7c75856333daac2f6a55d909ee9168fc15ac086132ce3d4b");
        assert_eq!(veloct_core::sha256_hex_str(css), "b65b637a1f7f2f02e62d45e8c60ba23583fc6eed356b72d58bbd8c6518b81cdf");
        for old in [old_css, css] {
            assert!(known_template(Path::new("site/src/styles/global.css"), old, ""));
            assert!(known_template(Path::new("site/src/styles/global.css"), &old.replace('\n', "\r\n"), ""));
            assert!(!known_template(Path::new("site/src/layouts/BaseLayout.astro"), old, ""));
            assert!(!known_template(Path::new("site/src/styles/global.css"), &format!("{old}/* custom */"), ""));
        }
        let layout = include_str!("../template-baselines/src/layouts/BaseLayout.previous.astro");
        assert!(!layout.contains("ClientRouter"));
        assert!(layout.contains("data-page-transition"));
        assert_eq!(veloct_core::sha256_hex_str(layout), "2e150e5755ed7335813662b835126757360b2e1e00d164997cd8f47e8ed3a197");
        let mut original = layout.replace("import { readSiteConfig } from '../lib/site-config.mjs';\n", "");
        let config_start = original.find("\n\n// Read once, here,").unwrap();
        let config_end = config_start + original[config_start..].find("\n---").unwrap();
        original.replace_range(config_start..config_end, "");
        original = original.replace("<html lang=\"zh-CN\" data-page-transition={site.pageTransition}>", "<html lang=\"zh-CN\">");
        let editor_start = original.find("    <script is:inline>\n      // The desktop editor").unwrap();
        let editor_end = editor_start + original[editor_start..].find("    </script>\n").unwrap() + "    </script>\n".len();
        original.replace_range(editor_start..editor_end, "");
        assert_eq!(veloct_core::sha256_hex_str(&original), "759149f5bc82aab6822eb668c8d7f1549f93e2c0537db3ee5b44061474165d0f");
        assert!(known_template(Path::new("site/src/layouts/BaseLayout.astro"), &original, ""));
        assert!(!known_template(Path::new("site/src/layouts/BaseLayout.astro"), &format!("{original}<!-- user edit -->"), ""));
    }

    // Reconstruct the last release from frozen older fixtures, never from the editable site tree.
    fn last_release_filing_templates() -> [(&'static str, String, &'static str); 3] {
        let layout = template_text(include_str!("../template-baselines/src/layouts/BaseLayout.previous.astro"))
            .replace("import Seo from", "import { ClientRouter } from 'astro:transitions';\nimport Seo from")
            .replace("  <head>\n", "  <head>\n    <ClientRouter fallback=\"animate\" />\n")
            .replace("<script is:inline>", "<script is:inline data-astro-rerun>")
            .replace("      if (new URLSearchParams(window.location.search).get('veloct_editor') === '1') {",
                "      (() => {\n        if (new URLSearchParams(window.location.search).get('veloct_editor') !== '1') return;")
            .replace("          window.addEventListener('message', (event) => {",
                "          window.__veloctEditorCleanup?.();\n          const receive = (event) => {")
            .replace("          });\n          const send =", "          };\n          window.addEventListener('message', receive);\n          window.__veloctEditorCleanup = () => window.removeEventListener('message', receive);\n          const send =")
            .replace("        else enableEditor();\n      }", "        else enableEditor();\n      })();");
        let config = template_text(include_str!("../template-baselines/src/lib/site-config.previous.mjs"))
            .replace("    name: 'Veloct',", "    name: '',")
            .replace("    title: 'Veloct',", "    // Empty means Hero follows the site name rather than a hard-coded template brand.\n    title: '',")
            .replace("  },\n};", "  },\n  // The effect the site uses between two navigations. This is CONFIGURATION rather than a local\n  // preference: the thing that animates is the reader's browser, so it has to travel with the build.\n  // `rise` settles the new page up into place, `dissolve` cross-fades, `none` switches instantly.\n  pageTransition: 'rise',\n};")
            .replace("/** Merge `override` onto `base`, one level deep for plain objects. */",
                "/** The three effects the template knows how to draw, in the order the form offers them. */\nexport const PAGE_TRANSITIONS = ['rise', 'dissolve', 'none'];\n\n/** Merge `override` onto `base`, one level deep for plain objects. */")
            .replace("  return config;", "  // Refused rather than defaulted, for the reason the whole file exists: a build that quietly\n  // ignored the setting would publish a site whose behaviour differs from its configuration.\n  if (!PAGE_TRANSITIONS.includes(config.pageTransition)) {\n    throw new Error(`pageTransition must be one of ${PAGE_TRANSITIONS.join(', ')}`);\n  }\n  return config;");
        let footer = template_text(include_str!("../template-baselines/src/components/SiteFooter.astro"))
            .replace("site.author?.name ?? site.name", "site.author?.name || site.name");
        [
            ("site/src/layouts/BaseLayout.astro", layout, "210a4b878c5f668a862101673c8989c7db99d5dfb86b644191b036d548e0bf16"),
            ("site/src/lib/site-config.mjs", config, "37a2b344f9990f3f552da2d476d325352f52feb36b9089f9839a34cdc145e6ec"),
            ("site/src/components/SiteFooter.astro", footer, "56c007d0c655e26c85540b2320e872f3fea5db7242364d777db14b51f3a9a158"),
        ]
    }

    #[test]
    fn last_release_filing_hashes_upgrade_exact_files_and_preserve_custom_files() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let template = dir.path().join("template");
        std::fs::create_dir_all(project.join("site")).unwrap();
        std::fs::write(project.join("site/package.json"), "{}").unwrap();
        for (relative, _) in TEMPLATE_BASELINES {
            let source = template.join(relative);
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(source, format!("new official {relative}\n")).unwrap();
        }
        // Populate every managed file once so later warnings can only come from custom test cases.
        assert!(ProjectService::upgrade_builtin_templates(&project, &template).unwrap().is_empty());
        for (relative, old, hash) in last_release_filing_templates() {
            assert_eq!(veloct_core::sha256_hex_str(&old), hash, "{relative}");
            assert_eq!(TEMPLATE_BASELINE_HASHES.iter().filter(|entry| **entry == (relative, hash)).count(), 1);
            assert!(known_template(Path::new(relative), &old, ""), "{relative}");
            assert!(!known_template(Path::new("site/src/lib/pages.ts"), &old, ""));
            let destination = project.join(relative);
            for official in [old.clone(), old.replace('\n', "\r\n")] {
                assert!(known_template(Path::new(relative), &official, ""));
                std::fs::write(&destination, &official).unwrap();
                assert!(ProjectService::upgrade_builtin_templates(&project, &template).unwrap().is_empty());
                assert_eq!(std::fs::read(&destination).unwrap(), std::fs::read(template.join(relative)).unwrap());
                let backup = project.join(".veloct/template-backups").join(veloct_core::sha256_hex_str(&official)).join(relative);
                assert_eq!(std::fs::read_to_string(backup).unwrap(), official);
                let modified = std::fs::metadata(&destination).unwrap().modified().unwrap();
                assert!(ProjectService::upgrade_builtin_templates(&project, &template).unwrap().is_empty());
                assert_eq!(std::fs::metadata(&destination).unwrap().modified().unwrap(), modified);
            }
            // Even one changed byte, removed newline, or appended whitespace is a customization.
            let mut changed_byte = old.clone();
            changed_byte.replace_range(0..1, "x");
            for customized in [changed_byte, old.trim_end_matches('\n').to_string(), format!("{old} "), format!("{old}<!-- user edit -->\n")] {
                assert!(!known_template(Path::new(relative), &customized, ""), "{relative}");
                std::fs::write(&destination, &customized).unwrap();
                let modified = std::fs::metadata(&destination).unwrap().modified().unwrap();
                let backup = project.join(".veloct/template-backups").join(veloct_core::sha256_hex_str(&customized)).join(relative);
                let warnings = ProjectService::upgrade_builtin_templates(&project, &template).unwrap();
                assert_eq!(warnings.len(), 1, "{warnings:?}");
                assert!(warnings[0].contains(relative));
                assert_eq!(std::fs::read_to_string(&destination).unwrap(), customized);
                assert_eq!(std::fs::metadata(&destination).unwrap().modified().unwrap(), modified);
                assert!(!backup.exists());
                assert_eq!(ProjectService::upgrade_builtin_templates(&project, &template).unwrap(), warnings);
            }
            std::fs::write(&destination, std::fs::read(template.join(relative)).unwrap()).unwrap();
        }
    }

    #[test]
    fn empty_home_upgrades_official_templates_but_preserves_content_and_customizations() {
        let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let old_home = template_text(include_str!("../template-baselines/src/pages/index.before-clear.astro"));
        // Reconstruct the exact preceding official layout, not a marker-based approximation.
        let old_layout = template_text(&std::fs::read_to_string(template.join("site/src/layouts/BaseLayout.astro")).unwrap())
            .replace("// A legacy/custom home may omit the new prop: never attach the old global sidebar to the home route.\nconst isHome = canonical === '/' || Astro.url.pathname.replace(/\\/+$/, '') === import.meta.env.BASE_URL.replace(/\\/+$/, '');\nconst renderRightSidebar = showRightSidebar && !isHome;\n", "")
            .replace("'author.bio': settings.author?.bio || '', 'description': settings.description || '',", "'author.bio': settings.author?.bio || '',")
            .replace("{renderRightSidebar && (", "{showRightSidebar && (")
            .replace("  /** A blank home canvas omits the sidebar entirely; every other page keeps the default. */\n  showRightSidebar?: boolean;\n", "")
            .replace("  showRightSidebar = true,\n", "")
            .replace("      {showRightSidebar && (\n        <RightSidebar\n          headings={headings}\n          tags={wordCloud}\n          heatmap={heatmap}\n          commentsEnabled={commentsEnabled}\n          isProduction={isProduction}\n        />\n      )}",
                "      <RightSidebar\n        headings={headings}\n        tags={wordCloud}\n        heatmap={heatmap}\n        commentsEnabled={commentsEnabled}\n        isProduction={isProduction}\n      />");
        let previous = [
            ("site/src/components/ProfileHome.astro", template_text(include_str!("../template-baselines/src/components/ProfileHome.bottom-toggle.astro")), "b55fc5ac787f2e849622ff67b5b99f8293eddb2548fb81b2723a1742ec87a7be"),
            ("site/src/components/ActivityHeatmap.astro", template_text(include_str!("../template-baselines/src/components/ActivityHeatmap.annual-bottom-toggle.astro")), "6ca019c471fdb930c6cb314de8118eb639d345f9debbe4b37a99dad3d6007461"),
            ("site/src/styles/profile-home.css", template_text(include_str!("../template-baselines/src/styles/profile-home.annual-bottom-toggle.css")), "c8a31f20fb7ba01a57ff815f2db2dc8fe8adb2255f3380567fda022dfa19c930"),
            ("site/src/lib/profile-home.mjs", template_text(&std::fs::read_to_string(template.join("site/src/lib/profile-home.mjs")).unwrap()).replace("dayCount = 365", "dayCount = 90"), "aea8fd909402035a8c84c67a312262e2967b1bea396c4cfe7750066c5f7d3503"),
            ("site/src/components/ActivityHeatmap.astro", template_text(include_str!("../template-baselines/src/components/ActivityHeatmap.annual-bottom-toggle.astro")).replace("365 天", "90 天"), "9ba7770223f9d57dd9d3078577054fd7f64705bc5dd84478ebb2780d4fa8fd2e"),
            ("site/src/styles/profile-home.css", template_text(include_str!("../template-baselines/src/styles/profile-home.annual-bottom-toggle.css")).replace(".activity-grid { display: block; min-width: 590px; width: 100%; height: auto; }", ".activity-grid { display: block; width: 100%; max-width: 420px; height: auto; margin-inline: auto; }"), "1c03a7dcf96565c9f8fd5d93cf2afd9ae14b2400dfdabbce16f345545a731705"),
            ("site/src/components/ProfileHome.astro", template_text(include_str!("../template-baselines/src/components/ProfileHome.before-switch.astro")), "813169e74790c6f01c39d544b1aba7a04fa1067f7d44340e32f760fc908ab36f"),
            ("site/src/components/ActivityHeatmap.astro", template_text(include_str!("../template-baselines/src/components/ActivityHeatmap.before-switch.astro")), "f7f18adf6d9ea1d5c61367c884bbc3c0139194b8a28cce3382674d3b4f1a467d"),
            ("site/src/lib/profile-home.mjs", template_text(include_str!("../template-baselines/src/lib/profile-home.before-switch.mjs")), "6f55ed7774ed0d5712da7e3176a85baf62898e50764d73fa8a893847e136d19a"),
            ("site/src/styles/profile-home.css", template_text(include_str!("../template-baselines/src/styles/profile-home.before-switch.css")), "fc3002a5570956cd7a6e92f2c10b3b7ac02e1f9c708eeece3d848b67790957c7"),
            ("site/src/pages/index.astro", template_text(include_str!("../template-baselines/src/pages/index.managed-legacy.astro")), "dceb3bb8ca19ee135efffacd1ffb714669a50897c663678155c5c353d2734e1c"),
            ("site/src/layouts/BaseLayout.astro", template_text(include_str!("../template-baselines/src/layouts/BaseLayout.client-router-legacy.astro")), "1a4d3d56ec4fea98dc76b0e54e5346fe40191fa61ed1e0b808e62cc4e113973b"),
            ("site/src/lib/site-config.mjs", template_text(include_str!("../template-baselines/src/lib/site-config.empty-brand-legacy.mjs")), "cc5b5cfe9e72f73ce6c359f9026be5470f0a0c724754618a2e2e66d01628b4ff"),
            ("site/src/pages/index.astro", old_home, "9e30e762bf85265c48d8a71de6a885e443398b70e52073ec487be6412066aa53"),
            ("site/src/pages/index.astro", template_text(include_str!("../template-baselines/src/pages/index.blank.astro")), "fd9183efea2f17bc36e04cd1125aac35f2219885c152c87f3f5c400ea94d96c1"),
            ("site/src/layouts/BaseLayout.astro", old_layout, "d1e912891e1dda225d4e6c55b9ac221dc22c46eb1d0eda227bf8059712c55dab"),
        ];
        for crlf in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let project = dir.path();
            std::fs::create_dir_all(project.join("site")).unwrap();
            std::fs::write(project.join("site/package.json"), "{}").unwrap();
            assert!(ProjectService::upgrade_builtin_templates(project, &template).unwrap().is_empty());
            for path in ["content/pages/home.md", "content/articles/article.md", "site/site.config.json"] {
                let destination = project.join(path);
                std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
                std::fs::write(destination, "owner content must remain byte-identical\r\n").unwrap();
            }
            for (relative, text, hash) in &previous {
                assert_eq!(veloct_core::sha256_hex_str(text), *hash, "{relative}");
                let original = if crlf { text.replace('\n', "\r\n") } else { text.clone() };
                std::fs::write(project.join(relative), &original).unwrap();
                assert!(ProjectService::upgrade_builtin_templates(project, &template).unwrap().is_empty());
                assert_eq!(std::fs::read(project.join(relative)).unwrap(), std::fs::read(template.join(relative)).unwrap());
                let backup = project.join(".veloct/template-backups").join(veloct_core::sha256_hex_str(&original)).join(relative);
                assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
                let modified = std::fs::metadata(project.join(relative)).unwrap().modified().unwrap();
                assert!(ProjectService::upgrade_builtin_templates(project, &template).unwrap().is_empty());
                assert_eq!(std::fs::metadata(project.join(relative)).unwrap().modified().unwrap(), modified);
                let custom = format!("{text}<!-- owner customization -->\n");
                std::fs::write(project.join(relative), &custom).unwrap();
                let warnings = ProjectService::upgrade_builtin_templates(project, &template).unwrap();
                assert!(warnings.iter().any(|message| message.contains(relative)));
                assert_eq!(std::fs::read_to_string(project.join(relative)).unwrap(), custom);
                std::fs::write(project.join(relative), std::fs::read(template.join(relative)).unwrap()).unwrap();
            }
            for path in ["content/pages/home.md", "content/articles/article.md", "site/site.config.json"] {
                assert_eq!(std::fs::read_to_string(project.join(path)).unwrap(), "owner content must remain byte-identical\r\n");
            }
        }
    }

    #[test]
    fn transition_template_upgrade_from_real_bundle_is_complete() {
        let template = std::env::var_os("VELOCT_TEST_TEMPLATE").map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        std::fs::create_dir_all(project.join("site")).unwrap();
        std::fs::write(project.join("site/package.json"), "{}").unwrap();
        for (relative, baseline) in TEMPLATE_BASELINES {
            assert!(template.join(relative).is_file(), "bundle missing {relative}");
            if !baseline.is_empty() {
                let destination = project.join(relative);
                std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
                std::fs::write(destination, baseline).unwrap();
            }
        }
        assert!(ProjectService::upgrade_builtin_templates(project, &template).unwrap().is_empty());
        for (relative, _) in TEMPLATE_BASELINES {
            assert_eq!(std::fs::read(project.join(relative)).unwrap(), std::fs::read(template.join(relative)).unwrap(), "{relative}");
        }
        assert!(ProjectService::upgrade_builtin_templates(project, &template).unwrap().is_empty());
        for (relative, from, to) in [
            ("site/src/scripts/knowledge-cards.js", "document.addEventListener('astro:page-load', resolveAll);", "resolveAll();"),
            ("site/src/components/Comments.astro", " data-astro-rerun", ""),
            ("site/src/pages/wip/[id].astro", "    // Preview-only drafts must have a dev route, but articleRoute returns None for them in builds.\n    if (route.kind === WipRoute.Wip || route.kind === WipRoute.PreviewOnly) {", "    if (route.kind === WipRoute.Wip) {"),
        ] {
            let current = std::fs::read_to_string(template.join(relative)).unwrap();
            let old = template_text(&current).replace(from, to);
            assert!(known_template(Path::new(relative), &old, ""), "{relative}");
            assert!(!known_template(Path::new(relative), &format!("{old}\n/* user edit */"), ""));
            let destination = project.join(relative);
            std::fs::write(&destination, &old).unwrap();
            assert!(ProjectService::upgrade_builtin_templates(project, &template).unwrap().is_empty());
            assert_eq!(std::fs::read(&destination).unwrap(), current.as_bytes());
            let backup = project.join(".veloct/template-backups").join(veloct_core::sha256_hex_str(&old)).join(relative);
            assert_eq!(std::fs::read_to_string(backup).unwrap(), old);
        }
        for relative in ["site/src/layouts/BaseLayout.astro", "site/src/styles/global.css", "site/src/pages/wip/[id].astro"] {
            let destination = project.join(relative);
            let customized = format!("{}\n/* user edit */", std::fs::read_to_string(&destination).unwrap());
            std::fs::write(&destination, &customized).unwrap();
            let warnings = ProjectService::upgrade_builtin_templates(project, &template).unwrap();
            assert!(warnings.iter().any(|warning| warning.contains(relative)));
            assert_eq!(std::fs::read_to_string(destination).unwrap(), customized);
        }
    }

    #[test]
    fn base_pages_are_idempotent_and_ordinary_editable_documents() {
        use veloct_content::{ContentKind, ContentService};
        let dir = tempfile::tempdir().unwrap();
        let now = "2026-01-01T12:00:00Z";
        ProjectService::ensure_base_pages(dir.path(), now).unwrap();
        let content = ContentService::open(dir.path());
        let before: Vec<_> = content.list(ContentKind::Page).unwrap().into_iter()
            .map(|reference| content.read(reference).unwrap()).collect();
        assert_eq!(before.len(), 4);
        let home = before.iter().find(|doc| doc.slug() == "home").unwrap();
        let edited = home.source.replace("欢迎来到我的博客。这里记录我的思考、学习和生活。", "这是用户编辑的首页正文。");
        let saved = content.save(ContentKind::Page, &home.reference.id_text(), &home.revision, &edited, "2026-02-01T00:00:00Z").unwrap();
        assert!(saved.is_success());
        let persisted = content.read(home.reference).unwrap();
        ProjectService::ensure_base_pages(dir.path(), "2026-03-01T00:00:00Z").unwrap();
        assert_eq!(content.list(ContentKind::Page).unwrap().len(), 4);
        for original in &before {
            let current = content.read(original.reference).unwrap();
            assert_eq!(current.source, if original.slug() == "home" { &persisted.source } else { &original.source }.to_string());
            assert!(!current.document.diagnostics.has_errors());
        }
        let routes: std::collections::BTreeMap<_, _> = content.ribbon().unwrap().into_iter()
            .map(|row| (row.slug, row.route)).collect();
        assert_eq!(routes["home"], "/");
        assert_eq!(routes["about"], "/about/");
        assert_eq!(routes["links"], "/links/");
        assert_eq!(routes["archive"], "/archive/");
    }

    #[test]
    fn base_pages_preserve_existing_page_and_non_page_content() {
        use veloct_content::{ContentKind, ContentService, CreateOptions};
        let dir = tempfile::tempdir().unwrap();
        let content = ContentService::open(dir.path());
        let now = "2025-01-01T00:00:00Z";
        let mut options = CreateOptions::new(ContentKind::Page, "自定义关于标题")
            .with_slug("about").with_body("我的已有介绍。\n");
        options.summary = Some("已有摘要".into());
        let about = content.create(&options, now, StableId::new()).unwrap();
        let article = content.create(&CreateOptions::new(ContentKind::Article, "Article").with_slug("home"), now, StableId::new()).unwrap();
        ProjectService::ensure_base_pages(dir.path(), "2026-01-01T00:00:00Z").unwrap();
        assert_eq!(content.read(about.reference).unwrap().source, about.source);
        assert_eq!(content.read(article.reference).unwrap().source, article.source);
        assert_eq!(content.list(ContentKind::Page).unwrap().len(), 4);
    }

    #[test]
    fn base_pages_do_not_duplicate_a_legacy_nested_slug() {
        use veloct_content::{ContentKind, ContentService, CreateOptions};
        let dir = tempfile::tempdir().unwrap();
        let content = ContentService::open(dir.path());
        let about = content.create(&CreateOptions::new(ContentKind::Page, "Legacy about").with_slug("about"), "2025-01-01T00:00:00Z", StableId::new()).unwrap();
        let legacy = dir.path().join("content/pages/legacy/about.md");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::rename(&about.path, &legacy).unwrap();
        ProjectService::ensure_base_pages(dir.path(), "2026-01-01T00:00:00Z").unwrap();
        assert_eq!(std::fs::read_to_string(&legacy).unwrap(), about.source);
        assert_eq!(content.list(ContentKind::Page).unwrap().len(), 3);
    }

    #[test]
    fn create_materialises_the_documented_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(template.join("src")).unwrap();
        std::fs::write(template.join("astro.config.mjs"), "export default {}").unwrap();
        std::fs::create_dir_all(template.join("node_modules")).unwrap();
        std::fs::write(template.join("node_modules").join("junk.js"), "x").unwrap();

        let project_dir = dir.path().join("project");
        let service = ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "test-template",
            &clock(),
        )
        .unwrap();

        assert!(project_dir.join(".veloct").join("project.json").exists());
        assert!(project_dir.join("content").join("articles").is_dir());
        assert!(project_dir.join("knowledge").is_dir());
        assert!(project_dir.join("works").is_dir());
        assert!(project_dir.join("data").join("site.json").exists());
        assert!(project_dir.join(".gitignore").exists());
        let content = veloct_content::ContentService::open(&project_dir);
        let articles = content.list(veloct_content::ContentKind::Article).unwrap();
        assert_eq!(articles.len(), 1);
        let smoke_test = content.read(articles[0]).unwrap();
        assert_eq!(smoke_test.title(), "测试文章");
        assert_eq!(smoke_test.slug(), "test-article");
        assert!(smoke_test.source.contains("这是一篇测试文章，当你能看到这行文字时代表网站已成功部署。"));
        assert!(!smoke_test.is_preview_only());
        // The site template is copied, minus build noise.
        assert!(project_dir.join("site").join("astro.config.mjs").exists());
        assert!(!project_dir.join("site").join("node_modules").exists());

        assert_eq!(service.record().name, "My Site");
        assert_eq!(service.record().site_template_version, "test-template");
        assert!(service.db_path().ends_with(".veloct\\veloct.db")
            || service.db_path().ends_with(".veloct/veloct.db"));
    }

    #[test]
    fn creating_over_an_existing_project_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");

        ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap();

        let error = ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("already a Veloct project"));
    }

    #[test]
    fn opening_a_foreign_directory_fails_instead_of_guessing() {
        let dir = tempfile::tempdir().unwrap();
        let error = ProjectService::open(dir.path(), &clock()).unwrap_err();
        assert_eq!(error.code(), "PROJECT_NOT_FOUND");
    }

    #[test]
    fn open_then_reopen_detects_the_project_and_updates_last_opened() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");

        ProjectService::create(
            &project_dir,
            CreateProjectOptions {
                name: "Vellow's Blog".into(),
                default_author: "vellow".into(),
            },
            &template,
            "v1",
            &clock(),
        )
        .unwrap();
        drop(ProjectService::open(&project_dir, &clock()).unwrap());

        let later = FixedClock::new(time::macros::datetime!(2026-02-02 08:00:00 UTC));
        let reopened = ProjectService::open(&project_dir, &later).unwrap();
        assert_eq!(reopened.record().name, "Vellow's Blog");
        assert_eq!(reopened.record().last_opened_at, "2026-02-02T08:00:00Z");
        assert_eq!(reopened.record().created_at, "2026-01-01T12:00:00Z");
    }

    #[test]
    fn creating_a_project_takes_an_exclusive_lock_against_publish() {
        // handoff §20.4: a working-tree-replacing operation must not race a scaffold.
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let publish = ProjectLock::exclusive(&project_dir, "publish").unwrap();
        let error = ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "EXCLUSIVE_BUSY");
        drop(publish);

        ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap();
    }

    #[test]
    fn open_uses_a_shared_lock_so_editing_and_preview_coexist() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");
        ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap();

        let first = ProjectService::open(&project_dir, &clock()).unwrap();
        let second = ProjectService::open(&project_dir, &clock()).unwrap();
        assert_eq!(first.record().id, second.record().id);

        // But an exclusive operation is refused while they are open.
        let error = ProjectLock::exclusive(&project_dir, "publish").unwrap_err();
        assert_eq!(error.code(), "LOCK_CONFLICT");
    }

    #[test]
    fn project_record_round_trips_through_json() {
        let record = ProjectRecord {
            id: StableId::new(),
            name: "Veloct".into(),
            site_template_version: "1.0.0".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            last_opened_at: "2026-01-02T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<ProjectRecord>(&json).unwrap(), record);
    }

    #[test]
    fn shared_lock_mode_permits_concurrent_readers() {
        // handoff §20.4: editing, preview and attachment upload run in parallel.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = ProjectLock::acquire(root, veloct_core::LockMode::Shared, "a").unwrap();
        let b = ProjectLock::acquire(root, veloct_core::LockMode::Shared, "b").unwrap();
        assert!(!ProjectLock::is_exclusive_busy(root));
        drop((a, b));
    }
}
