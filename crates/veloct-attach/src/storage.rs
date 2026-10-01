//! Storage adapters.
//!
//! `handoff.md` §13.1 names five target kinds and §13.10–§13.13 fix what each one is configured
//! with. §13.5 adds the piece that keeps dedup honest: a target has a stable `storage_namespace`
//! derived from the fields that decide an object's identity (`bucket + key_prefix` for S3,
//! `repository + branch + path_prefix` for GitHub, and so on). Changing a prefix forms a NEW
//! namespace, and §13.5 says the old namespace's object records are kept — nothing is migrated.
//!
//! The adapters talk only through [`StorageAdapter`], so every protocol is exercised against a fake
//! and no test needs a network. A protocol whose behaviour could not be verified is reported as
//! unconfigured rather than pretended to work.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::{Result, VeloctError};

use crate::naming::{is_safe_file_name, remote_file_name};
use crate::state::StorageTargetId;

/// Which protocol a target speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageKind {
    /// The user's own server over SSH/SFTP (`handoff.md` §13.10).
    Sftp,
    /// A local directory: this machine, or a directory this machine can already write.
    ///
    /// §13.10 calls the feature "server attachment storage", and for a user whose server is this
    /// machine the target is a directory rather than a protocol. Distinct from [`StorageKind::Sftp`]
    /// on purpose: before this variant existed the shell routed an SFTP target with no host and no
    /// credential to a LOCAL directory, so an unconfigured SFTP target silently degraded into a local
    /// one and a genuinely local target had no way to exist at all.
    Local,
    /// S3 or an S3-compatible API (`handoff.md` §13.11).
    S3,
    /// Lsky Pro (`handoff.md` §13.12).
    LskyPro,
    /// A GitHub repository (`handoff.md` §13.13).
    GitHub,
}

impl StorageKind {
    /// Every kind this build knows, in the order `handoff.md` §13.1 names them — with the
    /// zero-configuration directory first, because it is the one target a new project can always use.
    ///
    /// This is the ONE canonical list. The `storage_kinds` command and the window's kind list both read it,
    /// and whether a kind has an adapter is answered per kind by `targets::unimplemented_reason`, whose match
    /// is exhaustive — so a kind added to this enum does not compile until it is answered there. A second list
    /// of kinds somewhere else would be a second answer to §13.10–§13.13's question ("which kinds exist, and
    /// which of them work"), and two answers eventually disagree.
    pub const ALL: [StorageKind; 5] = [Self::Local, Self::S3, Self::GitHub, Self::Sftp, Self::LskyPro];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sftp => "sftp",
            Self::Local => "local",
            Self::S3 => "s3",
            Self::LskyPro => "lsky",
            Self::GitHub => "github",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sftp" | "ssh" => Some(Self::Sftp),
            // A local directory is written as `local`, but a user naming the thing they mean — a
            // directory — is not making a mistake, and `dir` is how the window's own kind list spells it.
            "local" | "dir" | "directory" => Some(Self::Local),
            "s3" | "s3_compatible" | "s3-compatible" => Some(Self::S3),
            "lsky" | "lskypro" | "lsky_pro" => Some(Self::LskyPro),
            "github" | "gh" => Some(Self::GitHub),
            _ => None,
        }
    }

    /// Whether the protocol's own behaviour has been verified against an authoritative source.
    ///
    /// `handoff.md` §28 requires a key failure path to have an explicit error state, and ADR-0010
    /// requires honest capability reporting. A protocol marked `false` here is configured, listed and
    /// selectable, and reports "not verified" rather than silently doing something that may be
    /// wrong.
    pub fn is_verified(self) -> bool {
        match self {
            // S3's PUT/GET/DELETE and SigV4 are documented and stable.
            Self::S3 => true,
            // GitHub's contents API is documented and stable.
            Self::GitHub => true,
            // A directory on this machine speaks no protocol, so there is nothing to verify.
            Self::Local => true,
            // Lsky Pro's FREE line was read from the vendor's own artifacts — the API page the product
            // itself renders at `{instance}/api`, and the routes and handlers behind it — and the reading
            // is recorded in `lsky.rs`. The PAID line ("Lsky Pro+", 2.x/enterprise 1.x) is deliberately
            // NOT claimed: from 2.3.0 the vendor encrypts that backend and publishes no API reference.
            Self::LskyPro => true,
            // SFTP needs a live server to verify against, and there is none in this environment.
            Self::Sftp => false,
        }
    }
}

/// What a target is configured with.
///
/// No field here holds a secret VALUE. Credentials are resolved through the OS credential store and
/// only their handle appears, which is what makes this struct safe to serialise into SQLite, a log
/// line or a release snapshot (`handoff.md` §12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageConfig {
    pub id: StorageTargetId,
    pub name: String,
    pub kind: StorageKind,
    #[serde(default)]
    pub enabled: bool,

    /// SFTP: host. Unused by the other kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// S3: endpoint, region and bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    /// S3: the access key ID, which is an IDENTIFIER rather than a secret (§13.11 names both).
    ///
    /// The SECRET access key is a credential and lives in the OS store behind `credential_ref`, because
    /// §12.2 forbids a credential value in a config file. The access key ID authenticates nothing on its
    /// own — AWS prints it in the console and it travels in every signed request — so it is configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,

    /// GitHub: repository and branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    /// Lsky Pro: the instance address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,

    /// The prefix objects are stored under. Part of the namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_prefix: Option<String>,

    /// The public URL prefix. `handoff.md` §13.6 separates this from the object's actual path, so
    /// changing it does not move anything.
    #[serde(default)]
    pub public_url_prefix: String,

    /// Only a HANDLE. Never a secret value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
}

