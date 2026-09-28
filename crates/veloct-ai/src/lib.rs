//! The optional AI subsystem.
//!
//! AI is never a dependency of editing, building, publishing or backup (`handoff.md` §11):
//! with no provider configured, every other feature must still work.
//!
//! The load-bearing rules:
//!
//! * One global fallback model, possibly on a completely independent provider. A **real call
//!   error** on the preferred model switches to it; a fallback failure is terminal and must
//!   never re-enter the fallback loop.
//! * A user-initiated stop is `CANCELLED` and is **not** an error, so it must not trigger the
//!   fallback (`handoff.md` §11.4, §25 C1). Every other failure — connection, timeout, 4xx,
//!   5xx, missing model, context overflow, provider refusal — does.
//! * AI output never rewrites Markdown before the user confirms, and a partial apply touches
//!   only the selected range.
//! * Credentials resolve through the OS credential store and never reach SQLite, a config file,
//!   a log or a backup.
//!
//! Ownership: `systems-ai-attach` (team `veloct-complete`).

#![forbid(unsafe_code)]
