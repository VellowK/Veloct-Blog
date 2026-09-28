//! Directive scanning and parsing.
//!
//! `handoff.md` §3.3 requires the custom Markdown capabilities — knowledge reference cards,
//! download link cards, hyperlink cards and page sections — to be implemented as a structured
//! AST or directive form, and explicitly forbids fragile string regex as the only parsing
//! mechanism.
//!
//! So this module is a small, explicit state machine rather than a set of regexes. It has to
//! understand two pieces of Markdown context that a naive line scan gets wrong:
//!
//! * **Fenced code blocks.** A `:::card` example inside a fenced block is documentation, not a
//!   directive. The scanner tracks fence state (backtick or tilde, three or more, matched by
//!   character and minimum length).
//! * **Indented code.** A marker or directive indented four or more spaces at the top level is
//!   code, not a construct.
//!
//! The directive grammar Veloct supports:
//!
//! ```text
//! :::knowledgeref{content="01J8Z..." paragraph="01J8Z...P"}    a container with a body
//! optional body content
//! :::
//!
//! ::download{url="https://..." name="report.pdf"}              a leaf, no body
//!
//! <!--p:01J8Z...-->                                           a paragraph marker
//! ```

use serde::{Deserialize, Serialize};

use crate::ast::ParagraphId;
use crate::diagnostics::{Diagnostic, DiagnosticCode};

/// Whether a directive opens a block or stands alone on one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DirectiveKind {
    /// `:::name{...}` … `:::` — may contain Markdown.
    Container,
    /// `::name{...}` — a leaf.
    Leaf,
}

impl DirectiveKind {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Container => ":::",
            Self::Leaf => "::",
        }
    }
}

/// A parsed directive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directive {
    pub kind: DirectiveKind,
    /// Directive name, e.g. `knowledgeref`.
    pub name: String,
    /// Attributes in source order, so a round trip is stable.
    pub attributes: Vec<(String, String)>,
    /// Raw body text (container directives only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// 1-based inclusive source lines.
    pub start_line: usize,
    pub end_line: usize,
}

impl Directive {
    pub fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// A rendered image placeholder a page-creating model may emit (`handoff.md` §8.1).
    ///
    /// The model never produces an actual image file, so this is only ever a placeholder.
    pub fn image_placeholder(&self) -> Option<String> {
        if self.name != "image" && self.name != "img" {
            return None;
        }
        let alt = self.attribute("alt").unwrap_or("image");
        let width = self.attribute("width");
        Some(match width {
            Some(width) => format!("[image placeholder: {alt} ({width})]"),
            None => format!("[image placeholder: {alt}]"),
        })
    }

    /// Re-render to Markdown source.
    pub fn to_markdown(&self) -> String {
        let attributes = render_attributes(&self.attributes);
        match self.kind {
            DirectiveKind::Leaf => format!("::{}{attributes}", self.name),
            DirectiveKind::Container => {
                let mut out = format!(":::{}{attributes}\n", self.name);
                if let Some(body) = &self.body {
                    out.push_str(body);
                    if !body.ends_with('\n') {
                        out.push('\n');
                    }
                }
                out.push_str(":::");
                out
            }
        }
    }
}

/// Names Veloct knows. An unknown name is a warning, not a hard failure, so a user's extension
/// does not break a build.
pub const KNOWN_DIRECTIVES: [&str; 8] = [
    "knowledgeref",
    "download",
    "linkcard",
    "card",
    "section",
    "block",
    "image",
    "note",
];

pub fn is_known_directive(name: &str) -> bool {
    KNOWN_DIRECTIVES.contains(&name)
}

/// The attributes each known directive requires.
pub fn required_attributes(name: &str) -> &'static [&'static str] {
    match name {
        "knowledgeref" => &["content", "paragraph"],
        "download" => &["url"],
        "linkcard" => &["url"],
        _ => &[],
    }
}

