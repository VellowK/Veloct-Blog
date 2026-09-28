//! Markdown truth pipeline: frontmatter, AST, directives, stable paragraph ids, references.
//!
//! This crate is the implementation of `handoff.md` §3.2 (frontmatter contract), §3.3 (dialect
//! and directives), §3.4 (source of truth), §4.4 (word count and reading time), §9.2 (stable
//! paragraph ids), §9.3 (reference conflict rules) and §9.4 (dynamic references and the Harvard
//! reference list). The on-disk shapes it reads and writes are fixed by
//! `docs/spec/01-data-model.md` §2.
//!
//! Two rules shape the whole design:
//!
//! * **Markdown is the only body truth** (§3.4). Nothing here stores a body anywhere else, and
//!   [`rewrite`] changes the source by surgical replacement rather than regeneration, so a user's
//!   formatting survives an edit.
//! * **Directives are parsed into a real AST, never matched with ad-hoc string regex** (§3.3).
//!   [`directives`] scans with explicit state (code fences, indentation) and [`ast`] holds the
//!   result as structured nodes that the Astro side renders.

#![forbid(unsafe_code)]

pub mod ast;
pub mod diagnostics;
pub mod directives;
pub mod frontmatter;
pub mod parse;
pub mod references;
pub mod rewrite;
pub mod stats;

pub use ast::{Ast, DocNode, Heading, Inline, ParagraphId, SourceSpan};
pub use diagnostics::{Diagnostic, DiagnosticCode, Severity};
pub use directives::{Directive, DirectiveKind};
pub use frontmatter::{ContentType, Frontmatter, Timestamps};
pub use parse::{parse, parse_body, Document};
pub use references::{Citation, KnowledgeRef, ReferenceList, ReferenceResolution};
pub use stats::{count_words, reading_minutes, Stats};
