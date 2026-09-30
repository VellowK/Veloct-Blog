//! AWS Signature Version 4, for the `Authorization` header with a single-chunk payload.
//!
//! `handoff.md` names S3 twice — §13.11 as an attachment storage target and §17.4 as a cloud backup
//! target — and both need exactly this. It is implemented ONCE, here, for the reason ADR-0015 gives for
//! the GitHub client: a second copy of a signing algorithm is a second chance for every request to be
//! rejected with a signature mismatch nobody can read.
//!
//! The algorithm is the one in AWS's "Signature Calculations for the Authorization Header: Transferring
//! Payload in a Single Chunk" (<https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sig-v4-header-based-auth.html>):
//!
//! ```text
//! CanonicalRequest =
//!   HTTPRequestMethod + '\n' +
//!   CanonicalURI + '\n' +
//!   CanonicalQueryString + '\n' +
//!   CanonicalHeaders + '\n' +
//!   SignedHeaders + '\n' +
//!   Hex(SHA256Hash(RequestPayload))
//!
//! StringToSign =
//!   "AWS4-HMAC-SHA256" + '\n' +
//!   TimeStampISO8601 + '\n' +
//!   Scope + '\n' +
//!   Hex(SHA256Hash(CanonicalRequest))
//!
//! SigningKey = HMAC(HMAC(HMAC(HMAC("AWS4" + SecretKey, Date), Region), Service), "aws4_request")
//! Signature  = Hex(HMAC(SigningKey, StringToSign))
//! ```
//!
//! **The tests below are AWS's own published examples, not values this code produced.** AWS describes
//! them as usable "as a test suite to verify your code", and they are the reason this module can claim
//! to be correct without ever having talked to a live bucket: four documented requests — GET Object,
//! PUT Object, GET Bucket Lifecycle and List Objects — whose canonical request, StringToSign and final
//! signature are all published. A signer that reproduces all four is the signer AWS expects.
//!
//! Three details are easy to get wrong and are therefore stated:
//!
//! * **`UriEncode` is written here rather than taken from the platform.** The documentation says so
//!   explicitly, because URL libraries differ over unreserved characters and over `+` versus `%20`.
//!   This one works on BYTES, so a multi-byte key is encoded as UTF-8 rather than per `char`.
//! * **The canonical URI is NOT normalised.** S3 object keys can contain `//` and `..`, and normalising
//!   them addresses a different object.
//! * **`x-amz-content-sha256` is signed even though S3 does not require it.** The documentation makes it
//!   optional; signing it is what stops the payload being swapped without invalidating the signature.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use veloct_core::secret::Redacted;

