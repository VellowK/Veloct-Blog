//! Writing Markdown back.
//!
//! `handoff.md` §3.4 makes Markdown the only body truth, so every write is a *surgical* edit of
//! the user's own file rather than a regeneration from the AST. Regenerating would silently
//! reformat a document the user owns, and the difference would then show up as a phantom change
//! in the next publish comparison (§14.3).
//!
//! Frontmatter is edited key by key for the same reason: unknown keys and the user's own key
//! order survive.

use std::collections::BTreeMap;

use veloct_core::{Result, VeloctError};

use crate::ast::{Ast, DocNode, ParagraphId};
use crate::frontmatter::Frontmatter;
use crate::parse::split_frontmatter;
use crate::directives;

/// The delimiter line that opens and closes a frontmatter block.
const DELIMITER: &str = "---";

/// A document split into its parts, preserving everything not explicitly edited.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceParts {
    /// Raw frontmatter YAML text, without the delimiters.
    pub frontmatter_yaml: String,
    pub body: String,
    /// True when the source had a frontmatter block at all.
    pub had_frontmatter: bool,
}

/// Split a source file into frontmatter text and body, without interpreting either.
pub fn split_parts(source: &str) -> SourceParts {
    let mut lines = source.split('\n');
    let first = lines
        .next()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .unwrap_or_default();

    if first.trim() != DELIMITER {
        return SourceParts {
            frontmatter_yaml: String::new(),
            body: source.to_string(),
            had_frontmatter: false,
        };
    }

    let mut yaml = String::new();
    let mut body_lines: Vec<String> = Vec::new();
    let mut closed = false;
    for line in lines {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !closed && line.trim() == DELIMITER {
            closed = true;
            continue;
        }
        if closed {
            body_lines.push(line.to_string());
        } else {
            yaml.push_str(line);
            yaml.push('\n');
        }
    }

    SourceParts {
        frontmatter_yaml: yaml,
        body: body_lines.join("\n"),
        had_frontmatter: true,
    }
}

/// Reassemble a source file.
///
/// The YAML is trimmed of the trailing newline the splitter adds, so a round trip through
/// [`split_parts`] and [`assemble`] is byte-identical for an untouched file.
pub fn assemble(frontmatter_yaml: &str, body: &str) -> String {
    let mut out = String::with_capacity(frontmatter_yaml.len() + body.len() + 16);
    out.push_str(DELIMITER);
    out.push('\n');
    out.push_str(frontmatter_yaml.trim_end_matches('\n'));
    out.push('\n');
    out.push_str(DELIMITER);
    out.push('\n');
    out.push_str(body);
    out
}

/// Serialise frontmatter and a body into a document.
pub fn serialize(frontmatter: &Frontmatter, body: &str) -> Result<String> {
    let yaml = serde_yaml::to_string(frontmatter).map_err(|e| {
        VeloctError::Frontmatter {
            path: std::path::PathBuf::from("<document>"),
            message: format!("could not serialise frontmatter: {e}"),
        }
    })?;
    Ok(assemble(&yaml, body))
}