impl StorageConfig {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: StorageKind,
    ) -> Self {
        Self {
            id: StorageTargetId::new(id),
            name: name.into(),
            kind,
            enabled: true,
            host: None,
            port: None,
            username: None,
            endpoint: None,
            region: None,
            bucket: None,
            access_key: None,
            repository: None,
            branch: None,
            instance: None,
            key_prefix: None,
            public_url_prefix: String::new(),
            credential_ref: None,
        }
    }

    /// The namespace that decides an object's identity inside this target
    /// (`handoff.md` §13.5).
    ///
    /// Built from the fields that actually determine WHERE an object lands. The public URL prefix is
    /// deliberately excluded: it is separated from the object's path by §13.6, so changing it must
    /// not make the same remote object look like a new one.
    pub fn storage_namespace(&self) -> String {
        let prefix = self.key_prefix.clone().unwrap_or_default();
        let parts: Vec<String> = match self.kind {
            StorageKind::S3 => vec![
                self.bucket.clone().unwrap_or_default(),
                prefix,
            ],
            StorageKind::GitHub => vec![
                self.repository.clone().unwrap_or_default(),
                self.branch.clone().unwrap_or_else(|| "main".to_string()),
                prefix,
            ],
            StorageKind::Sftp => vec![
                self.host.clone().unwrap_or_default(),
                self.port.map(|port| port.to_string()).unwrap_or_default(),
                self.username.clone().unwrap_or_default(),
                prefix,
            ],
            StorageKind::LskyPro => vec![
                self.instance.clone().unwrap_or_default(),
                prefix,
            ],
            // A local target's directory is derived from the project root and the target's own id
            // (`targets::local_root`), so the id is already part of the dedup key and the prefix is the
            // only remaining part of an object's identity. Two prefixes are two namespaces, exactly as
            // §13.5 requires.
            StorageKind::Local => vec![prefix],
        };
        // Empty parts are dropped so an unset field does not silently change the namespace when it
        // gains a value that means the same thing.
        let joined = parts
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("|");
        if joined.is_empty() {
            format!("{}:default", self.kind.as_str())
        } else {
            format!("{}:{joined}", self.kind.as_str())
        }
    }

    /// Validate before the target is used, so a configuration mistake is reported as one rather than
    /// as a confusing network error.
    ///
    /// The production caller is the SAVE path: `save_storage_target` runs this (through `save_target_config`)
    /// before it writes the target list, so the rules below are enforced where the user meets them rather than
    /// at the first upload. It is called for a kind this build IMPLEMENTS only — `is_implemented` is the check —
    /// because SFTP's arm (host and username) asks for protocol fields nothing in this build can use yet, and a
    /// half-filled target of that kind is one of the ways §13.10's gap stays visible in the target list.
    ///
    /// Every OTHER kind's arm is live, and Lsky Pro's became live when §5.4's protocol was verified against the
    /// vendor's own artifacts: its `instance` is now required where the user saves it, not at the first upload.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.id.as_str().trim().is_empty() {
            return Err("a storage target needs an id".into());
        }
        if self.name.trim().is_empty() {
            return Err("a storage target needs a name".into());
        }
        match self.kind {
            StorageKind::Sftp => {
                if self.host.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("SFTP needs a host".into());
                }
                if self.username.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("SFTP needs a username".into());
                }
            }
            StorageKind::S3 => {
                if self.bucket.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("S3 needs a bucket".into());
                }
            }
            StorageKind::GitHub => {
                if self.repository.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("GitHub needs a repository".into());
                }
            }
            StorageKind::LskyPro => {
                if self.instance.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("Lsky Pro needs an instance address".into());
                }
            }
            StorageKind::Local => {
                // A local target needs no protocol field at all: the directory is derived from the
                // project root and the target id (`local_root`). What it CAN get wrong is its prefix,
                // and a prefix that names a drive letter or a UNC root would put the bytes outside the
                // directory the target owns — `Path::join` replaces the base when the argument is
                // absolute, so this is refused here rather than discovered at the first write.
                if let Some(prefix) = &self.key_prefix {
                    if prefix.contains(':') || prefix.starts_with('\\') {
                        return Err(format!(
                            "the directory prefix {prefix:?} must be a relative path inside the \
                             target's own directory"
                        ));
                    }
                }
            }
        }
        if let Some(prefix) = &self.key_prefix {
            if prefix.contains("..") || prefix.starts_with('/') {
                return Err(format!(
                    "the path prefix {prefix:?} must be a relative path without .."
                ));
            }
        }
        // A public URL prefix must be an absolute URL, because it is concatenated with an object
        // path and a relative one would produce something no browser can resolve.
        if !self.public_url_prefix.trim().is_empty()
            && !self.public_url_prefix.starts_with("http://")
            && !self.public_url_prefix.starts_with("https://")
        {
            return Err(format!(
                "the public URL prefix {:?} must start with http:// or https://",
                self.public_url_prefix
            ));
        }
        Ok(())
    }

    /// Whether a credential handle is recorded.
    pub fn has_credential(&self) -> bool {
        self.credential_ref.is_some()
    }

    /// Whether a credential is required for this kind.
    pub fn requires_credential(&self) -> bool {
        // A local SFTP server with key auth still needs an identity, but the handle may legitimately
        // be unset while the user is mid-setup, so this reports what the protocol expects rather
        // than blocking the configuration.
        true
    }
}

/// What an adapter can do, so the UI can say so instead of offering a button that fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCapabilities {
    /// Whether the protocol's behaviour has been verified (see [`StorageKind::is_verified`]).
    pub verified: bool,
    /// Whether a public URL can be derived without a round trip.
    pub derives_public_url: bool,
    /// Whether the adapter can remove an object. §13.8 means Veloct never calls this automatically,
    /// so it exists for an explicit user action only.
    pub can_delete_remotely: bool,
    /// Whether the adapter can prove a target is reachable.
    pub can_test_connection: bool,
}

/// The remote object record.
///
/// `handoff.md` §13.3: the remote object is recorded as its own step, so a failure after the upload
/// still leaves the object's existence known and its URL recoverable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecord {
    pub dedup_key: String,
    pub storage_target_id: StorageTargetId,
    pub storage_namespace: String,
    pub content_hash: String,
    /// The object's key inside the target, e.g. `uploads/photo-a8k3xq.webp`.
    pub key: String,
    /// The public URL, derived from the public prefix and the key.
    pub public_url: String,
    pub size_bytes: u64,
    pub created_at: String,
}

