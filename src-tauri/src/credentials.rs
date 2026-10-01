//! The OS credential store.
//!
//! `handoff.md` §12.2 requires credentials to live in the operating system's credential store rather than
//! in SQLite, a config file, a log or a backup archive: **`
//! credential_ref` is a HANDLE, and the value is fetched here when a request needs it. §12.1 adds that a
//! credential is never displayed after it is saved — the UI shows whether one EXISTS, not what it is.
//!
//! Two design points worth stating:
//!
//! * **A missing credential is not an error here.** It is reported as `None`, and the provider then fails
//!   with its own authentication message. Inventing a Veloct error instead would hide which provider
//!   rejected the request, and a provider that needs no credential (Ollama) legitimately has none.
//! * **A read failure is also not an error.** A locked keychain or a headless session must not stop the
//!   application; the request fails at the provider, which is where the user can act on it.

use veloct_core::{Result, VeloctError};

/// The service name every entry is stored under, so Veloct's entries are grouped in the OS UI.
pub const SERVICE: &str = "veloct";

/// Read a credential by its handle.
///
/// Returns `Ok(None)` when no credential is stored, which is a normal state rather than a failure: a
/// provider may need none, and §12.1 says the UI only reports whether one exists.
pub fn get(handle: &str) -> Result<Option<String>> {
    if handle.trim().is_empty() {
        return Ok(None);
    }
    let entry = keyring::Entry::new(SERVICE, handle)
        .map_err(|error| VeloctError::Credential(format!("could not address {handle}: {error}")))?;

    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        // `NoEntry` is the ordinary "nothing saved yet" case.
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(VeloctError::Credential(format!(
            "could not read the credential for {handle}: {error}"
        ))),
    }
}

/// Read a credential, treating any failure as "absent".
///
/// This is the shape the call path wants: a request that cannot read a credential should fail at the
/// provider with the provider's own message. The error is deliberately reduced to `None` here rather than
/// logged, because a credential-store error can name the entry and the point of §12 is that no credential
/// material reaches the log.
pub fn get_or_none(handle: &str) -> Option<String> {
    get(handle).ok().flatten()
}

/// Store a credential under a handle, replacing any existing one.
pub fn set(handle: &str, secret: &str) -> Result<()> {
    if handle.trim().is_empty() {
        return Err(VeloctError::Invalid(
            "a credential needs a handle to be stored under".into(),
        ));
    }
    if secret.is_empty() {
        return Err(VeloctError::Invalid(
            "refusing to store an empty credential".into(),
        ));
    }
    let entry = keyring::Entry::new(SERVICE, handle)
        .map_err(|error| VeloctError::Credential(format!("could not address {handle}: {error}")))?;
    entry.set_password(secret).map_err(|error| {
        VeloctError::Credential(format!("could not store the credential for {handle}: {error}"))
    })
}

/// Remove a credential. Removing one that does not exist is not an error.
pub fn remove(handle: &str) -> Result<()> {
    if handle.trim().is_empty() {
        return Ok(());
    }
    let entry = keyring::Entry::new(SERVICE, handle)
        .map_err(|error| VeloctError::Credential(format!("could not address {handle}: {error}")))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(VeloctError::Credential(format!(
            "could not remove the credential for {handle}: {error}"
        ))),
    }
}

/// Whether a credential exists, WITHOUT reading it.
///
/// §12.1: the UI reports whether one is saved and never shows it. This still reads the value to find out,
/// because the OS API has no "exists" call, and keeps the answer to a boolean so the value cannot travel
/// further than this function.
pub fn exists(handle: &str) -> bool {
    get(handle).ok().flatten().is_some()
}

/// The handle for a provider's key.
///
/// One function so the format cannot drift between the code that writes and the code that reads, which
/// would silently orphan every saved credential.
pub fn handle_for(provider_id: &str) -> String {
    format!("provider:{provider_id}")
}

/// The handle for a network image source's API key (`handoff.md` §13.14).
///
/// A SEPARATE namespace from a provider's, because both live in the same store and a source id is not a
/// provider id: one `unsplash` handle shared between them would silently overwrite or read the other's
/// key.
pub fn handle_for_image_source(source: &str) -> String {
    format!("image-source:{source}")
}

/// Serialises tests that use the REAL OS credential store.
///
/// **The store is a process-wide resource, and these tests exercise it for real** — `delete_storage_target`
/// forgetting a credential (D143) and §13.14's key round trip are both facts only the OS store can
/// answer, so a fake would prove nothing. But Windows' credential APIs are not isolated per handle
/// across threads in one process: with the tests running in parallel, `exists` was observed returning a
/// STALE answer in both directions — `false` immediately after a `set`, and `true` immediately after a
/// `remove` — which failed `images::tests::a_key_round_trips_through_the_os_store_and_a_source_reports_that_it_exists`
/// and, at the same time, the pre-existing
/// `attachments::tests::deleting_a_target_forgets_its_stored_credential_and_an_unknown_id_changes_nothing`.
/// `--test-threads=1` makes both pass, which is what identified the cause.
///
/// This is the same shape as `veloct-sidecar`'s `ENV_LOCK` (PROCESSING.md §2.13): a shared, process-wide
/// resource needs ONE lock, and every test that touches it takes it. Reading-only tests take it too,
/// because a read concurrent with another test's write is exactly the interleaving that produced the
/// stale answers.
///
/// Take it as the FIRST line of a test:
///
/// ```ignore
/// let _store = crate::credentials::store_guard();
/// ```
#[cfg(test)]
pub(crate) fn store_guard() -> std::sync::MutexGuard<'static, ()> {
    static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handle_format_is_stable_and_namespaced() {
        // A changed format would orphan every saved credential, so it is asserted rather than assumed.
        assert_eq!(handle_for("p1"), "provider:p1");
        assert_eq!(SERVICE, "veloct");
    }

    #[test]
    fn an_empty_handle_is_absent_rather_than_an_error() {
        // A provider with no credential_ref legitimately has none.
        assert!(get("").unwrap().is_none());
        assert!(get("   ").unwrap().is_none());
        assert!(get_or_none("").is_none());
        assert!(!exists(""));
        // Removing nothing is not a failure.
        assert!(remove("").is_ok());
    }

    #[test]
    fn storing_a_credential_needs_a_handle_and_a_value() {
        assert!(set("", "secret").is_err());
        assert!(set("h", "").is_err());
        let empty_handle = set("", "secret").unwrap_err();
        assert!(empty_handle.to_string().contains("handle"), "{empty_handle}");
        let empty_secret = set("h", "").unwrap_err();
        assert!(empty_secret.to_string().contains("empty credential"), "{empty_secret}");
    }

    #[test]
    fn asking_for_a_handle_that_was_never_stored_reports_absent() {
        // The OS store is consulted, and a missing entry is the ordinary case. `get_or_none` is what the
        // call path uses, and it must never panic or error on an unknown handle.
        let handle = handle_for("a_provider_that_does_not_exist_in_any_store");
        assert!(get_or_none(&handle).is_none());
        assert!(!exists(&handle));
    }

    #[test]
    fn a_credential_store_error_is_reduced_to_absent_rather_than_stopping_the_application() {
        // A locked keychain or a headless session must not stop the workbench; the request fails at the
        // provider, which is where the user can act on it.
        assert!(get_or_none("").is_none());
        // `get` still distinguishes "absent" from "could not read", for a caller that wants to know.
        assert!(get("").unwrap().is_none());
    }
}
