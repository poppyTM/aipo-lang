//! Offline GitHub source seam and explicit recursive graph resolution for packages.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use crate::coordinate::PackageId;
use crate::error::{LockfileError, ManifestError, ResolveError};
use crate::lock::{GitHubSource, PackageSource};
use crate::resolver::{PackageInput, ResolvedPackageGraph, resolve_package_inputs};

const MAX_REMOTE_GRAPH_PACKAGES: usize = 256;

/// Raw bytes fetched for one GitHub package directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubArtifact {
    /// Canonical `aipo.toml` bytes.
    pub manifest: Vec<u8>,
    /// Entry file bytes declared by the manifest.
    pub entry: Vec<u8>,
}

impl GitHubArtifact {
    /// Creates an artifact from manifest and entry bytes.
    #[must_use]
    pub fn new(manifest: impl Into<Vec<u8>>, entry: impl Into<Vec<u8>>) -> Self {
        Self {
            manifest: manifest.into(),
            entry: entry.into(),
        }
    }
}

/// Error produced by an offline GitHub source provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubFetchError {
    /// A caller supplied a non-GitHub source.
    UnsupportedSource {
        /// Source label.
        source: String,
    },
    /// The requested pinned source is not present in the provider.
    NotFound {
        /// Source label.
        source: String,
    },
    /// The source contract itself is invalid.
    InvalidSource {
        /// Source label.
        source: String,
        /// Validation detail.
        detail: String,
    },
    /// Fetched bytes did not contain a valid manifest.
    InvalidManifest {
        /// Source label.
        source: String,
        /// Manifest validation detail.
        error: ManifestError,
    },
    /// The HTTP transport failed before returning a response.
    Transport {
        /// Source label.
        source: String,
        /// Sanitized transport detail without response bodies.
        detail: String,
    },
    /// GitHub returned an unexpected HTTP status.
    HttpStatus {
        /// Source label.
        source: String,
        /// HTTP status code.
        status: u16,
    },
    /// The server attempted a redirect.
    Redirect {
        /// Source label.
        source: String,
        /// Redirect status code.
        status: u16,
    },
    /// The response exceeded the configured byte limit.
    ResponseTooLarge {
        /// Source label.
        source: String,
        /// Maximum accepted bytes.
        limit: u64,
    },
    /// The response body was not a valid package artifact.
    InvalidResponse {
        /// Source label.
        source: String,
        /// Response validation detail.
        detail: String,
    },
    /// A local cache operation failed while serving a source.
    Cache {
        /// Source label.
        source: String,
        /// Cache detail.
        detail: String,
    },
}

impl fmt::Display for GitHubFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSource { source } => {
                write!(formatter, "source '{source}' is not a GitHub source")
            }
            Self::NotFound { source } => {
                write!(formatter, "GitHub source '{source}' was not found")
            }
            Self::InvalidSource { source, detail } => {
                write!(formatter, "GitHub source '{source}' is invalid: {detail}")
            }
            Self::InvalidManifest { source, error } => {
                write!(
                    formatter,
                    "GitHub source '{source}' has an invalid manifest: {error}"
                )
            }
            Self::Transport { source, detail } => {
                write!(
                    formatter,
                    "GitHub transport failed for '{source}': {detail}"
                )
            }
            Self::HttpStatus { source, status } => {
                write!(formatter, "GitHub source '{source}' returned HTTP {status}")
            }
            Self::Redirect { source, status } => {
                write!(
                    formatter,
                    "GitHub source '{source}' attempted HTTP redirect {status}"
                )
            }
            Self::ResponseTooLarge { source, limit } => {
                write!(
                    formatter,
                    "GitHub source '{source}' exceeded the {limit}-byte limit"
                )
            }
            Self::InvalidResponse { source, detail } => {
                write!(
                    formatter,
                    "GitHub source '{source}' returned invalid data: {detail}"
                )
            }
            Self::Cache { source, detail } => {
                write!(formatter, "GitHub source '{source}' cache failed: {detail}")
            }
        }
    }
}

impl std::error::Error for GitHubFetchError {}

