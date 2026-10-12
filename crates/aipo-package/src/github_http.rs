//! Read-only GitHub artifact cache with an opt-in HTTP transport.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "http")]
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest as ShaDigest, Sha256};

use crate::digest::ContentDigest;
use crate::error::ResolveError;
use crate::github::{GitHubArtifact, GitHubFetchError, GitHubFetcher, package_input_from_artifact};
use crate::lock::{GitHubSource, PackageSource};
use crate::manifest::Manifest;
use crate::resolver::{PackageInput, PackagePathMap, ResolvedPackageGraph, resolve_package_inputs};

#[cfg(feature = "http")]
const RAW_GITHUB_HOST: &str = "raw.githubusercontent.com";
#[cfg(feature = "http")]
const DEFAULT_TIMEOUT_SECONDS: u64 = 10;
#[cfg(feature = "http")]
const DEFAULT_MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CACHE_FILE_BYTES: u64 = 4 * 1024 * 1024;
const CACHE_FORMAT_VERSION: u32 = 1;
static NEXT_CACHE_TEMP: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "http")]
mod http {
    use super::*;

    /// HTTP limits and policy for public GitHub fetches.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct GitHubHttpConfig {
        /// Maximum duration of one request.
        pub timeout: Duration,
        /// Maximum bytes accepted for one response.
        pub max_response_bytes: u64,
        /// User-Agent sent with every request.
        pub user_agent: String,
    }

    impl Default for GitHubHttpConfig {
        fn default() -> Self {
            Self {
                timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
                max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                user_agent: format!("aipo-package/{}", env!("CARGO_PKG_VERSION")),
            }
        }
    }

    /// Opaque GitHub credential supplied by the host.
    #[derive(Clone, PartialEq, Eq)]
    pub struct GitHubToken(String);

    impl GitHubToken {
        /// Creates a token without exposing its value through formatting or diagnostics.
        ///
        /// # Errors
        ///
        /// Returns [`GitHubTokenError`] for empty, oversized or control-character-bearing values.
        pub fn new(value: impl Into<String>) -> Result<Self, GitHubTokenError> {
            let value = value.into();
            if value.is_empty() {
                return Err(GitHubTokenError::Empty);
            }
            if value.len() > MAX_GITHUB_TOKEN_BYTES {
                return Err(GitHubTokenError::TooLong {
                    limit: MAX_GITHUB_TOKEN_BYTES,
                });
            }
            if value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            {
                return Err(GitHubTokenError::InvalidCharacter);
            }
            Ok(Self(value))
        }

        pub(super) fn value(&self) -> &str {
            &self.0
        }
    }

    impl std::fmt::Debug for GitHubToken {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_tuple("GitHubToken")
                .field(&"[REDACTED]")
                .finish()
        }
    }

    /// Validation failure for an environment-backed GitHub credential.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GitHubTokenError {
        /// The credential was empty.
        Empty,
        /// The credential exceeded the bounded transport size.
        TooLong {
            /// Maximum accepted credential size.
            limit: usize,
        },
        /// The credential contained whitespace or control characters.
        InvalidCharacter,
    }

    impl std::fmt::Display for GitHubTokenError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Empty => write!(formatter, "GitHub token is empty"),
                Self::TooLong { limit } => {
                    write!(formatter, "GitHub token exceeds {limit} bytes")
                }
                Self::InvalidCharacter => {
                    write!(
                        formatter,
                        "GitHub token contains whitespace or control characters"
                    )
                }
            }
        }
    }

    impl std::error::Error for GitHubTokenError {}

    pub(super) const MAX_GITHUB_TOKEN_BYTES: usize = 4096;

    /// A bounded response returned by an HTTP transport.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct GitHubHttpResponse {
        /// HTTP status code.
        pub status: u16,
        /// Optional redirect location, retained only for policy diagnostics.
        pub location: Option<String>,
        /// Response body, already bounded by the transport.
        pub body: Vec<u8>,
    }

    /// Error returned by the low-level HTTP transport.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GitHubHttpTransportError {
        /// The request could not be completed.
        Request(String),
        /// The request exceeded its timeout.
        Timeout,
        /// The response exceeded the configured byte limit.
        ResponseTooLarge {
            /// Maximum accepted bytes.
            limit: u64,
        },
    }

    impl std::fmt::Display for GitHubHttpTransportError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Request(detail) => write!(formatter, "HTTP request failed: {detail}"),
                Self::Timeout => write!(formatter, "HTTP request timed out"),
                Self::ResponseTooLarge { limit } => {
                    write!(formatter, "HTTP response exceeded {limit} bytes")
                }
            }
        }
    }

    impl std::error::Error for GitHubHttpTransportError {}

    /// Narrow transport seam used by the GitHub fetcher.
    pub trait GitHubHttpTransport {
        /// Performs one public GET request.
        ///
        /// # Errors
        ///
        /// Returns a typed transport error for connection, timeout or response-size failures.
        fn get(
            &self,
            url: &str,
            max_bytes: u64,
        ) -> Result<GitHubHttpResponse, GitHubHttpTransportError>;
    }

    /// Rustls-backed ureq transport with redirects disabled and an optional bearer credential.
    #[derive(Debug, Clone)]
    pub struct UreqGitHubTransport {
        agent: ureq::Agent,
        token: Option<GitHubToken>,
    }

    impl UreqGitHubTransport {
        /// Builds a transport from the public policy configuration.
        #[must_use]
        pub fn new(config: &GitHubHttpConfig) -> Self {
            Self::with_token(config, None)
        }

        /// Builds a transport with an optional host-supplied credential.
        #[must_use]
        pub fn with_token(config: &GitHubHttpConfig, token: Option<GitHubToken>) -> Self {
            let config = ureq::Agent::config_builder()
                .timeout_global(Some(config.timeout))
                .https_only(true)
                .proxy(None)
                .http_status_as_error(false)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .max_response_header_size(64 * 1024)
                .user_agent(config.user_agent.clone())
                .build();
            Self {
                agent: config.into(),
                token,
            }
        }
    }

    impl GitHubHttpTransport for UreqGitHubTransport {
        fn get(
            &self,
            url: &str,
            max_bytes: u64,
        ) -> Result<GitHubHttpResponse, GitHubHttpTransportError> {
            if !is_allowed_github_url(url) {
                return Err(GitHubHttpTransportError::Request(
                    "GitHub URL host is not allowed".to_string(),
                ));
            }
            let mut request = self
                .agent
                .get(url)
                .header("Accept", "application/octet-stream");
            if let Some(authorization) = bearer_authorization(self.token.as_ref()) {
                request = request.header("Authorization", &authorization);
            }
            let mut response = request.call().map_err(map_ureq_error)?;
            if let Some(length) = response
                .headers()
                .get("content-length")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
            {
                if length > max_bytes {
                    return Err(GitHubHttpTransportError::ResponseTooLarge { limit: max_bytes });
                }
            }
            let status = response.status().as_u16();
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string);
            let body = response
                .body_mut()
                .with_config()
                .limit(max_bytes)
                .read_to_vec()
                .map_err(map_ureq_error)?;
            Ok(GitHubHttpResponse {
                status,
                location,
                body,
            })
        }
    }

    pub(super) fn is_allowed_github_url(url: &str) -> bool {
        url.strip_prefix("https://raw.githubusercontent.com/")
            .is_some_and(|path| !path.is_empty() && !path.contains('?'))
    }

    pub(super) fn bearer_authorization(token: Option<&GitHubToken>) -> Option<String> {
        token.map(|token| format!("Bearer {}", token.value()))
    }

    fn map_ureq_error(error: ureq::Error) -> GitHubHttpTransportError {
        match error {
            ureq::Error::Timeout(_) => GitHubHttpTransportError::Timeout,
            ureq::Error::BodyExceedsLimit(limit) => {
                GitHubHttpTransportError::ResponseTooLarge { limit }
            }
            other => GitHubHttpTransportError::Request(other.to_string()),
        }
    }

    /// Read-only GitHub fetcher that resolves raw files at an exact commit.
    #[derive(Debug, Clone)]
    pub struct GitHubHttpFetcher<T = UreqGitHubTransport> {
        transport: T,
        max_response_bytes: u64,
    }

    impl GitHubHttpFetcher<UreqGitHubTransport> {
        /// Builds the default public GitHub transport.
        #[must_use]
        pub fn new() -> Self {
            Self::with_config(&GitHubHttpConfig::default())
        }

        /// Builds the default transport with explicit limits.
        #[must_use]
        pub fn with_config(config: &GitHubHttpConfig) -> Self {
            Self {
                transport: UreqGitHubTransport::new(config),
                max_response_bytes: config.max_response_bytes,
            }
        }

        /// Builds the default transport with a host-supplied credential.
        #[must_use]
        pub fn with_token(token: GitHubToken) -> Self {
            Self::with_config_and_token(&GitHubHttpConfig::default(), token)
        }

        /// Builds a transport with explicit limits and a host-supplied credential.
        #[must_use]
        pub fn with_config_and_token(config: &GitHubHttpConfig, token: GitHubToken) -> Self {
            Self {
                transport: UreqGitHubTransport::with_token(config, Some(token)),
                max_response_bytes: config.max_response_bytes,
            }
        }
    }

    impl Default for GitHubHttpFetcher<UreqGitHubTransport> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<T> GitHubHttpFetcher<T>
    where
        T: GitHubHttpTransport,
    {
        /// Builds a fetcher around a caller-provided transport.
        #[must_use]
        pub fn with_transport(transport: T) -> Self {
            Self {
                transport,
                max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            }
        }

        /// Overrides the response byte limit.
        #[must_use]
        pub fn with_max_response_bytes(mut self, max_response_bytes: u64) -> Self {
            self.max_response_bytes = max_response_bytes;
            self
        }

        /// Returns the configured response limit.
        #[must_use]
        pub fn max_response_bytes(&self) -> u64 {
            self.max_response_bytes
        }

        fn fetch_path(
            &self,
            source: &GitHubSource,
            path: &str,
        ) -> Result<Vec<u8>, GitHubFetchError> {
            let url =
                raw_url(source, path).map_err(|detail| GitHubFetchError::InvalidResponse {
                    source: source.label(),
                    detail,
                })?;
            let response = self
                .transport
                .get(&url, self.max_response_bytes)
                .map_err(|error| map_transport_error(source, error))?;
            if response.body.len() as u64 > self.max_response_bytes {
                return Err(GitHubFetchError::ResponseTooLarge {
                    source: source.label(),
                    limit: self.max_response_bytes,
                });
            }
            match response.status {
                200..=299 => Ok(response.body),
                300..=399 => Err(GitHubFetchError::Redirect {
                    source: source.label(),
                    status: response.status,
                }),
                404 => Err(GitHubFetchError::NotFound {
                    source: source.label(),
                }),
                status => Err(GitHubFetchError::HttpStatus {
                    source: source.label(),
                    status,
                }),
            }
        }
    }

    impl<T> GitHubFetcher for GitHubHttpFetcher<T>
    where
        T: GitHubHttpTransport,
    {
        fn fetch(&self, source: &PackageSource) -> Result<GitHubArtifact, GitHubFetchError> {
            let source =
                source
                    .github_source()
                    .ok_or_else(|| GitHubFetchError::UnsupportedSource {
                        source: source.location(),
                    })?;
            source_validation_error(&source)?;
            let manifest_bytes = self.fetch_path(&source, "aipo.toml")?;
            let manifest = Manifest::from_bytes(&manifest_bytes).map_err(|error| {
                GitHubFetchError::InvalidResponse {
                    source: source.label(),
                    detail: error.to_string(),
                }
            })?;
            join_remote_path(&source.subpath, &manifest.entry).map_err(|detail| {
                GitHubFetchError::InvalidResponse {
                    source: source.label(),
                    detail,
                }
            })?;
            let entry_bytes = self.fetch_path(&source, &manifest.entry)?;
            Ok(GitHubArtifact::new(manifest_bytes, entry_bytes))
        }
    }

    fn map_transport_error(
        source: &GitHubSource,
        error: GitHubHttpTransportError,
    ) -> GitHubFetchError {
        match error {
            GitHubHttpTransportError::Request(detail) => GitHubFetchError::Transport {
                source: source.label(),
                detail,
            },
            GitHubHttpTransportError::Timeout => GitHubFetchError::Transport {
                source: source.label(),
                detail: "request timed out".to_string(),
            },
            GitHubHttpTransportError::ResponseTooLarge { limit } => {
                GitHubFetchError::ResponseTooLarge {
                    source: source.label(),
                    limit,
                }
            }
        }
    }

    fn source_validation_error(source: &GitHubSource) -> Result<(), GitHubFetchError> {
        PackageSource::GitHub {
            repository: source.repository.clone(),
            revision: source.revision.clone(),
            subpath: source.subpath.clone(),
        }
        .validate()
        .map_err(|error| GitHubFetchError::InvalidSource {
            source: source.label(),
            detail: error.to_string(),
        })
    }

    pub(super) fn raw_url(source: &GitHubSource, path: &str) -> Result<String, String> {
        let (owner, repository) = source
            .repository
            .split_once('/')
            .ok_or_else(|| "GitHub repository is not owner/repository".to_string())?;
        let path = join_remote_path(&source.subpath, path)?;
        Ok(format!(
            "https://{RAW_GITHUB_HOST}/{owner}/{repository}/{}/{}",
            source.revision, path
        ))
    }

    fn join_remote_path(base: &str, entry: &str) -> Result<String, String> {
        let combined = if base == "." {
            entry.to_string()
        } else {
            format!("{base}/{entry}")
        };
        if combined.is_empty()
            || combined.starts_with('/')
            || combined.contains('\\')
            || combined.chars().any(char::is_control)
        {
            return Err("remote path is empty or contains an unsafe character".to_string());
        }
        let mut normalized = Vec::new();
        for segment in combined.split('/') {
            match segment {
                "" | "." => continue,
                ".." => return Err("remote path contains parent traversal".to_string()),
                value
                    if value.chars().all(|character| {
                        character.is_ascii_alphanumeric() || "-_.".contains(character)
                    }) =>
                {
                    normalized.push(value);
                }
                _ => return Err("remote path contains an unsupported character".to_string()),
            }
        }
        if normalized.is_empty() {
            Err("remote path is empty".to_string())
        } else {
            Ok(normalized.join("/"))
        }
    }
}