/// Update frontmatter keys without disturbing anything else in the file.
///
/// This is the primitive behind every editor action that touches metadata (toggling WIP, editing
/// a summary, changing a slug): the body is never reparsed and never rewritten.
pub fn set_frontmatter_values(
    source: &str,
    updates: &[(&str, serde_yaml::Value)],
) -> Result<String> {
    let parts = split_parts(source);

    let mut mapping: serde_yaml::Mapping = if parts.frontmatter_yaml.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        match serde_yaml::from_str::<serde_yaml::Value>(&parts.frontmatter_yaml) {
            Ok(serde_yaml::Value::Mapping(mapping)) => mapping,
            Ok(serde_yaml::Value::Null) => serde_yaml::Mapping::new(),
            Ok(_) => {
                return Err(VeloctError::Frontmatter {
                    path: std::path::PathBuf::from("<document>"),
                    message: "frontmatter must be a YAML mapping".into(),
                })
            }
            Err(e) => {
                return Err(VeloctError::Frontmatter {
                    path: std::path::PathBuf::from("<document>"),
                    message: format!("invalid YAML: {e}"),
                })
            }
        }
    };

    for (key, value) in updates {
        let key_value = serde_yaml::Value::String((*key).to_string());
        if value.is_null() {
            mapping.remove(&key_value);
        } else {
            // `Mapping::insert` keeps an existing key's position, which is what preserves the
            // user's own ordering for keys we are updating in place.
            mapping.insert(key_value, value.clone());
        }
    }

    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(mapping)).map_err(|e| {
        VeloctError::Frontmatter {
            path: std::path::PathBuf::from("<document>"),
            message: format!("could not serialise frontmatter: {e}"),
        }
    })?;

    Ok(assemble(&yaml, &parts.body))
}

/// Whether a trimmed line opens a Markdown block construct that is not a paragraph.
fn is_block_opener(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.starts_with('#') {
        return true;
    }
    if trimmed.starts_with('>') {
        return true;
    }
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        return true;
    }
    // Directives.
    if trimmed.starts_with(":::") || trimmed.starts_with("::") {
        return true;
    }
    // Paragraph markers.
    if trimmed.starts_with("<!--p:") {
        return true;
    }
    // Thematic breaks.
    if trimmed.len() >= 3
        && (trimmed.chars().all(|c| c == '-')
            || trimmed.chars().all(|c| c == '*')
            || trimmed.chars().all(|c| c == '_'))
    {
        return true;
    }
    // List items.
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        let _ = rest;
        return true;
    }
    // Ordered list items such as `1. ` or `2) `.
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let rest = &trimmed[digits.len()..];
        if rest.starts_with(". ") || rest.starts_with(") ") {
            return true;
        }
    }
    false
}

/// Insert the missing paragraph markers a knowledge document needs.
///
/// `handoff.md` §9.2 requires ids the editor maintains; this is that maintenance, expressed as a
/// minimal edit of the source. Existing markers are never moved or renumbered, and a paragraph
/// that already has an id -- recognised by the marker line immediately above it -- is left alone.
/// That makes the operation idempotent, which matters because it runs on every save.
///
/// Returns the new source and the ids that were added.
pub fn assign_paragraph_ids(
    source: &str,
    mint: &mut dyn FnMut() -> ParagraphId,
) -> (String, Vec<ParagraphId>) {
    let parts = split_parts(source);
    let scan = directives::scan(&parts.body);
    let mut out_lines: Vec<String> = Vec::with_capacity(scan.lines.len() + 8);
    let mut added: Vec<ParagraphId> = Vec::new();

    // `paragraph_open` is true once at least one content line of the current paragraph has been
    // emitted, so continuation lines are not mistaken for new paragraphs.
    let mut paragraph_open = false;

    for (index, line) in scan.lines.iter().enumerate() {
        let is_blank = line.raw.trim().is_empty();
        let is_block = is_blank || is_block_opener(&line.raw);
        let has_marker_above = index > 0
            && scan.lines[index - 1].paragraph_marker.is_some()
            && !scan.lines[index - 1].in_fence;

        let starts_paragraph = !is_block && !paragraph_open && line.eligible_for_construct();

        if starts_paragraph && !has_marker_above {
            let id = mint();
            // The marker is emitted as its own block, immediately above the paragraph.
            out_lines.push(id.to_marker());
            added.push(id);
        }

        // A blank line, a block construct or a top-level marker ends the current paragraph.
        paragraph_open = !is_block && !line.raw.trim_start().starts_with("<!--p:");

        out_lines.push(line.raw.clone());
    }

    let body = out_lines.join("\n");
    (assemble(&parts.frontmatter_yaml, &body), added)
}