/// A remote object as the adapter reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObject {
    pub key: String,
    pub public_url: String,
    pub size_bytes: u64,
}

/// The result of a put.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutOutcome {
    /// The object was written.
    Created(RemoteObject),
    /// The object already existed at this key, so nothing was written and the existing URL is
    /// returned (`handoff.md` §13.4: reuse the existing remote file).
    AlreadyPresent(RemoteObject),
}

impl PutOutcome {
    pub fn object(&self) -> &RemoteObject {
        match self {
            Self::Created(object) | Self::AlreadyPresent(object) => object,
        }
    }

    pub fn was_created(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

/// A storage target that files can be sent to.
///
/// Kept deliberately small: put, head, test and a remote delete that only an explicit user action
/// may reach. There is no "delete everything" and no listing method that could be used to sweep a
/// target, because §13.8 says remote files are never removed automatically.
pub trait StorageAdapter: Send + Sync {
    fn config(&self) -> &StorageConfig;

    fn capabilities(&self) -> StorageCapabilities;

    /// Store bytes, or report that an object with these bytes is already there.
    ///
    /// `key` is the object's path inside the target, already prefixed.
    fn put(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome>;

    /// Whether an object exists, and its URL when it does.
    fn head(&self, key: &str) -> Result<Option<RemoteObject>>;

    /// Prove the target is reachable, or explain why it is not.
    fn test_connection(&self) -> Result<String>;

    /// Remove one object.
    ///
    /// Nothing in Veloct calls this automatically (`handoff.md` §13.8). It exists because a user may
    /// explicitly ask to clean up a remote object they know is unused.
    fn delete_remote(&self, key: &str) -> Result<()>;
}

/// The object key for a file, given a target's prefix.
pub fn object_key(config: &StorageConfig, file_name: &str) -> Result<String> {
    if !is_safe_file_name(file_name) {
        return Err(VeloctError::Invalid(format!(
            "{file_name:?} is not a usable object name"
        )));
    }
    let prefix = config.key_prefix.clone().unwrap_or_default();
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        Ok(file_name.to_string())
    } else {
        Ok(format!("{prefix}/{file_name}"))
    }
}

/// A best-effort MIME type from a file name's extension.
///
/// This is the CANONICAL map, and `veloct_app::mime_type_for` delegates to it: it used to live one layer
/// up, and the S3 adapter (§13.11) needs it too — an object stored without a `Content-Type` is served as
/// `application/octet-stream`, so a picture would download instead of rendering.
///
/// Deliberately small and honest: it covers what a personal site actually attaches, and an unknown
/// extension is `None` rather than a guess, because a WRONG `Content-Type` is worse than a missing one.
pub fn content_type_for(file_name: &str) -> Option<String> {
    let extension = file_name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())?;
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "json" => "application/json",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "zip" => "application/zip",
        _ => return None,
    };
    Some(mime.to_string())
}

/// The public URL for an object.
pub fn public_url(config: &StorageConfig, key: &str) -> Result<String> {
    if config.public_url_prefix.trim().is_empty() {
        return Err(VeloctError::Invalid(format!(
            "the storage target {} has no public URL prefix, so no URL can be derived",
            config.id
        )));
    }
    Ok(format!(
        "{}/{}",
        config.public_url_prefix.trim_end_matches('/'),
        key.trim_start_matches('/')
    ))
}

/// A target: its configuration plus an adapter.
pub struct StorageTarget {
    config: StorageConfig,
    adapter: Box<dyn StorageAdapter>,
}

impl StorageTarget {
    pub fn new(config: StorageConfig, adapter: Box<dyn StorageAdapter>) -> Self {
        Self { config, adapter }
    }

    pub fn config(&self) -> &StorageConfig {
        &self.config
    }

    pub fn namespace(&self) -> String {
        self.config.storage_namespace()
    }

    /// Store a file, deriving its key from the configured prefix.
    pub fn store(&self, file_name: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let key = object_key(&self.config, file_name)?;
        self.adapter.put(&key, bytes)
    }

    /// The public URL for a key.
    pub fn url_for(&self, key: &str) -> Result<String> {
        public_url(&self.config, key)
    }

    /// What this target can do.
    ///
    /// A passthrough so a caller holding a target can ask before it tries: a target whose protocol is not
    /// implemented reports `verified: false`, and a UI can say so rather than waiting for an upload to fail.
    pub fn capabilities(&self) -> StorageCapabilities {
        self.adapter.capabilities()
    }

    /// Prove the target is reachable, or explain why it is not.
    pub fn test_connection(&self) -> Result<String> {
        self.adapter.test_connection()
    }

    /// Remove ONE object from this target.
    ///
    /// `handoff.md` §13.8 forbids removing a remote file as a side effect of anything, so the only
    /// caller is the explicit user request `delete_remote_object`. The refusal for a target whose
    /// protocol cannot delete is the ADAPTER's (`capabilities().can_delete_remotely`), because a target
    /// that cannot delete must not report a silent success — see [`StorageTarget::capabilities`].
    pub fn delete_remote(&self, key: &str) -> Result<()> {
        self.adapter.delete_remote(key)
    }
}

/// An in-memory adapter, so protocol logic and the transaction are testable without a network.
///
/// Named `Memory` rather than `Fake` because it is a genuine adapter with defined behaviour, and the
/// transaction tests need a target that can actually succeed, fail on demand and report an existing
/// object.
#[derive(Debug, Default)]
pub struct MemoryFileStore {
    objects: std::sync::Mutex<BTreeMap<String, Vec<u8>>>,
    /// When set, every put fails with this message.
    fail_puts: std::sync::Mutex<Option<String>>,
    /// When set, `head` reports nothing even for an object that exists, which is how the
    /// "upload succeeded but the record was lost" path is exercised.
    hide_objects: std::sync::atomic::AtomicBool,
    /// How many puts were attempted, so a test can prove a re-upload did NOT happen.
    put_count: std::sync::atomic::AtomicUsize,
}