#[cfg(feature = "http")]
pub use http::{
    GitHubHttpConfig, GitHubHttpFetcher, GitHubHttpResponse, GitHubHttpTransport,
    GitHubHttpTransportError, GitHubToken, GitHubTokenError, UreqGitHubTransport,
};

/// Error produced by the local GitHub artifact cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubCacheError {
    /// The cache root or an entry path is not a safe directory.
    InvalidRoot {
        /// Cache path.
        path: String,
        /// Reason.
        detail: String,
    },
    /// A requested source is not present in the cache.
    NotFound {
        /// Source label.
        source: String,
    },
    /// Cache I/O failed.
    Io {
        /// Cache path.
        path: String,
        /// I/O detail.
        detail: String,
    },
    /// Cached bytes are malformed or do not match their digest.
    Corrupt {
        /// Source label.
        source: String,
        /// Corruption detail.
        detail: String,
    },
    /// A source is not a GitHub source.
    UnsupportedSource {
        /// Source label.
        source: String,
    },
}

impl std::fmt::Display for GitHubCacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRoot { path, detail } => {
                write!(formatter, "invalid GitHub cache root '{path}': {detail}")
            }
            Self::NotFound { source } => {
                write!(
                    formatter,
                    "GitHub source '{source}' is not present in the cache"
                )
            }
            Self::Io { path, detail } => write!(formatter, "GitHub cache I/O '{path}': {detail}"),
            Self::Corrupt { source, detail } => {
                write!(
                    formatter,
                    "GitHub cache entry '{source}' is corrupt: {detail}"
                )
            }
            Self::UnsupportedSource { source } => {
                write!(formatter, "source '{source}' is not a GitHub source")
            }
        }
    }
}