/// The algorithm identifier that begins the `Authorization` header.
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// `Hex(SHA256Hash(""))`.
///
/// The payload hash of a request with no body, which S3 requires as a header on every signed request.
pub const EMPTY_PAYLOAD_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// The literal a caller sends instead of a payload hash when the body is not signed.
///
/// Veloct computes the real hash instead, and the reason is in ADR-0016: `UNSIGNED-PAYLOAD` is only
/// valid over HTTPS, so a local `http://` S3-compatible endpoint would be refused. Hashing costs one
/// extra pass over the file and works everywhere.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// `UriEncode()` exactly as the SigV4 documentation defines it.
///
/// Every byte is encoded except `A-Z`, `a-z`, `0-9`, `-`, `.`, `_` and `~`. A space becomes `%20` and
/// never `+`, hex letters are uppercase, and `/` is preserved only when `encode_slash` is false.
pub fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char);
            }
            b'/' if !encode_slash => out.push('/'),
            other => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(HEX[(other >> 4) as usize] as char);
                out.push(HEX[(other & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// The canonical URI for an object key: encoded, with the separators preserved.
///
/// The caller passes the RAW key. Passing an already-encoded one would encode it twice, which is a
/// different object and a signature S3 rejects.
pub fn encode_path(key: &str) -> String {
    format!("/{}", uri_encode(key, false))
}

/// The canonical query string: encoded name/value pairs, sorted by encoded name.
///
/// A subresource with no value keeps its `=` (`lifecycle=`), because that is what the documentation's
/// own example shows, and an empty query string is still a line in the canonical request.
pub fn canonical_query_string<I, K, V>(parameters: I) -> String
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut encoded: Vec<(String, String)> = parameters
        .into_iter()
        .map(|(name, value)| {
            (
                uri_encode(name.as_ref(), true),
                uri_encode(value.as_ref(), true),
            )
        })
        .collect();
    // The documentation: "you must also sort the parameters ... The sorting occurs after encoding."
    encoded.sort();
    encoded
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// A request in the form SigV4 signs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalRequest {
    /// The HTTP method, uppercase.
    pub method: String,
    /// The RAW object key or path, without a leading slash. Empty means the bucket root.
    pub key: String,
    /// The query parameters, unencoded and unsorted.
    pub query: Vec<(String, String)>,
    /// The headers to sign, unencoded and unsorted. `host` must be among them.
    pub headers: Vec<(String, String)>,
    /// `Hex(SHA256Hash(payload))`, or [`EMPTY_PAYLOAD_SHA256`] for no body.
    pub payload_hash: String,
}

impl CanonicalRequest {
    pub fn new(
        method: impl Into<String>,
        key: impl Into<String>,
        query: Vec<(String, String)>,
        headers: Vec<(String, String)>,
        payload_hash: impl Into<String>,
    ) -> Self {
        Self {
            method: method.into(),
            key: key.into(),
            query,
            headers,
            payload_hash: payload_hash.into(),
        }
    }

    /// The encoded URI line.
    pub fn canonical_uri(&self) -> String {
        encode_path(&self.key)
    }

    /// The sorted, lowercase, semicolon-separated header list.
    pub fn signed_headers(&self) -> String {
        let mut names: Vec<String> = self
            .headers
            .iter()
            .map(|(name, _)| name.trim().to_ascii_lowercase())
            .collect();
        names.sort();
        names.join(";")
    }

    /// The canonical headers: `name:trimmed value\n` for each, sorted by lowercase name.
    fn canonical_headers(&self) -> String {
        let mut pairs: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name.trim().to_ascii_lowercase(),
                    // Trim() per the documentation. A value's INTERNAL spaces are preserved.
                    value.trim().to_string(),
                )
            })
            .collect();
        pairs.sort();
        pairs
            .into_iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect()
    }

    /// The canonical request text, which is what gets hashed.
    pub fn render(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            self.method,
            self.canonical_uri(),
            canonical_query_string(self.query.clone()),
            self.canonical_headers(),
            self.signed_headers(),
            self.payload_hash
        )
    }

    /// `Hex(SHA256Hash(CanonicalRequest))`, the last line of the string to sign.
    pub fn canonical_hash(&self) -> String {
        sha256_hex(self.render().as_bytes())
    }
}

/// The two halves of an access key pair.
///
/// The secret is [`Redacted`], so a `Debug` print of a context cannot leak it — §18.1 is about logs, and
/// a signing context is the one struct in this crate that holds the secret.
#[derive(Clone)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: Redacted,
}

impl Credentials {
    pub fn new(access_key: impl Into<String>, secret_key: impl Into<String>) -> Self {
        Self {
            access_key: access_key.into(),
            secret_key: Redacted::new(secret_key.into()),
        }
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("access_key", &self.access_key)
            .field("secret_key", &self.secret_key)
            .finish()
    }
}

/// Everything a signature is scoped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningContext {
    pub region: String,
    pub service: String,
    /// `YYYYMMDDTHHMMSSZ`, the `x-amz-date` value. The date part must agree with it.
    pub timestamp: String,
}

impl SigningContext {
    /// A context for S3 in one region at one instant.
    pub fn s3(region: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            service: "s3".to_string(),
            timestamp: timestamp.into(),
        }
    }

    /// `YYYYMMDD`, the first component of the scope and the first key in the derivation.
    pub fn date(&self) -> &str {
        &self.timestamp[..8.min(self.timestamp.len())]
    }

    /// `<date>/<region>/<service>/aws4_request`.
    pub fn scope(&self) -> String {
        format!(
            "{}/{}/{}/aws4_request",
            self.date(),
            self.region,
            self.service
        )
    }
}

/// The `StringToSign`.
pub fn string_to_sign(canonical_hash: &str, context: &SigningContext) -> String {
    format!(
        "{ALGORITHM}\n{}\n{}\n{canonical_hash}",
        context.timestamp,
        context.scope()
    )
}

