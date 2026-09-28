//! The document AST.
//!
//! `handoff.md` §3.3 requires the custom capabilities to be expressed as a structured AST rather
//! than as fragile string matching, and §9.2 requires paragraph identity to be a first-class
//! property of a block rather than something re-derived from text.
//!
//! The node set is deliberately a *superset of what the current templates use*: unknown
//! constructs are represented instead of being dropped, because a document must survive a
//! load/save cycle even when a template does not yet render every node type.

use serde::{Deserialize, Serialize};

use veloct_core::{Result, StableId, VeloctError};

/// A stable, hidden identifier for one paragraph (`handoff.md` §9.2).
///
/// Stored in Markdown as `<!--p:<ULID>-->`. It is deliberately independent of the paragraph
/// text, so editing the words cannot change which paragraph a knowledge reference points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParagraphId(StableId);

impl ParagraphId {
    pub fn new() -> Self {
        Self(StableId::new())
    }

    /// Deterministic constructor for tests and reproducible fixtures.
    pub fn from_parts(timestamp_ms: u64, randomness: u128) -> Self {
        Self(StableId::from_parts(timestamp_ms, randomness))
    }

    pub fn as_stable_id(self) -> StableId {
        self.0
    }

    /// The HTML-comment form written into Markdown.
    pub fn to_marker(self) -> String {
        format!("<!--p:{}-->", self.0)
    }
}

impl Default for ParagraphId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ParagraphId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ParagraphId {
    type Err = VeloctError;

    fn from_str(value: &str) -> Result<Self> {
        value
            .parse::<StableId>()
            .map(Self)
            .map_err(|_| VeloctError::Reference(format!("invalid paragraph id {value:?}")))
    }
}

/// Where a block came from in the source, in 1-based inclusive line numbers.
///
/// Used by the editor to apply a targeted change and by diagnostics to point at a location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_line: usize,
    pub end_line: usize,
}

impl SourceSpan {
    pub fn new(start_line: usize, end_line: usize) -> Self {
        Self {
            start_line,
            end_line: end_line.max(start_line),
        }
    }

    pub fn contains(&self, line: usize) -> bool {
        line >= self.start_line && line <= self.end_line
    }

    pub fn line_count(&self) -> usize {
        self.end_line - self.start_line + 1
    }
}

/// Inline content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Inline {
    Text {
        text: String,
    },
    Emphasis {
        children: Vec<Inline>,
    },
    Strong {
        children: Vec<Inline>,
    },
    Strikethrough {
        children: Vec<Inline>,
    },
    Code {
        text: String,
    },
    Link {
        url: String,
        title: Option<String>,
        children: Vec<Inline>,
    },
    /// An image is inline in Markdown, even though it renders as a block-level figure.
    Image {
        url: String,
        alt: String,
        title: Option<String>,
    },
    SoftBreak,
    HardBreak,
    /// Inline raw HTML, kept so a document round-trips.
    Html {
        text: String,
    },
    FootnoteReference {
        label: String,
    },
}

impl Inline {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text { text: value.into() }
    }

    /// Concatenate the visible text of this node and its descendants.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        self.write_plain_text(&mut out);
        out
    }

    fn write_plain_text(&self, out: &mut String) {
        match self {
            Self::Text { text } | Self::Code { text } | Self::Html { text } => out.push_str(text),
            Self::Emphasis { children }
            | Self::Strong { children }
            | Self::Strikethrough { children }
            | Self::Link { children, .. } => {
                for child in children {
                    child.write_plain_text(out);
                }
            }
            Self::Image { alt, .. } => out.push_str(alt),
            Self::FootnoteReference { label } => {
                out.push('[');
                out.push_str(label);
                out.push(']');
            }
            Self::SoftBreak | Self::HardBreak => out.push(' '),
        }
    }
}

/// Concatenate the visible text of a sequence of inline nodes.
pub fn plain_text(nodes: &[Inline]) -> String {
    let mut out = String::new();
    for node in nodes {
        node.write_plain_text(&mut out);
    }
    out
}

/// A heading, with the collected text and a stable anchor slug.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// Anchor id, derived from the text but uniquified within the document.
    pub anchor: String,
}

/// One item in a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub checked: Option<bool>,
    pub children: Vec<DocNode>,
}

