//! Deterministic local package discovery and resolution.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use aipo_host::{CapabilityError, CapabilitySet};
use serde::{Deserialize, Serialize};

use crate::capabilities::{first_outside_bound, parse_capabilities, union_capabilities};
use crate::coordinate::PackageId;
use crate::digest::ContentDigest;
use crate::error::ResolveError;
use crate::lock::{LockedPackage, Lockfile, PackageSource};
use crate::manifest::Manifest;

/// Bytes and metadata for one package supplied by a caller or the filesystem helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInput {
    /// Validated package manifest.
    pub manifest: Manifest,
    /// Original manifest bytes when the input came from a file or caller.
    pub manifest_bytes: Vec<u8>,
    /// Relative entry path used for the package digest.
    pub entry: String,
    /// Entry file bytes.
    pub entry_bytes: Vec<u8>,
    /// Relative source location for the lockfile.
    pub source: PackageSource,
}

impl PackageInput {
    /// Creates an input from a validated manifest and entry bytes.
    ///
    /// # Errors
    ///
    /// Returns the manifest validation error when the supplied manifest is invalid.
    pub fn from_manifest(
        manifest: Manifest,
        entry_bytes: Vec<u8>,
    ) -> Result<Self, crate::error::ManifestError> {
        manifest.validate()?;
        let entry = manifest.entry.clone();
        Ok(Self {
            manifest,
            manifest_bytes: Vec::new(),
            entry,
            entry_bytes,
            source: PackageSource::Path {
                path: ".".to_string(),
            },
        })
    }

    /// Creates an input by parsing manifest bytes.
    ///
    /// # Errors
    ///
    /// Returns the manifest decoding or validation error.
    pub fn from_manifest_bytes(
        manifest_bytes: &[u8],
        entry_bytes: Vec<u8>,
        source: PackageSource,
    ) -> Result<Self, crate::error::ManifestError> {
        let manifest = Manifest::from_bytes(manifest_bytes)?;
        let entry = manifest.entry.clone();
        Ok(Self {
            manifest,
            manifest_bytes: manifest_bytes.to_vec(),
            entry,
            entry_bytes,
            source,
        })
    }

    /// Replaces the source path used in a generated lockfile.
    #[must_use]
    pub fn with_source(mut self, source: PackageSource) -> Self {
        self.source = source;
        self
    }

    /// Replaces the relative entry path used for digesting.
    #[must_use]
    pub fn with_entry(mut self, entry: impl Into<String>) -> Self {
        self.entry = entry.into();
        self
    }
}

/// One package node in a resolved graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPackage {
    /// Canonical package coordinate.
    pub coordinate: PackageId,
    /// Exact manifest SemVer version.
    pub version: String,
    /// Relative source path.
    pub source: PackageSource,
    /// SHA-256 digest of canonical manifest and entry content.
    pub digest: ContentDigest,
    /// Direct dependency coordinates in canonical order.
    pub dependencies: Vec<PackageId>,
    /// Normalized package capabilities.
    pub capabilities: Vec<String>,
    /// Normalized compilation targets.
    pub targets: Vec<String>,
    /// Discovered relative entry path.
    pub entry: String,
}

impl ResolvedPackage {
    fn from_input(input: &PackageInput) -> Result<Self, ResolveError> {
        let mut manifest = input.manifest.clone();
        manifest
            .canonicalize()
            .map_err(|error| ResolveError::Manifest {
                path: input.source.location(),
                error,
            })?;
        input.source.validate().map_err(ResolveError::Lockfile)?;
        if !is_safe_entry_path(&input.entry) {
            return Err(ResolveError::InvalidInput {
                context: input.manifest.name.to_string(),
                detail: "entry path must be relative and portable".to_string(),
            });
        }
        let digest = ContentDigest::for_package(&manifest, &input.entry, &input.entry_bytes)?;
        Ok(Self {
            coordinate: manifest.name,
            version: manifest.version,
            source: input.source.clone(),
            digest,
            dependencies: manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.name.clone())
                .collect(),
            capabilities: manifest.capabilities,
            targets: manifest.targets,
            entry: input.entry.clone(),
        })
    }

    /// Converts this graph node to its lockfile representation.
    #[must_use]
    pub fn to_locked_package(&self) -> LockedPackage {
        LockedPackage::new(
            self.coordinate.clone(),
            self.version.clone(),
            self.source.clone(),
            self.digest.clone(),
            self.dependencies.clone(),
            self.capabilities.clone(),
        )
        .with_targets(self.targets.clone())
    }
}

/// A deterministic resolved package graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPackageGraph {
    /// Root package coordinate.
    pub root: PackageId,
    /// Packages in dependency-first topological order.
    pub packages: Vec<ResolvedPackage>,
    /// Normalized capabilities used by the root after transitive computation.
    pub capabilities: Vec<String>,
    /// Normalized upper bound declared by the root manifest.
    pub root_capabilities: Vec<String>,
}