/// The derived signing key.
///
/// Four chained HMACs, in the documented order: the secret, the date, the region, the service.
pub fn signing_key(secret_key: &str, context: &SigningContext) -> [u8; 32] {
    let mut mac = hmac_sha256(
        format!("AWS4{secret_key}").as_bytes(),
        context.date().as_bytes(),
    );
    mac = hmac_sha256(&mac, context.region.as_bytes());
    mac = hmac_sha256(&mac, context.service.as_bytes());
    hmac_sha256(&mac, b"aws4_request")
}

/// The `Authorization` header value for a request.
pub fn authorization(
    request: &CanonicalRequest,
    credentials: &Credentials,
    context: &SigningContext,
) -> String {
    let signature = signature(request, credentials, context);
    format!(
        "{ALGORITHM} Credential={}/{},SignedHeaders={},Signature={signature}",
        credentials.access_key,
        context.scope(),
        request.signed_headers()
    )
}

/// The hex signature for a request.
pub fn signature(
    request: &CanonicalRequest,
    credentials: &Credentials,
    context: &SigningContext,
) -> String {
    let key = signing_key(credentials.secret_key.expose(), context);
    let to_sign = string_to_sign(&request.canonical_hash(), context);
    hex_encode(&hmac_sha256(&key, to_sign.as_bytes()))
}