/// A table, which `handoff.md` §3.3 requires the dialect to support.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub alignments: Vec<Option<String>>,
    pub head: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

impl Table {
    /// Render as GitHub-Flavored Markdown so the table can be written back into a body.
    pub fn to_gfm(&self) -> String {
        fn row(cells: &[Vec<Inline>]) -> String {
            let rendered: Vec<String> = cells
                .iter()
                .map(|cell| plain_text(cell).replace('|', "\\|"))
                .collect();
            format!("| {} |", rendered.join(" | "))
        }
        let columns = self.head.len().max(1);
        let mut out = String::new();
        out.push_str(&row(&self.head));
        out.push('\n');
        let separators: Vec<String> = (0..columns)
            .map(|index| match self.alignments.get(index).and_then(|a| a.as_deref()) {
                Some("left") => ":---".to_string(),
                Some("right") => "---:".to_string(),
                Some("center") => ":---:".to_string(),
                _ => "---".to_string(),
            })
            .collect();
        out.push_str(&format!("| {} |", separators.join(" | ")));
        for row_cells in &self.rows {
            out.push('\n');
            out.push_str(&row(row_cells));
        }
        out
    }
}

/// Block-level content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocNode {
    Paragraph {
        /// Stable identity required by `handoff.md` §9.2. Present on knowledge documents and
        /// assigned by the editor elsewhere.
        paragraph_id: Option<ParagraphId>,
        content: Vec<Inline>,
        span: SourceSpan,
    },
    Heading {
        level: u8,
        content: Vec<Inline>,
        span: SourceSpan,
    },
    CodeBlock {
        language: Option<String>,
        text: String,
        span: SourceSpan,
    },
    BlockQuote {
        children: Vec<DocNode>,
        span: SourceSpan,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
        span: SourceSpan,
    },
    Table {
        table: Table,
        span: SourceSpan,
    },
    ThematicBreak {
        span: SourceSpan,
    },
    /// A Veloct directive such as a knowledge reference card (`handoff.md` §9.1) or a download
    /// link card (§6.7).
    Directive {
        kind: String,
        attributes: Vec<(String, String)>,
        /// An optional image placeholder a page-creating model may emit (`handoff.md` §8.1).
        image_placeholder: Option<String>,
        span: SourceSpan,
    },
    /// Raw HTML that is not a Veloct construct, preserved verbatim.
    Html {
        text: String,
        span: SourceSpan,
    },
}

impl DocNode {
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Paragraph { span, .. }
            | Self::Heading { span, .. }
            | Self::CodeBlock { span, .. }
            | Self::BlockQuote { span, .. }
            | Self::List { span, .. }
            | Self::Table { span, .. }
            | Self::ThematicBreak { span }
            | Self::Directive { span, .. }
            | Self::Html { span, .. } => *span,
        }
    }

    /// Direct children, for a uniform walk.
    pub fn children(&self) -> &[DocNode] {
        match self {
            Self::BlockQuote { children, .. } => children,
            _ => &[],
        }
    }

    pub fn as_paragraph(&self) -> Option<(Option<ParagraphId>, &[Inline])> {
        match self {
            Self::Paragraph {
                paragraph_id,
                content,
                ..
            } => Some((*paragraph_id, content)),
            _ => None,
        }
    }

    pub fn as_directive(&self) -> Option<(&str, &[(String, String)])> {
        match self {
            Self::Directive {
                kind, attributes, ..
            } => Some((kind.as_str(), attributes)),
            _ => None,
        }
    }

    /// Visible text of the node.
    pub fn plain_text(&self) -> String {
        match self {
            Self::Paragraph { content, .. } | Self::Heading { content, .. } => plain_text(content),
            Self::CodeBlock { text, .. } | Self::Html { text, .. } => text.clone(),
            Self::BlockQuote { children, .. } => children
                .iter()
                .map(|child| child.plain_text())
                .collect::<Vec<_>>()
                .join(" "),
            Self::List { items, .. } => items
                .iter()
                .map(|item| {
                    item.children
                        .iter()
                        .map(|child| child.plain_text())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join(" "),
            Self::Table { table, .. } => {
                let mut cells: Vec<String> = table.head.iter().map(|c| plain_text(c)).collect();
                for row in &table.rows {
                    cells.extend(row.iter().map(|c| plain_text(c)));
                }
                cells.join(" ")
            }
            Self::ThematicBreak { .. } => String::new(),
            Self::Directive {
                image_placeholder, ..
            } => image_placeholder.clone().unwrap_or_default(),
        }
    }
}

/// A parsed document body.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Ast {
    pub nodes: Vec<DocNode>,
}

