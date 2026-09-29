//! The AI page creator.
//!
//! `handoff.md` §8.1 is a five-step flow with one hard rule:
//!
//! ```text
//! AI generates a whole page -> the user previews it -> the user confirms -> the page is created
//! ```
//!
//! > Without confirmation, a page must not be officially created or overwritten.
//!
//! and §8.2 constrains the FORMAT:
//!
//! > A page the AI generates must output the Markdown / directives / frontmatter structures the project
//! > supports, and must not create a private JSON page format that only the AI understands.
//!
//! So the model is asked for Markdown, and the result is PARSED with the project's own parser rather
//! than trusted. That gives three things a JSON-shaped answer could not:
//!
//! * The preview the user sees is the real document, parsed by the real parser, so a page that previews
//!   correctly will save correctly.
//! * Structural problems — an unknown directive, a malformed table — are surfaced BEFORE the page
//!   exists, as diagnostics the user can read.
//! * Identity stays with Veloct. The model may propose a title, a slug, a summary, tags and a category;
//!   the `id`, `type` and timestamps are assigned here, because §3.2 makes the id permanent and a model
//!   that invented one would produce a document the content layer refuses.
//!
//! Image placeholders are part of the requested output and no image is ever generated (§8.1's
//! "will not actually generate image files"). A placeholder is a `:::image` directive, which is the
//! project's own construct, so the user replaces it through the attachment flow.

use serde::{Deserialize, Serialize};

use veloct_core::ids::StableId;
use veloct_core::{Result, VeloctError};
use veloct_markdown::directives;
use veloct_markdown::frontmatter::{ContentType, Frontmatter, Timestamps};
use veloct_markdown::rewrite;

use crate::error::{AiError, AiErrorKind};

/// What the model was asked to produce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    pub title: String,
    /// What the page is for, in the user's own words.
    pub brief: String,
    /// The page kind. A `page` is the ordinary case; an `article` starts as a WIP.
    pub kind: ContentKindChoice,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    /// Whether the page should include image placeholders (§8.1 asks for them).
    pub with_image_placeholders: bool,
}

/// Which collection the page lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKindChoice {
    /// An ordinary page, which has no WIP state (`handoff.md` §3.2).
    Page,
    /// An article, which starts as a WIP.
    Article,
    /// A knowledge entry.
    Knowledge,
}

impl ContentKindChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Article => "article",
            Self::Knowledge => "knowledge",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "page" => Some(Self::Page),
            "article" => Some(Self::Article),
            "knowledge" => Some(Self::Knowledge),
            _ => None,
        }
    }

    pub fn content_type(self) -> ContentType {
        match self {
            Self::Page => ContentType::Page,
            Self::Article => ContentType::Article,
            Self::Knowledge => ContentType::Knowledge,
        }
    }

    /// Whether a document of this kind starts as a WIP.
    pub fn starts_as_wip(self) -> bool {
        matches!(self, Self::Article)
    }
}