impl std::error::Error for GitHubCacheError {}

/// Local, atomic cache for fetched GitHub package artifacts.
#[derive(Debug, Clone)]
pub struct GitHubCache {
    root: PathBuf,
    max_file_bytes: u64,
}

/// Read-only result of auditing an existing GitHub cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheVerification {
    /// Number of cache namespace entries inspected.
    pub entries: usize,
    /// Verified GitHub sources in deterministic namespace order.
    pub sources: Vec<GitHubSource>,
    /// Independent entry errors found during verification.
    pub errors: Vec<GitHubCacheError>,
}

impl GitHubCache {
    /// Creates a cache handle. The root is created only when an artifact is stored or loaded.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_file_bytes: MAX_CACHE_FILE_BYTES,
        }
    }

    /// Overrides the maximum size of each cached file.
    #[must_use]
    pub fn with_max_file_bytes(mut self, max_file_bytes: u64) -> Self {
        self.max_file_bytes = max_file_bytes;
        self
    }

    /// Returns the configured cache root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Verifies every entry currently present in the cache without changing the filesystem.
    ///
    /// Missing and empty cache roots are valid zero-entry results. Namespace and entry safety,
    /// metadata identity, manifest parsing and content digests are checked independently for
    /// each entry. The operation never creates, repairs, deletes or fetches cache data.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubCacheError`] when the cache root or namespace cannot be inspected safely.
    /// Per-entry validation failures are returned in [`CacheVerification::errors`].
    pub fn verify(&self) -> Result<CacheVerification, GitHubCacheError> {
        let Some(namespace) = self.existing_namespace()? else {
            return Ok(CacheVerification {
                entries: 0,
                sources: Vec::new(),
                errors: Vec::new(),
            });
        };
        let mut names = Vec::new();
        for entry in fs::read_dir(&namespace).map_err(|error| GitHubCacheError::Io {
            path: namespace.display().to_string(),
            detail: error.to_string(),
        })? {
            let entry = entry.map_err(|error| GitHubCacheError::Io {
                path: namespace.display().to_string(),
                detail: error.to_string(),
            })?;
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        let entry_count = names.len();
        let mut sources = Vec::new();
        let mut errors = Vec::new();
        for name in names {
            let entry_dir = namespace.join(&name);
            match fs::symlink_metadata(&entry_dir) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    errors.push(GitHubCacheError::InvalidRoot {
                        path: entry_dir.display().to_string(),
                        detail: format!("symbolic links are not allowed for cache entry '{name}'"),
                    });
                    continue;
                }
                Ok(metadata) if !metadata.is_dir() => {
                    errors.push(GitHubCacheError::InvalidRoot {
                        path: entry_dir.display().to_string(),
                        detail: format!("cache entry '{name}' is not a directory"),
                    });
                    continue;
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    errors.push(GitHubCacheError::Io {
                        path: entry_dir.display().to_string(),
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            let metadata_bytes =
                match read_bounded(&entry_dir.join("metadata.json"), self.max_file_bytes, &name) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        errors.push(error);
                        continue;
                    }
                };
            let metadata: CacheMetadata = match serde_json::from_slice(&metadata_bytes) {
                Ok(metadata) => metadata,
                Err(error) => {
                    errors.push(GitHubCacheError::Corrupt {
                        source: name.clone(),
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            let source = match parse_github_source_label(&metadata.source) {
                Ok(source) => source,
                Err(detail) => {
                    errors.push(GitHubCacheError::Corrupt {
                        source: name.clone(),
                        detail,
                    });
                    continue;
                }
            };
            if source.label() != metadata.source {
                errors.push(GitHubCacheError::Corrupt {
                    source: name.clone(),
                    detail: "cache metadata source is not canonical".to_string(),
                });
                continue;
            }
            if cache_key(&source) != name {
                errors.push(GitHubCacheError::Corrupt {
                    source: name.clone(),
                    detail: "cache entry key does not match its source".to_string(),
                });
                continue;
            }
            if let Err(error) = self.load_entry_at(&source, &entry_dir) {
                errors.push(error);
            } else {
                sources.push(source);
            }
        }
        Ok(CacheVerification {
            entries: entry_count,
            sources,
            errors,
        })
    }

    /// Loads and verifies a cached artifact, if present.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubCacheError`] for unsafe roots, malformed metadata, digest mismatches or I/O
    /// failures. A missing entry returns `Ok(None)`.
    pub fn load(&self, source: &PackageSource) -> Result<Option<GitHubArtifact>, GitHubCacheError> {
        self.load_with_entry_path(source)
            .map(|loaded| loaded.map(|(artifact, _)| artifact))
    }

    /// Loads a verified cached artifact and its absolute entry path.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubCacheError`] for unsafe roots, malformed metadata, digest mismatches or
    /// I/O failures. A missing cache returns `Ok(None)` without creating the cache root.
    pub fn load_with_entry_path(
        &self,
        source: &PackageSource,
    ) -> Result<Option<(GitHubArtifact, PathBuf)>, GitHubCacheError> {
        let github = source
            .github_source()
            .ok_or_else(|| GitHubCacheError::UnsupportedSource {
                source: source.location(),
            })?;
        source
            .validate()
            .map_err(|error| GitHubCacheError::Corrupt {
                source: github.label(),
                detail: error.to_string(),
            })?;
        let Some(namespace) = self.existing_namespace()? else {
            return Ok(None);
        };
        self.load_entry_at(&github, &namespace.join(cache_key(&github)))
    }

    fn load_entry_at(
        &self,
        github: &GitHubSource,
        entry_dir: &Path,
    ) -> Result<Option<(GitHubArtifact, PathBuf)>, GitHubCacheError> {
        match fs::symlink_metadata(entry_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: entry_dir.display().to_string(),
                    detail: format!("symbolic links are not allowed for {}", github.label()),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: entry_dir.display().to_string(),
                    detail: "cache entry is not a directory".to_string(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(GitHubCacheError::Io {
                    path: entry_dir.display().to_string(),
                    detail: error.to_string(),
                });
            }
        }
        let source_label = github.label();
        validate_entry_layout(entry_dir, &source_label)?;
        let metadata_bytes = read_bounded(
            &entry_dir.join("metadata.json"),
            self.max_file_bytes,
            &source_label,
        )?;
        let metadata: CacheMetadata =
            serde_json::from_slice(&metadata_bytes).map_err(|error| GitHubCacheError::Corrupt {
                source: source_label.clone(),
                detail: error.to_string(),
            })?;
        if metadata.version != CACHE_FORMAT_VERSION || metadata.source != source_label {
            return Err(GitHubCacheError::Corrupt {
                source: source_label.clone(),
                detail: "cache metadata does not match the requested source".to_string(),
            });
        }
        let manifest_bytes = read_bounded(
            &entry_dir.join("manifest.toml"),
            self.max_file_bytes,
            &source_label,
        )?;
        let entry_path = entry_dir.join("entry.bin");
        let entry_bytes = read_bounded(&entry_path, self.max_file_bytes, &source_label)?;
        let manifest =
            Manifest::from_bytes(&manifest_bytes).map_err(|error| GitHubCacheError::Corrupt {
                source: source_label.clone(),
                detail: error.to_string(),
            })?;
        let digest = ContentDigest::for_package(&manifest, &manifest.entry, &entry_bytes).map_err(
            |error| GitHubCacheError::Corrupt {
                source: source_label.clone(),
                detail: error.to_string(),
            },
        )?;
        if digest.as_str() != metadata.digest {
            return Err(GitHubCacheError::Corrupt {
                source: source_label,
                detail: "cached content digest does not match metadata".to_string(),
            });
        }
        let entry_path = fs::canonicalize(&entry_path).map_err(|error| GitHubCacheError::Io {
            path: entry_path.display().to_string(),
            detail: error.to_string(),
        })?;
        Ok(Some((
            GitHubArtifact::new(manifest_bytes, entry_bytes),
            entry_path,
        )))
    }

    /// Removes one cache entry only after revalidating its complete contents.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubCacheError`] for invalid sources, unsafe roots, corrupt entries or I/O
    /// failures. A missing entry returns `Ok(false)`.
    pub fn remove_verified(&self, source: &PackageSource) -> Result<bool, GitHubCacheError> {
        let github = source
            .github_source()
            .ok_or_else(|| GitHubCacheError::UnsupportedSource {
                source: source.location(),
            })?;
        source
            .validate()
            .map_err(|error| GitHubCacheError::Corrupt {
                source: github.label(),
                detail: error.to_string(),
            })?;
        let Some(namespace) = self.existing_namespace()? else {
            return Ok(false);
        };
        let entry_dir = namespace.join(cache_key(&github));
        match fs::symlink_metadata(&entry_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: entry_dir.display().to_string(),
                    detail: format!("symbolic links are not allowed for {}", github.label()),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: entry_dir.display().to_string(),
                    detail: "cache entry is not a directory".to_string(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(GitHubCacheError::Io {
                    path: entry_dir.display().to_string(),
                    detail: error.to_string(),
                });
            }
        }
        if self.load_entry_at(&github, &entry_dir)?.is_none() {
            return Ok(false);
        }
        fs::remove_dir_all(&entry_dir).map_err(|error| GitHubCacheError::Io {
            path: entry_dir.display().to_string(),
            detail: error.to_string(),
        })?;
        Ok(true)
    }

    /// Atomically stores and verifies an artifact.
    ///
    /// Existing entries are never silently overwritten. A valid existing entry is accepted; a
    /// corrupt existing entry fails closed.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubCacheError`] for invalid content, unsafe roots, I/O failures or an existing
    /// corrupt entry.
    pub fn store(
        &self,
        source: &PackageSource,
        artifact: GitHubArtifact,
    ) -> Result<(), GitHubCacheError> {
        let github = source
            .github_source()
            .ok_or_else(|| GitHubCacheError::UnsupportedSource {
                source: source.location(),
            })?;
        source
            .validate()
            .map_err(|error| GitHubCacheError::Corrupt {
                source: github.label(),
                detail: error.to_string(),
            })?;
        let manifest = Manifest::from_bytes(&artifact.manifest).map_err(|error| {
            GitHubCacheError::Corrupt {
                source: github.label(),
                detail: error.to_string(),
            }
        })?;
        let digest = ContentDigest::for_package(&manifest, &manifest.entry, &artifact.entry)
            .map_err(|error| GitHubCacheError::Corrupt {
                source: github.label(),
                detail: error.to_string(),
            })?;
        let namespace = self.ensure_namespace()?;
        let final_dir = namespace.join(cache_key(&github));
        match fs::symlink_metadata(&final_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: final_dir.display().to_string(),
                    detail: format!("symbolic links are not allowed for {}", github.label()),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: final_dir.display().to_string(),
                    detail: "cache entry is not a directory".to_string(),
                });
            }
            Ok(_) => {
                self.load(source)?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(GitHubCacheError::Io {
                    path: final_dir.display().to_string(),
                    detail: error.to_string(),
                });
            }
        }
        let temp_dir = create_temp_dir(&namespace)?;
        let result = self.write_temp_entry(&temp_dir, &github, &artifact, digest.as_str());
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temp_dir, &final_dir) {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(GitHubCacheError::Io {
                path: final_dir.display().to_string(),
                detail: error.to_string(),
            });
        }
        Ok(())
    }

    fn write_temp_entry(
        &self,
        temp_dir: &Path,
        source: &GitHubSource,
        artifact: &GitHubArtifact,
        digest: &str,
    ) -> Result<(), GitHubCacheError> {
        write_new_file(&temp_dir.join("manifest.toml"), &artifact.manifest, source)?;
        write_new_file(&temp_dir.join("entry.bin"), &artifact.entry, source)?;
        let metadata = CacheMetadata {
            version: CACHE_FORMAT_VERSION,
            source: source.label(),
            digest: digest.to_string(),
        };
        let metadata =
            serde_json::to_vec_pretty(&metadata).map_err(|error| GitHubCacheError::Corrupt {
                source: source.label(),
                detail: error.to_string(),
            })?;
        write_new_file(&temp_dir.join("metadata.json"), &metadata, source)?;
        Ok(())
    }

    fn existing_namespace(&self) -> Result<Option<PathBuf>, GitHubCacheError> {
        if self
            .root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(GitHubCacheError::InvalidRoot {
                path: self.root.display().to_string(),
                detail: "parent traversal is not allowed in cache paths".to_string(),
            });
        }
        reject_symlink_components(&self.root)?;
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: self.root.display().to_string(),
                    detail: "symbolic links are not allowed in the cache root".to_string(),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(GitHubCacheError::InvalidRoot {
                    path: self.root.display().to_string(),
                    detail: "path is not a directory".to_string(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(GitHubCacheError::Io {
                    path: self.root.display().to_string(),
                    detail: error.to_string(),
                });
            }
        }
        let namespace = self.root.join("github-v1");
        reject_symlink_components(&namespace)?;
        match fs::symlink_metadata(&namespace) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(GitHubCacheError::InvalidRoot {
                    path: namespace.display().to_string(),
                    detail: "symbolic links are not allowed in the cache namespace".to_string(),
                })
            }
            Ok(metadata) if !metadata.is_dir() => Err(GitHubCacheError::InvalidRoot {
                path: namespace.display().to_string(),
                detail: "cache namespace is not a directory".to_string(),
            }),
            Ok(_) => Ok(Some(namespace)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(GitHubCacheError::Io {
                path: namespace.display().to_string(),
                detail: error.to_string(),
            }),
        }
    }

    fn ensure_namespace(&self) -> Result<PathBuf, GitHubCacheError> {
        ensure_directory(&self.root)?;
        let namespace = self.root.join("github-v1");
        ensure_directory(&namespace)?;
        Ok(namespace)
    }
}