impl Ast {
    pub fn new(nodes: Vec<DocNode>) -> Self {
        Self { nodes }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// All headings in document order, with uniquified anchors.
    pub fn headings(&self) -> Vec<Heading> {
        let mut used: Vec<String> = Vec::new();
        let mut out = Vec::new();
        self.visit(&mut |node| {
            if let DocNode::Heading { level, content, .. } = node {
                let text = plain_text(content);
                let base = slab(&text);
                let mut anchor = base.clone();
                let mut suffix = 2;
                while used.contains(&anchor) {
                    anchor = format!("{base}-{suffix}");
                    suffix += 1;
                }
                used.push(anchor.clone());
                out.push(Heading {
                    level: *level,
                    text,
                    anchor,
                });
            }
        });
        out
    }

    /// Every paragraph with its optional id, in document order.
    pub fn paragraphs(&self) -> Vec<(Option<ParagraphId>, Vec<Inline>, SourceSpan)> {
        let mut out = Vec::new();
        self.visit(&mut |node| {
            if let DocNode::Paragraph {
                paragraph_id,
                content,
                span,
            } = node
            {
                out.push((*paragraph_id, content.clone(), *span));
            }
        });
        out
    }

    /// Every directive in document order.
    pub fn directives(&self) -> Vec<&DocNode> {
        fn collect<'a>(node: &'a DocNode, out: &mut Vec<&'a DocNode>) {
            if matches!(node, DocNode::Directive { .. }) {
                out.push(node);
            }
            for child in node.children() {
                collect(child, out);
            }
        }

        let mut out: Vec<&DocNode> = Vec::new();
        for node in &self.nodes {
            collect(node, &mut out);
        }
        out
    }

    /// Depth-first walk, including nested block quotes.
    pub fn visit<F: FnMut(&DocNode)>(&self, visitor: &mut F) {
        for node in &self.nodes {
            walk(node, visitor);
        }
    }

    /// First paragraph's text, used to derive a knowledge reference excerpt.
    pub fn first_paragraph_text(&self) -> Option<String> {
        self.nodes.iter().find_map(|node| match node {
            DocNode::Paragraph { content, .. } => Some(plain_text(content)),
            _ => None,
        })
    }
}

fn walk<F: FnMut(&DocNode)>(node: &DocNode, visitor: &mut F) {
    visitor(node);
    for child in node.children() {
        walk(child, visitor);
    }
}