impl PageRequest {
    pub fn new(title: impl Into<String>, brief: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            brief: brief.into(),
            // A page is the ordinary case, because the page creator exists for pages; an article has its
            // own WIP lifecycle that the user drives.
            kind: ContentKindChoice::Page,
            tags: Vec::new(),
            category: None,
            with_image_placeholders: true,
        }
    }

    /// The instruction sent to the model.
    ///
    /// Written as a specification of the FORMAT rather than as a request for JSON, because §8.2 forbids
    /// a private format and because asking for Markdown is what makes the answer parseable by the project
    /// parser.
    pub fn prompt(&self) -> String {
        let mut prompt = String::new();
        prompt.push_str(
            "You are writing a document for a Markdown-based website. Answer with ONE Markdown document \
             and nothing else: no explanation before it, no code fence around it.\n\n",
        );
        prompt.push_str("Start with a YAML frontmatter block delimited by --- lines, containing exactly:\n");
        prompt.push_str("  title: <the page title>\n");
        prompt.push_str("  slug: <a short url-safe slug, lowercase, hyphens>\n");
        prompt.push_str("  summary: <one sentence>\n");
        if !self.tags.is_empty() {
            prompt.push_str(&format!("  tags: [{}]\n", self.tags.join(", ")));
        }
        if let Some(category) = &self.category {
            prompt.push_str(&format!("  category: {category}\n"));
        }
        prompt.push_str(
            "\nDo NOT include id, type, created_at, updated_at, wip or wip_publish: those are assigned \
             by the application.\n\n",
        );
        prompt.push_str("Then the body, using only these constructs:\n");
        prompt.push_str("  - headings, paragraphs, bold, italic, lists, blockquotes\n");
        prompt.push_str("  - fenced code blocks with a language\n");
        prompt.push_str("  - Markdown tables\n");
        prompt.push_str("  - standard Markdown links and images with an absolute https URL\n");
        if self.with_image_placeholders {
            prompt.push_str(
                "  - for a picture that does not exist yet, a directive on its own line:\n\
                 \x20     :::image{placeholder=\"a description of the picture\"}\n\
                 \x20     :::\n\
                 \x20   Do not invent image URLs and do not embed files.\n",
            );
        }
        prompt.push_str("\nThe document is:\n");
        prompt.push_str(&format!("title: {}\n", self.title));
        prompt.push_str(&format!("purpose: {}\n", self.brief.trim()));
        prompt
    }
}

/// An image placeholder the model proposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePlaceholder {
    /// The description the model wrote, which the user replaces with a real attachment.
    pub description: String,
    /// The 1-based body line it sits on, so the UI can point at it.
    pub line: usize,
}

/// A generated page, parsed by the project's own parser and ready for the user to review.
///
/// Nothing has been written. `handoff.md` §8.1 puts the preview before the creation, and this type is the
/// thing being previewed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageDraft {
    /// The proposed title, from the frontmatter the model wrote.
    pub title: String,
    /// The proposed slug, normalised. A model that writes an unusable slug still produces a usable page.
    pub slug: String,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub kind: ContentKindChoice,
    /// The body, exactly as the model wrote it.
    pub body: String,
    /// The whole document, with the frontmatter VELOCT assigned. This is what a confirm writes.
    pub source: String,
    /// The placeholders, so the UI can list them and the user can replace each one.
    pub placeholders: Vec<ImagePlaceholder>,
    /// Diagnostics from the project's parser: an unknown directive, a malformed table, a broken reference.
    ///
    /// Reported rather than repaired, because §9.3's rule about referencing applies here too: a
    /// structural problem the model introduced is the user's to see before the page exists.
    pub diagnostics: Vec<String>,
    /// The model that produced it and whether a fallback was used, for the record.
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub used_fallback: bool,
}

impl PageDraft {
    /// Whether the draft is structurally sound enough to create.
    ///
    /// A draft with errors is still RETURNED, so the user can read what went wrong; this is what a UI
    /// checks before enabling the confirm button.
    pub fn is_creatable(&self) -> bool {
        !self.body.trim().is_empty() && !self.has_errors()
    }

    /// Whether the parser reported an error rather than a warning.
    ///
    /// The diagnostics arrive as strings, so the check is on the code prefix the parser writes rather
    /// than on a severity field that is not carried across. That is a real compromise and it is why the
    /// prefix is produced by one function, [`format_diagnostic`].
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|line| line.starts_with("error "))
    }

    /// A summary for the confirmation dialog (§8.1's "the user previews it").
    pub fn summary(&self) -> String {
        format!(
            "{} · {} 字 · {} 个图片占位符 · {}",
            self.title,
            self.body.chars().count(),
            self.placeholders.len(),
            if self.has_errors() {
                "有结构性错误"
            } else {
                "结构正常"
            }
        )
    }
}

/// Format one diagnostic the way `PageDraft::has_errors` expects.
pub fn format_diagnostic(severity: &str, code: &str, message: &str) -> String {
    format!("{severity} {code}: {message}")
}