/// A cached package graph together with the physical entry paths used by module loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPackageGraph {
    /// Root package input, including verified manifest and entry bytes.
    pub root: PackageInput,
    /// Resolved root-plus-transitive package graph.
    pub graph: ResolvedPackageGraph,
    /// Absolute physical entry paths keyed by package coordinate.
    pub paths: PackagePathMap,
}

/// An error produced while loading a package graph from a local cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CachedGraphError {
    /// Cache lookup or verification failed.
    Cache(GitHubCacheError),
    /// A cached artifact could not be decoded into a package input.
    Fetch(GitHubFetchError),
    /// The cached graph violated the package resolver contract.
    Resolve(ResolveError),
}

impl std::fmt::Display for CachedGraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cache(error) => write!(formatter, "cached package graph failed: {error}"),
            Self::Fetch(error) => write!(formatter, "cached artifact is invalid: {error}"),
            Self::Resolve(error) => {
                write!(formatter, "cached package graph resolution failed: {error}")
            }
        }
    }
}

impl std::error::Error for CachedGraphError {}

impl From<GitHubCacheError> for CachedGraphError {
    fn from(error: GitHubCacheError) -> Self {
        Self::Cache(error)
    }
}

impl From<GitHubFetchError> for CachedGraphError {
    fn from(error: GitHubFetchError) -> Self {
        Self::Fetch(error)
    }
}