/// Find the line index of the marker for `paragraph_id`, plus the paragraph's content span.
///
/// Returning `None` for the span (rather than an empty span) is deliberate: a marker with no
/// paragraph after it is the orphan case, and the caller must be able to tell that apart from an
/// empty paragraph. Blank lines between the marker and the prose are tolerated, because a user may
/// leave one there.
fn paragraph_span(lines: &[directives::ScannedLine], paragraph_id: ParagraphId) -> Option<(usize, Option<(usize, usize)>)> {
    let marker_index = lines.iter().position(|line| {
        line.paragraph_marker == Some(paragraph_id) && !line.in_fence
    })?;

    // `ScannedLine::number` is 1-based and equals the index plus one, so the remainder of the
    // paragraph starts at `marker_index + 1` in slice terms.
    let mut start = marker_index + 1;
    while start < lines.len() && lines[start].raw.trim().is_empty() {
        start += 1;
    }
    if start >= lines.len() || is_block_opener(&lines[start].raw) {
        return Some((marker_index, None));
    }

    let mut end = start;
    while end < lines.len() {
        let trimmed = lines[end].raw.trim_start();
        if trimmed.is_empty()
            || directives::read_paragraph_marker(trimmed).is_some()
            || is_block_opener(&lines[end].raw)
        {
            break;
        }
        end += 1;
    }

    Some((marker_index, Some((start, end))))
}

/// Replace the text of one paragraph, identified by its stable id.
///
/// The replacement is line-based rather than AST-based so the surrounding document, including
/// formatting the AST does not model, is untouched. Returns `false` when the id is not present,
/// which is information the caller needs: it means the paragraph was removed by an external edit
/// (`handoff.md` §9.3, `PARAGRAPH_ID_LOST`).
pub fn replace_paragraph_text(
    source: &str,
    paragraph_id: ParagraphId,
    replacement: &str,
) -> Result<(String, bool)> {
    let parts = split_parts(source);
    let lines: Vec<&str> = parts.body.split('\n').collect();
    let scan = directives::scan(&parts.body);

    let Some((_, span)) = paragraph_span(&scan.lines, paragraph_id) else {
        return Ok((source.to_string(), false));
    };
    let Some((start, end)) = span else {
        return Ok((source.to_string(), false));
    };

    let mut rebuilt: Vec<String> = Vec::with_capacity(lines.len());
    rebuilt.extend(lines[..start].iter().map(|line| line.to_string()));
    rebuilt.extend(replacement.split('\n').map(|line| line.to_string()));
    rebuilt.extend(lines[end..].iter().map(|line| line.to_string()));

    Ok((assemble(&parts.frontmatter_yaml, &rebuilt.join("\n")), true))
}

/// Rewrite every public URL that points at an attachment, for the explicit tool
/// `handoff.md` §13.6 requires.
///
/// Changing a storage target's public URL prefix never moves or deletes remote files, so the
/// only safe action is this explicit rewrite, and the caller decides when to run it.
pub fn rewrite_references(source: &str, rewrites: &BTreeMap<String, String>) -> (String, usize) {
    let mut out = source.to_string();
    let mut total = 0usize;
    for (from, to) in rewrites {
        if from.is_empty() || from == to {
            continue;
        }
        let before = out.matches(from.as_str()).count();
        if before > 0 {
            out = out.replace(from.as_str(), to);
            total += before;
        }
    }
    (out, total)
}

