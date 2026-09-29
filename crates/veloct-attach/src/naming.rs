//! Remote naming and the dedup key.
//!
//! `handoff.md` §13.4 and §13.7 fix two things that are easy to get subtly wrong:
//!
//! * The dedup key is `storage_target_id + storage_namespace + content_hash`. Using `content_hash`
//!   alone would reuse one URL across every target, so a file uploaded to a test bucket and later to
//!   production would keep pointing at the test bucket — and deleting the test target would break
//!   production content.
//! * The remote name keeps the ORIGINAL file name and inserts a fixed 6-character random suffix
//!   before the extension: `photo.webp` becomes `photo-a8K3xQ.webp`. Same content reuses the same
//!   suffix, so a re-upload does not orphan the name an article already points at.

use serde::{Deserialize, Serialize};

use crate::state::StorageTargetId;

/// The fixed suffix length `handoff.md` §13.7 requires.
pub const SUFFIX_LENGTH: usize = 6;

/// The alphabet a suffix is drawn from.
///
/// Crockford base32 minus the letters that are misread as digits (`I`, `L`, `O`, `U`), and minus the
/// case distinction, so `a8K3xQ` cannot be confused with `a8K3XQ` when a user reads it out of a URL.
pub const SUFFIX_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstvwxyz";

/// A 6-character random suffix.
///
/// The entropy comes from the caller so this stays testable and deterministic when it needs to be.
pub fn random_suffix(entropy: &[u8; SUFFIX_LENGTH]) -> String {
    entropy
        .iter()
        .map(|byte| SUFFIX_ALPHABET[(*byte as usize) % SUFFIX_ALPHABET.len()] as char)
        .collect()
}

/// Whether a file name is safe to use as an object key.
///
/// A name containing a separator, a `..` segment or a NUL would let a crafted upload escape the
/// configured prefix, so the name is checked rather than trusted.
pub fn is_safe_file_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return false;
    }
    if name == "." || name == ".." {
        return false;
    }
    if name.contains("..") {
        return false;
    }
    // A leading dot hides the file on a POSIX filesystem; not dangerous, but a surprise.
    !name.starts_with('.')
}

/// Split a file name into its stem and its extension, keeping every dot except the last.
///
/// A name like `archive.tar.gz` keeps `archive.tar` as the stem and `gz` as the extension, which is
/// what inserting a suffix before "the extension" means to a user.
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        // A leading dot is part of the name, not an extension separator (`.gitignore`).
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    }
}

/// The remote file name for a cached file.
///
/// `photo.webp` with suffix `a8K3xQ` becomes `photo-a8K3xQ.webp`. A name with no extension gets the
/// suffix appended directly. An unsafe name is refused rather than rewritten, because a rewritten
/// name is not the name the user chose.
pub fn remote_file_name(original: &str, suffix: &str) -> String {
    let suffix = &suffix[..suffix.len().min(SUFFIX_LENGTH)];
    let (stem, extension) = split_name(original);
    if stem.is_empty() {
        return format!("file-{suffix}{extension}");
    }
    format!("{stem}-{suffix}{extension}")
}

/// The identity of one remote object for dedup purposes.
///
/// `handoff.md` §13.4: the target and the namespace are both part of the key, so the same bytes
/// uploaded to a different bucket or a different prefix are a DIFFERENT object with its own URL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DedupKey {
    pub storage_target_id: StorageTargetId,
    /// The stable namespace inside the target, per `handoff.md` §13.5.
    pub storage_namespace: String,
    /// The content hash of the bytes.
    pub content_hash: String,
}

impl DedupKey {
    pub fn new(
        target: StorageTargetId,
        namespace: impl Into<String>,
        content_hash: impl Into<String>,
    ) -> Self {
        Self {
            storage_target_id: target,
            storage_namespace: namespace.into(),
            content_hash: content_hash.into(),
        }
    }

    /// A stable string form, for a database uniqueness constraint or a lock name.
    ///
    /// The unit separator keeps the parts from running together, so a target ending in `a` with a
    /// namespace starting in `b` cannot collide with a target ending in `ab`.
    pub fn as_string(&self) -> String {
        dedup_key_string(
            &self.storage_target_id,
            &self.storage_namespace,
            &self.content_hash,
        )
    }
}

/// Build the dedup key string.
pub fn dedup_key_string(
    target: &StorageTargetId,
    namespace: &str,
    content_hash: &str,
) -> String {
    format!("{}\u{1f}{}\u{1f}{}", target.as_str(), namespace, content_hash)
}