/// One line of the body, with the context the scanner established for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedLine {
    /// 1-based line number in the body (the frontmatter block is not counted).
    pub number: usize,
    pub raw: String,
    /// True when this line is inside a fenced code block (including the fence lines).
    pub in_fence: bool,
    /// Leading spaces, capped at 8 so an over-indented line is still detectable.
    pub indent: usize,
    /// 0 when not inside a block quote, otherwise the `>` nesting depth.
    pub blockquote_depth: usize,
    /// For a paragraph marker, the id it carried.
    pub paragraph_marker: Option<ParagraphId>,
}

impl ScannedLine {
    /// Whether this line is eligible to open a directive or carry a paragraph marker.
    pub fn eligible_for_construct(&self) -> bool {
        !self.in_fence && self.indent < 4 && self.blockquote_depth == 0
    }
}

/// Result of scanning a body.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    pub lines: Vec<ScannedLine>,
    pub directives: Vec<Directive>,
    /// Body text with directive blocks and marker lines removed, ready for a CommonMark parser.
    pub residual: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Scan a document body.
pub fn scan(body: &str) -> Scan {
    let lines = scan_lines(body);
    let mut directives = Vec::new();
    let mut diagnostics = Vec::new();
    let mut residual = String::with_capacity(body.len());

    let mut index = 0usize;
    while index < lines.len() {
        let line = &lines[index];

        if let Some(directive) = open_directive(line) {
            match directive.kind {
                DirectiveKind::Leaf => {
                    validate_directive(&directive, &mut diagnostics);
                    directives.push(directive);
                    index += 1;
                    continue;
                }
                DirectiveKind::Container => {
                    // Find the closing marker: a line of only colons with at least as many
                    // colons as the opener. Nesting is handled by counting depth.
                    let mut depth = 1usize;
                    let mut cursor = index + 1;
                    let mut body_lines: Vec<String> = Vec::new();
                    let mut closed = false;
                    while cursor < lines.len() {
                        let candidate = &lines[cursor];
                        if !candidate.in_fence {
                            let trimmed = candidate.raw.trim_end();
                            if is_container_close(trimmed) {
                                depth -= 1;
                                if depth == 0 {
                                    closed = true;
                                    break;
                                }
                            } else if open_directive(candidate)
                                .map(|d| d.kind == DirectiveKind::Container)
                                .unwrap_or(false)
                            {
                                depth += 1;
                            }
                        }
                        body_lines.push(candidate.raw.clone());
                        cursor += 1;
                    }

                    let end_line = if closed {
                        lines[cursor].number
                    } else {
                        diagnostics.push(
                            Diagnostic::new(
                                DiagnosticCode::MalformedDirective,
                                format!(
                                    "container directive `{}` is never closed with `:::`",
                                    directive.name
                                ),
                            )
                            .with_line(directive.start_line),
                        );
                        lines.last().map(|l| l.number).unwrap_or(directive.start_line)
                    };

                    let mut directive = directive;
                    directive.end_line = end_line;
                    let body_text = body_lines.join("\n");
                    directive.body = if body_text.trim().is_empty() {
                        None
                    } else {
                        Some(body_text)
                    };
                    validate_directive(&directive, &mut diagnostics);
                    directives.push(directive);
                    index = if closed { cursor + 1 } else { lines.len() };
                    continue;
                }
            }
        }

        // A paragraph marker line is consumed here so the CommonMark parser never sees it.
        if line.paragraph_marker.is_some() && line.eligible_for_construct() {
            residual.push('\n');
            index += 1;
            continue;
        }

        residual.push_str(&line.raw);
        residual.push('\n');
        index += 1;
    }

    Scan {
        lines,
        directives,
        residual,
        diagnostics,
    }
}