impl From<ResolveError> for CachedGraphError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

const MAX_CACHED_GRAPH_PACKAGES: usize = 256;

/// Resolves a previously fetched GitHub root and its transitive dependencies from cache only.
///
/// The root source remains a GitHub source for provenance. The returned paths are physical
/// cache paths used only by the module loader. Missing cache entries never trigger a network
/// fallback.
///
/// # Errors
///
/// Returns [`CachedGraphError`] when an artifact is missing, corrupt, malformed, contains a
/// local path dependency, exceeds the graph bound or violates graph invariants.
pub fn resolve_cached_github_graph(
    root_input: PackageInput,
    root_path: impl Into<PathBuf>,
    cache: &GitHubCache,
) -> Result<CachedPackageGraph, CachedGraphError> {
    let root_coordinate = root_input.manifest.name.clone();
    let mut inputs = BTreeMap::new();
    inputs.insert(root_coordinate.clone(), root_input.clone());
    let mut paths = PackagePathMap::new();
    paths.insert(root_coordinate, root_path.into());
    let cache_only = CacheOnlyGitHubFetcher::new(cache);
    resolve_github_package_graph_with_loader(root_input, inputs, paths, cache, &cache_only, false)
}

/// Resolves a local root with its local path branches and pinned GitHub dependencies.
///
/// The supplied inputs and paths must come from [`crate::resolver::LocalPackageResolver::discover_local`].
/// The loader may fetch missing remote artifacts, but every resulting artifact is written and
/// verified through the supplied cache before it enters the graph.
///
/// # Errors
///
/// Returns [`CachedGraphError`] when local inputs are inconsistent, a remote artifact cannot be
/// loaded, a remote artifact declares a local path dependency, the graph exceeds its bound, or
/// graph invariants fail.
pub fn resolve_mixed_package_graph<F>(
    root_input: PackageInput,
    local_inputs: BTreeMap<crate::coordinate::PackageId, PackageInput>,
    local_paths: PackagePathMap,
    cache: &GitHubCache,
    fetcher: &F,
) -> Result<CachedPackageGraph, CachedGraphError>
where
    F: GitHubFetcher + ?Sized,
{
    resolve_github_package_graph_with_loader(
        root_input,
        local_inputs,
        local_paths,
        cache,
        fetcher,
        true,
    )
}

fn resolve_github_package_graph_with_loader<F>(
    root_input: PackageInput,
    base_inputs: BTreeMap<crate::coordinate::PackageId, PackageInput>,
    base_paths: PackagePathMap,
    cache: &GitHubCache,
    fetcher: &F,
    base_paths_are_local: bool,
) -> Result<CachedPackageGraph, CachedGraphError>
where
    F: GitHubFetcher + ?Sized,
{
    let root_coordinate = root_input.manifest.name.clone();
    let root_path =
        base_paths
            .get(&root_coordinate)
            .cloned()
            .ok_or_else(|| ResolveError::InvalidInput {
                context: root_coordinate.to_string(),
                detail: "package graph has no physical path for its root".to_string(),
            })?;
    let root_path = fs::canonicalize(&root_path).map_err(|error| ResolveError::Io {
        path: root_path.display().to_string(),
        detail: error.to_string(),
    })?;
    let mut inputs = BTreeMap::new();
    for input in base_inputs.values() {
        inputs.insert(input.source.location(), input.clone());
    }
    let mut paths = base_paths;
    paths.insert(root_coordinate, root_path);
    let mut requests = VecDeque::new();
    for input in base_inputs.values() {
        enqueue_cached_dependencies(input, &mut requests, base_paths_are_local)?;
    }

    while let Some((source, expected)) = requests.pop_front() {
        let source_label = source.location();
        if let Some(existing) = inputs.get(&source_label) {
            if let Some(expected) = expected {
                verify_cached_expected(existing, &expected)?;
            }
            continue;
        }
        if inputs.len() >= MAX_CACHED_GRAPH_PACKAGES {
            return Err(ResolveError::InvalidInput {
                context: source_label,
                detail: format!(
                    "cached package graph exceeds the {MAX_CACHED_GRAPH_PACKAGES}-package limit"
                ),
            }
            .into());
        }
        let _artifact = fetcher.fetch(&source)?;
        let (artifact, entry_path) =
            cache
                .load_with_entry_path(&source)?
                .ok_or_else(|| GitHubCacheError::NotFound {
                    source: source_label.clone(),
                })?;
        let input = package_input_from_artifact(source.clone(), artifact)?;
        if let Some(expected) = expected {
            verify_cached_expected(&input, &expected)?;
        }
        enqueue_cached_dependencies(&input, &mut requests, false)?;
        paths.insert(input.manifest.name.clone(), entry_path);
        inputs.insert(source_label, input);
    }

    let root_label = root_input.source.location();
    inputs.remove(&root_label);
    let graph = resolve_package_inputs(root_input.clone(), inputs.into_values())?;
    Ok(CachedPackageGraph {
        root: root_input,
        graph,
        paths,
    })
}