impl ResolvedPackageGraph {
    fn new(root: PackageId, packages: Vec<ResolvedPackage>) -> Result<Self, ResolveError> {
        let root_package = packages
            .iter()
            .find(|package| package.coordinate == root)
            .ok_or_else(|| ResolveError::InvalidInput {
                context: root.to_string(),
                detail: "resolved graph does not contain its root".to_string(),
            })?;
        let root_capabilities = parse_capabilities(&root_package.capabilities)?;
        let mut effective = root_capabilities.clone();
        for package in &packages {
            let package_capabilities = parse_capabilities(&package.capabilities)?;
            if let Some(capability) = first_outside_bound(&root_capabilities, &package_capabilities)
            {
                return Err(ResolveError::CapabilityLimitExceeded {
                    root: root.clone(),
                    package: package.coordinate.clone(),
                    capability: capability.to_string(),
                });
            }
            effective = union_capabilities([&effective, &package_capabilities]);
        }

        let graph = Self {
            root,
            packages,
            capabilities: effective.iter().map(ToString::to_string).collect(),
            root_capabilities: root_capabilities.iter().map(ToString::to_string).collect(),
        };
        graph.to_lockfile()?;
        Ok(graph)
    }

    /// Returns a package by coordinate.
    #[must_use]
    pub fn package(&self, coordinate: &PackageId) -> Option<&ResolvedPackage> {
        self.packages
            .iter()
            .find(|package| &package.coordinate == coordinate)
    }

    /// Returns the dependency-first coordinate order.
    #[must_use]
    pub fn topological_order(&self) -> Vec<PackageId> {
        self.packages
            .iter()
            .map(|package| package.coordinate.clone())
            .collect()
    }

    /// Parses the effective capability set.
    ///
    /// # Errors
    ///
    /// Returns [`CapabilityError`] only if a hand-constructed graph contains invalid names.
    pub fn capability_set(&self) -> Result<CapabilitySet, CapabilityError> {
        parse_capabilities(&self.capabilities)
    }

    /// Creates a canonical lockfile from this graph.
    ///
    /// # Errors
    ///
    /// Returns [`ResolveError::Lockfile`] if graph-derived records violate lock invariants.
    pub fn to_lockfile(&self) -> Result<Lockfile, ResolveError> {
        Lockfile::new(
            self.packages
                .iter()
                .map(ResolvedPackage::to_locked_package)
                .collect(),
        )
        .map_err(ResolveError::Lockfile)
    }

    /// Validates the graph's lock projection.
    ///
    /// # Errors
    ///
    /// Returns [`ResolveError::Lockfile`] when the projection is invalid.
    pub fn validate(&self) -> Result<(), ResolveError> {
        self.to_lockfile().map(|_| ())
    }
}

/// Filesystem entry paths keyed by their canonical package coordinate.
///
/// This map is a local filesystem adapter and is intentionally not part of the serializable
/// package graph or lockfile model.
pub type PackagePathMap = BTreeMap<PackageId, PathBuf>;

/// A resolved local package graph together with its discovered filesystem entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLocalPackages {
    /// Resolved package graph.
    pub graph: ResolvedPackageGraph,
    /// Absolute paths to each package's discovered entry file.
    pub paths: PackagePathMap,
}

impl ResolvedLocalPackages {
    /// Splits the serializable graph from the filesystem adapter.
    #[must_use]
    pub fn into_parts(self) -> (ResolvedPackageGraph, PackagePathMap) {
        (self.graph, self.paths)
    }
}

/// Local package inputs and entry paths discovered before remote edges are loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredLocalPackages {
    /// Coordinate of the package rooted at the configured filesystem root.
    pub root: PackageId,
    /// Inputs for every reachable local package.
    pub inputs: BTreeMap<PackageId, PackageInput>,
    /// Absolute entry paths for the discovered local packages.
    pub paths: PackagePathMap,
}

/// Resolves an in-memory package store supplied by a CLI or another adapter.
///
/// # Errors
///
/// Returns a typed [`ResolveError`] for invalid manifests, missing dependencies, cycles,
/// version conflicts, duplicate coordinates, or capability-bound violations.
pub fn resolve_manifests(
    root_input: PackageInput,
    packages: &BTreeMap<PackageId, PackageInput>,
) -> Result<ResolvedPackageGraph, ResolveError> {
    let root = root_input.manifest.name.clone();
    let mut inputs = BTreeMap::new();
    register_input(&mut inputs, root_input)?;
    for input in packages.values() {
        register_input(&mut inputs, input.clone())?;
    }
    resolve_from_inputs(inputs, root)
}

