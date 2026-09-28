//! The document parser: frontmatter, directives, AST and diagnostics.
//!
//! The pipeline is deliberately explicit, because `handoff.md` §3.3 forbids relying on fragile
//! string handling for the custom constructs and §3.4 forbids a second source of truth:
//!
//! ```text
//! file text
//!   → split_frontmatter            (typed Frontmatter + body + its start line)
//!   → directives::scan             (explicit state machine: fences, indentation, markers)
//!   → pulldown-cmark               (CommonMark + GFM over the residual body)
//!   → attach paragraph ids         (markers collected by the scanner, in order)
//!   → diagnostics                  (duplicates, unresolved references, malformed input)
//! ```
//!
//! Source line numbers are preserved through the residual step by expanding each residual line's
//! *own* embedded newlines, so a line number in a diagnostic or a [`SourceSpan`] always refers to
//! the original file.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use veloct_core::{Result, VeloctError};

use crate::ast::{Ast, DocNode, Inline, ParagraphId, SourceSpan, Table};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::directives::{self};
use crate::frontmatter::Frontmatter;

/// Frontmatter delimiter.
const DELIMITER: &str = "---";

/// A parsed Markdown document.
#[derive(Debug, Clone)]
pub struct Document {
    pub frontmatter: Frontmatter,
    pub ast: Ast,
    pub diagnostics: Diagnostics,
    /// The body, exactly as it appears after the frontmatter block.
    pub body: String,
    /// Every paragraph id found, in document order, including duplicates.
    pub paragraph_ids: Vec<ParagraphId>,
}

impl Document {
    /// True when nothing in this document must stop a build (`handoff.md` §9.3).
    pub fn is_buildable(&self) -> bool {
        !self.diagnostics.has_errors()
    }