/// Parse a model's answer into a draft.
///
/// The model is asked for Markdown, and this is the boundary that refuses to trust it beyond that: the
/// answer goes through the project's parser, and everything Veloct owns is assigned here.
pub fn build_draft(
    answer: &str,
    request: &PageRequest,
    now: &str,
    id: StableId,
) -> Result<PageDraft> {
    let text = strip_code_fence(answer);

    // A MODEL'S FRONTMATTER IS NOT A DOCUMENT FRONTMATTER.
    //
    // The first attempt at this parsed the model's own block with the project parser and read the fields
    // back out. That was wrong, and the tests said so twice. The contract REQUIRES `id`, `type`,
    // `created_at`, `updated_at` and `wip`, and it rejects a `type` that does not match the collection, so
    // a model that wrote its own `id`, or omitted the timestamps, produced a block the parser refused —
    // and the refusal cost the model's perfectly good title and summary for the sake of fields that are
    // discarded anyway.
    //
    // So the model's block is read as PROPOSALS, only the fields a model can meaningfully propose are
    // taken, and the real block is built here. That also puts the division of labour where `handoff.md`
    // §8.2 says it belongs: the model writes the prose, Veloct writes the structure.
    let proposed = read_proposed(&text);
    let body = body_after_frontmatter(&text);

    if body.trim().is_empty() {
        return Err(AiError::new(
            AiErrorKind::MalformedResponse,
            "the model returned no document body, so there is no page to preview",
        )
        .into());
    }

    // Veloct owns the identity and the lifecycle. `handoff.md` section 3.2 makes the id permanent, and a
    // model that invented one would produce a document the content layer refuses.
    let title = if proposed.title.trim().is_empty() {
        request.title.clone()
    } else {
        proposed.title.clone()
    };
    let slug = normalise_slug(&proposed.slug, &title);

    let mut frontmatter = Frontmatter::new(
        request.kind.content_type(),
        id,
        slug.clone(),
        title.clone(),
        now,
    );
    frontmatter.summary = proposed.summary.filter(|summary| !summary.trim().is_empty());
    frontmatter.tags = if proposed.tags.is_empty() {
        request.tags.clone()
    } else {
        proposed.tags.clone()
    };
    frontmatter.category = proposed
        .category
        .filter(|category| !category.trim().is_empty())
        .or_else(|| request.category.clone());
    frontmatter.timestamps = Timestamps {
        created_at: now.to_string(),
        updated_at: now.to_string(),
        published_at: None,
    };
    if request.kind.starts_as_wip() {
        // An article starts as a WIP, and its `wip_ever_published` is false, so no redirect is promised
        // until it is actually published.
        frontmatter.wip = true;
        frontmatter.wip_publish = false;
        frontmatter.wip_ever_published = false;
    }

    let placeholders = find_placeholders(&body);
    let assembled = rewrite::serialize(&frontmatter, &body)?;

    // Paragraph ids are assigned here, so a reference can be made to the new page immediately
    // (handoff.md section 9.2).
    let (source, _) = rewrite::assign_paragraph_ids(&assembled, &mut veloct_markdown::ast::ParagraphId::new);

    // Re-parse what will actually be written, so the diagnostics describe the SAVED document rather than
    // the model's raw answer.
    let final_document = veloct_markdown::parse::parse(&source)?;
    let diagnostics = final_document
        .diagnostics
        .sorted()
        .iter()
        .map(|diagnostic| {
            format_diagnostic(
                diagnostic.severity.as_str(),
                diagnostic.code.as_str(),
                &diagnostic.message,
            )
        })
        .collect();

    Ok(PageDraft {
        title,
        slug,
        summary: frontmatter.summary.clone(),
        tags: frontmatter.tags.clone(),
        category: frontmatter.category.clone(),
        kind: request.kind,
        body,
        source,
        placeholders,
        diagnostics,
        provider_id: None,
        model_id: None,
        used_fallback: false,
    })
}

