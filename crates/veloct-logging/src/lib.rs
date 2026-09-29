//! The event log.
//!
//! `handoff.md` §18 asks for four things: a log file, the key events of every subsystem, the three
//! levels, and a log page. §18.1 then constrains what may be written:
//!
//! > The log must not record API keys, tokens, secrets, Authorization headers, or complete sensitive
//! > request bodies. Sensitive fields must be redacted. The log should be rotated so it cannot grow
//! > without bound.
//!
//! Two design choices follow from that, and both are about making the rule impossible to forget rather
//! than merely documented:
//!
//! * **Redaction happens at construction.** [`LogEvent::new`] runs the message and every field value
//!   through the core scanner, so there is no code path that writes an unredacted line. A caller
//!   cannot opt out, and a test asserts a secret in a message does not reach the sink.
//! * **A request is logged by shape, never by body.** [`LogEvent::http_request`] records the method, a
//!   redacted URL, the header NAMES and the body's byte count. The body itself is never accepted,
//!   because §18.1 names "complete sensitive request bodies" specifically and a helper that took one
//!   would be the obvious way to violate it.

#![forbid(unsafe_code)]

pub mod event;
pub mod sink;

pub use event::{Category, Entry, Level, LogEvent};
pub use sink::{
    default_log_path, log_file_name, prune_old_logs, FileSink, LogSink, MemorySink, MemorySnapshot,
    RotationPolicy, WriterSink, DEFAULT_MAX_BYTES, DEFAULT_MAX_FILES,
};