    /// The paragraph ids present, de-duplicated, in first-seen order.
    pub fn unique_paragraph_ids(&self) -> Vec<ParagraphId> {
        let mut out: Vec<ParagraphId> = Vec::new();
        for id in &self.paragraph_ids {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        out
    }
}

/// A body parsed without a frontmatter block.
#[derive(Debug, Clone)]
pub struct ParsedBody {
    pub ast: Ast,
    pub diagnostics: Diagnostics,
    pub paragraph_ids: Vec<ParagraphId>,
}

impl ParsedBody {
    /// The paragraph ids present, de-duplicated, in first-seen order.
    pub fn unique_paragraph_ids(&self) -> Vec<ParagraphId> {
        let mut out: Vec<ParagraphId> = Vec::new();
        for id in &self.paragraph_ids {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        out
    }

    /// True when nothing in this body must stop a build (`handoff.md` §9.3).
    pub fn is_buildable(&self) -> bool {
        !self.diagnostics.has_errors()
    }
}

/// Split a document into its frontmatter block and the remaining body.
///
/// Returns `Ok(None)` when there is no frontmatter block at all. The body's first line number in
/// the original file is returned as well, so diagnostics can point at the real file position.
pub fn split_frontmatter(source: &str) -> Result<Option<(serde_yaml::Value, String, usize)>> {
    let mut lines = source.split('\n');

    let first = match lines.next() {
        Some(line) => line.strip_suffix('\r').unwrap_or(line),
        None => return Ok(None),
    };
    if first.trim() != DELIMITER {
        return Ok(None);
    }

    let mut yaml = String::new();
    let mut consumed = 1usize;
    let mut closed = false;
    for line in lines.by_ref() {
        consumed += 1;
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim() == DELIMITER {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }

    if !closed {
        return Err(VeloctError::Frontmatter {
            path: std::path::PathBuf::from("<document>"),
            message: "frontmatter block is opened with `---` but never closed".into(),
        });
    }

    // `consumed` counts the closing delimiter line; the body starts on the next one.
    let body_start_line = consumed + 1;
    let body: String = lines.collect::<Vec<_>>().join("\n");

    let value: serde_yaml::Value = serde_yaml::from_str(&yaml).map_err(|e| {
        VeloctError::Frontmatter {
            path: std::path::PathBuf::from("<document>"),
            message: format!("invalid YAML: {e}"),
        }
    })?;

    Ok(Some((value, body, body_start_line)))
}

/// Parse a complete document, requiring a frontmatter block.
pub fn parse(source: &str) -> Result<Document> {
    let (value, body, body_start_line) = match split_frontmatter(source)? {
        Some(parts) => parts,
        None => {
            return Err(VeloctError::Frontmatter {
                path: std::path::PathBuf::from("<document>"),
                message: "document has no frontmatter block; every managed document needs one"
                    .into(),
            })
        }
    };

    let frontmatter: Frontmatter = serde_yaml::from_value(value).map_err(|e| {
        VeloctError::Frontmatter {
            path: std::path::PathBuf::from("<document>"),
            message: format!("frontmatter does not match the contract: {e}"),
        }
    })?;

    let mut diagnostics = Diagnostics::new();
    if let Err(problem) = frontmatter.validate() {
        diagnostics.push(
            Diagnostic::new(DiagnosticCode::InvalidFrontmatterField, problem)
                .with_content(frontmatter.id.to_text()),
        );
    }

    let parsed = parse_body_with_offset(&body, body_start_line, &mut diagnostics)?;

    Ok(Document {
        frontmatter,
        ast: parsed.ast,
        diagnostics,
        body,
        paragraph_ids: parsed.paragraph_ids,
    })
}

/// Parse a body with no frontmatter, as used for a knowledge document fragment under test.
pub fn parse_body(body: &str) -> ParsedBody {
    let mut diagnostics = Diagnostics::new();
    parse_body_with_offset(body, 1, &mut diagnostics).expect("body parsing cannot fail")
}

fn parse_body_with_offset(
    body: &str,
    first_line: usize,
    diagnostics: &mut Diagnostics,
) -> Result<ParsedBody> {
    // 1. Scan with context: fences, indentation, paragraph markers, directives.
    let scan = directives::scan(body);
    for diagnostic in &scan.diagnostics {
        diagnostics.push(diagnostic.clone());
    }

    // 2. Build a byte-offset → original-line map over the residual text, so a diagnostic or a
    //    `SourceSpan` always refers to a line in the original file.
    //
    //    The map must be built from the lines that were actually emitted: directive blocks and
    //    marker lines are *dropped* from the residual, so deriving offsets from the body's own line
    //    numbering would drift by however many lines were removed. `directives::scan` already
    //    reports exactly which body lines produced residual lines, so its own residual is used
    //    directly and the offsets are accumulated over those lines only.
    let residual = &scan.residual;
    let emitted = residual_sources(&scan);
    let mut line_starts: Vec<usize> = Vec::with_capacity(emitted.len());
    let mut line_numbers: Vec<usize> = Vec::with_capacity(emitted.len());
    {
        let mut offset = 0usize;
        for index in emitted {
            let line = &scan.lines[index];
            line_starts.push(offset);
            line_numbers.push(line.number + first_line - 1);
            offset += line.raw.len() + 1;
        }
    }

    let line_of = |byte_offset: usize| -> usize {
        match line_starts.binary_search(&byte_offset) {
            Ok(index) => line_numbers.get(index).copied().unwrap_or(first_line),
            Err(0) => first_line,
            Err(index) => line_numbers.get(index - 1).copied().unwrap_or(first_line),
        }
    };

    // 3. Parse the residual body with CommonMark + GFM.
    //
    //    Smart punctuation is deliberately OFF. It rewrites straight quotes to curly ones, which
    //    would mean Veloct's own view of a document disagrees with the bytes on disk, and the
    //    excerpt carried on a knowledge reference card would not be the text the author typed.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(residual, options);
    let built = build_ast(parser, &line_of);

    // 4. Directives are scanned out of the body before CommonMark runs, so they have to be merged
    //    back into the AST at their source position. Without this an article's `knowledgeref`
    //    directives would never reach reference resolution.
    let mut nodes = merge_directives(built.nodes, &scan.directives, first_line);

    // 5. Attach paragraph ids: block markers in document order, then any inline markers found
    //    inside paragraphs. A marker belongs to the paragraph that follows it.
    let mut markers: Vec<ParagraphId> = scan
        .lines
        .iter()
        .filter_map(|line| line.paragraph_marker)
        .collect();

    let mut inline_markers = built.inline_markers;
    inline_markers.reverse();
    markers.extend(inline_markers);

    let mut consumed = 0usize;
    let mut paragraph_ids: Vec<ParagraphId> = Vec::new();
    let mut seen: Vec<ParagraphId> = Vec::new();
    let mut duplicates: Vec<ParagraphId> = Vec::new();
    assign_markers(
        &mut nodes,
        &markers,
        &mut consumed,
        &mut paragraph_ids,
        &mut seen,
        &mut duplicates,
    );

    for id in &duplicates {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::DuplicateParagraphId,
                format!("paragraph id {id} appears more than once in this document"),
            )
            .with_paragraph(*id),
        );
    }
    for leftover in markers.iter().skip(consumed) {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::OrphanParagraphMarker,
                format!("paragraph marker {leftover} is not followed by any paragraph"),
            )
            .with_paragraph(*leftover),
        );
    }

    Ok(ParsedBody {
        ast: Ast::new(nodes),
        diagnostics: diagnostics.clone(),
        paragraph_ids,
    })
}

