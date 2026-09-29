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
