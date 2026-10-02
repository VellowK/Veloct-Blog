//! S3 / S3-compatible, over AWS Signature Version 4.
#![forbid(unsafe_code)]

pub mod client;
pub mod sigv4;

pub use client::{
    amz_timestamp, bucket_placeholder_problem, hash_file, parse_list_response, Addressing, ListPage,
    S3Client, S3Config, S3Object,
};
pub use sigv4::{
    authorization, canonical_query_string, encode_path, hex_encode, sha256_hex, signature,
    signing_key, string_to_sign, uri_encode, CanonicalRequest, Credentials, SigningContext,
    ALGORITHM, EMPTY_PAYLOAD_SHA256, UNSIGNED_PAYLOAD,
};