fn enqueue_cached_dependencies(
    input: &PackageInput,
    requests: &mut VecDeque<(PackageSource, Option<CachedExpectedDependency>)>,
    allow_path_dependencies: bool,
) -> Result<(), ResolveError> {
    for dependency in &input.manifest.dependencies {
        let source = match &dependency.source {
            PackageSource::Path { path } if allow_path_dependencies => continue,
            PackageSource::Path { path } => {
                return Err(ResolveError::InvalidInput {
                    context: input.manifest.name.to_string(),
                    detail: format!(
                        "cached remote package declares local path dependency '{path}'; remote path dependencies are not enabled"
                    ),
                });
            }
            PackageSource::GitHub { .. } => dependency.source.clone(),
        };
        requests.push_back((
            source,
            Some(CachedExpectedDependency {
                dependent: input.manifest.name.clone(),
                coordinate: dependency.name.clone(),
                version: dependency.version.clone(),
            }),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct CachedExpectedDependency {
    dependent: crate::coordinate::PackageId,
    coordinate: crate::coordinate::PackageId,
    version: Option<String>,
}

fn verify_cached_expected(
    input: &PackageInput,
    expected: &CachedExpectedDependency,
) -> Result<(), ResolveError> {
    if input.manifest.name != expected.coordinate {
        return Err(ResolveError::CoordinateMismatch {
            expected: expected.coordinate.clone(),
            actual: input.manifest.name.clone(),
        });
    }
    if let Some(version) = &expected.version
        && version != &input.manifest.version
    {
        return Err(ResolveError::VersionConflict {
            coordinate: expected.coordinate.clone(),
            expected: version.clone(),
            actual: input.manifest.version.clone(),
            dependent: expected.dependent.clone(),
        });
    }
    Ok(())
}

/// A fetcher that serves verified cache entries before delegating to a transport fetcher.
#[derive(Debug, Clone)]
pub struct CachedGitHubFetcher<'a, F> {
    cache: &'a GitHubCache,
    fetcher: F,
}

impl<'a, F> CachedGitHubFetcher<'a, F> {
    /// Creates a cache-backed fetcher.
    #[must_use]
    pub fn new(cache: &'a GitHubCache, fetcher: F) -> Self {
        Self { cache, fetcher }
    }
}

impl<F> GitHubFetcher for CachedGitHubFetcher<'_, F>
where
    F: GitHubFetcher,
{
    fn fetch(&self, source: &PackageSource) -> Result<GitHubArtifact, GitHubFetchError> {
        match self.cache.load(source) {
            Ok(Some(artifact)) => Ok(artifact),
            Ok(None) => {
                let artifact = self.fetcher.fetch(source)?;
                self.cache
                    .store(source, artifact.clone())
                    .map_err(|error| GitHubFetchError::Cache {
                        source: source.location(),
                        detail: error.to_string(),
                    })?;
                Ok(artifact)
            }
            Err(error) => Err(GitHubFetchError::Cache {
                source: source.location(),
                detail: error.to_string(),
            }),
        }
    }
}

/// A GitHub loader that reads only verified local cache entries.
#[derive(Debug, Clone)]
pub struct CacheOnlyGitHubFetcher<'a> {
    cache: &'a GitHubCache,
}

impl<'a> CacheOnlyGitHubFetcher<'a> {
    /// Creates a cache-only loader.
    #[must_use]
    pub fn new(cache: &'a GitHubCache) -> Self {
        Self { cache }
    }
}

impl GitHubFetcher for CacheOnlyGitHubFetcher<'_> {
    fn fetch(&self, source: &PackageSource) -> Result<GitHubArtifact, GitHubFetchError> {
        match self.cache.load(source) {
            Ok(Some(artifact)) => Ok(artifact),
            Ok(None) => Err(GitHubFetchError::NotFound {
                source: source.location(),
            }),
            Err(error) => Err(GitHubFetchError::Cache {
                source: source.location(),
                detail: error.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CacheMetadata {
    version: u32,
    source: String,
    digest: String,
}

fn cache_key(source: &GitHubSource) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.label().as_bytes());
    crate::digest::hex(&hasher.finalize())
}

#[cfg(test)]
#[test]
fn cache_key_is_the_lowercase_sha256_of_the_source_label() {
    let source = PackageSource::github_at(
        "acme/packages",
        "0123456789abcdef0123456789abcdef01234567",
        ".",
    )
    .expect("valid source")
    .github_source()
    .expect("GitHub source");
    assert_eq!(
        cache_key(&source),
        "413ebdc5cf154d3f24e54df971cd3359a68f7ce8f8098378ffae8b2dc5af84be"
    );
}

fn parse_github_source_label(label: &str) -> Result<GitHubSource, String> {
    let value = label
        .strip_prefix("github:")
        .ok_or_else(|| "cache metadata source must start with 'github:'".to_string())?;
    let (repository, revision_and_subpath) = value
        .split_once('@')
        .ok_or_else(|| "cache metadata source is missing the revision".to_string())?;
    let (revision, subpath) = revision_and_subpath
        .split_once('/')
        .ok_or_else(|| "cache metadata source is missing the subpath".to_string())?;
    PackageSource::github_at(repository, revision, subpath)
        .map_err(|error| error.to_string())
        .and_then(|source| {
            source
                .github_source()
                .ok_or_else(|| "cache metadata source is not a GitHub source".to_string())
        })
}

fn ensure_directory(path: &Path) -> Result<(), GitHubCacheError> {
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(GitHubCacheError::InvalidRoot {
            path: path.display().to_string(),
            detail: "parent traversal is not allowed in cache paths".to_string(),
        });
    }
    reject_symlink_components(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(GitHubCacheError::InvalidRoot {
                    path: path.display().to_string(),
                    detail: "symbolic links are not allowed in the cache root".to_string(),
                });
            }
            if !metadata.is_dir() {
                return Err(GitHubCacheError::InvalidRoot {
                    path: path.display().to_string(),
                    detail: "path is not a directory".to_string(),
                });
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| GitHubCacheError::Io {
                path: path.display().to_string(),
                detail: error.to_string(),
            })?;
            ensure_directory(path)
        }
        Err(error) => Err(GitHubCacheError::Io {
            path: path.display().to_string(),
            detail: error.to_string(),
        }),
    }
}

fn reject_symlink_components(path: &Path) -> Result<(), GitHubCacheError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(ancestor) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            return Err(GitHubCacheError::InvalidRoot {
                path: ancestor.display().to_string(),
                detail: "symbolic links are not allowed in cache paths".to_string(),
            });
        }
    }
    Ok(())
}

fn create_temp_dir(namespace: &Path) -> Result<PathBuf, GitHubCacheError> {
    for _ in 0..16 {
        let sequence = NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = namespace.join(format!(".tmp-{}-{}", std::process::id(), sequence));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(GitHubCacheError::Io {
                    path: path.display().to_string(),
                    detail: error.to_string(),
                });
            }
        }
    }
    Err(GitHubCacheError::Io {
        path: namespace.display().to_string(),
        detail: "could not allocate a unique temporary cache directory".to_string(),
    })
}

fn write_new_file(
    path: &Path,
    bytes: &[u8],
    source: &GitHubSource,
) -> Result<(), GitHubCacheError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| GitHubCacheError::Io {
            path: path.display().to_string(),
            detail: error.to_string(),
        })?;
    file.write_all(bytes)
        .map_err(|error| GitHubCacheError::Io {
            path: path.display().to_string(),
            detail: error.to_string(),
        })?;
    file.sync_all().map_err(|error| GitHubCacheError::Io {
        path: path.display().to_string(),
        detail: error.to_string(),
    })?;
    let _ = source;
    Ok(())
}