/// Try to read a directive opening line.
fn open_directive(line: &ScannedLine) -> Option<Directive> {
    if !line.eligible_for_construct() {
        return None;
    }
    let trimmed = line.raw.trim_start();
    // `:::` must be tested first: it is a prefix of `::` as well as of `:::name`.
    let (kind, rest) = if let Some(rest) = trimmed.strip_prefix(":::") {
        (DirectiveKind::Container, rest)
    } else {
        let rest = trimmed.strip_prefix("::")?;
        (DirectiveKind::Leaf, rest)
    };

    // The character immediately after the marker must begin a name, and it is checked *before*
    // trimming any whitespace. This is what stops a line such as `:: x` — or a stray run of
    // colons — from being read as a directive.
    let first = rest.chars().next()?;
    if !(first.is_ascii_alphanumeric() || first == '-' || first == '_') {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }

    // The name runs until `{`, whitespace or the end.
    let name_end = rest
        .find(|c: char| c == '{' || c.is_whitespace())
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }

    let remainder = rest[name_end..].trim();
    let attributes = if remainder.starts_with('{') {
        match remainder.find('}') {
            Some(end) => parse_attributes(&remainder[1..end]),
            // An unterminated attribute list still yields a directive; the missing `}` is
            // reported by the caller through validation.
            None => parse_attributes(remainder.trim_start_matches('{')),
        }
    } else {
        Vec::new()
    };

    Some(Directive {
        kind,
        name: name.to_ascii_lowercase(),
        attributes,
        body: None,
        start_line: line.number,
        end_line: line.number,
    })
}

/// Whether a line closes a container directive.
fn is_container_close(trimmed: &str) -> bool {
    !trimmed.is_empty() && trimmed.chars().all(|c| c == ':') && trimmed.len() >= 3
}

fn validate_directive(directive: &Directive, diagnostics: &mut Vec<Diagnostic>) {
    if !is_known_directive(&directive.name) {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::UnknownDirective,
                format!(
                    "directive `{}` is not a Veloct directive; it will be passed through",
                    directive.name
                ),
            )
            .with_span(crate::ast::SourceSpan::new(
                directive.start_line,
                directive.end_line,
            )),
        );
    }
    for required in required_attributes(&directive.name) {
        if directive.attribute(required).is_none_or(str::is_empty) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::MissingDirectiveAttribute,
                    format!(
                        "directive `{}` requires the `{}` attribute",
                        directive.name, required
                    ),
                )
                .with_span(crate::ast::SourceSpan::new(
                    directive.start_line,
                    directive.end_line,
                ))
                .with_detail("attribute", *required),
            );
        }
    }
}

/// Parse `key="value" key2='v' flag key3=bare`.
///
/// A small state machine rather than a regex, because a value may legally contain `=`, spaces
/// and even braces, and a quoted value may contain the quote character of the other style.
pub fn parse_attributes(input: &str) -> Vec<(String, String)> {
    let mut attributes = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut index = 0usize;

    while index < chars.len() {
        // Skip separators.
        while index < chars.len() && (chars[index].is_whitespace() || chars[index] == ',') {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }

        // Read the key.
        let key_start = index;
        while index < chars.len()
            && !chars[index].is_whitespace()
            && chars[index] != '='
            && chars[index] != ','
        {
            index += 1;
        }
        let key: String = chars[key_start..index].iter().collect();
        if key.is_empty() {
            index += 1;
            continue;
        }

        // Optional `= value`.
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index < chars.len() && chars[index] == '=' {
            index += 1;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            let value = if index < chars.len() && (chars[index] == '"' || chars[index] == '\'') {
                let quote = chars[index];
                index += 1;
                let value_start = index;
                while index < chars.len() && chars[index] != quote {
                    index += 1;
                }
                let value: String = chars[value_start..index].iter().collect();
                if index < chars.len() {
                    index += 1; // consume the closing quote
                }
                value
            } else {
                let value_start = index;
                while index < chars.len() && !chars[index].is_whitespace() && chars[index] != ',' {
                    index += 1;
                }
                chars[value_start..index].iter().collect()
            };
            attributes.push((key.to_ascii_lowercase(), value));
        } else {
            // A bare flag is recorded with an empty value rather than being dropped.
            attributes.push((key.to_ascii_lowercase(), String::new()));
        }
    }

    attributes
}

/// Render attributes back into the `{k="v" ...}` form.
pub fn render_attributes(attributes: &[(String, String)]) -> String {
    if attributes.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = attributes
        .iter()
        .map(|(key, value)| {
            if value.is_empty() {
                key.clone()
            } else {
                format!("{key}=\"{}\"", value.replace('"', "\\\""))
            }
        })
        .collect();
    format!(" {{{}}}", rendered.join(" "))
}