/// `Hex(SHA256Hash(bytes))`.
///
/// Note the name: this HASHES and then encodes. A caller that already has a digest wants
/// [`hex_encode`] instead, and confusing the two hashes twice — which is exactly what happened once
/// here, and what the round-trip tests caught.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Lowercase hex, for bytes that are already a digest.
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// HMAC-SHA256, returning the raw 32 bytes.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .expect("HMAC accepts a key of any length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------------------------------
    // AWS's published examples. Every constant below comes from the documentation, not from this
    // code: <https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sig-v4-header-based-auth.html>
    // ---------------------------------------------------------------------------------------------

    const DOC_ACCESS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
    const DOC_SECRET_KEY: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
    const DOC_TIMESTAMP: &str = "20130524T000000Z";
    const DOC_HOST: &str = "examplebucket.s3.amazonaws.com";

    fn doc_credentials() -> Credentials {
        Credentials::new(DOC_ACCESS_KEY, DOC_SECRET_KEY)
    }

    fn doc_context() -> SigningContext {
        SigningContext::s3("us-east-1", DOC_TIMESTAMP)
    }

    #[test]
    fn the_documented_empty_payload_hash_is_what_this_computes() {
        // S3 requires this header on every signed request, and a wrong constant makes every request
        // fail with a signature mismatch rather than with anything readable.
        assert_eq!(sha256_hex(b""), EMPTY_PAYLOAD_SHA256);
    }

    #[test]
    fn uri_encode_follows_the_documented_rules() {
        // The documentation says to write this function yourself rather than trust a platform's URL
        // library, so its rules are asserted directly.
        assert_eq!(uri_encode("abcXYZ019-._~", false), "abcXYZ019-._~");
        assert_eq!(uri_encode("a b", false), "a%20b", "a space is %20, never +");
        assert_eq!(uri_encode("a/b", false), "a/b");
        assert_eq!(uri_encode("a/b", true), "a%2Fb");
        assert_eq!(uri_encode("test$file.text", true), "test%24file.text");
        assert_eq!(uri_encode("a+b", true), "a%2Bb");
        assert_eq!(uri_encode("^", true), "%5E", "hex letters are uppercase");
        // Multi-byte characters are encoded per UTF-8 BYTE, which a `char`-based implementation that
        // took the code point's value would get wrong.
        assert_eq!(uri_encode("中", true), "%E4%B8%AD");
        assert_eq!(uri_encode("é", true), "%C3%A9");
    }

    #[test]
    fn the_canonical_query_string_is_encoded_and_then_sorted() {
        // "The sorting occurs after encoding", which the documentation states outright.
        assert_eq!(
            canonical_query_string([("prefix", "J"), ("max-keys", "2")]),
            "max-keys=2&prefix=J"
        );
        // A subresource with no value keeps its `=`, as the GET Bucket Lifecycle example shows.
        assert_eq!(canonical_query_string([("lifecycle", "")]), "lifecycle=");
        assert_eq!(canonical_query_string::<[(String, String); 0], _, _>([]), "");
    }

    #[test]
    fn example_get_object_reproduces_the_documented_signature() {
        // The canonical request, verbatim from the documentation.
        let request = CanonicalRequest::new(
            "GET",
            "test.txt",
            Vec::new(),
            vec![
                ("host".to_string(), DOC_HOST.to_string()),
                ("range".to_string(), "bytes=0-9".to_string()),
                (
                    "x-amz-content-sha256".to_string(),
                    EMPTY_PAYLOAD_SHA256.to_string(),
                ),
                ("x-amz-date".to_string(), DOC_TIMESTAMP.to_string()),
            ],
            EMPTY_PAYLOAD_SHA256,
        );

        assert_eq!(
            request.render(),
            "GET\n\
             /test.txt\n\
             \n\
             host:examplebucket.s3.amazonaws.com\n\
             range:bytes=0-9\n\
             x-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n\
             x-amz-date:20130524T000000Z\n\
             \n\
             host;range;x-amz-content-sha256;x-amz-date\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // And the documented hash of that request, and the documented string to sign derived from it.
        assert_eq!(
            request.canonical_hash(),
            "7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972"
        );
        assert_eq!(
            string_to_sign(&request.canonical_hash(), &doc_context()),
            "AWS4-HMAC-SHA256\n\
             20130524T000000Z\n\
             20130524/us-east-1/s3/aws4_request\n\
             7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972"
        );
        assert_eq!(
            signature(&request, &doc_credentials(), &doc_context()),
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
        assert_eq!(
            authorization(&request, &doc_credentials(), &doc_context()),
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,\
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,\
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn example_put_object_reproduces_the_documented_signature() {
        // This one has a body, so the payload hash is a real hash of "Welcome to Amazon S3." — and the
        // documentation publishes that hash, which is asserted here as well.
        let payload_hash = "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072";
        assert_eq!(
            sha256_hex(b"Welcome to Amazon S3."),
            payload_hash,
            "the documented hash of the documented body"
        );

        let request = CanonicalRequest::new(
            "PUT",
            // The RAW key: the `$` is the signer's job to encode, and the documentation's canonical URI
            // shows it as `%24`.
            "test$file.text",
            Vec::new(),
            vec![
                (
                    "Date".to_string(),
                    "Fri, 24 May 2013 00:00:00 GMT".to_string(),
                ),
                ("host".to_string(), DOC_HOST.to_string()),
                (
                    "x-amz-content-sha256".to_string(),
                    payload_hash.to_string(),
                ),
                ("x-amz-date".to_string(), DOC_TIMESTAMP.to_string()),
                (
                    "x-amz-storage-class".to_string(),
                    "REDUCED_REDUNDANCY".to_string(),
                ),
            ],
            payload_hash,
        );

        // Header names are lowercased and sorted, and the value of `Date` keeps its internal spaces.
        assert!(request.render().contains("/test%24file.text\n"), "{}", request.render());
        assert_eq!(
            request.signed_headers(),
            "date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class"
        );
        assert_eq!(
            request.canonical_hash(),
            "9e0e90d9c76de8fa5b200d8c849cd5b8dc7a3be3951ddb7f6a76b4158342019d"
        );
        assert_eq!(
            signature(&request, &doc_credentials(), &doc_context()),
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        );
    }

    #[test]
    fn example_get_bucket_lifecycle_reproduces_the_documented_signature() {
        let request = CanonicalRequest::new(
            "GET",
            "",
            vec![("lifecycle".to_string(), String::new())],
            vec![
                ("host".to_string(), DOC_HOST.to_string()),
                (
                    "x-amz-content-sha256".to_string(),
                    EMPTY_PAYLOAD_SHA256.to_string(),
                ),
                ("x-amz-date".to_string(), DOC_TIMESTAMP.to_string()),
            ],
            EMPTY_PAYLOAD_SHA256,
        );

        assert_eq!(
            request.canonical_hash(),
            "9766c798316ff2757b517bc739a67f6213b4ab36dd5da2f94eaebf79c77395ca"
        );
        assert_eq!(
            signature(&request, &doc_credentials(), &doc_context()),
            "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543"
        );
    }

    #[test]
    fn example_list_objects_reproduces_the_documented_signature() {
        // This is the shape Veloct's own retention listing uses, so it is the most directly relevant
        // vector of the four.
        let request = CanonicalRequest::new(
            "GET",
            "",
            vec![
                ("max-keys".to_string(), "2".to_string()),
                ("prefix".to_string(), "J".to_string()),
            ],
            vec![
                ("host".to_string(), DOC_HOST.to_string()),
                (
                    "x-amz-content-sha256".to_string(),
                    EMPTY_PAYLOAD_SHA256.to_string(),
                ),
                ("x-amz-date".to_string(), DOC_TIMESTAMP.to_string()),
            ],
            EMPTY_PAYLOAD_SHA256,
        );

        assert_eq!(
            request.canonical_hash(),
            "df57d21db20da04d7fa30298dd4488ba3a2b47ca3a489c74750e0f1e7df1b9b7"
        );
        assert_eq!(
            signature(&request, &doc_credentials(), &doc_context()),
            "34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
        );
    }

    #[test]
    fn the_scope_binds_the_signature_to_a_date_a_region_and_a_service() {
        let context = doc_context();
        assert_eq!(context.date(), "20130524");
        assert_eq!(context.scope(), "20130524/us-east-1/s3/aws4_request");

        let elsewhere = SigningContext::s3("eu-central-1", DOC_TIMESTAMP);
        assert_eq!(elsewhere.scope(), "20130524/eu-central-1/s3/aws4_request");

        // A different region must produce a different signature for the same request, because that is
        // what "the signature is scoped" means.
        let request = CanonicalRequest::new(
            "GET",
            "test.txt",
            Vec::new(),
            vec![
                ("host".to_string(), DOC_HOST.to_string()),
                ("x-amz-date".to_string(), DOC_TIMESTAMP.to_string()),
            ],
            EMPTY_PAYLOAD_SHA256,
        );
        assert_ne!(
            signature(&request, &doc_credentials(), &context),
            signature(&request, &doc_credentials(), &elsewhere)
        );
    }

    #[test]
    fn the_signing_key_is_four_chained_hmacs_and_not_the_secret() {
        // A "signing key" equal to the secret would mean the derivation was skipped, and every
        // signature would be wrong in a way that looks like a permissions problem.
        let key = signing_key(DOC_SECRET_KEY, &doc_context());
        assert_eq!(key.len(), 32);
        assert_ne!(&key[..], DOC_SECRET_KEY.as_bytes());
        // Deterministic for the same scope.
        assert_eq!(key, signing_key(DOC_SECRET_KEY, &doc_context()));
        // And different for a different date, region or service.
        assert_ne!(
            key,
            signing_key(DOC_SECRET_KEY, &SigningContext::s3("us-east-1", "20130525T000000Z"))
        );
        assert_ne!(
            key,
            signing_key(DOC_SECRET_KEY, &SigningContext::s3("eu-west-1", DOC_TIMESTAMP))
        );
        assert_ne!(
            key,
            signing_key(
                DOC_SECRET_KEY,
                &SigningContext {
                    region: "us-east-1".to_string(),
                    service: "iam".to_string(),
                    timestamp: DOC_TIMESTAMP.to_string(),
                }
            )
        );
    }

    #[test]
    fn a_secret_never_reaches_a_debug_print() {
        // Section 18.1: the one struct here that holds a secret must not leak it into a log line.
        // The marker asserted is `Redacted`'s own `Debug` output, which is what a log line would carry.
        let credentials = doc_credentials();
        let printed = format!("{credentials:?}");
        assert!(printed.contains(DOC_ACCESS_KEY), "the access key is not a secret");
        assert!(
            !printed.contains(DOC_SECRET_KEY),
            "the secret reached a Debug print: {printed}"
        );
        assert!(printed.contains("Redacted(<secret>)"), "{printed}");
    }

    #[test]
    fn an_unsigned_payload_constant_exists_but_is_not_what_veloct_sends() {
        // ADR-0016: Veloct computes the real payload hash, because UNSIGNED-PAYLOAD is only valid over
        // TLS and a local S3-compatible endpoint is often plain http.
        assert_eq!(UNSIGNED_PAYLOAD, "UNSIGNED-PAYLOAD");
        assert_ne!(EMPTY_PAYLOAD_SHA256, UNSIGNED_PAYLOAD);
    }
}