/// Narrow provider seam for a GitHub-backed resolver.
///
/// Implementations may use a network client, a verified cache, or an offline fixture. The package
/// core does not choose a transport or perform I/O through this trait.
pub trait GitHubFetcher {
    /// Fetches one exact GitHub source.
    ///
    /// # Errors
    ///
    /// Returns a typed provider error when the source is unsupported, absent, or invalid.
    fn fetch(&self, source: &PackageSource) -> Result<GitHubArtifact, GitHubFetchError>;
}

/// A root package input and the graph resolved from its pinned GitHub dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubPackageGraph {
    /// Root package input, including the fetched manifest and entry bytes.
    pub root: PackageInput,
    /// Resolved graph containing the root and all reachable remote packages.
    pub graph: ResolvedPackageGraph,
}

/// An error produced while fetching and resolving a pinned GitHub graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubGraphError {
    /// An artifact could not be fetched or decoded.
    Fetch(GitHubFetchError),
    /// The fetched graph violated the package resolver contract.
    Resolve(ResolveError),
}

impl fmt::Display for GitHubGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(error) => write!(formatter, "GitHub graph fetch failed: {error}"),
            Self::Resolve(error) => write!(formatter, "GitHub graph resolution failed: {error}"),
        }
    }
}

impl std::error::Error for GitHubGraphError {}

impl From<GitHubFetchError> for GitHubGraphError {
    fn from(error: GitHubFetchError) -> Self {
        Self::Fetch(error)
    }
}

impl From<ResolveError> for GitHubGraphError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

/// Fetches and resolves a complete graph rooted at one exact GitHub source.
///
/// Remote packages may declare only pinned GitHub dependencies in this slice. Local path
/// dependencies inside a remote artifact are rejected because the artifact does not provide a
/// caller-owned filesystem root.
///
/// # Errors
///
/// Returns [`GitHubGraphError`] when a source cannot be fetched, a manifest is invalid, a
/// dependency is local, the graph exceeds its package bound, or the resulting graph violates
/// resolver invariants.
pub fn resolve_github_package_graph<F>(
    root_source: PackageSource,
    fetcher: &F,
) -> Result<GitHubPackageGraph, GitHubGraphError>
where
    F: GitHubFetcher + ?Sized,
{
    let root_label = root_source.location();
    let mut requests = VecDeque::new();
    requests.push_back(RemoteRequest {
        source: root_source,
        expected: None,
    });
    let mut inputs = BTreeMap::<String, PackageInput>::new();

    while let Some(request) = requests.pop_front() {
        let source_label = request.source.location();
        if let Some(existing) = inputs.get(&source_label) {
            if let Some(expected) = request.expected {
                verify_expected_dependency(existing, &expected)?;
            }
            continue;
        }
        if inputs.len() >= MAX_REMOTE_GRAPH_PACKAGES {
            return Err(ResolveError::InvalidInput {
                context: source_label,
                detail: format!(
                    "remote package graph exceeds the {MAX_REMOTE_GRAPH_PACKAGES}-package limit"
                ),
            }
            .into());
        }

        let artifact = fetcher.fetch(&request.source)?;
        let input = package_input_from_artifact(request.source.clone(), artifact)?;
        if let Some(expected) = request.expected {
            verify_expected_dependency(&input, &expected)?;
        }

        for dependency in &input.manifest.dependencies {
            let source = match &dependency.source {
                PackageSource::Path { path } => {
                    return Err(ResolveError::InvalidInput {
                        context: input.manifest.name.to_string(),
                        detail: format!(
                            "remote package declares local path dependency '{}'; remote path dependencies are not enabled",
                            path
                        ),
                    }
                    .into());
                }
                PackageSource::GitHub { .. } => dependency.source.clone(),
            };
            requests.push_back(RemoteRequest {
                source,
                expected: Some(ExpectedDependency {
                    dependent: input.manifest.name.clone(),
                    coordinate: dependency.name.clone(),
                    version: dependency.version.clone(),
                }),
            });
        }
        inputs.insert(source_label, input);
    }

    let root = inputs
        .remove(&root_label)
        .ok_or_else(|| ResolveError::InvalidInput {
            context: root_label,
            detail: "GitHub graph did not produce a root package input".to_string(),
        })?;
    let graph = resolve_package_inputs(root.clone(), inputs.into_values())?;
    Ok(GitHubPackageGraph { root, graph })
}