/// Turn heading text into an anchor slug, preserving CJK characters.
///
/// Veloct targets Chinese-language personal sites, so a heading of `架构设计` must produce a
/// usable anchor rather than an empty string.
pub fn slab(text: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        "section".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(id: Option<ParagraphId>, text: &str) -> DocNode {
        DocNode::Paragraph {
            paragraph_id: id,
            content: vec![Inline::text(text)],
            span: SourceSpan::new(1, 1),
        }
    }

    #[test]
    fn paragraph_ids_render_as_hidden_html_comments() {
        // handoff §9.2: the id must not be visible in the user's body text.
        let id = ParagraphId::from_parts(1_700_000_000_000, 7);
        let marker = id.to_marker();
        assert!(marker.starts_with("<!--p:"));
        assert!(marker.ends_with("-->"));
        let inner = marker
            .trim_start_matches("<!--p:")
            .trim_end_matches("-->");
        assert_eq!(inner.parse::<ParagraphId>().unwrap(), id);
    }

    #[test]
    fn invalid_paragraph_ids_are_rejected() {
        assert!("not-an-id".parse::<ParagraphId>().is_err());
        assert!("".parse::<ParagraphId>().is_err());
    }

    #[test]
    fn plain_text_flattens_nested_inline_content() {
        let inline = vec![
            Inline::text("Hello "),
            Inline::Strong {
                children: vec![Inline::text("bold")],
            },
            Inline::text(" and "),
            Inline::Link {
                url: "https://example.com".into(),
                title: None,
                children: vec![Inline::text("a link")],
            },
            Inline::Image {
                url: "a.png".into(),
                alt: "diagram".into(),
                title: None,
            },
            Inline::SoftBreak,
        ];
        assert_eq!(plain_text(&inline), "Hello bold and a linkdiagram ");
    }

    #[test]
    fn headings_are_listed_with_uniquified_anchors() {
        let ast = Ast::new(vec![
            DocNode::Heading {
                level: 2,
                content: vec![Inline::text("Setup")],
                span: SourceSpan::new(1, 1),
            },
            DocNode::Heading {
                level: 2,
                content: vec![Inline::text("Setup")],
                span: SourceSpan::new(2, 2),
            },
            DocNode::Heading {
                level: 3,
                content: vec![Inline::text("架构设计")],
                span: SourceSpan::new(3, 3),
            },
        ]);
        let headings = ast.headings();
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].anchor, "setup");
        assert_eq!(headings[1].anchor, "setup-2");
        // CJK must survive slugification rather than collapsing to an empty anchor.
        assert_eq!(headings[2].anchor, "架构设计");
    }

    #[test]
    fn paragraphs_and_directives_are_enumerable() {
        let id = ParagraphId::from_parts(1_700_000_000_000, 9);
        let ast = Ast::new(vec![
            para(Some(id), "first"),
            DocNode::Directive {
                kind: "knowledgeref".into(),
                attributes: vec![("content".into(), "ABC".into())],
                image_placeholder: None,
                span: SourceSpan::new(2, 2),
            },
            DocNode::BlockQuote {
                children: vec![para(None, "quoted")],
                span: SourceSpan::new(3, 4),
            },
        ]);

        let paragraphs = ast.paragraphs();
        assert_eq!(paragraphs.len(), 2, "nested paragraphs must be found too");
        assert_eq!(paragraphs[0].0, Some(id));
        assert_eq!(paragraphs[1].0, None);

        assert_eq!(ast.directives().len(), 1);
        let directive = ast.directives()[0];
        assert_eq!(directive.as_directive().unwrap().0, "knowledgeref");
    }

    #[test]
    fn spans_report_line_membership() {
        let span = SourceSpan::new(3, 7);
        assert_eq!(span.line_count(), 5);
        assert!(span.contains(3));
        assert!(span.contains(7));
        assert!(!span.contains(8));
        // A span can never be inverted.
        assert_eq!(SourceSpan::new(5, 2).end_line, 5);
    }

    #[test]
    fn tables_render_back_to_gfm_with_alignment() {
        let table = Table {
            alignments: vec![None, Some("right".into())],
            head: vec![vec![Inline::text("Name")], vec![Inline::text("Count")]],
            rows: vec![vec![
                vec![Inline::text("a|b")],
                vec![Inline::text("2")],
            ]],
        };
        let gfm = table.to_gfm();
        let lines: Vec<&str> = gfm.lines().collect();
        assert_eq!(lines[0], "| Name | Count |");
        assert_eq!(lines[1], "| --- | ---: |");
        // A literal pipe inside a cell must be escaped or the table breaks on re-parse.
        assert_eq!(lines[2], "| a\\|b | 2 |");
    }

    #[test]
    fn node_plain_text_covers_block_variants() {
        let list = DocNode::List {
            ordered: false,
            start: None,
            items: vec![
                ListItem {
                    checked: None,
                    children: vec![para(None, "one")],
                },
                ListItem {
                    checked: Some(true),
                    children: vec![para(None, "two")],
                },
            ],
            span: SourceSpan::new(1, 2),
        };
        assert_eq!(list.plain_text(), "one two");

        let code = DocNode::CodeBlock {
            language: Some("rust".into()),
            text: "fn main() {}".into(),
            span: SourceSpan::new(1, 1),
        };
        assert_eq!(code.plain_text(), "fn main() {}");

        let rule = DocNode::ThematicBreak {
            span: SourceSpan::new(1, 1),
        };
        assert_eq!(rule.plain_text(), "");
    }

    #[test]
    fn first_paragraph_text_is_available_for_reference_excerpts() {
        let ast = Ast::new(vec![
            DocNode::Heading {
                level: 1,
                content: vec![Inline::text("Title")],
                span: SourceSpan::new(1, 1),
            },
            para(None, "The opening line."),
        ]);
        assert_eq!(ast.first_paragraph_text().as_deref(), Some("The opening line."));
        assert!(Ast::default().first_paragraph_text().is_none());
    }
}
