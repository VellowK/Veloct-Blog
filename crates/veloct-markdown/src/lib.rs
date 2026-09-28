//! Markdown truth pipeline: frontmatter, AST, directives, stable paragraph ids, references.
//!
//! See `docs/spec/01-data-model.md` §2 for the frontmatter contract and §2.1/§2.2 for the
//! paragraph-id and reference rules, and `handoff.md` §3.2, §3.3, §9.2, §9.3, §9.4 for the
//! locked behaviour this crate implements.
//!
//! Ownership: `core-markdown` (team `veloct-complete`).

#![forbid(unsafe_code)]