fn validate_entry_layout(entry_dir: &Path, source_label: &str) -> Result<(), GitHubCacheError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(entry_dir).map_err(|error| GitHubCacheError::Io {
        path: entry_dir.display().to_string(),
        detail: error.to_string(),
    })? {
        let entry = entry.map_err(|error| GitHubCacheError::Io {
            path: entry_dir.display().to_string(),
            detail: error.to_string(),
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    let expected = vec![
        "entry.bin".to_string(),
        "manifest.toml".to_string(),
        "metadata.json".to_string(),
    ];
    if names != expected {
        return Err(GitHubCacheError::Corrupt {
            source: source_label.to_string(),
            detail: "cache entry contains unexpected files".to_string(),
        });
    }
    Ok(())
}

fn read_bounded(
    path: &Path,
    max_bytes: u64,
    source_label: &str,
) -> Result<Vec<u8>, GitHubCacheError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| GitHubCacheError::Io {
        path: path.display().to_string(),
        detail: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(GitHubCacheError::Corrupt {
            source: source_label.to_string(),
            detail: "cached file is not a regular non-symlink file".to_string(),
        });
    }
    if metadata.len() > max_bytes {
        return Err(GitHubCacheError::Corrupt {
            source: source_label.to_string(),
            detail: format!("cached file exceeds {max_bytes} bytes"),
        });
    }
    let mut file = File::open(path).map_err(|error| GitHubCacheError::Io {
        path: path.display().to_string(),
        detail: error.to_string(),
    })?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| GitHubCacheError::Io {
            path: path.display().to_string(),
            detail: error.to_string(),
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(GitHubCacheError::Corrupt {
            source: source_label.to_string(),
            detail: format!("cached file exceeds {max_bytes} bytes"),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinate::PackageId;
    #[cfg(feature = "http")]
    use crate::github::GitHubFetchError;
    use crate::github::{GitHubArtifact, GitHubFetcher, InMemoryGitHubStore};
    #[cfg(feature = "http")]
    use std::collections::BTreeMap;

    #[cfg(feature = "http")]
    struct FakeTransport {
        responses: BTreeMap<String, GitHubHttpResponse>,
        requests: std::sync::Mutex<Vec<String>>,
    }

    #[cfg(feature = "http")]
    struct ErrorTransport {
        error: GitHubHttpTransportError,
    }

    #[cfg(feature = "http")]
    impl GitHubHttpTransport for ErrorTransport {
        fn get(
            &self,
            _url: &str,
            _max_bytes: u64,
        ) -> Result<GitHubHttpResponse, GitHubHttpTransportError> {
            Err(self.error.clone())
        }
    }

    #[cfg(feature = "http")]
    impl GitHubHttpTransport for FakeTransport {
        fn get(
            &self,
            url: &str,
            _max_bytes: u64,
        ) -> Result<GitHubHttpResponse, GitHubHttpTransportError> {
            self.requests.lock().expect("lock").push(url.to_string());
            self.responses
                .get(url)
                .cloned()
                .ok_or_else(|| GitHubHttpTransportError::Request("fixture miss".to_string()))
        }
    }

    #[cfg(feature = "http")]
    fn source() -> GitHubSource {
        PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            "packages/http",
        )
        .expect("source")
        .github_source()
        .expect("github source")
    }

    fn manifest() -> Vec<u8> {
        b"[package]\nname = \"acme.http\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n"
            .to_vec()
    }

    fn entry() -> Vec<u8> {
        b"export answer\nfn answer()\nreturn 42\nend\n".to_vec()
    }

    #[cfg(feature = "http")]
    #[test]
    fn github_token_is_validated_and_redacted() {
        let token = GitHubToken::new("github-token-value").expect("token");
        let debug = format!("{token:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("github-token-value"));
        assert!(matches!(GitHubToken::new(""), Err(GitHubTokenError::Empty)));
        assert!(matches!(
            GitHubToken::new("token\nvalue"),
            Err(GitHubTokenError::InvalidCharacter)
        ));
        assert!(matches!(
            GitHubToken::new("x".repeat(http::MAX_GITHUB_TOKEN_BYTES + 1)),
            Err(GitHubTokenError::TooLong { .. })
        ));
        let authorization = http::bearer_authorization(Some(&token)).expect("authorization");
        assert!(authorization.starts_with("Bearer "));
        assert!(!http::bearer_authorization(None).is_some());
        assert!(http::is_allowed_github_url(
            "https://raw.githubusercontent.com/acme/packages/main/aipo.toml"
        ));
        assert!(!http::is_allowed_github_url(
            "https://example.com/aipo.toml"
        ));
        assert!(!http::is_allowed_github_url(
            "https://raw.githubusercontent.com.evil.test/aipo.toml"
        ));
    }

    #[cfg(feature = "http")]
    #[test]
    fn fetches_manifest_then_entry_without_credentials() {
        let source = source();
        let base = format!(
            "https://raw.githubusercontent.com/acme/packages/{}/packages/http",
            source.revision
        );
        let mut responses = BTreeMap::new();
        responses.insert(
            format!("{base}/aipo.toml"),
            GitHubHttpResponse {
                status: 200,
                location: None,
                body: manifest(),
            },
        );
        responses.insert(
            format!("{base}/src/main.aipo"),
            GitHubHttpResponse {
                status: 200,
                location: None,
                body: entry(),
            },
        );
        let transport = FakeTransport {
            responses,
            requests: std::sync::Mutex::new(Vec::new()),
        };
        let fetcher = GitHubHttpFetcher::with_transport(transport);
        let artifact = GitHubFetcher::fetch(
            &fetcher,
            &PackageSource::GitHub {
                repository: source.repository.clone(),
                revision: source.revision.clone(),
                subpath: source.subpath.clone(),
            },
        )
        .expect("fetch succeeds");
        assert_eq!(artifact.manifest, manifest());
        assert_eq!(artifact.entry, entry());
    }

    #[cfg(feature = "http")]
    #[test]
    fn rejects_status_redirect_and_transport_failures() {
        let source = source();
        let package_source = PackageSource::GitHub {
            repository: source.repository.clone(),
            revision: source.revision.clone(),
            subpath: source.subpath.clone(),
        };
        let url = http::raw_url(&source, "aipo.toml").expect("url");
        let mut responses = BTreeMap::new();
        responses.insert(
            url.clone(),
            GitHubHttpResponse {
                status: 302,
                location: Some("https://evil.example".to_string()),
                body: Vec::new(),
            },
        );
        let transport = FakeTransport {
            responses,
            requests: std::sync::Mutex::new(Vec::new()),
        };
        let fetcher = GitHubHttpFetcher::with_transport(transport);
        assert!(matches!(
            GitHubFetcher::fetch(&fetcher, &package_source),
            Err(GitHubFetchError::Redirect { status: 302, .. })
        ));

        let transport = FakeTransport {
            responses: BTreeMap::new(),
            requests: std::sync::Mutex::new(Vec::new()),
        };
        let fetcher = GitHubHttpFetcher::with_transport(transport);
        assert!(matches!(
            GitHubFetcher::fetch(&fetcher, &package_source),
            Err(GitHubFetchError::Transport { .. })
        ));
    }

    #[cfg(feature = "http")]
    #[test]
    fn maps_transport_timeout_and_size_errors() {
        let source = source();
        let package_source = PackageSource::GitHub {
            repository: source.repository.clone(),
            revision: source.revision.clone(),
            subpath: source.subpath.clone(),
        };
        let timeout = GitHubHttpFetcher::with_transport(ErrorTransport {
            error: GitHubHttpTransportError::Timeout,
        });
        assert!(matches!(
            GitHubFetcher::fetch(&timeout, &package_source),
            Err(GitHubFetchError::Transport { .. })
        ));
        let oversized = GitHubHttpFetcher::with_transport(ErrorTransport {
            error: GitHubHttpTransportError::ResponseTooLarge { limit: 7 },
        });
        assert!(matches!(
            GitHubFetcher::fetch(&oversized, &package_source),
            Err(GitHubFetchError::ResponseTooLarge { limit: 7, .. })
        ));
    }

    #[test]
    fn cached_graph_resolves_transitive_sources_and_physical_paths() {
        let revision = "0123456789abcdef0123456789abcdef01234567";
        let root_source =
            PackageSource::github_at("acme/root", revision, ".").expect("root source");
        let dependency_source =
            PackageSource::github_at("acme/packages", revision, "packages/http")
                .expect("dependency source");
        let root_manifest = format!(
            "[package]\nname = \"acme.app\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n[dependencies]\n\"acme.http\" = {{ version = \"1.0.0\", type = \"github\", repository = \"acme/packages\", revision = \"{revision}\", subpath = \"packages/http\" }}\n"
        );
        let root_entry = b"import acme.http\nlet answer = http.answer()\n".to_vec();
        let root_path = std::env::temp_dir().join(format!(
            "aipo-cached-graph-root-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root_path.parent().expect("root parent")).expect("root parent creates");
        fs::write(&root_path, &root_entry).expect("root entry writes");
        let cache_root = std::env::temp_dir().join(format!(
            "aipo-cached-graph-cache-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = GitHubCache::new(&cache_root);
        cache
            .store(
                &dependency_source,
                GitHubArtifact::new(
                    b"[package]\nname = \"acme.http\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n",
                    b"export answer\nfn answer()\nreturn 42\nend\n",
                ),
            )
            .expect("dependency caches");
        let root_input = PackageInput::from_manifest_bytes(
            root_manifest.as_bytes(),
            root_entry,
            root_source.clone(),
        )
        .expect("root input parses");

        let resolved = resolve_cached_github_graph(root_input, &root_path, &cache)
            .expect("cached graph resolves");
        assert_eq!(resolved.graph.topological_order().len(), 2);
        assert_eq!(
            resolved
                .graph
                .package(&PackageId::parse("acme.http").expect("coordinate"))
                .expect("dependency package")
                .source,
            dependency_source
        );
        let dependency_path = resolved
            .paths
            .get(&PackageId::parse("acme.http").expect("coordinate"))
            .expect("dependency path");
        assert!(dependency_path.is_absolute());
        assert!(dependency_path.ends_with("entry.bin"));

        let _ = fs::remove_file(root_path);
        let _ = fs::remove_dir_all(cache_root);
    }

    #[test]
    fn cache_read_does_not_create_a_missing_root() {
        let source = PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            ".",
        )
        .expect("source");
        let root = std::env::temp_dir().join(format!(
            "aipo-cache-read-missing-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = GitHubCache::new(&root);
        assert!(
            cache
                .load(&source)
                .expect("missing cache is clean")
                .is_none()
        );
        assert!(!root.exists());
        let verification = cache.verify().expect("missing cache verifies cleanly");
        assert_eq!(verification.entries, 0);
        assert!(verification.errors.is_empty());
        assert!(!root.exists());
    }

    #[test]
    fn cache_verify_reports_valid_and_corrupt_entries() {
        let source = PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            "packages/http",
        )
        .expect("source");
        let root = std::env::temp_dir().join(format!(
            "aipo-cache-verify-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = GitHubCache::new(&root);
        cache
            .store(&source, GitHubArtifact::new(manifest(), entry()))
            .expect("store");
        let verification = cache.verify().expect("valid cache verifies");
        assert_eq!(verification.entries, 1);
        assert_eq!(verification.sources.len(), 1);
        assert!(verification.errors.is_empty());

        let entry_path = root
            .join("github-v1")
            .join(cache_key(&source.github_source().expect("source")))
            .join("entry.bin");
        fs::write(&entry_path, b"corrupt").expect("corrupt fixture");
        let verification = cache.verify().expect("corrupt cache is inspected");
        assert_eq!(verification.entries, 1);
        assert!(matches!(
            verification.errors.as_slice(),
            [GitHubCacheError::Corrupt { .. }]
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_remove_verified_deletes_only_a_valid_entry() {
        let source = PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            ".",
        )
        .expect("source");
        let root = std::env::temp_dir().join(format!(
            "aipo-cache-remove-verified-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = GitHubCache::new(&root);
        cache
            .store(&source, GitHubArtifact::new(manifest(), entry()))
            .expect("store");
        assert!(cache.remove_verified(&source).expect("remove succeeds"));
        assert!(cache.load(&source).expect("load after remove").is_none());
        assert!(
            !cache
                .remove_verified(&source)
                .expect("missing remove is clean")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_verify_rejects_unexpected_namespace_entries() {
        let root = std::env::temp_dir().join(format!(
            "aipo-cache-verify-unexpected-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let namespace = root.join("github-v1");
        fs::create_dir_all(&namespace).expect("namespace creates");
        fs::write(namespace.join("unexpected"), b"data").expect("unexpected file writes");
        let cache = GitHubCache::new(&root);
        let verification = cache.verify().expect("namespace is inspected");
        assert_eq!(verification.entries, 1);
        assert!(!verification.errors.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn cache_rejects_symlinked_entry_files() {
        let source = PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            ".",
        )
        .expect("source");
        let root = std::env::temp_dir().join(format!(
            "aipo-cache-entry-link-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = GitHubCache::new(&root);
        cache
            .store(&source, GitHubArtifact::new(manifest(), entry()))
            .expect("store");
        let entry_path = root
            .join("github-v1")
            .join(cache_key(&source.github_source().expect("source")))
            .join("entry.bin");
        let target = entry_path.with_extension("target");
        fs::rename(&entry_path, &target).expect("entry rename");
        std::os::unix::fs::symlink(&target, &entry_path).expect("entry symlink");
        assert!(matches!(
            cache.load_with_entry_path(&source),
            Err(GitHubCacheError::Corrupt { .. })
        ));
        let verification = cache.verify().expect("symlink is inspected");
        assert_eq!(verification.entries, 1);
        assert!(matches!(
            verification.errors.as_slice(),
            [GitHubCacheError::Corrupt { .. }]
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_round_trips_and_detects_corruption() {
        let source = PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            "packages/http",
        )
        .expect("source");
        let root = std::env::temp_dir().join(format!(
            "aipo-github-cache-test-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = GitHubCache::new(&root);
        let artifact = GitHubArtifact::new(manifest(), entry());
        cache.store(&source, artifact.clone()).expect("store");
        assert_eq!(cache.load(&source).expect("load"), Some(artifact));
        let entry_path = cache
            .root()
            .join("github-v1")
            .join(cache_key(&source.github_source().expect("github")))
            .join("entry.bin");
        fs::write(entry_path, b"corrupt").expect("corrupt fixture");
        assert!(matches!(
            cache.load(&source),
            Err(GitHubCacheError::Corrupt { .. })
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cached_fetcher_populates_and_reuses_verified_entries() {
        let source = PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            ".",
        )
        .expect("source");
        let artifact = GitHubArtifact::new(manifest(), entry());
        let mut store = InMemoryGitHubStore::new();
        store
            .insert(source.clone(), artifact.clone())
            .expect("source inserts");
        let root = std::env::temp_dir().join(format!(
            "aipo-cached-fetcher-test-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = GitHubCache::new(&root);
        let fetcher = CachedGitHubFetcher::new(&cache, store);
        let first = GitHubFetcher::fetch(&fetcher, &source).expect("first fetch");
        let second = GitHubFetcher::fetch(&fetcher, &source).expect("cached fetch");
        assert_eq!(first, artifact);
        assert_eq!(second, artifact);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_rejects_unsafe_source_and_symlink_root() {
        let source = PackageSource::path("local").expect("path");
        let cache = GitHubCache::new(std::env::temp_dir().join("aipo-cache-not-created"));
        assert!(matches!(
            cache.store(&source, GitHubArtifact::new(manifest(), entry())),
            Err(GitHubCacheError::UnsupportedSource { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cache_rejects_symlink_components() {
        let base = std::env::temp_dir().join(format!(
            "aipo-github-cache-link-test-{}-{}",
            std::process::id(),
            NEXT_CACHE_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let target = base.join("target");
        let link = base.join("link");
        fs::create_dir_all(&target).expect("target directory");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let cache = GitHubCache::new(link.join("cache"));
        let source = PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            ".",
        )
        .expect("source");
        assert!(matches!(
            cache.store(&source, GitHubArtifact::new(manifest(), entry())),
            Err(GitHubCacheError::InvalidRoot { .. })
        ));
        let _ = fs::remove_dir_all(base);
    }
}