impl PageDraft {
    /// Record which model produced the draft.
    pub fn attributed_to(mut self, provider_id: &str, model_id: &str, used_fallback: bool) -> Self {
        self.provider_id = Some(provider_id.to_string());
        self.model_id = Some(model_id.to_string());
        self.used_fallback = used_fallback;
        self
    }
}

/// A model may wrap its answer in a code fence despite being asked not to.
///
/// Unwrapping it is safe and worth doing: the fence is not part of the document, and rejecting the whole
/// answer over it would make the feature annoying for a formatting slip that has one obvious reading.
fn strip_code_fence(answer: &str) -> String {
    let trimmed = answer.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    // Drop the opening fence line, which may carry a language.
    let after_open = match trimmed.find('\n') {
        Some(index) => &trimmed[index + 1..],
        None => return trimmed.to_string(),
    };
    // Drop the closing fence, which is the last line when it is a fence.
    match after_open.rfind("```") {
        Some(index) => after_open[..index].trim_end().to_string(),
        None => after_open.to_string(),
    }
}

/// Read the fields a model may meaningfully propose from its frontmatter.
///
/// Line-based, and deliberately lenient: the model wrote a YAML-ish block, not necessarily a valid
/// document, and the only fields taken are the ones it is allowed to choose. Everything else it wrote —
/// an id, a type, a timestamp, a WIP flag — is IGNORED rather than rejected, because rejecting the block
/// over a field that is thrown away anyway would cost the prose that is the point of the feature.
///
/// A name-based read rather than a YAML parse for the same reason: a YAML parser would fail the whole
/// block over one bad line, which is the failure mode this function exists to avoid.
fn read_proposed(text: &str) -> ProposedFrontmatter {
    let mut proposed = ProposedFrontmatter::default();
    let Some(block) = frontmatter_block(text) else {
        return proposed;
    };

    for line in block.lines() {
        // A nested or continuation line is part of a value already read.
        if line.starts_with(' ') || line.starts_with('\t') || line.trim().is_empty() {
            continue;
        }
        let Some((key, raw)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(raw.trim());
        match key.trim().to_ascii_lowercase().as_str() {
            "title" => proposed.title = value,
            "slug" => proposed.slug = value,
            "summary" | "description" => {
                if !value.is_empty() {
                    proposed.summary = Some(value);
                }
            }
            "category" => {
                if !value.is_empty() {
                    proposed.category = Some(value);
                }
            }
            "tags" => proposed.tags = parse_tag_list(&value),
            _ => {}
        }
    }

    proposed
}

/// The inline form of a list, which is what the prompt asks for: `[a, b]`.
fn parse_tag_list(value: &str) -> Vec<String> {
    let inner = value.trim();
    let inner = inner
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(inner);
    inner
        .split(',')
        .map(|tag| unquote(tag.trim()))
        .filter(|tag| !tag.is_empty())
        .collect()
}

/// Strip one layer of matching quotes.
fn unquote(value: &str) -> String {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// The lines BETWEEN the fences, without either fence.
///
/// Returns `None` when there is no block, because a model that wrote no frontmatter has proposed nothing
/// and the caller's own title is a perfectly good title.
fn frontmatter_block(text: &str) -> Option<String> {
    if !text.starts_with("---") {
        return None;
    }
    let after_open = text.find('\n').map(|index| &text[index + 1..])?;

    let mut collected: Vec<&str> = Vec::new();
    for line in after_open.split('\n') {
        if line.trim_end() == "---" {
            return Some(collected.join("\n"));
        }
        collected.push(line);
    }
    // An unterminated block: there is nothing to read, and treating the rest as frontmatter would swallow
    // the body.
    None
}

/// Everything after a frontmatter block, or the whole text when there is no block.
fn body_after_frontmatter(text: &str) -> String {
    if !text.starts_with("---") {
        return text.to_string();
    }
    let after_open = match text.find('\n') {
        Some(index) => &text[index + 1..],
        None => return text.to_string(),
    };
    // The closing fence is the first line that is exactly `---`.
    let mut offset = 0usize;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return after_open[offset + line.len()..].to_string();
        }
        offset += line.len();
    }
    text.to_string()
}

