//! Shared paragraph-splitting rules.
//!
//! Both the desktop app (Rust) and the site build (JavaScript, `site/src/lib/knowledge-scan.mjs`)
//! have to decide what a paragraph IS, because a reference resolves to a paragraph id and the card
//! links to a paragraph ordinal. If the two disagreed, a citation would point at a different
//! paragraph in the built site than the editor showed — the precise "reference silently points
//! somewhere else" failure `handoff.md` §9.3 forbids.
//!
//! So this module is the Rust side of a contract that is deliberately duplicated, and the rules are
//! stated here in the same order as in the JavaScript:
//!
//! 1. Normalise CRLF to LF.
//! 2. Split on a blank line (two or more newlines).
//! 3. A block that is ONLY a paragraph marker sets the id for the NEXT block.
//! 4. Otherwise the block takes a leading marker, or failing that the first marker found inside it.
//! 5. The marker text is removed from the content.
//! 6. A block with no content left is skipped entirely, which is why a trailing marker alone does
//!    not create an empty paragraph.
//!
//! `docs/spec/02-test-matrix.md` §H records the cross-check that keeps the two implementations
//! honest.

use veloct_markdown::ast::ParagraphId;

/// A paragraph of a knowledge document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedParagraph {
    /// The stable id, when the paragraph has one. A paragraph without an id is still returned so it
    /// counts toward the document, but it cannot be referenced: minting an id here would make a
    /// citation point at identity the editor cannot maintain.
    pub id: Option<ParagraphId>,
    /// 1-based position among the identified paragraphs, which is what a knowledge page uses to tag
    /// its blocks so a reference can open the right one.
    pub order: usize,
    /// The paragraph's text with the marker and inline Markdown markers removed.
    pub text: String,
    /// The text as written, before the readable-text reduction, for a caller that wants the source.
    pub raw: String,
}

/// Split a body into paragraphs.
pub fn extract_paragraphs(body: &str) -> Vec<ExtractedParagraph> {
    let text = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut paragraphs = Vec::new();
    let mut pending_id: Option<String> = None;

    for raw_block in text.split("\n\n").flat_map(split_on_blank_runs) {
        let trimmed = raw_block.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Rule 3: a block that is only a marker belongs to the next block.
        if let Some(id) = whole_marker(trimmed) {
            pending_id = Some(id);
            continue;
        }

        let mut id = pending_id.take();

        // Rule 4a: a leading marker on the first line.
        let mut content = trimmed.to_string();
        if let Some((leading_id, rest)) = leading_marker(&content) {
            id = Some(leading_id);
            content = rest;
        }

        // Rule 4b: otherwise the first marker anywhere inside.
        let inline = markers_in(&content);
        if !inline.is_empty() {
            if id.is_none() {
                id = Some(inline[0].clone());
            }
            content = remove_markers(&content);
        }

        let content = content.trim().to_string();
        // Rule 6: nothing left means there is no paragraph here.
        if content.is_empty() {
            continue;
        }

        paragraphs.push(ExtractedParagraph {
            id: id.and_then(|text| text.parse::<ParagraphId>().ok()),
            order: 0,
            text: plain_text(&content),
            raw: content,
        });
    }

    // The order counts IDENTIFIED paragraphs only, matching how the site numbers the blocks it
    // renders, so an ordinal means the same thing on both sides.
    let mut order = 0;
    for paragraph in &mut paragraphs {
        if paragraph.id.is_some() {
            order += 1;
            paragraph.order = order;
        }
    }

    paragraphs
}

/// Split a chunk on a run of two or more newlines.
///
/// `str::split("\n\n")` leaves empty pieces for a run of three or more, which the emptiness check
/// already handles, but it also fails to split `\n \n` (a blank line containing spaces), which is
/// legal Markdown. This handles both.
fn split_on_blank_runs(chunk: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut current_start = 0usize;
    let bytes = chunk.as_bytes();
    let mut index = 0usize;
    let mut newline_run = 0usize;

    while index < bytes.len() {
        if bytes[index] == b'\n' {
            newline_run += 1;
            index += 1;
            continue;
        }
        if bytes[index] == b' ' || bytes[index] == b'\t' {
            // Whitespace inside a blank run does not end it; whitespace inside a line does.
            index += 1;
            continue;
        }
        if newline_run >= 2 {
            out.push(&chunk[current_start..index - newline_run]);
            current_start = index;
        }
        newline_run = 0;
        index += 1;
    }

    if current_start < chunk.len() {
        out.push(&chunk[current_start..]);
    }
    out
}

/// A marker that makes up the entire block.
fn whole_marker(block: &str) -> Option<String> {
    let stripped = block.strip_prefix("<!--")?.strip_suffix("-->")?;
    parse_marker_body(stripped)
}