/// Resolves an arbitrary sequence of in-memory package inputs.
///
/// # Errors
///
/// Returns a typed [`ResolveError`] for invalid inputs or graph failures.
pub fn resolve_package_inputs<I>(
    root_input: PackageInput,
    inputs: I,
) -> Result<ResolvedPackageGraph, ResolveError>
where
    I: IntoIterator<Item = PackageInput>,
{
    let root = root_input.manifest.name.clone();
    let mut store = BTreeMap::new();
    register_input(&mut store, root_input)?;
    for input in inputs {
        register_input(&mut store, input)?;
    }
    resolve_from_inputs(store, root)
}

/// Resolves a local package directory through the filesystem helper.
///
/// # Errors
///
/// Returns a typed [`ResolveError`] for discovery, resolution, or lock validation failures.
pub fn resolve_path(root: impl AsRef<Path>) -> Result<ResolvedPackageGraph, ResolveError> {
    LocalPackageResolver::new(root.as_ref().to_path_buf()).resolve()
}

/// Resolves a local package directory and returns its filesystem entry paths.
///
/// # Errors
///
/// Returns a typed [`ResolveError`] for discovery, resolution, or lock validation failures.
pub fn resolve_path_with_paths(
    root: impl AsRef<Path>,
) -> Result<ResolvedLocalPackages, ResolveError> {
    LocalPackageResolver::new(root.as_ref().to_path_buf()).resolve_with_paths()
}

/// Minimal filesystem-backed local package resolver.
#[derive(Debug, Clone)]
pub struct LocalPackageResolver {
    root: PathBuf,
}

impl LocalPackageResolver {
    /// Creates a resolver rooted at a package directory.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the configured package root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Discovers all reachable manifests, entries, and path dependencies.
    ///
    /// This phase performs filesystem reads only. It does not execute scripts or access a
    /// network service.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ResolveError`] for I/O, manifest, coordinate, or cycle failures.
    pub fn discover(&self) -> Result<BTreeMap<PackageId, PackageInput>, ResolveError> {
        self.discover_with_root().map(|(_, packages)| {
            packages
                .into_iter()
                .map(|(coordinate, loaded)| (coordinate, loaded.input))
                .collect()
        })
    }

    /// Discovers local path branches while leaving pinned remote edges unresolved.
    ///
    /// This phase performs filesystem reads only. It does not reject a GitHub dependency and
    /// never invokes a remote loader.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ResolveError`] for local I/O, manifest, coordinate, or cycle failures.
    pub fn discover_local(&self) -> Result<DiscoveredLocalPackages, ResolveError> {
        let (root, loaded) = self.discover_with_root_mode(true)?;
        let inputs = loaded
            .iter()
            .map(|(coordinate, package)| (coordinate.clone(), package.input.clone()))
            .collect();
        let paths = loaded
            .into_iter()
            .map(|(coordinate, package)| (coordinate, package.directory.join(package.input.entry)))
            .collect();
        Ok(DiscoveredLocalPackages {
            root,
            inputs,
            paths,
        })
    }

    /// Discovers and resolves the complete local graph.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ResolveError`] for discovery, resolution, or lock validation failures.
    pub fn resolve(&self) -> Result<ResolvedPackageGraph, ResolveError> {
        self.resolve_with_paths().map(|resolved| resolved.graph)
    }

    /// Discovers and resolves the complete local graph with filesystem entry paths.
    ///
    /// The graph and paths come from the same discovery pass. The returned paths are absolute
    /// entry-file paths and are not part of the graph or lockfile serialization.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ResolveError`] for discovery, resolution, or lock validation failures.
    pub fn resolve_with_paths(&self) -> Result<ResolvedLocalPackages, ResolveError> {
        let (root, loaded) = self.discover_with_root()?;
        let root_input = loaded
            .get(&root)
            .map(|package| package.input.clone())
            .ok_or_else(|| ResolveError::InvalidInput {
                context: root.to_string(),
                detail: "root package disappeared after discovery".to_string(),
            })?;
        let packages = loaded
            .iter()
            .map(|(coordinate, package)| (coordinate.clone(), package.input.clone()))
            .collect();
        let graph = resolve_manifests(root_input, &packages)?;
        let paths = loaded
            .into_iter()
            .map(|(coordinate, package)| (coordinate, package.directory.join(package.input.entry)))
            .collect();
        Ok(ResolvedLocalPackages { graph, paths })
    }

    fn discover_with_root(
        &self,
    ) -> Result<(PackageId, BTreeMap<PackageId, LoadedPackage>), ResolveError> {
        self.discover_with_root_mode(false)
    }