/// The frontmatter the model proposed, read leniently.
#[derive(Debug, Default, Clone)]
struct ProposedFrontmatter {
    title: String,
    slug: String,
    summary: Option<String>,
    tags: Vec<String>,
    category: Option<String>,
}

/// Make a usable slug out of whatever the model wrote.
///
/// A model writing `My Page!` or `../escape` must still produce a usable page, and `slugify` is the
/// single definition of a slug, so it is used rather than a second rule being invented here.
fn normalise_slug(proposed: &str, title: &str) -> String {
    let candidate = if proposed.trim().is_empty() {
        veloct_core::paths::slugify(title)
    } else {
        veloct_core::paths::slugify(proposed)
    };
    if candidate.is_empty() {
        "untitled".to_string()
    } else {
        candidate
    }
}

/// Find the image placeholders the model wrote.
///
/// Read with the project's own directive scanner, so what counts as a placeholder is the same thing the
/// site will render.
fn find_placeholders(body: &str) -> Vec<ImagePlaceholder> {
    let scan = directives::scan(body);
    scan.directives
        .iter()
        // The directive NAME identifies it; `DirectiveKind` describes the syntax form (container or
        // leaf), which is a different question.
        .filter(|directive| directive.name == "image")
        .map(|directive| ImagePlaceholder {
            description: directive
                .attribute("placeholder")
                .or_else(|| directive.attribute("alt"))
                .unwrap_or("")
                .to_string(),
            line: directive.start_line,
        })
        .collect()
}