/// A marker at the very start of the block, followed by a newline.
fn leading_marker(block: &str) -> Option<(String, String)> {
    let rest = block.strip_prefix("<!--")?;
    let end = rest.find("-->")?;
    let id = parse_marker_body(&rest[..end])?;
    let after = &rest[end + 3..];
    // Only a leading marker if it is alone on its line.
    let after = after.strip_prefix('\n').or_else(|| {
        if after.trim().is_empty() {
            Some("")
        } else {
            None
        }
    })?;
    Some((id, after.to_string()))
}

/// Every marker found in a block, in order.
fn markers_in(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(start) = rest.find("<!--") {
        let after = &rest[start + 4..];
        let Some(end) = after.find("-->") else { break };
        if let Some(id) = parse_marker_body(&after[..end]) {
            out.push(id);
        }
        rest = &after[end + 3..];
    }
    out
}

/// Remove every marker from a block.
fn remove_markers(block: &str) -> String {
    let mut out = String::with_capacity(block.len());
    let mut rest = block;
    while let Some(start) = rest.find("<!--") {
        let after = &rest[start + 4..];
        let Some(end) = after.find("-->") else {
            out.push_str(rest);
            return out;
        };
        if parse_marker_body(&after[..end]).is_some() {
            out.push_str(&rest[..start]);
        } else {
            // Not one of ours: keep it verbatim.
            out.push_str(&rest[..start + 4 + end + 3]);
        }
        rest = &after[end + 3..];
    }
    out.push_str(rest);
    out
}

/// Parse the inside of a `<!-- ... -->` comment as a paragraph marker.
///
/// The body is `p:<26-character Crockford base32 ULID>`. The length and alphabet are checked rather
/// than assumed, so an ordinary HTML comment in a knowledge document is not mistaken for identity.
fn parse_marker_body(body: &str) -> Option<String> {
    let body = body.trim();
    let rest = body.strip_prefix("p:")?;
    let candidate = rest.trim();
    if candidate.len() != 26 {
        return None;
    }
    if !candidate
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() && !"ILOU".contains(c))
    {
        return None;
    }
    Some(candidate.to_string())
}

/// Reduce Markdown to readable text, for an excerpt.
///
/// Line-oriented rather than character-oriented: the leading markers that matter (`#`, `>`) are
/// only markers at the start of a line, and mixing the two passes is where this kind of function
/// usually goes wrong. A wrong reduction produces a worse excerpt, never a wrong reference, because
/// the reference uses the id.
pub fn plain_text(text: &str) -> String {
    let mut out_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        let mut line = line.trim_end();

        // A fenced code block's fence is dropped and its content kept.
        let fence_trimmed = line.trim_start();
        if fence_trimmed.starts_with("```") || fence_trimmed.starts_with("~~~") {
            continue;
        }

        // Leading block markers.
        line = line.trim_start();
        let mut chars = line.chars();
        let mut consumed = 0usize;
        match chars.next() {
            Some('#') => {
                consumed = line.chars().take_while(|c| *c == '#').count();
                if line[consumed..].starts_with(' ') {
                    consumed += 1;
                }
            }
            Some('>') => {
                consumed = line.chars().take_while(|c| *c == '>').count();
                if line[consumed..].starts_with(' ') {
                    consumed += 1;
                }
            }
            _ => {}
        }
        let line = &line[consumed..];

        out_lines.push(line.to_string());
    }

    // Inline markers, applied after the line pass. Order matters: images before links, because an
    // image is a link with a leading bang, and code before either, because a link inside code is
    // literal text.
    let joined = out_lines.join("\n");
    // Any comment that survived the marker extraction — a note to self, or a marker the block
    // splitter did not associate with a paragraph — must not appear in an excerpt.
    let joined = remove_html_comments(&joined);
    let joined = remove_pairs(&joined, "**");
    let joined = remove_pairs(&joined, "__");
    let joined = remove_inline_code(&joined);
    let joined = replace_images(&joined);
    let joined = replace_links(&joined);
    let joined = remove_pairs(&joined, "*");
    let joined = remove_pairs(&joined, "_");

    joined.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Remove every HTML comment.