/// Replace every occurrence of a directive attribute value.
///
/// Used to re-point a `knowledgeref` at a repaired paragraph without disturbing the rest of the
/// document, which is what the broken-reference repair flow needs (`handoff.md` §9.3).
pub fn retarget_directive_attribute(
    source: &str,
    directive_name: &str,
    attribute: &str,
    from: &str,
    to: &str,
) -> (String, usize) {
    let parts = split_parts(source);
    let scan = directives::scan(&parts.body);
    let mut replacements = 0usize;
    let mut out_lines: Vec<String> = Vec::with_capacity(scan.lines.len());
    let mut consumed_until = 0usize;

    for directive in &scan.directives {
        if directive.name != directive_name {
            continue;
        }
        if directive.attribute(attribute) != Some(from) {
            continue;
        }

        let start_index = directive.start_line.saturating_sub(1);
        let end_index = directive.end_line.saturating_sub(1);
        if start_index < consumed_until || end_index >= scan.lines.len() {
            continue;
        }

        while consumed_until < start_index {
            out_lines.push(scan.lines[consumed_until].raw.clone());
            consumed_until += 1;
        }

        let mut updated = directive.clone();
        for (key, value) in updated.attributes.iter_mut() {
            if key == attribute && value == from {
                *value = to.to_string();
            }
        }
        out_lines.push(updated.to_markdown());
        consumed_until = end_index + 1;
        replacements += 1;
    }

    while consumed_until < scan.lines.len() {
        out_lines.push(scan.lines[consumed_until].raw.clone());
        consumed_until += 1;
    }

    if replacements == 0 {
        return (source.to_string(), 0);
    }
    (assemble(&parts.frontmatter_yaml, &out_lines.join("\n")), replacements)
}

/// Extract the paragraph text found by id, for a repair UI that has to show the user what is
/// currently at a location.
pub fn paragraph_text_for_id(source: &str, paragraph_id: ParagraphId) -> Option<String> {
    let parts = split_parts(source);
    let scan = directives::scan(&parts.body);
    let (_, span) = paragraph_span(&scan.lines, paragraph_id)?;
    let (start, end) = span?;
    let collected: Vec<String> = scan.lines[start..end]
        .iter()
        .map(|line| line.raw.clone())
        .collect();
    if collected.is_empty() {
        None
    } else {
        Some(collected.join("\n"))
    }
}

/// Count the paragraphs in an AST that still lack an id, so a caller can report progress.
pub fn paragraphs_missing_ids(ast: &Ast) -> usize {
    let mut missing = 0usize;
    ast.visit(&mut |node| {
        if let DocNode::Paragraph { paragraph_id, .. } = node {
            if paragraph_id.is_none() {
                missing += 1;
            }
        }
    });
    missing
}

