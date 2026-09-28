//! Stable identifiers.
//!
//! `handoff.md` §3.2 requires that a content `id` never changes, in particular never as a
//! consequence of a `slug` edit. `handoff.md` §3.1 requires the same stability for work
//! directories. We therefore use ULIDs: lexicographically sortable, collision-resistant and
//! independent of any mutable field.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Result, VeloctError};

/// A permanently stable identifier for a piece of content or a work.
///
/// The inner representation is a ULID in Crockford base32, which keeps ids sortable by
/// creation time while remaining opaque to users.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableId(ulid::Ulid);

impl StableId {
    /// Generate a new id from the operating system entropy source.
    pub fn new() -> Self {
        Self(ulid::Ulid::new())
    }

    /// Generate a new id deterministically, for tests and reproducible fixtures.
    pub fn from_parts(timestamp_ms: u64, randomness: u128) -> Self {
        Self(ulid::Ulid::from_parts(timestamp_ms, randomness))
    }

    /// Milliseconds since the Unix epoch encoded in the id.
    pub fn timestamp_ms(self) -> u64 {
        self.0.timestamp_ms()
    }

    /// Canonical Crockford base32 text form.
    pub fn to_text(self) -> String {
        self.0.to_string()
    }
}

impl Default for StableId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl fmt::Debug for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StableId({})", self.0)
    }
}

impl FromStr for StableId {
    type Err = VeloctError;

    fn from_str(s: &str) -> Result<Self> {
        ulid::Ulid::from_string(s)
            .map(Self)
            .map_err(|e| VeloctError::Invalid(format!("invalid stable id {s:?}: {e}")))
    }
}

impl Serialize for StableId {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse::<StableId>().map_err(serde::de::Error::custom)
    }
}

/// A release identifier.
///
/// Format: `<UTC basic timestamp>-<6 hex>` — human-sortable and filename-safe, which matters
/// because it becomes both a directory name (`.veloct/releases/<id>/`) and a git tag
/// (`release/<id>`, `handoff.md` §14.4).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReleaseId(String);

impl ReleaseId {
    /// Create a release id for the given creation time plus 3 random bytes of entropy.
    pub fn new(created: time::OffsetDateTime) -> Self {
        let mut bytes = [0u8; 3];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut bytes);
        Self::from_stamp(created, &bytes)
    }

    /// Deterministic constructor used by tests.
    pub fn from_stamp(created: time::OffsetDateTime, entropy: &[u8; 3]) -> Self {
        let stamp = created
            .to_offset(time::UtcOffset::UTC)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        // 2026-01-01T12:00:00Z -> 20260101T120000Z
        let compact: String = stamp
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect();
        let suffix = crate::hex_lower(entropy);
        Self(format!("{compact}-{suffix}"))
    }

    /// The git tag that anchors this release to a commit (`handoff.md` §14.4 step 3).
    pub fn tag_name(&self) -> String {
        format!("release/{}", self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate an existing release id, e.g. one parsed from a git tag or a directory name.
    pub fn parse(raw: &str) -> Result<Self> {
        let (stamp, suffix) = raw
            .split_once('-')
            .ok_or_else(|| VeloctError::Invalid(format!("release id {raw:?} is missing '-'")))?;
        if stamp.len() < 8 || !stamp.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(VeloctError::Invalid(format!(
                "release id {raw:?} has an invalid timestamp part"
            )));
        }
        if suffix.len() != 6 || !suffix.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(VeloctError::Invalid(format!(
                "release id {raw:?} must end with 6 hex characters"
            )));
        }
        Ok(Self(raw.to_string()))
    }
}

impl fmt::Display for ReleaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ReleaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReleaseId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_are_unique_and_ordered_by_creation() {
        let a = StableId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = StableId::new();
        assert_ne!(a, b);
        assert!(a < b, "ULIDs must sort by creation time");
    }

    #[test]
    fn stable_ids_round_trip_through_text_and_json() {
        let id = StableId::new();
        let text = id.to_text();
        assert_eq!(text.parse::<StableId>().unwrap(), id);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<StableId>(&json).unwrap(), id);
    }

    #[test]
    fn invalid_stable_id_is_rejected() {
        assert!("not-a-ulid".parse::<StableId>().is_err());
        assert!("".parse::<StableId>().is_err());
    }

    #[test]
    fn release_id_has_a_filename_safe_shape_and_a_tag() {
        let created = time::macros::datetime!(2026-01-01 12:00:00 UTC);
        let id = ReleaseId::from_stamp(created, &[0x3f, 0x9a, 0x1c]);
        assert_eq!(id.as_str(), "20260101T120000Z-3f9a1c");
        assert_eq!(id.tag_name(), "release/20260101T120000Z-3f9a1c");
        assert_eq!(ReleaseId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn release_id_parse_rejects_bad_input() {
        assert!(ReleaseId::parse("nonsense").is_err());
        assert!(ReleaseId::parse("20260101T120000Z-ZZZZZZ").is_err());
        assert!(ReleaseId::parse("20260101T120000Z-abc").is_err());
    }
}