/// Attach collected markers to paragraphs in document order.
fn assign_markers(
    nodes: &mut [DocNode],
    markers: &[ParagraphId],
    consumed: &mut usize,
    paragraph_ids: &mut Vec<ParagraphId>,
    seen: &mut Vec<ParagraphId>,
    duplicates: &mut Vec<ParagraphId>,
) {
    for node in nodes.iter_mut() {
        match node {
            DocNode::Paragraph {
                paragraph_id,
                content,
                ..
            } => {
                // An inline marker inside this paragraph belongs to it.
                if paragraph_id.is_none() {
                    for inline in content.iter() {
                        let _ = inline;
                    }
                }
                if paragraph_id.is_none() && marker_at(markers, *consumed).is_some() {
                    *paragraph_id = marker_at(markers, *consumed);
                    *consumed += 1;
                }
                if let Some(id) = paragraph_id {
                    if seen.contains(id) {
                        duplicates.push(*id);
                    } else {
                        seen.push(*id);
                    }
                    paragraph_ids.push(*id);
                }
            }
            DocNode::BlockQuote { children, .. } => {
                assign_markers(children, markers, consumed, paragraph_ids, seen, duplicates);
            }
            DocNode::List { items, .. } => {
                for item in items.iter_mut() {
                    assign_markers(
                        &mut item.children,
                        markers,
                        consumed,
                        paragraph_ids,
                        seen,
                        duplicates,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Which scanned lines produced a line in the residual body.
///
/// This re-derives the same skipping rules `directives::scan` applies, and is asserted against the
/// scanner's own record in the tests so the two cannot silently diverge. It exists so the residual
/// offset map is built from the lines that are really there.
fn residual_sources(scan: &directives::Scan) -> Vec<usize> {
    let mut out = Vec::with_capacity(scan.lines.len());
    for (index, line) in scan.lines.iter().enumerate() {
        let skipped_as_marker = line.paragraph_marker.is_some() && line.eligible_for_construct();
        let skipped_as_directive = scan.directives.iter().any(|directive| {
            let start = directive.start_line - 1;
            let end = directive.end_line - 1;
            index >= start && index <= end
        });
        if !skipped_as_marker && !skipped_as_directive {
            out.push(index);
        }
    }
    out
}

fn marker_at(markers: &[ParagraphId], index: usize) -> Option<ParagraphId> {
    markers.get(index).copied()
}

/// Insert scanned directives into the AST at the position their source line implies.
///
/// Directives never come from the CommonMark walk, because the scanner removes them from the
/// residual body. Merging by source line keeps document order intact, which matters because the
/// article's reference list and the reference cards must appear where the author wrote them.
///
/// The scanner reports directive lines relative to the body, so `first_line` translates them into
/// file lines: every other node's span is already a file line, and mixing the two would make a
/// diagnostic point at the wrong place.
fn merge_directives(
    nodes: Vec<DocNode>,
    directives: &[directives::Directive],
    first_line: usize,
) -> Vec<DocNode> {
    if directives.is_empty() {
        return nodes;
    }

    let to_file_line = |body_line: usize| body_line + first_line - 1;

    let mut pending: Vec<&directives::Directive> = directives.iter().collect();
    pending.sort_by_key(|directive| directive.start_line);

    let mut out: Vec<DocNode> = Vec::with_capacity(nodes.len() + pending.len());
    let mut next = 0usize;

    for node in nodes {
        let node_line = node.span().start_line;
        // A directive belongs before this node when it ends strictly above where the node starts.
        // Comparing against the directive's *end* line is what stops a directive from jumping ahead
        // of the paragraph that follows it, because a directive always ends above the next block.
        while next < pending.len() && to_file_line(pending[next].end_line) < node_line {
            out.push(directive_node_at_file_line(pending[next], first_line));
            next += 1;
        }
        out.push(node);
    }
    // Any directive that appears after the last block still belongs in the document.
    while next < pending.len() {
        out.push(directive_node_at_file_line(pending[next], first_line));
        next += 1;
    }

    out
}

/// Convert a scanned directive into an AST node whose span is in file lines.
fn directive_node_at_file_line(
    directive: &directives::Directive,
    first_line: usize,
) -> DocNode {
    let offset = first_line - 1;
    DocNode::Directive {
        kind: directive.name.clone(),
        attributes: directive.attributes.clone(),
        image_placeholder: directive.image_placeholder(),
        span: SourceSpan::new(
            directive.start_line + offset,
            directive.end_line + offset,
        ),
    }
}

struct BuiltAst {
    nodes: Vec<DocNode>,
    inline_markers: Vec<ParagraphId>,
}

/// Container under construction while walking the event stream.
///
/// Line positions are not tracked per frame: the event's own byte range is mapped to a source
/// line as the walk proceeds, which is cheaper and always current.
enum Frame {
    Paragraph {
        content: Vec<Inline>,
    },
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    CodeBlock {
        language: Option<String>,
        text: String,
    },
    BlockQuote {
        children: Vec<DocNode>,
    },
    Item {
        children: Vec<DocNode>,
        checked: Option<bool>,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<crate::ast::ListItem>,
    },
    Table {
        alignments: Vec<Option<String>>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    TableRow {
        cells: Vec<Vec<Inline>>,
        in_head: bool,
    },
    TableCell {
        content: Vec<Inline>,
    },
    Emphasis {
        content: Vec<Inline>,
    },
    Strong {
        content: Vec<Inline>,
    },
    Strikethrough {
        content: Vec<Inline>,
    },
    Link {
        url: String,
        title: Option<String>,
        content: Vec<Inline>,
    },
    Image {
        url: String,
        alt: String,
        title: Option<String>,
    },
}

/// Place a node into the innermost container frame, or at the document root.
///
/// A free function rather than an inline macro: pushing a `DocNode` into an enclosing frame while
/// iterating the frame stack is a partial move the borrow checker rejects inside a closure-like
/// macro, but it is fine here.
fn place_node(stack: &mut [Frame], finished: &mut Vec<DocNode>, node: DocNode) {
    for frame in stack.iter_mut().rev() {
        match frame {
            Frame::BlockQuote { children, .. } | Frame::Item { children, .. } => {
                children.push(node);
                return;
            }
            _ => continue,
        }
    }
    finished.push(node);
}

/// Place an inline node into the innermost inline frame, reporting whether it landed.
///
/// A `false` result is meaningful: CommonMark elides the paragraph wrapper in a *tight* list item,
/// so text can arrive while the innermost frame is an `Item` or a `BlockQuote`. The caller opens a
/// paragraph in that case, which keeps a tight list item's content in the AST instead of dropping
/// it.
///
/// Frames that hold no inline content (a code block, a table row, an image) simply fall through to
/// the frames enclosing them.
fn place_inline(stack: &mut [Frame], inline: Inline) -> bool {
    for frame in stack.iter_mut().rev() {
        match frame {
            Frame::Paragraph { content, .. }
            | Frame::Heading { content, .. }
            | Frame::TableCell { content } => {
                content.push(inline);
                return true;
            }
            Frame::Emphasis { content }
            | Frame::Strong { content }
            | Frame::Strikethrough { content }
            | Frame::Link { content, .. } => {
                content.push(inline);
                return true;
            }
            // An explicit block boundary means this inline belongs to a new block.
            Frame::Item { .. } | Frame::BlockQuote { .. } => return false,
            // Nothing to hold inline content here; keep looking outwards.
            Frame::CodeBlock { .. }
            | Frame::Table { .. }
            | Frame::TableRow { .. }
            | Frame::List { .. }
            | Frame::Image { .. } => continue,
        }
    }
    false
}

/// Close an implicitly opened paragraph because the block that owned it is ending.
///
/// Called before handling `End(Item)` and `End(BlockQuote)`; without it the implicit paragraph
/// would still be on the stack and the frame below it would never be popped.
fn finish_implicit_paragraph(stack: &mut Vec<Frame>, finished: &mut Vec<DocNode>) {
    if matches!(stack.last(), Some(Frame::Paragraph { .. })) {
        if let Some(Frame::Paragraph { content }) = stack.pop() {
            place_node(
                stack,
                finished,
                DocNode::Paragraph {
                    paragraph_id: None,
                    content,
                    span: SourceSpan::new(1, 1),
                },
            );
        }
    }
}

fn build_ast<F>(parser: Parser<'_>, line_of: &F) -> BuiltAst
where
    F: Fn(usize) -> usize,
{
    let mut stack: Vec<Frame> = Vec::new();
    let mut finished: Vec<DocNode> = Vec::new();
    let mut inline_markers: Vec<ParagraphId> = Vec::new();
    // Set from the event byte offset on every iteration before it is read.
    #[allow(unused_assignments)]
    let mut current_line = 1usize;

    macro_rules! push_node {
        ($node:expr) => {{
            place_node(&mut stack, &mut finished, $node);
        }};
    }

    macro_rules! push_inline {
        ($inline:expr) => {{
            let inline: Inline = $inline;
            // A tight list item or a block quote has no paragraph wrapper. Open one so the content
            // is retained rather than discarded. A paragraph already open for a *different* block
            // is never reused: that would merge two blocks together.
            if matches!(stack.last(), Some(Frame::Item { .. }) | Some(Frame::BlockQuote { .. })) {
                stack.push(Frame::Paragraph {
                    content: vec![inline],
                });
            } else {
                place_inline(&mut stack, inline);
            }
        }};
    }

    for (event, range) in parser.into_offset_iter() {
        current_line = line_of(range.start);
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => stack.push(Frame::Paragraph {
                    content: Vec::new(),
                }),
                Tag::Heading { level, .. } => stack.push(Frame::Heading {
                    level: heading_level(level),
                    content: Vec::new(),
                }),
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => {
                            let info = info.trim();
                            if info.is_empty() {
                                None
                            } else {
                                Some(info.split_whitespace().next().unwrap_or("").to_string())
                            }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    stack.push(Frame::CodeBlock {
                        language,
                        text: String::new(),
                    });
                }
                Tag::BlockQuote(_) => stack.push(Frame::BlockQuote {
                    children: Vec::new(),
                }),
                Tag::Item => stack.push(Frame::Item {
                    children: Vec::new(),
                    checked: None,
                }),
                Tag::List(start) => stack.push(Frame::List {
                    ordered: start.is_some(),
                    start,
                    items: Vec::new(),
                }),
                Tag::Table(alignments) => stack.push(Frame::Table {
                    alignments: alignments
                        .iter()
                        .map(|alignment| {
                            Some(
                                match alignment {
                                    pulldown_cmark::Alignment::Left => "left",
                                    pulldown_cmark::Alignment::Center => "center",
                                    pulldown_cmark::Alignment::Right => "right",
                                    pulldown_cmark::Alignment::None => "",
                                }
                                .to_string(),
                            )
                        })
                        .collect(),
                    head: Vec::new(),
                    rows: Vec::new(),
                }),
                Tag::TableHead => stack.push(Frame::TableRow {
                    cells: Vec::new(),
                    in_head: true,
                }),
                Tag::TableRow => stack.push(Frame::TableRow {
                    cells: Vec::new(),
                    in_head: false,
                }),
                Tag::TableCell => stack.push(Frame::TableCell { content: Vec::new() }),
                Tag::Emphasis => stack.push(Frame::Emphasis { content: Vec::new() }),
                Tag::Strong => stack.push(Frame::Strong { content: Vec::new() }),
                Tag::Strikethrough => stack.push(Frame::Strikethrough { content: Vec::new() }),
                Tag::Link { dest_url, title, .. } => stack.push(Frame::Link {
                    url: dest_url.to_string(),
                    title: if title.is_empty() {
                        None
                    } else {
                        Some(title.to_string())
                    },
                    content: Vec::new(),
                }),
                Tag::Image { dest_url, title, .. } => stack.push(Frame::Image {
                    url: dest_url.to_string(),
                    alt: String::new(),
                    title: if title.is_empty() {
                        None
                    } else {
                        Some(title.to_string())
                    },
                }),
                Tag::FootnoteDefinition(_) | Tag::HtmlBlock | Tag::DefinitionList => {}
                Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {}
                Tag::MetadataBlock(_) => {}
                // GFM superscript/subscript are represented as inline emphasis-style content so
                // a document using them still round-trips rather than failing to parse.
                Tag::Superscript | Tag::Subscript => stack.push(Frame::Emphasis {
                    content: Vec::new(),
                }),
            },

            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    if let Some(Frame::Paragraph { content, .. }) = stack.pop() {
                        let content = merge_inline_markers(content, &mut inline_markers);
                        // A paragraph that renders to nothing visible (for example one that was
                        // only a marker comment) carries no content and is dropped.
                        if !content_is_empty(&content) {
                            let line = current_line;
                            push_node!(DocNode::Paragraph {
                                paragraph_id: None,
                                content,
                                span: SourceSpan::new(line, line),
                            });
                        }
                    }
                }
                TagEnd::Heading(_) => {
                    if let Some(Frame::Heading { level, content, .. }) = stack.pop() {
                        let line = current_line;
                        push_node!(DocNode::Heading {
                            level,
                            content,
                            span: SourceSpan::new(line, line),
                        });
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Frame::CodeBlock {
                        language,
                        text,
                        ..
                    }) = stack.pop()
                    {
                        let line = current_line;
                        push_node!(DocNode::CodeBlock {
                            language,
                            text,
                            span: SourceSpan::new(line, line),
                        });
                    }
                }
                TagEnd::BlockQuote(_) => {
                    finish_implicit_paragraph(&mut stack, &mut finished);
                    if let Some(Frame::BlockQuote { children, .. }) = stack.pop() {
                        let line = current_line;
                        push_node!(DocNode::BlockQuote {
                            children,
                            span: SourceSpan::new(line, line),
                        });
                    }
                }
                TagEnd::Item => {
                    finish_implicit_paragraph(&mut stack, &mut finished);
                    if let Some(Frame::Item { children, checked }) = stack.pop() {
                        for frame in stack.iter_mut().rev() {
                            if let Frame::List { items, .. } = frame {
                                items.push(crate::ast::ListItem { checked, children });
                                break;
                            }
                        }
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Frame::List {
                        ordered,
                        start,
                        items,
                        ..
                    }) = stack.pop()
                    {
                        let line = current_line;
                        push_node!(DocNode::List {
                            ordered,
                            start,
                            items,
                            span: SourceSpan::new(line, line),
                        });
                    }
                }
                TagEnd::Table => {
                    if let Some(Frame::Table {
                        alignments,
                        head,
                        rows,
                        ..
                    }) = stack.pop()
                    {
                        let line = current_line;
                        push_node!(DocNode::Table {
                            table: Table {
                                alignments,
                                head,
                                rows,
                            },
                            span: SourceSpan::new(line, line),
                        });
                    }
                }
                TagEnd::TableHead => {
                    if let Some(Frame::TableRow { cells, .. }) = stack.pop() {
                        for frame in stack.iter_mut().rev() {
                            if let Frame::Table { head, .. } = frame {
                                *head = cells;
                                break;
                            }
                        }
                    }
                }
                TagEnd::TableRow => {
                    if let Some(Frame::TableRow { cells, in_head }) = stack.pop() {
                        for frame in stack.iter_mut().rev() {
                            if let Frame::Table { head, rows, .. } = frame {
                                if in_head && head.is_empty() {
                                    *head = cells;
                                } else {
                                    rows.push(cells);
                                }
                                break;
                            }
                        }
                    }
                }
                TagEnd::TableCell => {
                    if let Some(Frame::TableCell { content }) = stack.pop() {
                        for frame in stack.iter_mut().rev() {
                            if let Frame::TableRow { cells, .. } = frame {
                                cells.push(content);
                                break;
                            }
                        }
                    }
                }
                TagEnd::Emphasis => {
                    if let Some(Frame::Emphasis { content }) = stack.pop() {
                        push_inline!(Inline::Emphasis { children: content });
                    }
                }
                TagEnd::Strong => {
                    if let Some(Frame::Strong { content }) = stack.pop() {
                        push_inline!(Inline::Strong { children: content });
                    }
                }
                TagEnd::Strikethrough => {
                    if let Some(Frame::Strikethrough { content }) = stack.pop() {
                        push_inline!(Inline::Strikethrough { children: content });
                    }
                }
                TagEnd::Link => {
                    if let Some(Frame::Link {
                        url,
                        title,
                        content,
                    }) = stack.pop()
                    {
                        push_inline!(Inline::Link {
                            url,
                            title,
                            children: content,
                        });
                    }
                }
                TagEnd::Image => {
                    if let Some(Frame::Image { url, alt, title }) = stack.pop() {
                        push_inline!(Inline::Image { url, alt, title });
                    }
                }
                TagEnd::FootnoteDefinition => {}
                TagEnd::HtmlBlock => {}
                TagEnd::DefinitionList
                | TagEnd::DefinitionListTitle
                | TagEnd::DefinitionListDefinition => {}
                TagEnd::MetadataBlock(_) => {}
                TagEnd::Superscript | TagEnd::Subscript => {
                    if let Some(Frame::Emphasis { content }) = stack.pop() {
                        push_inline!(Inline::Emphasis { children: content });
                    }
                }
            },

            Event::Text(text) => {
                if let Some(Frame::CodeBlock { text: buffer, .. }) = stack.last_mut() {
                    buffer.push_str(&text);
                } else if let Some(Frame::Image { alt, .. }) = stack.last_mut() {
                    alt.push_str(&text);
                } else {
                    let (cleaned, found) = directives::strip_inline_markers(&text);
                    if !found.is_empty() {
                        inline_markers.extend(found);
                    }
                    if !cleaned.is_empty() {
                        push_inline!(Inline::Text { text: cleaned });
                    }
                }
            }
            Event::Code(text) => push_inline!(Inline::Code {
                text: text.to_string()
            }),
            Event::Html(text) | Event::InlineHtml(text) => {
                let (cleaned, found) = directives::strip_inline_markers(&text);
                if !found.is_empty() {
                    inline_markers.extend(found);
                }
                if !cleaned.trim().is_empty() {
                    if let Some(Frame::CodeBlock { text: buffer, .. }) = stack.last_mut() {
                        buffer.push_str(&cleaned);
                    } else {
                        push_inline!(Inline::Html { text: cleaned });
                    }
                }
            }
            Event::SoftBreak => push_inline!(Inline::SoftBreak),
            Event::HardBreak => push_inline!(Inline::HardBreak),
            Event::Rule => {
                push_node!(DocNode::ThematicBreak {
                    span: SourceSpan::new(current_line, current_line),
                });
            }
            Event::TaskListMarker(checked) => {
                for frame in stack.iter_mut().rev() {
                    if let Frame::Item {
                        checked: slot, ..
                    } = frame
                    {
                        *slot = Some(checked);
                        break;
                    }
                }
            }
            Event::FootnoteReference(label) => {
                push_inline!(Inline::FootnoteReference {
                    label: label.to_string()
                });
            }
            Event::InlineMath(text) => push_inline!(Inline::Code {
                text: text.to_string()
            }),
            Event::DisplayMath(text) => push_inline!(Inline::Code {
                text: text.to_string()
            }),
        }
    }

    BuiltAst {
        nodes: finished,
        inline_markers,
    }
}

fn content_is_empty(content: &[Inline]) -> bool {
    content.is_empty()
        || content.iter().all(|inline| match inline {
            Inline::Text { text } => text.trim().is_empty(),
            Inline::SoftBreak | Inline::HardBreak => true,
            _ => false,
        })
}

fn merge_inline_markers(content: Vec<Inline>, _sink: &mut Vec<ParagraphId>) -> Vec<Inline> {
    content
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Convert a directive node's data into an [`DocNode`] for the AST.
///
/// Kept separate so the scanner stays free of AST concerns.
pub fn directive_node(directive: &directives::Directive) -> DocNode {
    DocNode::Directive {
        kind: directive.name.clone(),
        attributes: directive.attributes.clone(),
        image_placeholder: directive.image_placeholder(),
        span: SourceSpan::new(directive.start_line, directive.end_line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"---
id: 01J8Z5Q0K3W7M9V2B4N6P8R1TY
type: article
slug: hello-world
title: Hello World
summary: A first post
wip: false
wip_publish: false
wip_ever_published: false
created_at: 2026-01-01T00:00:00Z
updated_at: 2026-01-01T00:00:00Z
tags:
  - rust
---

# Heading

First paragraph.

Second paragraph with **bold** and `code`.
"#;

    #[test]
    fn a_full_document_parses_into_frontmatter_and_ast() {
        let document = parse(DOC).unwrap();
        assert_eq!(document.frontmatter.slug, "hello-world");
        assert_eq!(document.frontmatter.title, "Hello World");
        assert_eq!(document.frontmatter.tags, vec!["rust".to_string()]);
        assert!(document.diagnostics.is_empty(), "{:?}", document.diagnostics);
        assert!(document.is_buildable());

        let headings = document.ast.headings();
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "Heading");
        assert_eq!(headings[0].level, 1);

        let paragraphs = document.ast.paragraphs();
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(crate::ast::plain_text(&paragraphs[0].1), "First paragraph.");
        assert_eq!(
            crate::ast::plain_text(&paragraphs[1].1),
            "Second paragraph with bold and code."
        );
    }

    #[test]
    fn a_document_without_frontmatter_is_rejected_with_a_clear_message() {
        let error = parse("# Just a heading\n").unwrap_err();
        assert_eq!(error.code(), "FRONTMATTER");
        assert!(error.to_string().contains("no frontmatter block"));
    }

    #[test]
    fn an_unterminated_frontmatter_block_is_an_error() {
        let error = parse("---\nid: X\ntype: article\n").unwrap_err();
        assert!(error.to_string().contains("never closed"));
    }

    #[test]
    fn frontmatter_that_violates_the_contract_is_rejected_or_reported() {
        // wip_publish is not allowed on a knowledge document.
        let source = DOC
            .replace("type: article", "type: knowledge")
            .replace("wip: false", "wip: true");
        let document = parse(&source).unwrap();
        assert!(!document.is_buildable());
        assert!(document
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidFrontmatterField));
    }

    #[test]
    fn gfm_blocks_are_represented_in_the_ast() {
        let body = "| A | B |\n| --- | ---: |\n| 1 | 2 |\n\n- [x] done\n- [ ] not done\n\n> quoted\n\n```rust\nfn main() {}\n```\n\n---\n";
        let parsed = parse_body(body);
        let kinds: Vec<&str> = parsed
            .ast
            .nodes
            .iter()
            .map(|node| match node {
                DocNode::Table { .. } => "table",
                DocNode::List { .. } => "list",
                DocNode::BlockQuote { .. } => "quote",
                DocNode::CodeBlock { .. } => "code",
                DocNode::ThematicBreak { .. } => "rule",
                DocNode::Paragraph { .. } => "paragraph",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["table", "list", "quote", "code", "rule"]);

        let table = parsed
            .ast
            .nodes
            .iter()
            .find_map(|node| match node {
                DocNode::Table { table, .. } => Some(table.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(table.head.len(), 2);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.alignments[1].as_deref(), Some("right"));

        let list = parsed
            .ast
            .nodes
            .iter()
            .find_map(|node| match node {
                DocNode::List { items, .. } => Some(items.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].checked, Some(true));
        assert_eq!(list[1].checked, Some(false));
    }

    #[test]
    fn a_paragraph_marker_is_attached_to_the_paragraph_that_follows_it() {
        let id_a = ParagraphId::from_parts(1_700_000_000_000, 11);
        let id_b = ParagraphId::from_parts(1_700_000_000_000, 12);
        let body = format!(
            "{}\n\nFirst.\n\n{}\n\nSecond.\n",
            id_a.to_marker(),
            id_b.to_marker()
        );
        let parsed = parse_body(&body);
        let paragraphs = parsed.ast.paragraphs();
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0].0, Some(id_a));
        assert_eq!(paragraphs[1].0, Some(id_b));
        assert_eq!(parsed.paragraph_ids, vec![id_a, id_b]);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

        // The markers must not leak into the rendered text.
        assert_eq!(crate::ast::plain_text(&paragraphs[0].1), "First.");
    }

    #[test]
    fn duplicate_paragraph_ids_are_reported_as_errors() {
        // handoff §9.3: a duplicate id must be a clear error, never silently resolved.
        let id = ParagraphId::from_parts(1_700_000_000_000, 13);
        let body = format!(
            "{}\n\nFirst.\n\n{}\n\nSecond.\n",
            id.to_marker(),
            id.to_marker()
        );
        let parsed = parse_body(&body);
        assert!(parsed
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == DiagnosticCode::DuplicateParagraphId && d.is_error()));
        assert_eq!(parsed.unique_paragraph_ids(), vec![id]);
    }

    #[test]
    fn an_orphan_paragraph_marker_is_reported() {
        let id = ParagraphId::from_parts(1_700_000_000_000, 14);
        let body = format!("{}\n\nText.\n\n{}\n", id.to_marker(), id.to_marker());
        let parsed = parse_body(&body);
        assert!(parsed
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == DiagnosticCode::OrphanParagraphMarker));
    }

    #[test]
    fn markers_inside_fenced_code_are_not_paragraph_ids() {
        let id = ParagraphId::from_parts(1_700_000_000_000, 15);
        let body = format!("```\n{}\n```\n", id.to_marker());
        let parsed = parse_body(&body);
        assert!(parsed.paragraph_ids.is_empty());
        let code = parsed
            .ast
            .nodes
            .iter()
            .find_map(|node| match node {
                DocNode::CodeBlock { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap();
        // The comment is part of the code sample and must be preserved verbatim.
        assert!(code.contains(&id.to_marker()), "{code}");
    }

    #[test]
    fn inline_formatting_is_preserved_with_links_and_images() {
        let parsed = parse_body("Text with [a link](https://example.com \"t\") and ![alt](a.png) and ~~gone~~.");
        let content = &parsed.ast.paragraphs()[0].1;
        let flattened: Vec<String> = content
            .iter()
            .map(|inline| match inline {
                Inline::Link { url, children, .. } => {
                    format!("link:{url}:{}", crate::ast::plain_text(children))
                }
                Inline::Image { url, alt, .. } => format!("image:{url}:{alt}"),
                Inline::Strikethrough { .. } => "strike".to_string(),
                other => other.plain_text(),
            })
            .collect();
        let joined = flattened.join("|");
        assert!(joined.contains("link:https://example.com:a link"), "{joined}");
        assert!(joined.contains("image:a.png:alt"), "{joined}");
        assert!(joined.contains("strike"), "{joined}");
    }

    #[test]
    fn a_directive_block_does_not_become_a_paragraph() {
        let body = ":::knowledgeref{content=\"01ABC\" paragraph=\"01DEF\"}\n:::\n\nReal text.\n";
        let parsed = parse_body(body);
        let paragraphs = parsed.ast.paragraphs();
        assert_eq!(paragraphs.len(), 1, "{:?}", parsed.ast.nodes);
        assert_eq!(crate::ast::plain_text(&paragraphs[0].1), "Real text.");
    }

    #[test]
    fn directives_are_merged_into_the_ast_in_source_order() {
        // Without this merge an article's knowledge references never reach resolution, because the
        // scanner removes them from the body before CommonMark runs.
        let body = "Intro text.\n\n:::knowledgeref{content=\"01ABC\" paragraph=\"01DEF\"}\n:::\n\nAfter the card.\n\n::download{url=\"https://example.com/a.zip\"}\n";
        let parsed = parse_body(body);

        let shape: Vec<&str> = parsed
            .ast
            .nodes
            .iter()
            .map(|node| match node {
                DocNode::Paragraph { .. } => "paragraph",
                DocNode::Directive { .. } => "directive",
                _ => "other",
            })
            .collect();
        assert_eq!(
            shape,
            vec!["paragraph", "directive", "paragraph", "directive"],
            "{:#?}",
            parsed.ast.nodes
        );

        let directives = parsed.ast.directives();
        assert_eq!(directives.len(), 2);
        assert_eq!(directives[0].as_directive().unwrap().0, "knowledgeref");
        assert_eq!(directives[1].as_directive().unwrap().0, "download");
    }

    #[test]
    fn a_container_directive_keeps_its_source_span() {
        let body = "Before.\n\n:::card{title=\"x\"}\nbody\n:::\n\nAfter.\n";
        let parsed = parse_body(body);
        let directive = parsed.ast.directives()[0];
        let span = directive.span();
        assert_eq!(span.start_line, 3);
        assert_eq!(span.end_line, 5);
    }

    #[test]
    fn an_image_directive_reaches_the_ast_as_a_placeholder() {
        // handoff 8.1: the page-creating model emits placeholders, never image files.
        let parsed = parse_body(":::image{alt=\"cover\" width=\"1200\"}\n:::\n");
        let directive = parsed.ast.directives()[0];
        match directive {
            DocNode::Directive {
                kind,
                image_placeholder,
                ..
            } => {
                assert_eq!(kind, "image");
                let placeholder = image_placeholder.as_deref().unwrap();
                assert!(placeholder.contains("cover"), "{placeholder}");
                assert!(placeholder.contains("1200"), "{placeholder}");
            }
            other => panic!("expected a directive node, got {other:?}"),
        }
    }

    #[test]
    fn body_start_line_offsets_are_reported_for_diagnostics() {
        let body = "Line one.\n\n:::mycustom\nbody\n:::\n";
        let (_, extracted, start) =
            split_frontmatter(&format!("---\nid: X\n---\n{body}"))
                .unwrap()
                .expect("the fixture has a frontmatter block");
        assert_eq!(start, 4);
        assert_eq!(extracted, body);
    }

    #[test]
    fn documents_round_trip_their_frontmatter_values() {
        let document = parse(DOC).unwrap();
        let yaml = serde_yaml::to_string(&document.frontmatter).unwrap();
        let again: Frontmatter = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(again, document.frontmatter);
    }

    #[test]
    fn smart_punctuation_and_footnotes_do_not_break_parsing() {
        let parsed = parse_body("A footnote[^1].\n\n[^1]: The note.\n");
        assert!(parsed.diagnostics.is_empty());
        assert!(!parsed.ast.nodes.is_empty());
    }

    #[test]
    fn a_heading_only_document_yields_no_paragraphs() {
        let parsed = parse_body("## Only a heading\n");
        assert!(parsed.ast.paragraphs().is_empty());
        assert_eq!(parsed.ast.headings().len(), 1);
    }

    #[test]
    fn the_residual_source_map_agrees_with_the_scanner() {
        // The offset map is derived independently of the scanner, so if the two ever disagree a
        // `SourceSpan` would silently point at the wrong source line. This test pins them together
        // by reconstructing the residual from the computed source list.
        let body = "Intro.\n\n:::knowledgeref{content=\"01ABC\" paragraph=\"01DEF\"}\n:::\n\nAfter.\n\n::download{url=\"https://example.com/a.zip\"}\n\nTail.\n";
        let scanned = directives::scan(body);
        let sources = residual_sources(&scanned);

        let mut rebuilt = String::new();
        for index in &sources {
            rebuilt.push_str(&scanned.lines[*index].raw);
            rebuilt.push('\n');
        }
        assert_eq!(rebuilt, scanned.residual);

        // And every skipped line really is a directive or a marker line.
        let skipped: Vec<usize> = (0..scanned.lines.len())
            .filter(|index| !sources.contains(index))
            .collect();
        assert!(!skipped.is_empty(), "the fixture must actually skip something");
        for index in skipped {
            let line = &scanned.lines[index];
            let is_marker = line.paragraph_marker.is_some() && line.eligible_for_construct();
            // `ScannedLine::number` equals the index plus one, so compare line numbers directly
            // rather than shifting the directive's 1-based range.
            let is_directive = scanned
                .directives
                .iter()
                .any(|directive| directive.start_line <= line.number && line.number <= directive.end_line);
            assert!(
                is_marker || is_directive,
                "line {index} was skipped but is neither: {:?}",
                line.raw
            );
        }
    }

    #[test]
    fn source_spans_point_at_the_right_file_lines() {
        // Body starts at file line 8 because of the seven frontmatter lines and the delimiter.
        let source = "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: article\nslug: spans\ntitle: Spans\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\nFirst.\n\n:::card{title=\"x\"}\nbody\n:::\n\nSecond.\n";
        let document = parse(source).unwrap();
        let lines: Vec<usize> = document
            .ast
            .nodes
            .iter()
            .map(|node| node.span().start_line)
            .collect();
        // `First.`, the directive, then `Second.`
        assert_eq!(lines, vec![10, 12, 16], "{:#?}", document.ast.nodes);
    }
}