/// Round-trip check: parse a source, re-serialise the frontmatter, and confirm the body is
/// untouched. Used by tests and by the editor before writing.
pub fn body_round_trips(source: &str) -> Result<bool> {
    let (_, original_body, _) = match split_frontmatter(source)? {
        Some(parts) => parts,
        None => return Ok(false),
    };
    let parts = split_parts(source);
    Ok(parts.body == original_body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const DOC: &str = "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: knowledge\nslug: ownership\ntitle: Ownership\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\ncustom: keep me\n---\n\nFirst paragraph.\n\nSecond paragraph.\n";

    fn id(timestamp: u64, randomness: u128) -> ParagraphId {
        ParagraphId::from_parts(timestamp, randomness)
    }

    #[test]
    fn split_and_assemble_round_trip_byte_for_byte() {
        let parts = split_parts(DOC);
        assert!(parts.had_frontmatter);
        assert!(parts.frontmatter_yaml.contains("custom: keep me"));
        assert_eq!(assemble(&parts.frontmatter_yaml, &parts.body), DOC);
    }

    #[test]
    fn a_source_without_frontmatter_is_reported_as_such() {
        let parts = split_parts("# Heading\n\nBody\n");
        assert!(!parts.had_frontmatter);
        assert!(parts.frontmatter_yaml.is_empty());
        assert_eq!(parts.body, "# Heading\n\nBody\n");
    }

    #[test]
    fn setting_a_frontmatter_value_preserves_the_body_and_unknown_keys() {
        // handoff §3.4: the body must not be regenerated.
        let updated = set_frontmatter_values(
            DOC,
            &[("wip_publish", serde_yaml::Value::Bool(true))],
        )
        .unwrap();

        assert!(updated.contains("custom: keep me"));
        assert!(updated.contains("wip_publish: true"));
        let original_body = split_parts(DOC).body;
        assert_eq!(split_parts(&updated).body, original_body);

        // And the result still parses.
        let document = parse::parse(&updated).unwrap();
        assert!(document.frontmatter.wip_publish);
    }

    #[test]
    fn updating_an_existing_key_keeps_the_users_key_order() {
        let updated = set_frontmatter_values(DOC, &[("title", serde_yaml::Value::String("New".into()))])
            .unwrap();
        let keys: Vec<&str> = updated
            .lines()
            .skip(1)
            .take_while(|line| *line != "---")
            .filter(|line| !line.starts_with(' '))
            .filter_map(|line| line.split(':').next())
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "type",
                "slug",
                "title",
                "created_at",
                "updated_at",
                "custom"
            ]
        );
    }

    #[test]
    fn a_null_update_removes_the_key() {
        let updated =
            set_frontmatter_values(DOC, &[("custom", serde_yaml::Value::Null)]).unwrap();
        assert!(!updated.contains("custom:"));
    }

    #[test]
    fn invalid_frontmatter_yaml_is_rejected_rather_than_silently_replaced() {
        let broken = "---\nid: [unclosed\n---\n\nbody\n";
        let error = set_frontmatter_values(broken, &[("title", serde_yaml::Value::String("x".into()))])
            .unwrap_err();
        assert_eq!(error.code(), "FRONTMATTER");
    }

    #[test]
    fn missing_paragraph_ids_are_inserted_before_each_paragraph() {
        let mut counter = 0u128;
        let (updated, added) = assign_paragraph_ids(DOC, &mut || {
            counter += 1;
            id(1_700_000_000_000, counter)
        });

        assert_eq!(added.len(), 2, "two paragraphs needed ids: {updated}");
        assert_eq!(added[0], id(1_700_000_000_000, 1));

        let document = parse::parse(&updated).unwrap();
        let paragraphs = document.ast.paragraphs();
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0].0, Some(added[0]));
        assert_eq!(paragraphs[1].0, Some(added[1]));
        assert!(document.diagnostics.is_empty(), "{:?}", document.diagnostics);
        assert!(document.is_buildable());
    }

    #[test]
    fn assigning_ids_does_not_touch_content_that_is_not_a_paragraph() {
        let source = "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: knowledge\nslug: s\ntitle: T\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\n# Heading\n\n- a list item\n\n```\ncode\n```\n\n> quote\n\n:::note\nbody\n:::\n\nReal paragraph.\n";
        let mut counter = 0u128;
        let (updated, added) = assign_paragraph_ids(source, &mut || {
            counter += 1;
            id(1_700_000_000_000, counter)
        });
        // Only the real paragraph, the list item and the quote contents; the heading, code fence
        // and directive are block constructs.
        assert!(!added.is_empty());
        assert!(updated.contains("Real paragraph."));
        // The heading must not have gained a marker directly above it.
        let lines: Vec<&str> = updated.lines().collect();
        let heading_index = lines.iter().position(|l| l.starts_with("# Heading")).unwrap();
        assert!(!lines[heading_index - 1].starts_with("<!--p:"));
    }

    #[test]
    fn assigning_ids_twice_adds_nothing_the_second_time() {
        let mut counter = 0u128;
        let (once, added_first) = assign_paragraph_ids(DOC, &mut || {
            counter += 1;
            id(1_700_000_000_000, counter)
        });
        let (twice, added_second) = assign_paragraph_ids(&once, &mut || {
            counter += 1;
            id(1_700_000_000_000, counter)
        });
        assert!(!added_first.is_empty());
        assert!(added_second.is_empty(), "ids must not be re-minted");
        assert_eq!(once, twice);
    }

    #[test]
    fn replacing_a_paragraph_by_id_changes_only_that_paragraph() {
        let paragraph_id = id(1_700_000_000_000, 21);
        let source = format!(
            "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: knowledge\nslug: s\ntitle: T\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\n{}\n\nOld text.\n\nOther paragraph.\n",
            paragraph_id.to_marker()
        );

        let (updated, found) =
            replace_paragraph_text(&source, paragraph_id, "New text.").unwrap();
        assert!(found);
        assert!(updated.contains("New text."));
        assert!(!updated.contains("Old text."));
        assert!(updated.contains("Other paragraph."));
        assert!(updated.contains(&paragraph_id.to_marker()));
    }

    #[test]
    fn replacing_a_paragraph_that_no_longer_exists_reports_that_it_was_not_found() {
        // handoff §9.3: a lost id must be reportable, not silently recreated.
        let (updated, found) =
            replace_paragraph_text(DOC, id(1_700_000_000_000, 99), "irrelevant").unwrap();
        assert!(!found);
        assert_eq!(updated, DOC);
    }

    #[test]
    fn paragraph_text_can_be_read_back_by_id() {
        let paragraph_id = id(1_700_000_000_000, 31);
        let source = format!(
            "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: knowledge\nslug: s\ntitle: T\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\n{}\n\nSome text.\nMore of it.\n\nNext.\n",
            paragraph_id.to_marker()
        );
        let text = paragraph_text_for_id(&source, paragraph_id).unwrap();
        assert_eq!(text, "Some text.\nMore of it.");
        assert!(paragraph_text_for_id(&source, id(1_700_000_000_000, 32)).is_none());
    }

    #[test]
    fn a_directive_attribute_can_be_retargeted_for_repair() {
        // The broken-reference repair flow from handoff §9.3.
        let old = id(1_700_000_000_000, 41);
        let new = id(1_700_000_000_000, 42);
        let source = format!(
            "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: article\nslug: a\ntitle: A\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\n:::knowledgeref{{content=\"01ABC\" paragraph=\"{old}\"}}\n:::\n\nOther text.\n"
        );

        let (updated, count) =
            retarget_directive_attribute(&source, "knowledgeref", "paragraph", &old.to_string(), &new.to_string());
        assert_eq!(count, 1);
        assert!(updated.contains(&new.to_string()));
        assert!(!updated.contains(&old.to_string()));
        assert!(updated.contains("Other text."));

        // A non-matching target is left alone and reported as zero replacements.
        let (unchanged, count) =
            retarget_directive_attribute(&updated, "knowledgeref", "paragraph", &old.to_string(), &new.to_string());
        assert_eq!(count, 0);
        assert_eq!(unchanged, updated);
    }

    #[test]
    fn reference_rewriting_reports_how_many_it_changed() {
        // handoff §13.6: the explicit rewrite tool, never an automatic migration.
        let source = "![a](https://old.example.com/a.png)\n\n![b](https://old.example.com/b.png)\n\n![c](https://other.example.com/c.png)";
        let mut rewrites = BTreeMap::new();
        rewrites.insert(
            "https://old.example.com/".to_string(),
            "https://new.example.com/".to_string(),
        );
        let (updated, count) = rewrite_references(source, &rewrites);
        assert_eq!(count, 2);
        assert!(updated.contains("https://new.example.com/a.png"));
        assert!(updated.contains("https://other.example.com/c.png"));
        assert!(!updated.contains("old.example.com"));
    }

    #[test]
    fn paragraph_id_counting_reflects_the_ast() {
        let parsed = parse::parse_body("One.\n\nTwo.\n");
        assert_eq!(paragraphs_missing_ids(&parsed.ast), 2);
    }

    #[test]
    fn serialising_frontmatter_plus_body_reproduces_a_parseable_document() {
        let document = parse::parse(DOC).unwrap();
        let serialised = serialize(&document.frontmatter, &document.body).unwrap();
        let reparsed = parse::parse(&serialised).unwrap();
        assert_eq!(reparsed.frontmatter, document.frontmatter);
        assert_eq!(reparsed.body, document.body);
    }
}