#[derive(Debug)]
struct RemoteRequest {
    source: PackageSource,
    expected: Option<ExpectedDependency>,
}

#[derive(Debug)]
struct ExpectedDependency {
    dependent: PackageId,
    coordinate: PackageId,
    version: Option<String>,
}

fn verify_expected_dependency(
    input: &PackageInput,
    expected: &ExpectedDependency,
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

/// Deterministic in-memory provider used by tests and embedders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InMemoryGitHubStore {
    artifacts: BTreeMap<GitHubSource, GitHubArtifact>,
}

impl InMemoryGitHubStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts bytes for one exact GitHub source.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubFetchError::UnsupportedSource`] for a local path source and
    /// [`GitHubFetchError::InvalidSource`] for a malformed GitHub source.
    pub fn insert(
        &mut self,
        source: PackageSource,
        artifact: GitHubArtifact,
    ) -> Result<(), GitHubFetchError> {
        let label = source.location();
        let key = source
            .github_source()
            .ok_or_else(|| GitHubFetchError::UnsupportedSource {
                source: label.clone(),
            })?;
        validate_source(&source, &label)?;
        self.artifacts.insert(key, artifact);
        Ok(())
    }

    /// Returns the artifact for an exact source, if present.
    #[must_use]
    pub fn get(&self, source: &PackageSource) -> Option<&GitHubArtifact> {
        self.artifacts.get(&source.github_source()?)
    }

    /// Number of pinned sources in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    /// Whether the store has no sources.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }
}

impl GitHubFetcher for InMemoryGitHubStore {
    fn fetch(&self, source: &PackageSource) -> Result<GitHubArtifact, GitHubFetchError> {
        let label = source.location();
        let key = source
            .github_source()
            .ok_or_else(|| GitHubFetchError::UnsupportedSource {
                source: label.clone(),
            })?;
        validate_source(source, &label)?;
        self.artifacts
            .get(&key)
            .cloned()
            .ok_or(GitHubFetchError::NotFound { source: label })
    }
}

/// Converts fetched bytes into a validated package input while retaining the source identity.
///
/// # Errors
///
/// Returns [`GitHubFetchError::InvalidManifest`] when the manifest cannot be parsed or validated.
pub fn package_input_from_artifact(
    source: PackageSource,
    artifact: GitHubArtifact,
) -> Result<PackageInput, GitHubFetchError> {
    let label = source.location();
    if source.github_source().is_none() {
        return Err(GitHubFetchError::UnsupportedSource { source: label });
    }
    validate_source(&source, &label)?;
    PackageInput::from_manifest_bytes(&artifact.manifest, artifact.entry, source).map_err(|error| {
        GitHubFetchError::InvalidManifest {
            source: label,
            error,
        }
    })
}

fn validate_source(source: &PackageSource, label: &str) -> Result<(), GitHubFetchError> {
    source
        .validate()
        .map_err(|error: LockfileError| GitHubFetchError::InvalidSource {
            source: label.to_string(),
            detail: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinate::PackageId;
    use crate::digest::ContentDigest;
    use crate::manifest::Dependency;
    use crate::resolver::{PackageInput, resolve_package_inputs};

    fn source() -> PackageSource {
        PackageSource::github_at(
            "acme/packages",
            "0123456789abcdef0123456789abcdef01234567",
            "packages/http",
        )
        .expect("valid source")
    }

    #[test]
    fn offline_store_fetches_and_builds_package_input() {
        let source = source();
        let mut store = InMemoryGitHubStore::new();
        store
            .insert(
                source.clone(),
                GitHubArtifact::new(
                    "[package]\nname = \"acme.http\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n",
                    "export answer\nfn answer()\nreturn 42\nend\n",
                ),
            )
            .expect("source inserts");
        let artifact = store.fetch(&source).expect("source fetches");
        let input = package_input_from_artifact(source.clone(), artifact).expect("manifest parses");
        assert_eq!(input.manifest.name.as_str(), "acme.http");
        assert_eq!(input.source, source);
    }

    #[test]
    fn offline_store_reports_missing_and_non_github_sources() {
        let source = source();
        let store = InMemoryGitHubStore::new();
        assert!(matches!(
            store.fetch(&source),
            Err(GitHubFetchError::NotFound { .. })
        ));
        assert!(matches!(
            store.fetch(&PackageSource::path("local").expect("path source")),
            Err(GitHubFetchError::UnsupportedSource { .. })
        ));
    }

    #[test]
    fn resolved_graph_preserves_github_source_and_digest() {
        let root_source = PackageSource::path(".").expect("root source");
        let mut root_manifest = crate::Manifest::new(
            PackageId::parse("acme.app").expect("root coordinate"),
            "1.0.0",
            "src/main.aipo",
        )
        .expect("root manifest");
        root_manifest.dependencies.push(Dependency::new(
            PackageId::parse("acme.http").expect("remote coordinate"),
            "../remote",
        ));
        let root = PackageInput::from_manifest(root_manifest, b"import acme.http\n".to_vec())
            .expect("root input")
            .with_source(root_source);
        let remote_source = source();
        let remote = package_input_from_artifact(
            remote_source.clone(),
            GitHubArtifact::new(
                "[package]\nname = \"acme.http\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n",
                "export answer\nfn answer()\nreturn 42\nend\n",
            ),
        )
        .expect("remote input");
        let graph = resolve_package_inputs(root, [remote]).expect("graph resolves");
        let package = graph
            .package(&PackageId::parse("acme.http").expect("coordinate"))
            .expect("remote package");
        assert_eq!(package.source, remote_source);
        assert_eq!(
            package.digest,
            ContentDigest::for_package(
                &package_manifest(),
                "src/main.aipo",
                b"export answer\nfn answer()\nreturn 42\nend\n"
            )
            .expect("digest")
        );
    }

    #[test]
    fn resolves_pinned_remote_dependency_graph_offline() {
        let root_source =
            PackageSource::github_at("acme/root", "0123456789abcdef0123456789abcdef01234567", ".")
                .expect("root source");
        let dependency_source = source();
        let mut store = InMemoryGitHubStore::new();
        store
            .insert(
                root_source.clone(),
                GitHubArtifact::new(
                    "[package]\nname = \"acme.app\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n[dependencies]\n\"acme.http\" = { version = \"1.0.0\", type = \"github\", repository = \"acme/packages\", revision = \"0123456789abcdef0123456789abcdef01234567\", subpath = \"packages/http\" }\n",
                    b"import acme.http\n",
                ),
            )
            .expect("root source inserts");
        store
            .insert(
                dependency_source.clone(),
                GitHubArtifact::new(
                    "[package]\nname = \"acme.http\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n",
                    b"export answer\n",
                ),
            )
            .expect("dependency source inserts");

        let resolved = resolve_github_package_graph(root_source.clone(), &store)
            .expect("remote graph resolves");
        assert_eq!(resolved.root.source, root_source);
        assert_eq!(resolved.graph.topological_order().len(), 2);
        assert_eq!(
            resolved
                .graph
                .package(&PackageId::parse("acme.http").expect("coordinate"))
                .expect("dependency package")
                .source,
            dependency_source
        );
        assert_eq!(resolved.graph.to_lockfile().expect("lockfile").len(), 2);
    }

    #[test]
    fn remote_graph_rejects_local_dependencies_inside_remote_packages() {
        let root_source =
            PackageSource::github_at("acme/root", "0123456789abcdef0123456789abcdef01234567", ".")
                .expect("root source");
        let mut store = InMemoryGitHubStore::new();
        store
            .insert(
                root_source.clone(),
                GitHubArtifact::new(
                    "[package]\nname = \"acme.app\"\nversion = \"1.0.0\"\nentry = \"src/main.aipo\"\n[dependencies]\n\"acme.local\" = { path = \"../local\" }\n",
                    b"let answer = 1\n",
                ),
            )
            .expect("root source inserts");

        let error = resolve_github_package_graph(root_source, &store)
            .expect_err("local remote dependency fails");
        assert!(matches!(
            error,
            GitHubGraphError::Resolve(ResolveError::InvalidInput { .. })
        ));
    }

    fn package_manifest() -> crate::Manifest {
        crate::Manifest::new(
            PackageId::parse("acme.http").expect("coordinate"),
            "1.0.0",
            "src/main.aipo",
        )
        .expect("manifest")
    }
}