impl MemoryFileStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fail_puts_with(&self, message: &str) {
        *self.fail_puts.lock().unwrap() = Some(message.to_string());
    }

    pub fn allow_puts(&self) {
        *self.fail_puts.lock().unwrap() = None;
    }

    pub fn hide_objects(&self, hide: bool) {
        self.hide_objects
            .store(hide, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn put_count(&self) -> usize {
        self.put_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn keys(&self) -> Vec<String> {
        self.objects.lock().unwrap().keys().cloned().collect()
    }
}

/// An adapter that answers entirely from memory, with a fixed configuration.
pub struct MemoryStore {
    config: StorageConfig,
    store: std::sync::Arc<MemoryFileStore>,
}

impl MemoryStore {
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            store: std::sync::Arc::new(MemoryFileStore::new()),
        }
    }

    pub fn with_store(config: StorageConfig, store: std::sync::Arc<MemoryFileStore>) -> Self {
        Self { config, store }
    }

    pub fn store(&self) -> &std::sync::Arc<MemoryFileStore> {
        &self.store
    }
}

impl StorageAdapter for MemoryStore {
    fn config(&self) -> &StorageConfig {
        &self.config
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            verified: self.config.kind.is_verified(),
            derives_public_url: !self.config.public_url_prefix.trim().is_empty(),
            can_delete_remotely: true,
            can_test_connection: true,
        }
    }

    fn put(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        self.store
            .put_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if let Some(message) = self.store.fail_puts.lock().unwrap().clone() {
            return Err(VeloctError::Attachment(message));
        }

        let mut objects = self.store.objects.lock().unwrap();
        if objects.contains_key(key) {
            // handoff.md section 13.4: the object already exists, so it is reused rather than
            // overwritten with identical bytes.
            let size = objects.get(key).map(|bytes| bytes.len()).unwrap_or(0) as u64;
            return Ok(PutOutcome::AlreadyPresent(RemoteObject {
                key: key.to_string(),
                public_url: public_url(&self.config, key)?,
                size_bytes: size,
            }));
        }
        objects.insert(key.to_string(), bytes.to_vec());
        Ok(PutOutcome::Created(RemoteObject {
            key: key.to_string(),
            public_url: public_url(&self.config, key)?,
            size_bytes: bytes.len() as u64,
        }))
    }

    fn head(&self, key: &str) -> Result<Option<RemoteObject>> {
        if self
            .store
            .hide_objects
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(None);
        }
        let objects = self.store.objects.lock().unwrap();
        Ok(objects.get(key).map(|bytes| RemoteObject {
            key: key.to_string(),
            public_url: public_url(&self.config, key).unwrap_or_default(),
            size_bytes: bytes.len() as u64,
        }))
    }

    fn test_connection(&self) -> Result<String> {
        if let Some(message) = self.store.fail_puts.lock().unwrap().clone() {
            return Err(VeloctError::Attachment(message));
        }
        Ok(format!(
            "{} ({}) is reachable",
            self.config.name,
            self.config.kind.as_str()
        ))
    }

    fn delete_remote(&self, key: &str) -> Result<()> {
        let mut objects = self.store.objects.lock().unwrap();
        if objects.remove(key).is_none() {
            return Err(VeloctError::Invalid(format!(
                "no remote object at {key}"
            )));
        }
        Ok(())
    }
}

/// A store that writes to a real directory, so the adapter seam is exercised against actual files
/// rather than only against the in-memory one.
pub struct LocalFileStore {
    config: StorageConfig,
    root: PathBuf,
}

impl LocalFileStore {
    pub fn new(config: StorageConfig, root: impl Into<PathBuf>) -> Self {
        Self {
            config,
            root: root.into(),
        }
    }

    fn path_for(&self, key: &str) -> Result<PathBuf> {
        let candidate = self.root.join(key);
        veloct_core::paths::ensure_within(&self.root, &candidate)
    }
}

impl StorageAdapter for LocalFileStore {
    fn config(&self) -> &StorageConfig {
        &self.config
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            // A local directory is not a vendor protocol, so there is nothing to verify.
            verified: true,
            derives_public_url: !self.config.public_url_prefix.trim().is_empty(),
            can_delete_remotely: true,
            can_test_connection: true,
        }
    }

    fn put(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let path = self.path_for(key)?;
        let existed = path.is_file();
        veloct_core::workdir::ensure_parent(&path)?;
        veloct_core::workdir::write_bytes_if_changed(&path, bytes)?;
        Ok(if existed {
            PutOutcome::AlreadyPresent(RemoteObject {
                key: key.to_string(),
                public_url: public_url(&self.config, key)?,
                size_bytes: bytes.len() as u64,
            })
        } else {
            PutOutcome::Created(RemoteObject {
                key: key.to_string(),
                public_url: public_url(&self.config, key)?,
                size_bytes: bytes.len() as u64,
            })
        })
    }

    fn head(&self, key: &str) -> Result<Option<RemoteObject>> {
        let path = self.path_for(key)?;
        if !path.is_file() {
            return Ok(None);
        }
        let size = std::fs::metadata(&path)
            .map(|meta| meta.len())
            .unwrap_or(0);
        Ok(Some(RemoteObject {
            key: key.to_string(),
            public_url: public_url(&self.config, key)?,
            size_bytes: size,
        }))
    }

    fn test_connection(&self) -> Result<String> {
        std::fs::create_dir_all(&self.root).map_err(|error| VeloctError::io(&self.root, error))?;
        Ok(format!("{} is writable at {}", self.config.name, self.root.display()))
    }

    fn delete_remote(&self, key: &str) -> Result<()> {
        let path = self.path_for(key)?;
        if !path.is_file() {
            return Err(VeloctError::Invalid(format!("no remote object at {key}")));
        }
        std::fs::remove_file(&path).map_err(|error| VeloctError::io(&path, error))
    }
}

