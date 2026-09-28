//! Word counting and estimated reading time (`handoff.md` §4.4).
//!
//! Veloct targets Chinese-language personal sites, so a whitespace split is wrong: it reports
//! `你好世界` as one word. A CJK ideograph is counted as one word and a run of Latin letters or
//! digits as one word, which matches how readers of either script estimate length. The count is
//! deterministic for a given input, which is what a release comparison needs.

use serde::{Deserialize, Serialize};

use crate::ast::{Ast, DocNode};

/// Words per minute used for the estimate, per script.
pub const LATIN_WORDS_PER_MINUTE: u32 = 220;
pub const CJK_CHARACTERS_PER_MINUTE: u32 = 400;

/// Counting and time statistics for one document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// Latin/CJK word tokens.
    pub words: usize,
    /// CJK characters, which are also counted individually as words.
    pub cjk_characters: usize,
    /// Characters excluding whitespace and Markdown syntax noise.
    pub characters: usize,
    /// Human-facing "N minute read", never below 1 for a non-empty document.
    pub reading_minutes: u32,
}

impl Stats {
    pub fn is_empty(&self) -> bool {
        self.words == 0 && self.cjk_characters == 0
    }
}

/// Whether a character is counted individually as a word.
///
/// Covers the CJK ideograph blocks, Hiragana, Katakana and Hangul, because the same reasoning
/// applies to each: they are not whitespace-delimited.
pub fn is_cjk(character: char) -> bool {
    matches!(character as u32,
        0x3040..=0x30FF        // Hiragana + Katakana
        | 0x3400..=0x4DBF      // CJK Extension A
        | 0x4E00..=0x9FFF      // CJK Unified Ideographs
        | 0xF900..=0xFAFF      // CJK Compatibility Ideographs
        | 0xAC00..=0xD7AF      // Hangul syllables
        | 0x20000..=0x2FA1F    // CJK Extensions B..F
    )
}

/// Count words in a plain-text fragment.
pub fn count_words(text: &str) -> (usize, usize) {
    let mut words = 0usize;
    let mut cjk = 0usize;
    let mut in_latin_run = false;

    for character in text.chars() {
        if is_cjk(character) {
            cjk += 1;
            words += 1;
            in_latin_run = false;
        } else if character.is_alphanumeric() {
            if !in_latin_run {
                words += 1;
                in_latin_run = true;
            }
        } else if character == '\'' || character == '\u{2019}' || character == '-' {
            // An intra-word apostrophe or hyphen does not start a new word, but only when it is
            // actually inside one.
            if !in_latin_run {
                in_latin_run = false;
            }
        } else {
            in_latin_run = false;
        }
    }

    (words, cjk)
}

/// Reading time in minutes for a word/CJK pair.
pub fn reading_minutes(words: usize, cjk_characters: usize) -> u32 {
    if words == 0 && cjk_characters == 0 {
        return 0;
    }
    let latin_words = words.saturating_sub(cjk_characters) as f64;
    let minutes = latin_words / LATIN_WORDS_PER_MINUTE as f64
        + cjk_characters as f64 / CJK_CHARACTERS_PER_MINUTE as f64;
    // Never report "0 minutes" for something with content.
    (minutes.ceil() as u32).max(1)
}

/// Statistics for a text body, without parsing it into an AST.
pub fn stats_of_text(text: &str) -> Stats {
    let (words, cjk_characters) = count_words(text);
    let characters = text.chars().filter(|c| !c.is_whitespace()).count();
    Stats {
        words,
        cjk_characters,
        characters,
        reading_minutes: reading_minutes(words, cjk_characters),
    }
}