/// Refuse a draft that cannot be created, with the reason.
///
/// Called by the confirm step so the rule lives in one place rather than in a UI's disabled attribute.
pub fn ensure_creatable(draft: &PageDraft) -> Result<()> {
    if draft.body.trim().is_empty() {
        return Err(VeloctError::Invalid(
            "the draft has no body, so there is nothing to create".into(),
        ));
    }
    if draft.has_errors() {
        let errors: Vec<&str> = draft
            .diagnostics
            .iter()
            .filter(|line| line.starts_with("error "))
            .map(String::as_str)
            .collect();
        return Err(VeloctError::Invalid(format!(
            "the draft has {} structural error(s), so it is not created until they are fixed: {}",
            errors.len(),
            errors.join("; ")
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn id() -> StableId {
        StableId::from_parts(1_700_000_000_000, 7)
    }

    fn request() -> PageRequest {
        PageRequest::new("About Us", "a page describing the team")
    }

    /// A realistic model answer, with frontmatter it should not have written and a placeholder.
    const ANSWER: &str = r#"---
title: About Us
slug: About Us!
summary: Who we are.
tags: [team, about]
id: 01INVENTED00000000000000000
type: page
created_at: 1999-01-01T00:00:00Z
---

# About Us

We are a small team.

:::image{placeholder="a photo of the team"}
:::

## What we do

- Write
- Publish
"#;

    #[test]
    fn a_markdown_answer_becomes_a_draft_with_the_frontmatter_veloct_owns() {
        // handoff.md section 8.2: the output is the project's Markdown, and the identity is assigned
        // here rather than taken from the model.
        let draft = build_draft(ANSWER, &request(), NOW, id()).unwrap();

        assert_eq!(draft.title, "About Us");
        assert_eq!(draft.slug, "about-us", "the model's unusable slug is normalised");
        assert_eq!(draft.summary.as_deref(), Some("Who we are."));
        assert_eq!(draft.tags, vec!["team", "about"]);
        assert_eq!(draft.kind, ContentKindChoice::Page);

        // Veloct's id, not the invented one, and no 1999 timestamp.
        assert!(draft.source.contains(&id().to_text()), "{}", draft.source);
        assert!(
            !draft.source.contains("01INVENTED"),
            "the model's id must not survive: {}",
            draft.source
        );
        assert!(!draft.source.contains("1999"), "{}", draft.source);
        assert!(draft.source.contains("created_at: 2026-01-01T12:00:00Z"));

        // The body is Markdown, and the source round trips as the project's document.
        let reparsed = veloct_markdown::parse::parse(&draft.source).unwrap();
        assert!(reparsed.body.contains("# About Us"));
        assert!(reparsed.body.contains(":::image{placeholder=\"a photo of the team\"}"));
    }

    #[test]
    fn the_draft_is_parsed_by_the_project_parser_so_the_preview_is_the_real_document() {
        let draft = build_draft(ANSWER, &request(), NOW, id()).unwrap();
        let document = veloct_markdown::parse::parse(&draft.source).unwrap();
        assert_eq!(document.frontmatter.title, "About Us");
        assert_eq!(document.frontmatter.content_type, ContentType::Page);
        assert_eq!(document.frontmatter.id, id());
        // Paragraph ids are assigned, so a reference can be made to the new page right away.
        assert!(!document.paragraph_ids.is_empty());
    }

    #[test]
    fn a_page_draft_has_no_wip_state_and_an_article_starts_as_a_draft() {
        // handoff.md section 3.2: wip only has meaning for an article.
        let page = build_draft(ANSWER, &request(), NOW, id()).unwrap();
        assert!(!page.source.contains("wip: true"), "{}", page.source);

        let mut as_article = request();
        as_article.kind = ContentKindChoice::Article;
        let article = build_draft(ANSWER, &as_article, NOW, id()).unwrap();
        assert!(article.source.contains("wip: true"), "{}", article.source);
        assert!(article.source.contains("wip_publish: false"));
        assert!(
            article.source.contains("wip_ever_published: false"),
            "a redirect is only promised once it is actually published"
        );
    }

    #[test]
    fn an_image_placeholder_is_found_and_no_image_is_generated() {
        // handoff.md section 8.1: the AI does not generate image files.
        let draft = build_draft(ANSWER, &request(), NOW, id()).unwrap();
        assert_eq!(draft.placeholders.len(), 1);
        assert_eq!(draft.placeholders[0].description, "a photo of the team");
        assert!(draft.placeholders[0].line > 0);
        // The construct is the project's own directive, not a private marker.
        assert!(draft.source.contains(":::image"));
    }

    #[test]
    fn a_user_who_does_not_want_placeholders_gets_none_asked_for() {
        let mut without = request();
        without.with_image_placeholders = false;
        let prompt = without.prompt();
        assert!(!prompt.contains(":::image"), "{prompt}");
        // The rest of the instruction is still there, so the flag removes one construct rather than
        // emptying the prompt.
        assert!(prompt.contains("ONE Markdown document"), "{prompt}");
        assert!(prompt.contains("fenced code blocks"), "{prompt}");
    }

    #[test]
    fn the_prompt_asks_for_markdown_and_forbids_the_identity_fields() {
        // The two rules that make the answer parseable and keep identity with Veloct.
        let prompt = request().prompt();
        assert!(prompt.contains("ONE Markdown document"), "{prompt}");
        assert!(prompt.contains("Do NOT include id, type, created_at"), "{prompt}");
        assert!(prompt.contains(":::image"), "{prompt}");
    }

    #[test]
    fn an_answer_with_no_frontmatter_still_produces_a_usable_page() {
        // The model may ignore the format request. The user's own title is a perfectly good title.
        let answer = "Just a body with no frontmatter at all.\n";
        let draft = build_draft(answer, &request(), NOW, id()).unwrap();
        assert_eq!(draft.title, "About Us", "the request's title is the fallback");
        assert_eq!(draft.slug, "about-us");
        assert!(draft.source.starts_with("---\n"));
        assert!(draft.source.contains("Just a body"));
        assert!(draft.is_creatable());
    }

    #[test]
    fn an_answer_wrapped_in_a_code_fence_is_unwrapped() {
        // A formatting slip with one obvious reading, so rejecting the whole answer would be unhelpful.
        let fenced = format!("```markdown\n{ANSWER}\n```");
        let draft = build_draft(&fenced, &request(), NOW, id()).unwrap();
        assert_eq!(draft.title, "About Us");
        assert!(!draft.source.contains("```markdown"), "{}", draft.source);
        assert!(draft.source.contains("# About Us"));
    }

    #[test]
    fn a_slug_that_tries_to_escape_is_normalised_rather_than_used() {
        let answer = "---\ntitle: X\nslug: ../../etc/passwd\n---\n\nBody\n";
        let draft = build_draft(answer, &request(), NOW, id()).unwrap();
        assert_eq!(draft.slug, "etc-passwd");
        assert!(!draft.slug.contains('/'), "{}", draft.slug);
        assert!(!draft.slug.contains(".."), "{}", draft.slug);
    }

    #[test]
    fn a_title_with_nothing_sluggable_falls_back_rather_than_producing_an_empty_slug() {
        let answer = "---\ntitle: 你好\nslug: \"\"\n---\n\nBody\n";
        let draft = build_draft(answer, &request(), NOW, id()).unwrap();
        assert!(!draft.slug.is_empty(), "a slug is always usable");
    }

    #[test]
    fn an_empty_answer_is_reported_rather_than_producing_an_empty_page() {
        let error = build_draft("", &request(), NOW, id()).unwrap_err();
        assert_eq!(error.code(), "AI_PROVIDER");
        assert!(error.to_string().contains("no document body"), "{error}");

        let whitespace = build_draft("   \n\n  ", &request(), NOW, id()).unwrap_err();
        assert!(whitespace.to_string().contains("no document body"));
    }

    /// A container directive the model never closed.
    ///
    /// This is `MalformedDirective`, the parser's PARSE-TIME error, and it is exactly what a model
    /// produces when it half-remembers the syntax. The first two attempts at this fixture were wrong in
    /// instructive ways: an unterminated attribute brace is accepted, and a `knowledgeref` to a missing
    /// target is well formed and simply does not resolve.
    const MALFORMED: &str =
        "---\ntitle: X\nslug: x\n---\n\nText.\n\n:::card{title=\"unclosed\"}\nbody with no close\n";

    #[test]
    fn a_structural_problem_the_model_introduced_is_reported_as_a_diagnostic() {
        // handoff.md section 9.3's principle: a broken construct is surfaced, not silently repaired, so the
        // user sees it BEFORE the page exists.
        let draft = build_draft(MALFORMED, &request(), NOW, id()).unwrap();
        assert!(
            !draft.diagnostics.is_empty(),
            "the malformed directive should be reported: {draft:?}"
        );
        assert!(draft.has_errors(), "{:?}", draft.diagnostics);
        assert!(!draft.is_creatable(), "a draft with errors is not creatable");
        assert!(draft.summary().contains("有结构性错误"), "{}", draft.summary());
    }

    #[test]
    fn ensure_creatable_refuses_a_broken_draft_with_the_errors_named() {
        // The rule lives here rather than in a UI's disabled attribute, so it cannot be bypassed.
        let draft = build_draft(MALFORMED, &request(), NOW, id()).unwrap();
        let error = ensure_creatable(&draft).unwrap_err();
        assert!(error.to_string().contains("structural error"), "{error}");
        assert!(error.to_string().contains("MALFORMED"), "{error}");
    }

    #[test]
    fn a_reference_to_a_missing_target_parses_and_is_left_to_the_reference_checker() {
        // This test exists because its PREVIOUS version was wrong. It asserted that a `knowledgeref` to a
        // missing target is a parse error; it is not. The directive is well formed, so it parses, and the
        // unresolved target is what the content index reports and `repair_reference` fixes (sections 9.3
        // and 9.4). Asserting otherwise would have made the page creator refuse a draft it should accept.
        let answer = "---\ntitle: X\nslug: x\n---\n\n:::knowledgeref{content=\"missing\" paragraph=\"01MISSING\"}\n:::\n";
        let draft = build_draft(answer, &request(), NOW, id()).unwrap();
        assert!(
            !draft.has_errors(),
            "a well-formed reference is not a parse error: {:?}",
            draft.diagnostics
        );
        assert!(draft.is_creatable(), "so the page can be created");
        assert!(ensure_creatable(&draft).is_ok());
    }

    #[test]
    fn ensure_creatable_accepts_a_sound_draft() {
        let draft = build_draft(ANSWER, &request(), NOW, id()).unwrap();
        assert!(ensure_creatable(&draft).is_ok());
        assert!(draft.is_creatable());
    }

    #[test]
    fn the_draft_records_which_model_produced_it_and_whether_a_fallback_was_used() {
        let draft = build_draft(ANSWER, &request(), NOW, id())
            .unwrap()
            .attributed_to("p1", "m1", true);
        assert_eq!(draft.provider_id.as_deref(), Some("p1"));
        assert_eq!(draft.model_id.as_deref(), Some("m1"));
        assert!(draft.used_fallback);
    }

    #[test]
    fn a_table_the_model_wrote_survives_as_a_table() {
        // handoff.md section 3.3 lists tables as part of the dialect, so section 8.2 means the model's
        // table must land as a table rather than as text.
        let answer = "---\ntitle: T\nslug: t\n---\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
        let draft = build_draft(answer, &request(), NOW, id()).unwrap();
        assert!(draft.body.contains("| a | b |"), "{}", draft.body);
        assert!(veloct_markdown::parse::parse(&draft.source).unwrap().is_buildable());
    }

    #[test]
    fn a_code_block_the_model_wrote_survives_with_its_language() {
        let answer = "---\ntitle: C\nslug: c\n---\n\n```rust\nlet x = 1;\n```\n";
        let draft = build_draft(answer, &request(), NOW, id()).unwrap();
        assert!(draft.body.contains("```rust"), "{}", draft.body);
    }

    #[test]
    fn a_draft_round_trips_through_json_for_the_preview_ui() {
        let draft = build_draft(ANSWER, &request(), NOW, id()).unwrap();
        let json = serde_json::to_string(&draft).unwrap();
        assert!(json.contains("\"slug\":\"about-us\""), "{json}");
        assert!(json.contains("placeholders"), "{json}");
        assert_eq!(serde_json::from_str::<PageDraft>(&json).unwrap(), draft);
    }

    #[test]
    fn the_content_kind_choice_parses_and_reports_its_wip_behaviour() {
        assert_eq!(ContentKindChoice::parse("page"), Some(ContentKindChoice::Page));
        assert_eq!(ContentKindChoice::parse("ARTICLE"), Some(ContentKindChoice::Article));
        assert_eq!(ContentKindChoice::parse("knowledge"), Some(ContentKindChoice::Knowledge));
        assert_eq!(ContentKindChoice::parse("wip"), None);
        assert!(!ContentKindChoice::Page.starts_as_wip());
        assert!(ContentKindChoice::Article.starts_as_wip());
        assert!(!ContentKindChoice::Knowledge.starts_as_wip());
        for choice in [
            ContentKindChoice::Page,
            ContentKindChoice::Article,
            ContentKindChoice::Knowledge,
        ] {
            assert_eq!(ContentKindChoice::parse(choice.as_str()), Some(choice));
            assert_eq!(choice.content_type().as_str(), choice.as_str());
        }
    }

    #[test]
    fn the_request_carries_the_user_tags_and_category_into_the_prompt() {
        let mut with_extras = request();
        with_extras.tags = vec!["team".to_string()];
        with_extras.category = Some("about".to_string());
        let prompt = with_extras.prompt();
        assert!(prompt.contains("tags: [team]"), "{prompt}");
        assert!(prompt.contains("category: about"), "{prompt}");
    }

    #[test]
    fn a_user_tag_is_used_when_the_model_proposed_none() {
        let mut with_tag = request();
        with_tag.tags = vec!["from-the-user".to_string()];
        let answer = "---\ntitle: X\nslug: x\n---\n\nBody\n";
        let draft = build_draft(answer, &with_tag, NOW, id()).unwrap();
        assert_eq!(draft.tags, vec!["from-the-user"]);
    }
}