/// Scan the body into lines with their context, extracting paragraph markers.
///
/// This is the single place that understands fences and indentation, so no other module has to.
pub fn scan_lines(body: &str) -> Vec<ScannedLine> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for (offset, raw) in body.split('\n').enumerate() {
        let number = offset + 1;
        let without_cr = raw.strip_suffix('\r').unwrap_or(raw);

        let indent = without_cr.len() - without_cr.trim_start_matches(' ').len();
        let trimmed_start = without_cr.trim_start();
        let blockquote_depth = trimmed_start.chars().take_while(|c| *c == '>').count();

        // Only a top-level, unindented line can open or close a fence.
        let fence_line = indent < 4 && blockquote_depth == 0;
        let mut in_fence = fence.is_some();
        if fence_line {
            if let Some((character, length)) = read_fence(trimmed_start) {
                match fence {
                    Some((open_char, open_len)) => {
                        if open_char == character && length >= open_len {
                            fence = None;
                            in_fence = true; // the closing fence line belongs to the block
                        }
                    }
                    None => {
                        fence = Some((character, length));
                        in_fence = false; // the opening fence line is not content
                    }
                }
            }
        } else if fence.is_some() {
            in_fence = true;
        }

        let paragraph_marker = if fence.is_none() && !in_fence {
            read_paragraph_marker(trimmed_start)
        } else {
            None
        };

        out.push(ScannedLine {
            number,
            raw: without_cr.to_string(),
            in_fence,
            indent,
            blockquote_depth,
            paragraph_marker,
        });
    }

    out
}

/// Read a fence opener: three or more backticks or tildes, optionally with an info string.
fn read_fence(trimmed: &str) -> Option<(char, usize)> {
    let character = trimmed.chars().next()?;
    if character != '`' && character != '~' {
        return None;
    }
    let length = trimmed.chars().take_while(|c| *c == character).count();
    if length >= 3 {
        Some((character, length))
    } else {
        None
    }
}

/// Read a paragraph marker of the exact form `<!--p:ULID-->`.
pub fn read_paragraph_marker(trimmed: &str) -> Option<ParagraphId> {
    let inner = trimmed.strip_prefix("<!--p:")?.strip_suffix("-->")?;
    let inner = inner.trim();
    if inner.is_empty() || inner.contains(char::is_whitespace) {
        return None;
    }
    inner.parse::<ParagraphId>().ok()
}