    fn discover_with_root_mode(
        &self,
        allow_remote: bool,
    ) -> Result<(PackageId, BTreeMap<PackageId, LoadedPackage>), ResolveError> {
        let root_directory = fs::canonicalize(&self.root).map_err(|error| ResolveError::Io {
            path: self.root.display().to_string(),
            detail: error.to_string(),
        })?;
        let mut active = Vec::new();
        let mut discovered = BTreeMap::new();
        let root_input = self.discover_at(
            &root_directory,
            ".".to_string(),
            None,
            allow_remote,
            &mut active,
            &mut discovered,
        )?;
        let root = root_input.manifest.name.clone();
        Ok((root, discovered))
    }

    fn discover_at(
        &self,
        package_directory: &Path,
        source_path: String,
        expected: Option<&PackageId>,
        allow_remote: bool,
        active: &mut Vec<ActivePackage>,
        discovered: &mut BTreeMap<PackageId, LoadedPackage>,
    ) -> Result<PackageInput, ResolveError> {
        let manifest_path = package_directory.join("aipo.toml");
        let manifest_bytes = fs::read(&manifest_path).map_err(|error| ResolveError::Io {
            path: source_path.clone(),
            detail: error.to_string(),
        })?;
        let manifest =
            Manifest::from_bytes(&manifest_bytes).map_err(|error| ResolveError::Manifest {
                path: source_path.clone(),
                error,
            })?;
        let coordinate = manifest.name.clone();
        if let Some(expected) = expected
            && expected != &coordinate
        {
            return Err(ResolveError::CoordinateMismatch {
                expected: expected.clone(),
                actual: coordinate,
            });
        }

        if let Some(position) = active
            .iter()
            .position(|active_package| active_package.coordinate == coordinate)
        {
            let mut cycle = active[position..]
                .iter()
                .map(|active_package| active_package.coordinate.clone())
                .collect::<Vec<_>>();
            cycle.push(coordinate);
            return Err(ResolveError::DependencyCycle { cycle });
        }

        if let Some(existing) = discovered.get(&coordinate) {
            if existing.directory != package_directory {
                return Err(ResolveError::DuplicateCoordinate {
                    coordinate,
                    first_source: existing.input.source.location(),
                    second_source: source_path,
                });
            }
            if existing.input.manifest != manifest {
                return Err(ResolveError::InvalidInput {
                    context: coordinate.to_string(),
                    detail: "the same package path produced different manifests".to_string(),
                });
            }
            return Ok(existing.input.clone());
        }

        let (entry, entry_bytes) = discover_entry(package_directory, &coordinate, &manifest)?;
        active.push(ActivePackage {
            coordinate: coordinate.clone(),
        });
        for dependency in &manifest.dependencies {
            let PackageSource::Path { path } = &dependency.source else {
                if allow_remote {
                    continue;
                }
                return Err(ResolveError::RemoteDependencyUnsupported {
                    dependent: coordinate.clone(),
                    dependency: dependency.name.clone(),
                    source: dependency.source.location(),
                });
            };
            let dependency_directory = package_directory.join(path);
            let dependency_directory = match fs::canonicalize(&dependency_directory) {
                Ok(path) => path,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    return Err(ResolveError::MissingDependency {
                        dependent: coordinate.clone(),
                        dependency: dependency.name.clone(),
                    });
                }
                Err(error) => {
                    return Err(ResolveError::Io {
                        path: path.clone(),
                        detail: error.to_string(),
                    });
                }
            };
            let dependency_input = self.discover_at(
                &dependency_directory,
                path.clone(),
                Some(&dependency.name),
                allow_remote,
                active,
                discovered,
            )?;
            if let Some(required_version) = &dependency.version
                && required_version != &dependency_input.manifest.version
            {
                return Err(ResolveError::VersionConflict {
                    coordinate: dependency.name.clone(),
                    expected: required_version.clone(),
                    actual: dependency_input.manifest.version.clone(),
                    dependent: coordinate.clone(),
                });
            }
        }
        active.pop();

        let input = PackageInput {
            manifest,
            manifest_bytes,
            entry,
            entry_bytes,
            source: PackageSource::Path { path: source_path },
        };
        discovered.insert(
            coordinate,
            LoadedPackage {
                directory: package_directory.to_path_buf(),
                input: input.clone(),
            },
        );
        Ok(input)
    }
}

#[derive(Debug)]
struct ActivePackage {
    coordinate: PackageId,
}

#[derive(Debug)]
struct LoadedPackage {
    directory: PathBuf,
    input: PackageInput,
}