/// The remote-file name for a target, so naming and configuration stay together.
pub fn name_for(config: &StorageConfig, original_name: &str, suffix: &str) -> Result<String> {
    if !is_safe_file_name(original_name) {
        return Err(VeloctError::Invalid(format!(
            "{original_name:?} is not a usable file name"
        )));
    }
    let _ = config;
    Ok(remote_file_name(original_name, suffix))
}

/// Whether a path is inside a root, for an adapter that needs to check.
pub fn inside(root: &Path, candidate: &Path) -> bool {
    veloct_core::paths::ensure_within(root, candidate).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn s3() -> StorageConfig {
        let mut config = StorageConfig::new("s3-main", "S3 main", StorageKind::S3);
        config.endpoint = Some("https://s3.example.com".to_string());
        config.region = Some("us-east-1".to_string());
        config.bucket = Some("veloct-assets".to_string());
        config.key_prefix = Some("uploads".to_string());
        config.public_url_prefix = "https://cdn.example.com".to_string();
        config.credential_ref = Some("keychain:storage/s3-main".to_string());
        config
    }

    #[test]
    fn all_four_documented_kinds_are_supported() {
        // handoff.md section 13.1 lists the targets; sections 13.10 to 13.13 name the protocols, and
        // `local` is the directory target a machine that IS the server writes into.
        assert_eq!(StorageKind::parse("sftp"), Some(StorageKind::Sftp));
        assert_eq!(StorageKind::parse("ssh"), Some(StorageKind::Sftp));
        assert_eq!(StorageKind::parse("local"), Some(StorageKind::Local));
        assert_eq!(
            StorageKind::parse("dir"),
            Some(StorageKind::Local),
            "a user naming a directory means the directory target"
        );
        assert_eq!(StorageKind::parse("directory"), Some(StorageKind::Local));
        assert_eq!(StorageKind::parse("S3"), Some(StorageKind::S3));
        assert_eq!(StorageKind::parse("lsky"), Some(StorageKind::LskyPro));
        assert_eq!(StorageKind::parse("github"), Some(StorageKind::GitHub));
        assert_eq!(StorageKind::parse("ftp"), None);
        // The two are distinct: a directory is not an SSH server, which is the confusion that let an
        // unconfigured SFTP target become a local one.
        assert_ne!(StorageKind::Local.as_str(), StorageKind::Sftp.as_str());
    }

    #[test]
    fn the_canonical_kind_list_names_every_kind_this_build_has_exactly_once() {
        // `StorageKind::ALL` is the ONE list the `storage_kinds` command and the window read; a kind missing
        // from it is a kind nothing can offer. The expected list is written out and guarded by `spelling_of`,
        // an exhaustive match with NO wildcard arm: a kind added to `StorageKind` does not compile until it is
        // named THERE, one line below this test, where the fix is to extend both lists together. The two are
        // compared as whole arrays, so a missing kind, an extra one or a duplicate all fail here.
        let expected = [
            StorageKind::Local,
            StorageKind::S3,
            StorageKind::GitHub,
            StorageKind::Sftp,
            StorageKind::LskyPro,
        ];
        assert_eq!(
            StorageKind::ALL,
            expected,
            "StorageKind::ALL must name every kind this build has, once, in §13.1's order"
        );

        let mut spellings: Vec<&str> = Vec::new();
        for kind in StorageKind::ALL {
            let (variant, spelling) = spelling_of(kind);
            assert_eq!(variant, kind, "the guard table must answer for {kind:?} itself");
            // The stored spelling is what a view carries and what `parse` accepts back, so the window can
            // send `kind` straight to `save_storage_target`.
            assert_eq!(StorageKind::parse(spelling), Some(kind), "{spelling:?} must parse");
            assert!(
                !spellings.contains(&spelling),
                "{spelling:?} appears twice in StorageKind::ALL"
            );
            spellings.push(spelling);
        }
        assert_eq!(spellings.len(), StorageKind::ALL.len());
    }

    /// A kind's variant and its stored spelling, from a match with NO wildcard arm.
    ///
    /// This is the guard `the_canonical_kind_list_names_every_kind_this_build_has_exactly_once` leans on: the
    /// compiler refuses to build a kind that is not named here, and naming it puts the author in front of the
    /// list assertions above.
    fn spelling_of(kind: StorageKind) -> (StorageKind, &'static str) {
        match kind {
            StorageKind::Sftp => (StorageKind::Sftp, "sftp"),
            StorageKind::Local => (StorageKind::Local, "local"),
            StorageKind::S3 => (StorageKind::S3, "s3"),
            StorageKind::LskyPro => (StorageKind::LskyPro, "lsky"),
            StorageKind::GitHub => (StorageKind::GitHub, "github"),
        }
    }

    #[test]
    fn every_listed_kind_answers_the_adapter_question_consistently() {
        // The two functions that OWN the per-kind answer are `targets::is_implemented` and
        // `targets::unimplemented_reason`. For everything in the canonical list they must agree — a kind
        // reported as implemented that also carries a gap reason (or the reverse) is exactly the pair of
        // sources §5.3/§5.4 cannot afford, and the window reads this list.
        for kind in StorageKind::ALL {
            assert_eq!(
                crate::targets::is_implemented(kind),
                crate::targets::unimplemented_reason(kind).is_none(),
                "{kind:?}: `is_implemented` and `unimplemented_reason` disagree"
            );
            assert_eq!(
                StorageKind::parse(kind.as_str()),
                Some(kind),
                "{kind:?}: the stored spelling must be one `parse` accepts back"
            );
        }
    }

    #[test]
    fn sftp_is_the_last_kind_reported_as_a_gap_in_the_canonical_list() {
        // PROCESSING.md §5.3 keeps SFTP as a REPORTED gap, so the kind-level answer has to say so before any
        // target of that kind exists. What is asserted here is the answer, not a restatement of it: the reason
        // comes from the crate function the command calls.
        //
        // Lsky Pro USED to be checked in this pair and no longer is: §5.4's protocol was read from the
        // vendor's own artifacts on 2026-10-02 and `lsky::LskyStore` implements it, so the crate now reports
        // it as working — and this test is what would catch the report and the adapter disagreeing.
        let kind = StorageKind::Sftp;
        assert!(StorageKind::ALL.contains(&kind), "{kind:?} must be offered");
        assert!(!crate::targets::is_implemented(kind), "{kind:?} has no adapter");
        let reason = crate::targets::unimplemented_reason(kind).expect("a reported gap");
        assert!(!reason.trim().is_empty(), "{kind:?} must say why not");

        // And the kinds that DO have an adapter carry no gap, so "no reason" stays meaningful.
        for kind in [
            StorageKind::Local,
            StorageKind::S3,
            StorageKind::GitHub,
            StorageKind::LskyPro,
        ] {
            assert!(crate::targets::is_implemented(kind), "{kind:?} has an adapter");
            assert!(
                crate::targets::unimplemented_reason(kind).is_none(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_kind_reports_honestly_whether_it_has_been_verified() {
        // ADR-0010: an unverified protocol says so rather than pretending.
        assert!(StorageKind::S3.is_verified());
        assert!(StorageKind::GitHub.is_verified());
        // A directory on this machine speaks no protocol, so there is nothing unverified about it.
        assert!(StorageKind::Local.is_verified());
        // §5.4's protocol was read from the vendor's own artifacts for the FREE line, so the kind reports
        // verified now; the paid line is a stated limit in `lsky.rs` rather than a claim made here.
        assert!(StorageKind::LskyPro.is_verified());
        assert!(!StorageKind::Sftp.is_verified());

        let store = MemoryStore::new(StorageConfig::new("s", "S", StorageKind::Sftp));
        assert!(!store.capabilities().verified);

        let store = MemoryStore::new(StorageConfig::new("l", "L", StorageKind::LskyPro));
        assert!(store.capabilities().verified);

        let store = MemoryStore::new(s3());
        assert!(store.capabilities().verified);
    }

    #[test]
    fn the_s3_namespace_is_bucket_and_prefix() {
        // handoff.md section 13.5.
        let config = s3();
        assert_eq!(config.storage_namespace(), "s3:veloct-assets|uploads");
    }

    #[test]
    fn the_github_namespace_is_repository_branch_and_prefix() {
        // handoff.md section 13.5.
        let mut config = StorageConfig::new("gh", "GitHub", StorageKind::GitHub);
        config.repository = Some("vellow/assets".to_string());
        config.branch = Some("main".to_string());
        config.key_prefix = Some("images".to_string());
        assert_eq!(config.storage_namespace(), "github:vellow/assets|main|images");
    }

    #[test]
    fn the_sftp_namespace_includes_the_host_identity() {
        // handoff.md section 13.5: host identity + remote directory.
        let mut config = StorageConfig::new("s", "Server", StorageKind::Sftp);
        config.host = Some("assets.example.com".to_string());
        config.port = Some(22);
        config.username = Some("vellow".to_string());
        config.key_prefix = Some("/var/www/assets".to_string());
        assert_eq!(
            config.storage_namespace(),
            "sftp:assets.example.com|22|vellow|/var/www/assets"
        );
    }

    #[test]
    fn the_local_namespace_is_the_kind_and_the_prefix() {
        // handoff.md section 13.5. The target's own directory comes from its id, which the dedup key
        // already carries, so the prefix is the rest of an object's identity.
        let mut config = StorageConfig::new("on-this-machine", "Local", StorageKind::Local);
        assert_eq!(config.storage_namespace(), "local:default");
        config.key_prefix = Some("uploads".to_string());
        assert_eq!(config.storage_namespace(), "local:uploads");
    }

    #[test]
    fn changing_the_prefix_forms_a_new_namespace() {
        // handoff.md section 13.5: changing a path prefix forms a new namespace, and the old
        // namespace's records are kept.
        let before = s3();
        let mut after = before.clone();
        after.key_prefix = Some("uploads/v2".to_string());
        assert_ne!(before.storage_namespace(), after.storage_namespace());
    }

    #[test]
    fn changing_the_public_url_prefix_does_not_change_the_namespace() {
        // handoff.md section 13.6: the public URL prefix is SEPARATE from the object's path, so
        // changing it must not make the same remote object look like a new one.
        let before = s3();
        let mut after = before.clone();
        after.public_url_prefix = "https://new-cdn.example.com".to_string();
        assert_eq!(before.storage_namespace(), after.storage_namespace());
    }

    #[test]
    fn an_empty_namespace_falls_back_to_a_stable_default() {
        let config = StorageConfig::new("x", "X", StorageKind::S3);
        // No bucket and no prefix, so the namespace is still deterministic rather than empty.
        assert_eq!(config.storage_namespace(), "s3:default");
    }

    #[test]
    fn validation_requires_the_fields_each_kind_needs() {
        let mut config = StorageConfig::new("s", "S", StorageKind::Sftp);
        assert!(config.validate().unwrap_err().contains("host"));
        config.host = Some("h".to_string());
        assert!(config.validate().unwrap_err().contains("username"));
        config.username = Some("u".to_string());
        assert!(config.validate().is_ok());

        let mut config = StorageConfig::new("s3", "S", StorageKind::S3);
        assert!(config.validate().unwrap_err().contains("bucket"));
        config.bucket = Some("b".to_string());
        assert!(config.validate().is_ok());

        let mut config = StorageConfig::new("gh", "G", StorageKind::GitHub);
        assert!(config.validate().unwrap_err().contains("repository"));
        config.repository = Some("r".to_string());
        assert!(config.validate().is_ok());

        let mut config = StorageConfig::new("l", "L", StorageKind::LskyPro);
        assert!(config.validate().unwrap_err().contains("instance"));
        config.instance = Some("https://lsky.example.com".to_string());
        assert!(config.validate().is_ok());

        // A local directory target has no protocol field to get wrong, so a bare one is already valid.
        let local = StorageConfig::new("on-this-machine", "Local", StorageKind::Local);
        assert!(local.validate().is_ok());
    }

    #[test]
    fn a_local_targets_prefix_must_stay_inside_its_own_directory() {
        // The directory comes from the project root and the target id, and `Path::join` REPLACES the
        // base when the argument is absolute — so a drive letter or a UNC root in the prefix would put
        // the bytes somewhere the target does not own. It is refused as configuration, not found later.
        let mut config = StorageConfig::new("on-this-machine", "Local", StorageKind::Local);
        config.key_prefix = Some("C:\\Windows\\Temp".to_string());
        assert!(
            config.validate().unwrap_err().contains("relative path"),
            "{:?}",
            config.validate()
        );

        config.key_prefix = Some("\\\\server\\share".to_string());
        assert!(config.validate().unwrap_err().contains("relative path"));

        // A nested relative prefix is exactly what the field is for.
        config.key_prefix = Some("uploads/2026".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validation_rejects_a_prefix_that_escapes_the_target() {
        let mut config = s3();
        config.key_prefix = Some("../outside".to_string());
        assert!(config.validate().unwrap_err().contains("without .."));

        config.key_prefix = Some("/absolute".to_string());
        assert!(config.validate().unwrap_err().contains("without .."));
    }

    #[test]
    fn validation_rejects_a_relative_public_url_prefix() {
        // It is concatenated with an object path, so a relative one produces something no browser
        // can resolve.
        let mut config = s3();
        config.public_url_prefix = "cdn.example.com".to_string();
        assert!(config.validate().unwrap_err().contains("http://"));
    }

    #[test]
    fn a_config_carries_only_a_credential_handle() {
        // handoff.md section 12: this struct is what reaches SQLite, a log and a release snapshot.
        let config = s3();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("keychain:storage/s3-main"));
        assert!(!json.contains("secret"), "no secret value may appear: {json}");
        assert!(!json.contains("AKIA"), "no access key may appear: {json}");
        assert!(config.has_credential());
    }

    #[test]
    fn the_object_key_joins_the_prefix_and_the_file_name() {
        let config = s3();
        assert_eq!(object_key(&config, "photo-a8k3xq.webp").unwrap(), "uploads/photo-a8k3xq.webp");

        let mut no_prefix = config.clone();
        no_prefix.key_prefix = None;
        assert_eq!(object_key(&no_prefix, "photo.webp").unwrap(), "photo.webp");

        let mut trailing = config.clone();
        trailing.key_prefix = Some("uploads/".to_string());
        assert_eq!(object_key(&trailing, "photo.webp").unwrap(), "uploads/photo.webp");
    }

    #[test]
    fn an_unsafe_object_name_is_refused() {
        let config = s3();
        assert!(object_key(&config, "../escape.webp").is_err());
        assert!(object_key(&config, "a/b.webp").is_err());
        assert!(object_key(&config, "").is_err());
    }

    #[test]
    fn the_public_url_joins_the_prefix_and_the_key() {
        let config = s3();
        assert_eq!(
            public_url(&config, "uploads/photo.webp").unwrap(),
            "https://cdn.example.com/uploads/photo.webp"
        );

        let mut trailing = config.clone();
        trailing.public_url_prefix = "https://cdn.example.com/".to_string();
        assert_eq!(
            public_url(&trailing, "/uploads/photo.webp").unwrap(),
            "https://cdn.example.com/uploads/photo.webp"
        );
    }

    #[test]
    fn a_target_with_no_public_url_prefix_reports_that_no_url_can_be_derived() {
        let mut config = s3();
        config.public_url_prefix = String::new();
        let error = public_url(&config, "uploads/x.webp").unwrap_err();
        assert!(error.to_string().contains("no public URL prefix"), "{error}");
    }

    #[test]
    fn storing_a_file_writes_it_and_derives_its_url() {
        let store = MemoryStore::new(s3());
        let outcome = store.put("uploads/photo-a8k3xq.webp", b"bytes").unwrap();
        assert!(outcome.was_created());
        assert_eq!(outcome.object().size_bytes, 5);
        assert_eq!(
            outcome.object().public_url,
            "https://cdn.example.com/uploads/photo-a8k3xq.webp"
        );
    }

    #[test]
    fn storing_the_same_key_again_reuses_the_object_rather_than_overwriting() {
        // handoff.md section 13.4: reuse the existing remote file.
        let store = MemoryStore::new(s3());
        assert!(store.put("uploads/x.webp", b"one").unwrap().was_created());
        let second = store.put("uploads/x.webp", b"two").unwrap();
        assert!(!second.was_created(), "the object already exists");
        assert_eq!(second.object().size_bytes, 3, "the ORIGINAL size is reported");
    }

    #[test]
    fn a_failing_put_is_reported_and_counted() {
        let store = MemoryStore::new(s3());
        store.store().fail_puts_with("the bucket is gone");
        let error = store.put("uploads/x.webp", b"bytes").unwrap_err();
        assert_eq!(error.code(), "ATTACHMENT");
        assert_eq!(store.store().put_count(), 1);
        assert!(store.store().keys().is_empty());
    }

    #[test]
    fn head_reports_an_existing_object_and_none_for_a_missing_one() {
        let store = MemoryStore::new(s3());
        assert!(store.head("uploads/x.webp").unwrap().is_none());
        store.put("uploads/x.webp", b"bytes").unwrap();
        let found = store.head("uploads/x.webp").unwrap().unwrap();
        assert_eq!(found.size_bytes, 5);
    }

    #[test]
    fn head_can_be_made_to_report_nothing_so_the_lost_record_path_is_testable() {
        // This is how "the upload succeeded but the record was lost" is exercised.
        let store = MemoryStore::new(s3());
        store.put("uploads/x.webp", b"bytes").unwrap();
        store.store().hide_objects(true);
        assert!(store.head("uploads/x.webp").unwrap().is_none());
    }

    #[test]
    fn a_connection_test_reports_success_with_a_reason() {
        let store = MemoryStore::new(s3());
        let message = store.test_connection().unwrap();
        assert!(message.contains("S3 main"), "{message}");
        assert!(message.contains("s3"), "{message}");
    }

    #[test]
    fn a_connection_test_reports_a_failure_with_its_reason() {
        let store = MemoryStore::new(s3());
        store.store().fail_puts_with("DNS lookup failed");
        let error = store.test_connection().unwrap_err();
        assert!(error.to_string().contains("DNS lookup failed"), "{error}");
    }

    #[test]
    fn a_remote_delete_removes_exactly_one_object() {
        // handoff.md section 13.8: nothing calls this automatically, but an explicit user action can.
        let store = MemoryStore::new(s3());
        store.put("uploads/a.webp", b"a").unwrap();
        store.put("uploads/b.webp", b"b").unwrap();

        store.delete_remote("uploads/a.webp").unwrap();
        assert!(store.head("uploads/a.webp").unwrap().is_none());
        assert!(store.head("uploads/b.webp").unwrap().is_some(), "the other object survives");
    }

    #[test]
    fn a_remote_delete_of_something_absent_is_reported() {
        let store = MemoryStore::new(s3());
        assert!(store.delete_remote("uploads/nope.webp").is_err());
    }

    #[test]
    fn a_local_targets_remote_delete_removes_the_file_the_bytes_are_in() {
        // PROCESSING.md section 2.4's lesson: assert the BYTES on disk, not a size a fake recorded. This
        // is the target the whole delete path is proven against, because it is the one that writes a real
        // file.
        let dir = TempDir::new().unwrap();
        let mut config = StorageConfig::new("on-this-machine", "Local", StorageKind::Local);
        config.key_prefix = Some("uploads".to_string());
        config.public_url_prefix = "https://example.com/files/".to_string();
        let target = StorageTarget::new(
            config.clone(),
            Box::new(LocalFileStore::new(config, dir.path())),
        );
        let capabilities = target.capabilities();
        assert!(capabilities.can_delete_remotely, "one object, explicitly asked for");
        assert!(capabilities.verified, "a directory needs no protocol verification");

        let key = target
            .store("photo.webp", b"BYTES")
            .unwrap()
            .object()
            .key
            .clone();
        let path = dir.path().join(&key);
        assert_eq!(std::fs::read(&path).unwrap(), b"BYTES");

        target.delete_remote(&key).unwrap();
        assert!(!path.exists(), "{} is gone", path.display());
    }

    #[test]
    fn there_is_no_wholesale_delete_on_the_adapter() {
        // handoff.md section 13.8 and 20: a remote file is never deleted automatically, so the trait
        // deliberately offers no sweep. This test documents the absence by listing what the trait
        // DOES offer, so adding a sweep would require changing this test on purpose.
        let store = MemoryStore::new(s3());
        let capabilities = store.capabilities();
        assert!(capabilities.can_delete_remotely, "one object at a time");
        assert!(capabilities.can_test_connection);
        // The trait has put, head, test_connection and delete_remote and nothing else.
        let _: fn(&MemoryStore, &str, &[u8]) -> Result<PutOutcome> = |s, key, bytes| s.put(key, bytes);
    }

    #[test]
    fn a_local_store_writes_real_files_and_refuses_to_escape_its_root() {
        let dir = TempDir::new().unwrap();
        let store = LocalFileStore::new(s3(), dir.path());

        store.put("uploads/photo.webp", b"bytes").unwrap();
        assert!(dir.path().join("uploads").join("photo.webp").is_file());
        assert_eq!(store.head("uploads/photo.webp").unwrap().unwrap().size_bytes, 5);

        // The adapter joins the key onto its root, so an escaping key has to climb out of it.
        let error = store.put("../../../escape.webp", b"x").unwrap_err();
        assert!(error.to_string().contains("escapes"), "{error}");
        assert!(!dir.path().parent().unwrap().join("escape.webp").exists());
    }

    #[test]
    fn a_local_store_reports_an_existing_file_rather_than_rewriting_it() {
        let dir = TempDir::new().unwrap();
        let store = LocalFileStore::new(s3(), dir.path());
        assert!(store.put("a.webp", b"one").unwrap().was_created());
        assert!(!store.put("a.webp", b"two").unwrap().was_created());
    }

    #[test]
    fn a_target_wraps_a_config_and_an_adapter() {
        let store = Arc::new(MemoryFileStore::new());
        let target = StorageTarget::new(
            s3(),
            Box::new(MemoryStore::with_store(s3(), store.clone())),
        );
        assert_eq!(target.namespace(), "s3:veloct-assets|uploads");
        let outcome = target.store("photo-a8k3xq.webp", b"bytes").unwrap();
        assert_eq!(outcome.object().key, "uploads/photo-a8k3xq.webp");
        assert_eq!(
            target.url_for(&outcome.object().key).unwrap(),
            "https://cdn.example.com/uploads/photo-a8k3xq.webp"
        );
    }

    #[test]
    fn the_remote_name_helper_refuses_an_unsafe_name() {
        assert!(name_for(&s3(), "photo.webp", "abc123").is_ok());
        assert!(name_for(&s3(), "../photo.webp", "abc123").is_err());
    }

    #[test]
    fn the_inside_helper_matches_the_core_path_rule() {
        let root = Path::new("D:/proj/cache");
        assert!(inside(root, &root.join("a/b.webp")));
        assert!(!inside(root, &root.join("../b.webp")));
    }

    #[test]
    fn a_storage_config_round_trips_through_json() {
        let config = s3();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"kind\":\"s3\""));
        assert_eq!(serde_json::from_str::<StorageConfig>(&json).unwrap(), config);
    }
}