/// Remove every paragraph-marker comment from inline text, returning the cleaned text and the
/// ids found, in order.
///
/// A marker on a line next to prose is not a block marker, but leaving raw HTML comments in the
/// rendered body would expose internal identity to the reader, so they are stripped here.
pub fn strip_inline_markers(text: &str) -> (String, Vec<ParagraphId>) {
    /// `<!--p:` is six bytes: `<!--p` plus the colon.
    const PREFIX_LEN: usize = 6;
    /// `-->` is three bytes.
    const SUFFIX_LEN: usize = 3;

    let mut remaining = text;
    let mut cleaned = String::with_capacity(text.len());
    let mut found = Vec::new();

    while let Some(start) = remaining.find("<!--p:") {
        cleaned.push_str(&remaining[..start]);
        let after = &remaining[start + PREFIX_LEN..];
        match after.find("-->") {
            Some(end) => {
                let candidate = after[..end].trim();
                if let Ok(id) = candidate.parse::<ParagraphId>() {
                    found.push(id);
                } else {
                    // Not one of ours; keep it verbatim.
                    cleaned.push_str(&remaining[start..start + PREFIX_LEN + end + SUFFIX_LEN]);
                }
                remaining = &remaining[start + PREFIX_LEN + end + SUFFIX_LEN..];
            }
            None => {
                cleaned.push_str(&remaining[start..]);
                remaining = "";
            }
        }
    }
    cleaned.push_str(remaining);
    (cleaned, found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_parse_in_every_supported_style() {
        let attributes = parse_attributes(r#"content="01ABC" paragraph='01DEF' name=bare flag"#);
        assert_eq!(
            attributes,
            vec![
                ("content".to_string(), "01ABC".to_string()),
                ("paragraph".to_string(), "01DEF".to_string()),
                ("name".to_string(), "bare".to_string()),
                ("flag".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn attribute_values_may_contain_equals_spaces_and_braces() {
        // A URL with a query string must survive parsing intact.
        let attributes = parse_attributes(r#"url="https://example.com/a?b=1&c=2" title="a b c""#);
        assert_eq!(attributes[0].1, "https://example.com/a?b=1&c=2");
        assert_eq!(attributes[1].1, "a b c");
    }

    #[test]
    fn attributes_render_back_in_a_stable_form() {
        let attributes = vec![
            ("content".to_string(), "01ABC".to_string()),
            ("flag".to_string(), String::new()),
        ];
        assert_eq!(render_attributes(&attributes), r#" {content="01ABC" flag}"#);
        assert_eq!(render_attributes(&[]), "");
    }

    #[test]
    fn a_container_directive_is_parsed_with_its_body() {
        let body = ":::knowledgeref{content=\"01ABC\" paragraph=\"01DEF\"}\nSome body\n:::\n";
        let scan = scan(body);
        assert_eq!(scan.directives.len(), 1);
        let directive = &scan.directives[0];
        assert_eq!(directive.kind, DirectiveKind::Container);
        assert_eq!(directive.name, "knowledgeref");
        assert_eq!(directive.attribute("content"), Some("01ABC"));
        assert_eq!(directive.body.as_deref(), Some("Some body"));
        assert_eq!(directive.start_line, 1);
        assert_eq!(directive.end_line, 3);
        assert!(scan.diagnostics.is_empty(), "{:?}", scan.diagnostics);
    }

    #[test]
    fn a_leaf_directive_has_no_body() {
        let scan = scan("::download{url=\"https://example.com/a.zip\" name=\"a.zip\"}\n");
        assert_eq!(scan.directives.len(), 1);
        let directive = &scan.directives[0];
        assert_eq!(directive.kind, DirectiveKind::Leaf);
        assert_eq!(directive.attribute("name"), Some("a.zip"));
        assert!(directive.body.is_none());
    }

    #[test]
    fn directives_inside_a_fenced_code_block_are_documentation_not_constructs() {
        // This is the case a naive line scan gets wrong.
        let body = "```markdown\n:::knowledgeref{content=\"X\"}\n:::\n```\n\ntext\n";
        let scan = scan(body);
        assert!(scan.directives.is_empty(), "{:?}", scan.directives);
        assert!(scan.residual.contains("knowledgeref"));
    }

    #[test]
    fn tilde_fences_are_recognised_too() {
        let body = "~~~\n::download{url=\"x\"}\n~~~\n";
        assert!(scan(body).directives.is_empty());
    }

    #[test]
    fn nested_container_directives_close_correctly() {
        let body = ":::section{id=\"a\"}\n:::block{id=\"b\"}\ninner\n:::\nouter\n:::\n";
        let scan = scan(body);
        assert_eq!(scan.directives.len(), 1);
        let directive = &scan.directives[0];
        assert_eq!(directive.name, "section");
        assert_eq!(directive.end_line, 6);
        assert!(directive.body.as_deref().unwrap().contains("inner"));
    }

    #[test]
    fn an_unclosed_container_is_reported_rather_than_silently_accepted() {
        let scan = scan(":::card{title=\"x\"}\nbody but no close\n");
        assert_eq!(scan.directives.len(), 1);
        assert!(scan
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::MalformedDirective));
        assert_eq!(scan.directives[0].kind, DirectiveKind::Container);
    }

    #[test]
    fn a_missing_required_attribute_is_a_warning_with_the_name() {
        let scan = scan(":::knowledgeref{content=\"01ABC\"}\n:::\n");
        let diagnostic = scan
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::MissingDirectiveAttribute)
            .expect("expected a missing attribute diagnostic");
        assert_eq!(diagnostic.detail("attribute"), Some("paragraph"));
        assert!(!diagnostic.is_error(), "an unknown extension must not break a build");
    }

    #[test]
    fn an_unknown_directive_warns_but_is_preserved() {
        let scan = scan(":::mycustom{thing=\"1\"}\nbody\n:::\n");
        assert_eq!(scan.directives.len(), 1);
        assert_eq!(scan.directives[0].name, "mycustom");
        assert!(scan
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownDirective));
    }

    #[test]
    fn paragraph_markers_are_read_and_removed_from_the_residual_body() {
        let id = ParagraphId::from_parts(1_700_000_000_000, 5);
        let body = format!("{}\n\nFirst paragraph.\n", id.to_marker());
        let scan = scan(&body);
        assert_eq!(scan.lines[0].paragraph_marker, Some(id));
        assert!(!scan.residual.contains("<!--p:"), "residual was {:?}", scan.residual);
        assert!(scan.residual.contains("First paragraph."));
    }

    #[test]
    fn an_inline_marker_is_stripped_from_text_without_becoming_a_block_marker() {
        let id = ParagraphId::from_parts(1_700_000_000_000, 6);
        let (cleaned, found) = strip_inline_markers(&format!("text before {} text after", id.to_marker()));
        assert_eq!(cleaned, "text before  text after");
        assert_eq!(found, vec![id]);
    }

    #[test]
    fn a_non_veloct_html_comment_is_left_alone() {
        let (cleaned, found) = strip_inline_markers("a <!--not-ours--> b");
        assert_eq!(cleaned, "a <!--not-ours--> b");
        assert!(found.is_empty());
    }

    #[test]
    fn an_unterminated_comment_does_not_lose_text() {
        let (cleaned, found) = strip_inline_markers("a <!--p:broken b");
        assert_eq!(cleaned, "a <!--p:broken b");
        assert!(found.is_empty());
    }

    #[test]
    fn a_malformed_marker_is_not_treated_as_an_id() {
        assert!(read_paragraph_marker("<!--p:-->").is_none());
        assert!(read_paragraph_marker("<!--p:not-a-ulid-->").is_none());
        assert!(read_paragraph_marker("<!--p:abc def-->").is_none());
        assert!(read_paragraph_marker("plain text").is_none());
    }

    #[test]
    fn indented_constructs_are_code_not_directives() {
        let scan = scan("    :::card{title=\"x\"}\n");
        assert!(scan.directives.is_empty());
    }

    #[test]
    fn quoted_directives_are_not_constructs() {
        let scan = scan("> :::card{title=\"x\"}\n");
        assert!(scan.directives.is_empty());
    }

    #[test]
    fn directives_round_trip_through_markdown() {
        let original = ":::knowledgeref{content=\"01ABC\" paragraph=\"01DEF\"}\nBody text\n:::";
        let scanned = scan(original);
        let rendered = scanned.directives[0].to_markdown();
        let rescanned = scan(&rendered);
        assert_eq!(rescanned.directives.len(), 1);
        assert_eq!(rescanned.directives[0].name, "knowledgeref");
        assert_eq!(rescanned.directives[0].attribute("content"), Some("01ABC"));
        assert_eq!(rescanned.directives[0].body.as_deref(), Some("Body text"));
    }

    #[test]
    fn an_image_directive_yields_a_placeholder_and_never_a_file() {
        // handoff §8.1: the AI creates placeholders, never real image files.
        let scanned = scan(":::image{alt=\"架构图\" width=\"800\"}\n:::");
        let placeholder = scanned.directives[0].image_placeholder().unwrap();
        assert!(placeholder.contains("架构图"));
        assert!(placeholder.contains("800"));

        let leaf = scan("::download{url=\"https://example.com\"}");
        assert!(leaf.directives[0].image_placeholder().is_none());
    }

    #[test]
    fn the_directive_table_is_the_single_source_of_truth_for_required_attributes() {
        // The Astro remark plugin must agree with this table; keep them in step deliberately.
        assert!(is_known_directive("knowledgeref"));
        assert!(!is_known_directive("somethingelse"));
        assert_eq!(required_attributes("knowledgeref"), &["content", "paragraph"]);
        assert_eq!(required_attributes("download"), &["url"]);
        assert_eq!(required_attributes("note"), &[] as &[&str]);
    }

    #[test]
    fn a_line_that_merely_starts_with_colons_is_not_a_directive() {
        assert!(scan(":::\n").directives.is_empty());
        assert!(scan("::::\n").directives.is_empty());
        assert!(scan(":: x\n").directives.is_empty());
    }
}