fn discover_entry(
    package_directory: &Path,
    coordinate: &PackageId,
    manifest: &Manifest,
) -> Result<(String, Vec<u8>), ResolveError> {
    let mut candidates = Vec::new();
    let entry_path = Path::new(&manifest.entry);
    candidates.push(manifest.entry.clone());
    if entry_path.extension().is_none() {
        let entry_without_suffix = manifest.entry.trim_end_matches(".aipo");
        candidates.push(format!("{entry_without_suffix}.aipo"));
        if !entry_without_suffix.starts_with("src/") {
            candidates.push(format!("src/{entry_without_suffix}.aipo"));
        }
    }
    for candidate in &candidates {
        let path = package_directory.join(candidate);
        if path.is_file() {
            let bytes = fs::read(&path).map_err(|error| ResolveError::Io {
                path: candidate.clone(),
                detail: error.to_string(),
            })?;
            return Ok((candidate.replace('\\', "/"), bytes));
        }
    }
    Err(ResolveError::EntryNotFound {
        coordinate: coordinate.clone(),
        entry: manifest.entry.clone(),
        candidates,
    })
}

fn is_safe_entry_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && !value.starts_with('/')
        && value.as_bytes().get(1) != Some(&b':')
        && !value.split('/').any(|segment| segment.is_empty())
        && !value
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

fn register_input(
    inputs: &mut BTreeMap<PackageId, PackageInput>,
    input: PackageInput,
) -> Result<(), ResolveError> {
    input
        .manifest
        .validate()
        .map_err(|error| ResolveError::Manifest {
            path: input.source.location(),
            error,
        })?;
    input.source.validate().map_err(ResolveError::Lockfile)?;
    let coordinate = input.manifest.name.clone();
    if let Some(existing) = inputs.get(&coordinate) {
        if same_input(existing, &input) {
            return Ok(());
        }
        if existing.source != input.source {
            return Err(ResolveError::DuplicateCoordinate {
                coordinate,
                first_source: existing.source.location(),
                second_source: input.source.location(),
            });
        }
        if existing.manifest.version != input.manifest.version {
            return Err(ResolveError::VersionConflict {
                coordinate: coordinate.clone(),
                expected: existing.manifest.version.clone(),
                actual: input.manifest.version,
                dependent: coordinate,
            });
        }
        return Err(ResolveError::InvalidInput {
            context: coordinate.to_string(),
            detail: "the same source produced different package inputs".to_string(),
        });
    }
    inputs.insert(coordinate, input);
    Ok(())
}

fn same_input(left: &PackageInput, right: &PackageInput) -> bool {
    left.source == right.source
        && left.manifest == right.manifest
        && left.entry == right.entry
        && left.entry_bytes == right.entry_bytes
}

fn resolve_from_inputs(
    inputs: BTreeMap<PackageId, PackageInput>,
    root: PackageId,
) -> Result<ResolvedPackageGraph, ResolveError> {
    if !inputs.contains_key(&root) {
        return Err(ResolveError::InvalidInput {
            context: root.to_string(),
            detail: "root package is not present in the input store".to_string(),
        });
    }
    let mut states = BTreeMap::new();
    let mut stack = Vec::new();
    let mut packages = Vec::new();
    visit_package(&root, &inputs, &mut states, &mut stack, &mut packages)?;
    ResolvedPackageGraph::new(root, packages)
}