/// Statistics for a parsed document.
///
/// Counts visible prose only: headings are included because a reader reads them, code blocks are
/// excluded because a reader does not, and a generated reference list is not authored content.
/// The rule lives in one place so the number the editor shows and the number the published page
/// shows cannot drift.
pub fn stats_of_ast(ast: &Ast) -> Stats {
    let mut text = String::new();

    // `Ast::visit` walks nested block quotes but not list items or table cells, so those two are
    // handled explicitly here and everything else is picked up by the node match. Keeping the
    // split in one visitor is what prevents double counting.
    ast.visit(&mut |node| match node {
        DocNode::Paragraph { content, .. } | DocNode::Heading { content, .. } => {
            text.push_str(&crate::ast::plain_text(content));
            text.push(' ');
        }
        DocNode::List { items, .. } => {
            for item in items {
                for child in &item.children {
                    text.push_str(&child.plain_text());
                    text.push(' ');
                }
            }
        }
        DocNode::Table { table, .. } => {
            for cell in &table.head {
                text.push_str(&crate::ast::plain_text(cell));
                text.push(' ');
            }
            for row in &table.rows {
                for cell in row {
                    text.push_str(&crate::ast::plain_text(cell));
                    text.push(' ');
                }
            }
        }
        _ => {}
    });

    stats_of_text(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_body;

    #[test]
    fn latin_words_are_counted_by_whitespace_runs() {
        let (words, cjk) = count_words("Hello there, world!");
        assert_eq!(words, 3);
        assert_eq!(cjk, 0);
    }

    #[test]
    fn cjk_characters_are_counted_individually() {
        // The whole point: a whitespace split would report this as one word.
        let (words, cjk) = count_words("你好世界");
        assert_eq!(words, 4);
        assert_eq!(cjk, 4);
    }

    #[test]
    fn mixed_scripts_add_up() {
        let (words, cjk) = count_words("Rust 是一门系统编程语言");
        assert_eq!(cjk, 9);
        assert_eq!(words, 10, "Rust plus nine CJK characters");
    }

    #[test]
    fn punctuation_and_numbers_behave() {
        assert_eq!(count_words("1 2 3").0, 3);
        assert_eq!(count_words("---").0, 0);
        assert_eq!(count_words("").0, 0);
        assert_eq!(count_words("   ").0, 0);
        // An intra-word hyphen must not split a word.
        assert_eq!(count_words("state-of-the-art").0, 1);
        // An apostrophe inside a word is the same word.
        assert_eq!(count_words("don't").0, 1);
    }

    #[test]
    fn reading_time_is_never_zero_for_content_and_zero_for_nothing() {
        assert_eq!(reading_minutes(0, 0), 0);
        assert_eq!(reading_minutes(1, 0), 1);
        let long = "word ".repeat(2200);
        assert_eq!(reading_minutes(count_words(&long).0, 0), 10);
        // 800 CJK characters at 400 per minute is two minutes.
        assert_eq!(reading_minutes(800, 800), 2);
    }

    #[test]
    fn stats_of_text_ignores_whitespace_for_the_character_count() {
        let stats = stats_of_text("a b\nc");
        assert_eq!(stats.characters, 3);
        assert_eq!(stats.words, 3);
        assert_eq!(stats.reading_minutes, 1);
        assert!(!stats.is_empty());
        assert!(stats_of_text("   \n").is_empty());
    }

    #[test]
    fn stats_of_ast_counts_prose_and_headings_but_not_code() {
        let parsed = parse_body(
            "# A heading\n\nA paragraph with five words.\n\n```rust\nfn main() { let unused = 1; }\n```\n",
        );
        let stats = stats_of_ast(&parsed.ast);
        // "A heading A paragraph with five words." -> 7 words
        assert_eq!(stats.words, 7, "{stats:?}");
        assert!(stats.characters > 0);
        assert_eq!(stats.reading_minutes, 1);
    }

    #[test]
    fn stats_of_ast_includes_lists_and_tables() {
        let parsed = parse_body("- alpha beta\n\n| x | y |\n| --- | --- |\n| one | two |\n");
        let stats = stats_of_ast(&parsed.ast);
        // alpha, beta, x, y, one, two
        assert_eq!(stats.words, 6, "{stats:?}");
    }

    #[test]
    fn counting_is_deterministic_for_the_same_input() {
        // A release comparison depends on this being stable.
        let text = "Hello 世界 mixed content 123";
        assert_eq!(count_words(text), count_words(text));
        assert_eq!(stats_of_text(text), stats_of_text(text));
    }

    #[test]
    fn cjk_detection_covers_the_expected_ranges() {
        assert!(is_cjk('你'));
        assert!(is_cjk('の'));
        assert!(is_cjk('ア'));
        assert!(is_cjk('한'));
        assert!(!is_cjk('a'));
        assert!(!is_cjk('1'));
        assert!(!is_cjk('，'));
    }
}