fn remove_html_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        let after = &rest[start + 4..];
        match after.find("-->") {
            Some(end) => {
                out.push_str(&rest[..start]);
                rest = &after[end + 3..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Remove a symmetric inline marker such as `**` or `*`.
///
/// A lone marker with no partner is left alone rather than swallowing the rest of the text.
fn remove_pairs(text: &str, marker: &str) -> String {
    if text.matches(marker).count() < 2 {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(marker) {
        out.push_str(&rest[..index]);
        rest = &rest[index + marker.len()..];
        // Remove every second occurrence.
        if let Some(next) = rest.find(marker) {
            out.push_str(&rest[..next]);
            rest = &rest[next + marker.len()..];
        } else {
            out.push_str(marker);
            break;
        }
    }
    out.push_str(rest);
    out
}

fn remove_inline_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(start) = rest.find('`') else {
            out.push_str(rest);
            return out;
        };
        // Count the run length so ``` is handled as one marker.
        let run = rest[start..].chars().take_while(|c| *c == '`').count();
        let fence = &rest[start..start + run];
        let after = &rest[start + run..];
        match after.find(fence) {
            Some(end) => {
                out.push_str(&rest[..start]);
                out.push_str(&after[..end]);
                rest = &after[end + run..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

fn replace_images(text: &str) -> String {
    replace_link_like(text, "![", true)
}

fn replace_links(text: &str) -> String {
    replace_link_like(text, "[", false)
}

/// Replace `[label](target)` with `label`, and `![label](target)` with `label`.
fn replace_link_like(text: &str, opener: &str, _image: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(opener) {
        // An image's `![` must not be consumed by the link pass.
        if opener == "[" && start > 0 && rest.as_bytes()[start - 1] == b'!' {
            out.push_str(&rest[..start + 1]);
            rest = &rest[start + 1..];
            continue;
        }
        let label_start = start + opener.len();
        let Some(label_end) = rest[label_start..].find(']') else {
            out.push_str(rest);
            return out;
        };
        let label_end = label_start + label_end;
        let Some(target_start) = rest[label_end..].find('(') else {
            out.push_str(rest);
            return out;
        };
        let target_start = label_end + target_start;
        if target_start != label_end + 1 {
            out.push_str(&rest[..label_end + 1]);
            rest = &rest[label_end + 1..];
            continue;
        }
        let Some(target_end) = rest[target_start..].find(')') else {
            out.push_str(rest);
            return out;
        };
        let target_end = target_start + target_end;
        out.push_str(&rest[..start]);
        out.push_str(rest[label_start..label_end].trim());
        rest = &rest[target_end + 1..];
    }
    out.push_str(rest);
    out
}

/// Truncate by character and add an ellipsis, which is what a card shows.
pub fn excerpt(text: &str, limit: usize) -> String {
    let characters: Vec<char> = text.chars().collect();
    let body: String = if characters.len() <= limit {
        text.to_string()
    } else {
        characters[..limit].iter().collect()
    };
    format!("{body}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> String {
        ParagraphId::from_parts(1_700_000_000_000, n).as_stable_id().to_text()
    }

    #[test]
    fn a_marker_on_its_own_block_belongs_to_the_next_paragraph() {
        let body = format!("<!--p:{}-->\n\nThe paragraph text.\n", id(1));
        let paragraphs = extract_paragraphs(&body);
        assert_eq!(paragraphs.len(), 1);
        assert_eq!(paragraphs[0].id.unwrap().as_stable_id().to_text(), id(1));
        assert_eq!(paragraphs[0].text, "The paragraph text.");
        assert_eq!(paragraphs[0].order, 1);
    }

    #[test]
    fn a_leading_marker_on_the_first_line_of_a_block_is_taken() {
        let body = format!("<!--p:{}-->\nThe paragraph text.\n", id(2));
        let paragraphs = extract_paragraphs(&body);
        assert_eq!(paragraphs.len(), 1);
        assert_eq!(paragraphs[0].text, "The paragraph text.");
        assert!(paragraphs[0].id.is_some());
    }

    #[test]
    fn a_marker_inside_a_block_is_found_and_removed() {
        let body = format!("Some text <!--p:{}--> more text.\n", id(3));
        let paragraphs = extract_paragraphs(&body);
        assert_eq!(paragraphs.len(), 1);
        assert!(paragraphs[0].id.is_some());
        assert!(!paragraphs[0].raw.contains("<!--"), "the marker is removed: {}", paragraphs[0].raw);
        assert!(paragraphs[0].text.contains("Some text"));
    }

    #[test]
    fn a_block_that_is_only_a_marker_creates_no_empty_paragraph() {
        // This is why a trailing marker alone does not add a paragraph.
        let body = format!("A real paragraph.\n\n<!--p:{}-->\n", id(4));
        let paragraphs = extract_paragraphs(&body);
        assert_eq!(paragraphs.len(), 1);
        assert_eq!(paragraphs[0].text, "A real paragraph.");
    }

    #[test]
    fn paragraphs_are_split_on_blank_lines() {
        let body = format!(
            "<!--p:{}-->\n\nFirst.\n\n<!--p:{}-->\n\nSecond.\n",
            id(5),
            id(6)
        );
        let paragraphs = extract_paragraphs(&body);
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0].text, "First.");
        assert_eq!(paragraphs[1].text, "Second.");
        assert_eq!(paragraphs[0].order, 1);
        assert_eq!(paragraphs[1].order, 2);
    }

    #[test]
    fn a_blank_line_containing_spaces_still_splits() {
        // Legal Markdown, and a naive split on "\n\n" would miss it.
        let body = "First.\n \nSecond.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs.len(), 2, "{paragraphs:?}");
    }

    #[test]
    fn an_extra_blank_line_does_not_create_an_empty_paragraph() {
        let body = "First.\n\n\n\nSecond.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs.len(), 2);
    }

    #[test]
    fn an_unidentified_paragraph_is_returned_but_has_no_id() {
        // handoff.md section 9.2: identity is minted by the editor, never guessed here.
        let body = "First.\n\nSecond.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs.len(), 2);
        assert!(paragraphs.iter().all(|p| p.id.is_none()));
        assert!(paragraphs.iter().all(|p| p.order == 0));
    }

    #[test]
    fn the_order_counts_only_identified_paragraphs() {
        // The site numbers the blocks it renders, and it can only number the ones with identity, so
        // an ordinal must mean the same thing on both sides.
        let body = format!("No id here.\n\n<!--p:{}-->\n\nIdentified.\n", id(7));
        let paragraphs = extract_paragraphs(&body);
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0].id, None);
        assert_eq!(paragraphs[0].order, 0);
        assert_eq!(paragraphs[1].order, 1, "the identified paragraph is the first");
    }

    #[test]
    fn an_ordinary_html_comment_is_not_mistaken_for_a_paragraph_id() {
        // A comment that is not exactly a 26-character ULID must survive verbatim.
        let body = "Text.\n\n<!-- a note to self -->\n\nMore.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs.len(), 3, "the comment is its own paragraph: {paragraphs:?}");
        assert!(paragraphs.iter().all(|p| p.id.is_none()));
    }

    #[test]
    fn a_marker_with_the_wrong_length_is_not_an_id() {
        let body = "Text <!--p:SHORT--> more.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs.len(), 1);
        assert!(paragraphs[0].id.is_none());
        assert!(paragraphs[0].raw.contains("<!--p:SHORT-->"), "it is left alone");
    }

    #[test]
    fn crlf_is_normalised_so_a_windows_file_splits_the_same_way() {
        let body = format!("<!--p:{}-->\r\n\r\nFirst.\r\n\r\nSecond.\r\n", id(8));
        let paragraphs = extract_paragraphs(&body);
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0].text, "First.");
        assert_eq!(paragraphs[1].text, "Second.");
    }

    #[test]
    fn headings_and_emphasis_are_reduced_for_the_excerpt() {
        let body = "## A heading\n\nSome **bold** and *italic* and `code` text.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraphs[0].text, "A heading");
        assert_eq!(paragraphs[1].text, "Some bold and italic and code text.");
    }

    #[test]
    fn a_link_keeps_its_label_and_an_image_keeps_its_alt_text() {
        let body = "See [the docs](https://example.com) and ![a diagram](pic.png).\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(
            paragraphs[0].text,
            "See the docs and a diagram."
        );
    }

    #[test]
    fn blockquote_markers_are_removed() {
        let body = "> Quoted text.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs[0].text, "Quoted text.");
    }

    #[test]
    fn whitespace_is_collapsed_so_the_excerpt_has_no_line_breaks() {
        let body = "A sentence\nthat wrapped\nacross lines.\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs[0].text, "A sentence that wrapped across lines.");
    }

    #[test]
    fn an_excerpt_is_always_marked_as_one() {
        // handoff.md section 9.1: a card shows an excerpt. A short paragraph is still an excerpt, so
        // the ellipsis is unconditional rather than appearing only when text was cut.
        assert_eq!(excerpt("short", 20), "short…");
        assert_eq!(excerpt("0123456789", 4), "0123…");
    }

    #[test]
    fn an_excerpt_counts_characters_not_bytes() {
        // A byte-based cut would split a CJK character.
        assert_eq!(excerpt("你好世界", 2), "你好…");
    }

    #[test]
    fn an_empty_body_yields_no_paragraphs() {
        assert!(extract_paragraphs("").is_empty());
        assert!(extract_paragraphs("\n\n\n").is_empty());
        assert!(extract_paragraphs("   \n  \n").is_empty());
    }

    #[test]
    fn a_fenced_code_block_keeps_its_content_but_loses_its_fence() {
        let body = "```\nlet x = 1;\n```\n";
        let paragraphs = extract_paragraphs(body);
        assert_eq!(paragraphs.len(), 1);
        assert!(paragraphs[0].text.contains("let x = 1;"), "{}", paragraphs[0].text);
        assert!(!paragraphs[0].text.contains("```"), "{}", paragraphs[0].text);
    }
}