fn visit_package(
    coordinate: &PackageId,
    inputs: &BTreeMap<PackageId, PackageInput>,
    states: &mut BTreeMap<PackageId, u8>,
    stack: &mut Vec<PackageId>,
    packages: &mut Vec<ResolvedPackage>,
) -> Result<(), ResolveError> {
    match states.get(coordinate).copied() {
        Some(1) => {
            if let Some(position) = stack.iter().position(|item| item == coordinate) {
                let mut cycle = stack[position..].to_vec();
                cycle.push(coordinate.clone());
                return Err(ResolveError::DependencyCycle { cycle });
            }
            return Err(ResolveError::DependencyCycle {
                cycle: vec![coordinate.clone(), coordinate.clone()],
            });
        }
        Some(2) => return Ok(()),
        _ => {}
    }

    let input = inputs
        .get(coordinate)
        .ok_or_else(|| ResolveError::InvalidInput {
            context: coordinate.to_string(),
            detail: "package is not present in the input store".to_string(),
        })?;
    let mut manifest = input.manifest.clone();
    manifest
        .canonicalize()
        .map_err(|error| ResolveError::Manifest {
            path: input.source.location(),
            error,
        })?;

    states.insert(coordinate.clone(), 1);
    stack.push(coordinate.clone());
    for dependency in &manifest.dependencies {
        let dependency_input =
            inputs
                .get(&dependency.name)
                .ok_or_else(|| ResolveError::MissingDependency {
                    dependent: coordinate.clone(),
                    dependency: dependency.name.clone(),
                })?;
        if dependency_input.manifest.name != dependency.name {
            return Err(ResolveError::CoordinateMismatch {
                expected: dependency.name.clone(),
                actual: dependency_input.manifest.name.clone(),
            });
        }
        if let Some(required_version) = &dependency.version
            && required_version != &dependency_input.manifest.version
        {
            return Err(ResolveError::VersionConflict {
                coordinate: dependency.name.clone(),
                expected: required_version.clone(),
                actual: dependency_input.manifest.version.clone(),
                dependent: coordinate.clone(),
            });
        }
        visit_package(&dependency.name, inputs, states, stack, packages)?;
    }
    stack.pop();
    states.insert(coordinate.clone(), 2);
    packages.push(ResolvedPackage::from_input(&PackageInput {
        manifest,
        manifest_bytes: input.manifest_bytes.clone(),
        entry: input.entry.clone(),
        entry_bytes: input.entry_bytes.clone(),
        source: input.source.clone(),
    })?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Dependency;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn in_memory_resolution_is_topological_and_repeatable() {
        let root = package_input(
            "acme.root",
            "1.0.0",
            vec![
                Dependency::new(coordinate("acme.zeta"), "../zeta"),
                Dependency::new(coordinate("acme.alpha"), "../alpha"),
            ],
            Vec::new(),
            b"root",
        );
        let alpha = package_input("acme.alpha", "1.0.0", Vec::new(), Vec::new(), b"alpha");
        let zeta = package_input("acme.zeta", "1.0.0", Vec::new(), Vec::new(), b"zeta");
        let mut packages = BTreeMap::new();
        packages.insert(coordinate("acme.alpha"), alpha);
        packages.insert(coordinate("acme.zeta"), zeta);

        let first = resolve_manifests(root.clone(), &packages).expect("graph resolves");
        let second = resolve_manifests(root, &packages).expect("graph resolves");
        assert_eq!(first, second);
        assert_eq!(
            first
                .topological_order()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["acme.alpha", "acme.zeta", "acme.root"]
        );
    }

    #[test]
    fn in_memory_resolution_reports_missing_and_cyclic_dependencies() {
        let missing_root = package_input(
            "acme.root",
            "1.0.0",
            vec![Dependency::new(coordinate("acme.missing"), "../missing")],
            Vec::new(),
            b"root",
        );
        let error =
            resolve_package_inputs(missing_root, Vec::new()).expect_err("missing dependency fails");
        assert!(matches!(error, ResolveError::MissingDependency { .. }));

        let root = package_input(
            "acme.root",
            "1.0.0",
            vec![Dependency::new(coordinate("acme.cycle"), "../cycle")],
            Vec::new(),
            b"root",
        );
        let cycle = package_input(
            "acme.cycle",
            "1.0.0",
            vec![Dependency::new(coordinate("acme.root"), "../root")],
            Vec::new(),
            b"cycle",
        );
        let error = resolve_package_inputs(root, vec![cycle]).expect_err("cycle fails");
        assert!(matches!(error, ResolveError::DependencyCycle { .. }));
    }

    #[test]
    fn in_memory_resolution_detects_duplicate_coordinates_and_version_conflicts() {
        let root = package_input("acme.root", "1.0.0", Vec::new(), Vec::new(), b"root");
        let duplicate = package_input("acme.root", "1.0.0", Vec::new(), Vec::new(), b"other")
            .with_source(PackageSource::Path {
                path: "../other".to_string(),
            });
        let error = resolve_package_inputs(root.clone(), vec![duplicate])
            .expect_err("duplicate coordinate fails");
        assert!(matches!(error, ResolveError::DuplicateCoordinate { .. }));

        let dependent = package_input(
            "acme.dependent",
            "1.0.0",
            vec![Dependency::new(coordinate("acme.dep"), "../dep").with_version("2.0.0")],
            Vec::new(),
            b"dependent",
        );
        let dependency = package_input("acme.dep", "1.0.0", Vec::new(), Vec::new(), b"dep");
        let error = resolve_package_inputs(dependent, vec![dependency])
            .expect_err("version conflict fails");
        assert!(matches!(error, ResolveError::VersionConflict { .. }));

        let build_dependent = package_input(
            "acme.build_dependent",
            "1.0.0",
            vec![Dependency::new(coordinate("acme.dep"), "../dep").with_version("1.0.0+build.1")],
            Vec::new(),
            b"dependent",
        );
        let build_dependency = package_input("acme.dep", "1.0.0", Vec::new(), Vec::new(), b"dep");
        let error = resolve_package_inputs(build_dependent, vec![build_dependency])
            .expect_err("textual version conflict fails");
        assert!(matches!(error, ResolveError::VersionConflict { .. }));
    }

    #[test]
    fn capability_upper_bound_rejects_transitive_widening() {
        let root = package_input(
            "acme.root",
            "1.0.0",
            vec![Dependency::new(coordinate("acme.dep"), "../dep")],
            vec!["clock.wall".to_string()],
            b"root",
        );
        let dependency = package_input(
            "acme.dep",
            "1.0.0",
            Vec::new(),
            vec!["clock.monotonic".to_string()],
            b"dep",
        );
        let error =
            resolve_package_inputs(root, vec![dependency]).expect_err("capability widening fails");
        assert!(matches!(
            error,
            ResolveError::CapabilityLimitExceeded { .. }
        ));
    }

    #[test]
    fn filesystem_resolver_discovers_entry_and_missing_path() {
        let tree = TempTree::new("filesystem");
        let root = tree.path().join("root");
        let dependency = tree.path().join("dependency");
        write_package(&root, "acme.root", None, "let root = 1");
        write_package(&dependency, "acme.dep", None, "let dependency = 1");
        let root_manifest = root.join("aipo.toml");
        let mut root_text = std::fs::read_to_string(&root_manifest).expect("root manifest exists");
        root_text.push_str(
            "\n[dependencies]\n\"acme.dep\" = { path = \"../dependency\", version = \"1.0.0\" }\n",
        );
        std::fs::write(&root_manifest, root_text).expect("root manifest writes");

        let graph = LocalPackageResolver::new(&root)
            .resolve()
            .expect("filesystem graph resolves");
        assert_eq!(graph.topological_order().len(), 2);
        assert_eq!(
            graph
                .package(&coordinate("acme.dep"))
                .expect("dependency is present")
                .entry,
            "src/main.aipo"
        );

        let missing_root = tree.path().join("missing-root");
        write_package(
            &missing_root,
            "acme.missing_root",
            Some(("acme.absent", "../absent", None)),
            "let missing = 1",
        );
        let error = LocalPackageResolver::new(&missing_root)
            .resolve()
            .expect_err("missing filesystem dependency fails");
        assert!(matches!(error, ResolveError::MissingDependency { .. }));
    }

    #[test]
    fn filesystem_resolver_rejects_remote_dependency_without_loader() {
        let tree = TempTree::new("remote-without-loader");
        let root = tree.path().join("root");
        fs::create_dir_all(root.join("src")).expect("package directory creates");
        fs::write(
            root.join("aipo.toml"),
            "[package]\nname = \"acme.root\"\nversion = \"1.0.0\"\nentry = \"main\"\n[dependencies]\n\"acme.remote\" = { version = \"1.0.0\", type = \"github\", repository = \"acme/packages\", revision = \"0123456789abcdef0123456789abcdef01234567\" }\n",
        )
        .expect("manifest writes");
        fs::write(root.join("src/main.aipo"), "let answer = 1\n").expect("entry writes");

        let error = LocalPackageResolver::new(&root)
            .resolve()
            .expect_err("remote dependency requires an explicit loader");
        assert!(matches!(
            error,
            ResolveError::RemoteDependencyUnsupported { .. }
        ));
    }

    #[test]
    fn local_discovery_records_remote_edges_without_loading_them() {
        let tree = TempTree::new("mixed-local-discovery");
        let root = tree.path().join("root");
        let local = tree.path().join("local");
        write_package(&local, "acme.local", None, "let local = 1");
        fs::create_dir_all(root.join("src")).expect("root directory creates");
        fs::write(
            root.join("aipo.toml"),
            "[package]\nname = \"acme.root\"\nversion = \"1.0.0\"\nentry = \"main\"\n[dependencies]\n\"acme.local\" = { path = \"../local\" }\n\"acme.remote\" = { version = \"1.0.0\", type = \"github\", repository = \"acme/packages\", revision = \"0123456789abcdef0123456789abcdef01234567\" }\n",
        )
        .expect("manifest writes");
        fs::write(root.join("src/main.aipo"), "let root = 1\n").expect("entry writes");

        let discovered = LocalPackageResolver::new(&root)
            .discover_local()
            .expect("local discovery succeeds");
        assert!(discovered.inputs.contains_key(&coordinate("acme.root")));
        assert!(discovered.inputs.contains_key(&coordinate("acme.local")));
        assert!(!discovered.inputs.contains_key(&coordinate("acme.remote")));
        assert!(discovered.paths.contains_key(&coordinate("acme.local")));
    }

    #[test]
    fn filesystem_path_api_returns_canonical_entry_paths() {
        let tree = TempTree::new("paths");
        let root = tree.path().join("root");
        let dependency = tree.path().join("dependency");
        write_package(&root, "acme.root", None, "let root = 1");
        write_package(&dependency, "acme.dep", None, "let dependency = 1");
        let root_manifest = root.join("aipo.toml");
        let manifest = std::fs::read_to_string(&root_manifest).expect("root manifest exists")
            + "\n[dependencies]\n\"acme.dep\" = { path = \"../dependency\" }\n";
        std::fs::write(&root_manifest, manifest).expect("root manifest writes");

        let resolved = resolve_path_with_paths(&root).expect("path-aware resolution succeeds");
        let graph = LocalPackageResolver::new(&root)
            .resolve()
            .expect("graph-only resolution succeeds");

        assert_eq!(resolved.graph, graph);
        assert_eq!(resolved.paths.len(), 2);
        assert_eq!(
            resolved.paths.get(&coordinate("acme.root")),
            Some(&fs::canonicalize(root.join("src/main.aipo")).expect("root entry canonicalizes"))
        );
        assert_eq!(
            resolved.paths.get(&coordinate("acme.dep")),
            Some(
                &fs::canonicalize(dependency.join("src/main.aipo"))
                    .expect("dependency entry canonicalizes")
            )
        );
    }

    #[test]
    fn filesystem_resolver_detects_cycles_and_duplicate_coordinates() {
        let cycle_tree = TempTree::new("cycle");
        let root = cycle_tree.path().join("root");
        let dependency = cycle_tree.path().join("dependency");
        write_package(
            &root,
            "acme.root",
            Some(("acme.dep", "../dependency", None)),
            "let root = 1",
        );
        write_package(
            &dependency,
            "acme.dep",
            Some(("acme.root", "../root", None)),
            "let dependency = 1",
        );
        let error = LocalPackageResolver::new(&root)
            .resolve()
            .expect_err("filesystem cycle fails");
        assert!(matches!(error, ResolveError::DependencyCycle { .. }));

        let duplicate_tree = TempTree::new("duplicate");
        let duplicate_root = duplicate_tree.path().join("root");
        let first = duplicate_tree.path().join("first");
        let second = duplicate_tree.path().join("second");
        let shared_first = duplicate_tree.path().join("shared-first");
        let shared_second = duplicate_tree.path().join("shared-second");
        write_package(
            &duplicate_root,
            "acme.root",
            Some(("acme.first", "../first", None)),
            "let root = 1",
        );
        let duplicate_manifest = std::fs::read_to_string(duplicate_root.join("aipo.toml"))
            .expect("root manifest exists")
            + "\"acme.second\" = { path = \"../second\" }\n";
        std::fs::write(duplicate_root.join("aipo.toml"), duplicate_manifest)
            .expect("root manifest writes");
        write_package(
            &first,
            "acme.first",
            Some(("acme.dep", "../shared-first", None)),
            "let first = 1",
        );
        write_package(
            &second,
            "acme.second",
            Some(("acme.dep", "../shared-second", None)),
            "let second = 1",
        );
        write_package(&shared_first, "acme.dep", None, "let shared_first = 1");
        write_package(&shared_second, "acme.dep", None, "let shared_second = 1");
        let error = LocalPackageResolver::new(&duplicate_root)
            .resolve()
            .expect_err("duplicate filesystem coordinate fails");
        assert!(matches!(error, ResolveError::DuplicateCoordinate { .. }));
    }

    fn coordinate(value: &str) -> PackageId {
        PackageId::parse(value).expect("test coordinate")
    }

    fn package_input(
        name: &str,
        version: &str,
        dependencies: Vec<Dependency>,
        capabilities: Vec<String>,
        entry_bytes: &[u8],
    ) -> PackageInput {
        let mut manifest =
            Manifest::new(coordinate(name), version, "main").expect("test manifest is valid");
        manifest.dependencies = dependencies;
        manifest.capabilities = capabilities;
        manifest.validate().expect("test manifest validates");
        let mut input = PackageInput::from_manifest(manifest, entry_bytes.to_vec())
            .expect("test input is valid");
        input.source = PackageSource::Path {
            path: format!("../{name}"),
        };
        input
    }

    fn write_package(
        directory: &Path,
        name: &str,
        dependency: Option<(&str, &str, Option<&str>)>,
        entry: &str,
    ) {
        fs::create_dir_all(directory.join("src")).expect("package directory creates");
        let dependency_text = dependency
            .map(|(package, path, version)| {
                let version =
                    version.map_or_else(String::new, |value| format!(", version = \"{value}\""));
                format!("\n[dependencies]\n\"{package}\" = {{ path = \"{path}\"{version} }}\n")
            })
            .unwrap_or_default();
        let manifest =
            format!("name = \"{name}\"\nversion = \"1.0.0\"\nentry = \"main\"\n{dependency_text}");
        fs::write(directory.join("aipo.toml"), manifest).expect("manifest writes");
        fs::write(directory.join("src/main.aipo"), entry).expect("entry writes");
    }

    struct TempTree {
        path: PathBuf,
    }

    impl TempTree {
        fn new(label: &str) -> Self {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "aipo-package-test-{}-{label}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("temporary tree creates");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
