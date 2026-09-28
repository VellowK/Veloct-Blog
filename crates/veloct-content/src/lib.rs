//! Content model and truth-source I/O: Markdown documents, WIP publication rules and works.
//!
//! Markdown is the body truth and `works/<stable-id>/work.json` + `site/` is the work truth
//! (`handoff.md` §3.1, §3.2, §10.1). SQLite only ever holds an index of either.
//!
//! Ownership: `content-app` (team `veloct-complete`).

#![forbid(unsafe_code)]