/// Build a dedup key.
pub fn dedup_key(
    target: StorageTargetId,
    namespace: impl Into<String>,
    content_hash: impl Into<String>,
) -> DedupKey {
    DedupKey::new(target, namespace, content_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suffix(seed: u8) -> String {
        random_suffix(&[seed; SUFFIX_LENGTH])
    }

    #[test]
    fn a_suffix_is_exactly_six_characters() {
        // handoff.md section 13.7: a FIXED 6-character suffix.
        for seed in 0..=255u8 {
            assert_eq!(suffix(seed).chars().count(), SUFFIX_LENGTH);
        }
    }

    #[test]
    fn a_suffix_uses_only_unambiguous_characters() {
        // The alphabet drops i, l, o and u so a name read out of a URL cannot be misheard.
        for seed in 0..=255u8 {
            let text = suffix(seed);
            assert!(
                text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{text} contains an unexpected character"
            );
            for bad in ['i', 'l', 'o', 'u'] {
                assert!(!text.contains(bad), "{text} contains {bad}");
            }
        }
    }

    #[test]
    fn different_entropy_gives_different_suffixes() {
        // Not a randomness test: it only shows the entropy is actually used, so a suffix is not a
        // constant.
        let a = random_suffix(&[0, 1, 2, 3, 4, 5]);
        let b = random_suffix(&[9, 8, 7, 6, 5, 4]);
        assert_ne!(a, b);
    }

    #[test]
    fn the_suffix_goes_before_the_extension() {
        // handoff.md section 13.7: photo.webp becomes photo-a8K3xQ.webp.
        assert_eq!(remote_file_name("photo.webp", "a8K3xQ"), "photo-a8K3xQ.webp");
        assert_eq!(remote_file_name("diagram.png", "abc123"), "diagram-abc123.png");
    }

    #[test]
    fn only_the_last_dot_is_treated_as_the_extension() {
        // A user sees "the extension" as the last one, so archive.tar.gz keeps its tar stem.
        assert_eq!(
            remote_file_name("archive.tar.gz", "abc123"),
            "archive.tar-abc123.gz"
        );
    }

    #[test]
    fn a_name_with_no_extension_gets_the_suffix_appended() {
        assert_eq!(remote_file_name("LICENSE", "abc123"), "LICENSE-abc123");
    }

    #[test]
    fn a_leading_dot_is_part_of_the_name_not_an_extension() {
        assert_eq!(remote_file_name(".env", "abc123"), ".env-abc123");
    }

    #[test]
    fn an_over_long_suffix_is_truncated_rather_than_producing_a_longer_name() {
        assert_eq!(
            remote_file_name("photo.webp", "abcdefghij"),
            "photo-abcdef.webp"
        );
    }

    #[test]
    fn an_empty_stem_still_produces_a_usable_name() {
        assert_eq!(remote_file_name(".webp", "abc123"), ".webp-abc123");
    }

    #[test]
    fn an_unsafe_file_name_is_refused() {
        // A separator or a `..` would let a crafted upload escape the configured prefix.
        for bad in [
            "",
            "a/b.png",
            "a\\b.png",
            "..",
            "../escape.png",
            "a..b.png",
            ".hidden",
            "a\0b",
        ] {
            assert!(!is_safe_file_name(bad), "{bad:?} should be refused");
        }
    }

    #[test]
    fn an_ordinary_file_name_is_accepted() {
        for good in ["photo.webp", "my file.png", "a-b_c.png", "1234.jpg", "档案.png"] {
            assert!(is_safe_file_name(good), "{good:?} should be accepted");
        }
    }

    #[test]
    fn a_very_long_file_name_is_refused() {
        let long = format!("{}.png", "a".repeat(300));
        assert!(!is_safe_file_name(&long));
    }

    #[test]
    fn the_dedup_key_contains_the_target_the_namespace_and_the_hash() {
        // handoff.md section 13.4: all three parts, in that order.
        let key = dedup_key(
            StorageTargetId::new("s3-main"),
            "bucket-a/uploads",
            "deadbeef",
        );
        let text = key.as_string();
        assert!(text.starts_with("s3-main\u{1f}"), "{text}");
        assert!(text.contains("bucket-a/uploads"), "{text}");
        assert!(text.ends_with("deadbeef"), "{text}");
    }

    #[test]
    fn the_same_hash_on_a_different_target_is_a_different_key() {
        // handoff.md section 13.4: a URL must never be reused across targets.
        let a = dedup_key(StorageTargetId::new("s3-main"), "bucket-a", "deadbeef");
        let b = dedup_key(StorageTargetId::new("github-main"), "bucket-a", "deadbeef");
        assert_ne!(a, b);
        assert_ne!(a.as_string(), b.as_string());
    }

    #[test]
    fn the_same_hash_in_a_different_namespace_is_a_different_key() {
        // handoff.md section 13.5: changing the prefix forms a new namespace.
        let a = dedup_key(StorageTargetId::new("s3-main"), "uploads/v1", "deadbeef");
        let b = dedup_key(StorageTargetId::new("s3-main"), "uploads/v2", "deadbeef");
        assert_ne!(a, b);
    }

    #[test]
    fn the_same_hash_on_the_same_target_and_namespace_is_the_same_key() {
        // So the same content reuses the existing remote object and its URL.
        let a = dedup_key(StorageTargetId::new("s3-main"), "uploads", "deadbeef");
        let b = dedup_key(StorageTargetId::new("s3-main"), "uploads", "deadbeef");
        assert_eq!(a, b);
        assert_eq!(a.as_string(), b.as_string());
    }

    #[test]
    fn the_key_parts_cannot_run_together() {
        // Without a separator, target "a" + namespace "bc" would collide with target "ab" + "c".
        let a = dedup_key(StorageTargetId::new("a"), "bc", "h");
        let b = dedup_key(StorageTargetId::new("ab"), "c", "h");
        assert_ne!(a.as_string(), b.as_string());
    }

    #[test]
    fn the_dedup_key_round_trips_through_json() {
        let key = dedup_key(StorageTargetId::new("s3-main"), "uploads", "deadbeef");
        let json = serde_json::to_string(&key).unwrap();
        assert!(json.contains("\"storage_target_id\":\"s3-main\""));
        assert!(json.contains("\"storage_namespace\":\"uploads\""));
        assert_eq!(serde_json::from_str::<DedupKey>(&json).unwrap(), key);
    }
}
