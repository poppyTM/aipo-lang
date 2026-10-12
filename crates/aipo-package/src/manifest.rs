//! Declarative `aipo.toml` models and validation.
//!
//! The canonical manifest places package identity under `[package]`; dependencies,
//! capabilities, and targets remain at the document root. Legacy flat and `[project]` forms
//! are accepted when parsing.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::capabilities::normalize_capabilities;
use crate::coordinate::PackageId;
use crate::error::{ManifestError, ValidationProblem};
use crate::lock::PackageSource;

/// One local or pinned remote dependency declared by a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// Canonical coordinate of the dependency package.
    #[serde(alias = "coordinate", alias = "package")]
    pub name: PackageId,
    /// Exact SemVer version required by the dependency, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Local path or exact pinned GitHub source.
    #[serde(flatten)]
    pub source: PackageSource,
}

impl Dependency {
    /// Returns the canonical dependency coordinate.
    #[must_use]
    pub fn coordinate(&self) -> &PackageId {
        &self.name
    }

    /// Creates a path dependency without a version constraint.
    #[must_use]
    pub fn new(name: PackageId, path: impl Into<String>) -> Self {
        Self {
            name,
            version: None,
            source: PackageSource::Path { path: path.into() },
        }
    }

    /// Creates a pinned GitHub dependency with an exact package version.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Invalid`] when the source or version is malformed.
    pub fn github(
        name: PackageId,
        version: impl Into<String>,
        repository: impl Into<String>,
        revision: impl Into<String>,
        subpath: impl Into<String>,
    ) -> Result<Self, ManifestError> {
        let source = PackageSource::github_at(repository, revision, subpath).map_err(|error| {
            ManifestError::Invalid {
                problems: vec![ValidationProblem::new(
                    "dependency.source",
                    error.to_string(),
                )],
            }
        })?;
        let dependency = Self {
            name,
            version: Some(version.into()),
            source,
        };
        dependency.validate()?;
        Ok(dependency)
    }

    /// Adds an exact SemVer version requirement; range constraints are not supported.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Validates this dependency in isolation.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Invalid`] when the source or SemVer version is malformed.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let mut problems = Vec::new();
        self.validate_at("dependency", &mut problems);
        finish_validation(problems)
    }

    fn validate_at(&self, prefix: &str, problems: &mut Vec<ValidationProblem>) {
        validate_version(
            self.version.as_deref().unwrap_or(""),
            &format!("{prefix}.version"),
            problems,
            self.version.is_some(),
        );
        if matches!(&self.source, PackageSource::GitHub { .. }) && self.version.is_none() {
            problems.push(ValidationProblem::new(
                format!("{prefix}.version"),
                "GitHub dependencies require an exact SemVer version",
            ));
        }
        if let Err(error) = self.source.validate() {
            problems.push(ValidationProblem::new(
                format!("{prefix}.source"),
                error.to_string(),
            ));
        }
    }
}

/// The validated contents of an `aipo.toml` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ManifestWire")]
pub struct Manifest {
    /// Canonical package coordinate.
    pub name: PackageId,
    /// Exact SemVer package version.
    pub version: String,
    /// Logical entry module or relative entry file.
    pub entry: String,
    /// Local path or exact pinned GitHub dependencies.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// Declared compilation targets.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Capabilities declared as the package's upper bound.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl Manifest {
    /// Returns the canonical package coordinate.
    #[must_use]
    pub fn coordinate(&self) -> &PackageId {
        &self.name
    }

    /// Creates a minimal manifest after validating its required fields.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Invalid`] for a malformed SemVer version or entry.
    pub fn new(
        name: PackageId,
        version: impl Into<String>,
        entry: impl Into<String>,
    ) -> Result<Self, ManifestError> {
        let manifest = Self {
            name,
            version: version.into(),
            entry: entry.into(),
            dependencies: Vec::new(),
            targets: Vec::new(),
            capabilities: Vec::new(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Parses a manifest from UTF-8 TOML bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::InvalidUtf8`], [`ManifestError::Parse`], or
    /// [`ManifestError::Invalid`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ManifestError> {
        let text = std::str::from_utf8(bytes).map_err(|error| ManifestError::InvalidUtf8 {
            detail: error.to_string(),
        })?;
        Self::parse_toml(text)
    }

    /// Parses a manifest from TOML text.
    ///
    /// The canonical form places `name`, `version`, and `entry` under `[package]`, while
    /// dependencies, capabilities, and targets remain at the document root. The legacy flat
    /// model and the `[project]` form are accepted for compatibility. Parsed manifests are
    /// canonicalized before they are returned.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Parse`] or [`ManifestError::Invalid`].
    pub fn parse_toml(text: &str) -> Result<Self, ManifestError> {
        let wire: ManifestWire = toml::from_str(text).map_err(|error| ManifestError::Parse {
            detail: error.to_string(),
        })?;
        let mut manifest = Self::try_from(wire)?;
        manifest.canonicalize()?;
        Ok(manifest)
    }

    /// Parses TOML text using the conventional method name.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Manifest::parse_toml`].
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Result<Self, ManifestError> {
        Self::parse_toml(text)
    }

    /// Parses TOML text using the explicit manifest format name.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Manifest::parse_toml`].
    pub fn from_toml(text: &str) -> Result<Self, ManifestError> {
        Self::parse_toml(text)
    }

    /// Canonicalizes vectors and capabilities in place.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Invalid`] before changing invalid input.
    pub fn canonicalize(&mut self) -> Result<(), ManifestError> {
        self.validate()?;
        self.dependencies.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.source.location().cmp(&right.source.location()))
                .then_with(|| left.version.cmp(&right.version))
        });
        self.targets.sort();
        self.targets.dedup();
        self.capabilities =
            normalize_capabilities(&self.capabilities).map_err(|error| ManifestError::Invalid {
                problems: vec![ValidationProblem::new("capabilities", error.to_string())],
            })?;
        Ok(())
    }

    /// Returns a canonical copy of this manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Invalid`] when the manifest is invalid.
    pub fn canonicalized(mut self) -> Result<Self, ManifestError> {
        self.canonicalize()?;
        Ok(self)
    }

    /// Validates all manifest invariants without changing ordering.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Invalid`] with all detected problems.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let mut problems = Vec::new();
        validate_version(&self.version, "version", &mut problems, true);
        validate_path(&self.entry, "entry", true, &mut problems);

        let mut dependency_names = std::collections::BTreeSet::new();
        for (index, dependency) in self.dependencies.iter().enumerate() {
            let prefix = format!("dependencies[{index}]");
            if !dependency_names.insert(dependency.name.clone()) {
                problems.push(ValidationProblem::new(
                    format!("{prefix}.name"),
                    "dependency is declared more than once",
                ));
            }
            dependency.validate_at(&prefix, &mut problems);
        }

        for (index, target) in self.targets.iter().enumerate() {
            validate_target(target, &format!("targets[{index}]"), &mut problems);
        }

        for (index, capability) in self.capabilities.iter().enumerate() {
            if let Err(error) = aipo_host::Capability::parse(capability) {
                problems.push(ValidationProblem::new(
                    format!("capabilities[{index}]"),
                    error.to_string(),
                ));
            }
        }

        finish_validation(problems)
    }

    /// Serializes a canonical copy as TOML with package metadata under `[package]` and
    /// dependencies, capabilities, and targets at the document root.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Serialize`] when TOML encoding fails.
    pub fn to_toml(&self) -> Result<String, ManifestError> {
        let canonical = self.clone().canonicalized()?;
        let wire = ManifestTomlWire {
            dependencies: &canonical.dependencies,
            targets: &canonical.targets,
            capabilities: &canonical.capabilities,
            package: PackageTomlWire {
                name: &canonical.name,
                version: &canonical.version,
                entry: &canonical.entry,
            },
        };
        toml::to_string_pretty(&wire).map_err(|error| ManifestError::Serialize {
            detail: error.to_string(),
        })
    }

    /// Serializes a canonical copy as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Serialize`] when JSON encoding fails.
    pub fn to_json(&self) -> Result<String, ManifestError> {
        let canonical = self.clone().canonicalized()?;
        serde_json::to_string_pretty(&canonical).map_err(|error| ManifestError::Serialize {
            detail: error.to_string(),
        })
    }
}

impl FromStr for Manifest {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse_toml(text)
    }
}

impl TryFrom<ManifestWire> for Manifest {
    type Error = ManifestError;

    fn try_from(wire: ManifestWire) -> Result<Self, Self::Error> {
        let project = wire.project.as_ref();
        let name = project
            .and_then(|value| value.name.clone())
            .or(wire.name)
            .ok_or_else(|| ManifestError::Parse {
                detail: "missing package name".to_string(),
            })?;
        let version = project
            .and_then(|value| value.version.clone())
            .or(wire.version)
            .ok_or_else(|| ManifestError::Parse {
                detail: "missing package version".to_string(),
            })?;
        let entry = project
            .and_then(|value| value.entry.clone())
            .or(wire.entry)
            .ok_or_else(|| ManifestError::Parse {
                detail: "missing package entry".to_string(),
            })?;
        let dependencies = match project
            .and_then(|value| value.dependencies.clone())
            .or(wire.dependencies)
        {
            Some(value) => value.try_into()?,
            None => Vec::new(),
        };
        let targets = project
            .and_then(|value| value.targets.clone())
            .or(wire.targets)
            .unwrap_or_default();
        let capabilities = project
            .and_then(|value| value.capabilities.clone())
            .or(wire.capabilities)
            .unwrap_or_default();

        Ok(Self {
            name,
            version,
            entry,
            dependencies,
            targets,
            capabilities,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestWire {
    #[serde(default)]
    name: Option<PackageId>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default, alias = "package")]
    project: Option<ProjectWire>,
    #[serde(default)]
    dependencies: Option<DependenciesWire>,
    #[serde(default)]
    targets: Option<Vec<String>>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ManifestTomlWire<'a> {
    dependencies: &'a [Dependency],
    targets: &'a [String],
    capabilities: &'a [String],
    package: PackageTomlWire<'a>,
}

#[derive(Serialize)]
struct PackageTomlWire<'a> {
    name: &'a PackageId,
    version: &'a str,
    entry: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectWire {
    #[serde(default)]
    name: Option<PackageId>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    dependencies: Option<DependenciesWire>,
    #[serde(default)]
    targets: Option<Vec<String>>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum DependenciesWire {
    Map(BTreeMap<String, DependencyWire>),
    Array(Vec<DependencyArrayWire>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum DependencyWire {
    Version(String),
    Table(DependencyTableWire),
}

#[derive(Debug, Clone, Deserialize)]
struct DependencyTableWire {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    package: Option<PackageId>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    subpath: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DependencyArrayWire {
    #[serde(default)]
    name: Option<PackageId>,
    #[serde(default)]
    coordinate: Option<PackageId>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    subpath: Option<String>,
}

impl TryFrom<DependenciesWire> for Vec<Dependency> {
    type Error = ManifestError;

    fn try_from(value: DependenciesWire) -> Result<Self, Self::Error> {
        match value {
            DependenciesWire::Map(entries) => entries
                .into_iter()
                .map(|(key, entry)| dependency_from_map_entry(&key, entry))
                .collect(),
            DependenciesWire::Array(entries) => {
                entries.into_iter().map(Dependency::try_from).collect()
            }
        }
    }
}

impl TryFrom<DependencyArrayWire> for Dependency {
    type Error = ManifestError;

    fn try_from(value: DependencyArrayWire) -> Result<Self, Self::Error> {
        let name = value
            .coordinate
            .or(value.name)
            .ok_or_else(|| ManifestError::Parse {
                detail: "dependency array entry is missing a coordinate".to_string(),
            })?;
        let source = dependency_source_from_wire(
            &name,
            value.kind.as_deref(),
            value.path,
            value.repository,
            value.revision,
            value.subpath,
        )?;
        Ok(Self {
            name,
            version: value.version,
            source,
        })
    }
}

fn dependency_from_map_entry(
    key: &str,
    entry: DependencyWire,
) -> Result<Dependency, ManifestError> {
    let fallback_name = PackageId::from_str(key).map_err(|error| ManifestError::Parse {
        detail: error.to_string(),
    })?;
    match entry {
        DependencyWire::Version(version) => Err(ManifestError::Parse {
            detail: format!(
                "dependency '{fallback_name}' must declare a path or a pinned GitHub source; version-only value '{version}' is not supported"
            ),
        }),
        DependencyWire::Table(table) => {
            let name = table.package.unwrap_or(fallback_name);
            let source = dependency_source_from_wire(
                &name,
                table.kind.as_deref(),
                table.path,
                table.repository,
                table.revision,
                table.subpath,
            )?;
            Ok(Dependency {
                name,
                version: table.version,
                source,
            })
        }
    }
}

fn dependency_source_from_wire(
    coordinate: &PackageId,
    kind: Option<&str>,
    path: Option<String>,
    repository: Option<String>,
    revision: Option<String>,
    subpath: Option<String>,
) -> Result<PackageSource, ManifestError> {
    let has_github_fields = repository.is_some() || revision.is_some() || subpath.is_some();
    match kind {
        Some("path") => {
            if has_github_fields {
                return Err(dependency_parse_error(
                    coordinate,
                    "path source cannot declare repository, revision or subpath",
                ));
            }
            let path = path
                .ok_or_else(|| dependency_parse_error(coordinate, "path source is missing path"))?;
            PackageSource::path(path).map_err(|error| dependency_source_error(coordinate, error))
        }
        Some("github") => {
            if path.is_some() {
                return Err(dependency_parse_error(
                    coordinate,
                    "GitHub source cannot declare path",
                ));
            }
            let repository = repository.ok_or_else(|| {
                dependency_parse_error(coordinate, "GitHub source is missing repository")
            })?;
            let revision = revision.ok_or_else(|| {
                dependency_parse_error(coordinate, "GitHub source is missing revision")
            })?;
            PackageSource::github_at(
                repository,
                revision,
                subpath.unwrap_or_else(|| ".".to_string()),
            )
            .map_err(|error| dependency_source_error(coordinate, error))
        }
        Some(other) => Err(dependency_parse_error(
            coordinate,
            format!("unsupported dependency source type '{other}'"),
        )),
        None => {
            if has_github_fields {
                return Err(dependency_parse_error(
                    coordinate,
                    "GitHub source fields require type = \"github\"",
                ));
            }
            let path = path.ok_or_else(|| {
                dependency_parse_error(coordinate, "dependency is missing path or source type")
            })?;
            PackageSource::path(path).map_err(|error| dependency_source_error(coordinate, error))
        }
    }
}

fn dependency_parse_error(coordinate: &PackageId, detail: impl Into<String>) -> ManifestError {
    ManifestError::Parse {
        detail: format!("dependency '{coordinate}': {}", detail.into()),
    }
}

fn dependency_source_error(coordinate: &PackageId, error: impl std::fmt::Display) -> ManifestError {
    dependency_parse_error(coordinate, format!("invalid source: {error}"))
}

fn validate_version(
    value: &str,
    path: &str,
    problems: &mut Vec<ValidationProblem>,
    required: bool,
) {
    if value.is_empty() {
        if required {
            problems.push(ValidationProblem::new(path, "version is empty"));
        }
        return;
    }
    if !is_valid_semver(value) {
        problems.push(ValidationProblem::new(
            path,
            "version must be an exact SemVer version: major.minor.patch with optional prerelease and build metadata",
        ));
    }
}

pub(crate) fn is_valid_semver(value: &str) -> bool {
    let (version_without_build, build) = match value.split_once('+') {
        Some((version, build)) => (version, Some(build)),
        None => (value, None),
    };
    let (core, prerelease) = match version_without_build.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (version_without_build, None),
    };

    let core_parts: Vec<&str> = core.split('.').collect();
    if core_parts.len() != 3 || !core_parts.iter().all(|part| is_numeric_identifier(part)) {
        return false;
    }
    if let Some(prerelease) = prerelease
        && !is_valid_prerelease(prerelease)
    {
        return false;
    }
    if let Some(build) = build
        && !is_valid_build(build)
    {
        return false;
    }
    true
}

fn is_numeric_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
}

fn is_valid_prerelease(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            let is_numeric = identifier.bytes().all(|byte| byte.is_ascii_digit());
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (!is_numeric || is_numeric_identifier(identifier))
        })
}

fn is_valid_build(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn validate_path(
    value: &str,
    path: &str,
    disallow_parent: bool,
    problems: &mut Vec<ValidationProblem>,
) {
    if value.is_empty() {
        problems.push(ValidationProblem::new(path, "path is empty"));
        return;
    }
    if value.contains('\\') {
        problems.push(ValidationProblem::new(
            path,
            "backslash separators are not portable",
        ));
    }
    if value.starts_with('/') || (value.as_bytes().get(1) == Some(&b':')) {
        problems.push(ValidationProblem::new(
            path,
            "absolute paths are not allowed",
        ));
    }
    if value.split('/').any(|segment| segment.is_empty()) {
        problems.push(ValidationProblem::new(
            path,
            "path contains an empty segment",
        ));
        return;
    }
    if disallow_parent
        && value
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        problems.push(ValidationProblem::new(
            path,
            "entry paths cannot contain '.' or '..' segments",
        ));
    }
}

fn validate_target(value: &str, path: &str, problems: &mut Vec<ValidationProblem>) {
    if value.is_empty() {
        problems.push(ValidationProblem::new(path, "target is empty"));
    } else if value.trim() != value
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        problems.push(ValidationProblem::new(
            path,
            "target must not contain surrounding whitespace or control characters",
        ));
    }
}

fn finish_validation(problems: Vec<ValidationProblem>) -> Result<(), ManifestError> {
    if problems.is_empty() {
        Ok(())
    } else {
        Err(ManifestError::Invalid { problems })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_flat_and_project_forms() {
        let flat = Manifest::from_str(
            r#"
name = "acme.root"
version = "1.2.3"
entry = "main"
targets = ["js", "native"]
capabilities = ["clock.wall", "clock"]

[[dependencies]]
name = "acme.dep"
path = "../dep"
version = "1.2.3"
"#,
        )
        .expect("flat manifest is valid");
        assert_eq!(flat.name.as_str(), "acme.root");
        assert_eq!(flat.dependencies[0].name.as_str(), "acme.dep");
        assert_eq!(flat.capabilities, vec!["clock"]);

        let project = Manifest::from_str(
            r#"
[project]
name = "acme.project"
version = "0.1.0"
entry = "main"
"#,
        )
        .expect("project manifest is valid");
        assert_eq!(project.name.as_str(), "acme.project");
        assert!(project.dependencies.is_empty());
    }

    #[test]
    fn manifest_parses_and_round_trips_pinned_github_dependency() {
        let manifest = Manifest::from_str(
            r#"
[package]
name = "acme.app"
version = "1.0.0"
entry = "src/main.aipo"

[dependencies]
"acme.http" = { version = "1.0.0", type = "github", repository = "acme/packages", revision = "0123456789abcdef0123456789abcdef01234567", subpath = "packages/http" }
"#,
        )
        .expect("remote dependency manifest is valid");
        assert_eq!(
            manifest.dependencies[0].source,
            PackageSource::github_at(
                "acme/packages",
                "0123456789abcdef0123456789abcdef01234567",
                "packages/http",
            )
            .expect("valid source")
        );
        let encoded = manifest.to_toml().expect("remote manifest serializes");
        assert!(encoded.contains("type = \"github\""));
        let decoded = Manifest::from_str(&encoded).expect("remote manifest round-trips");
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn manifest_rejects_ambiguous_or_unversioned_github_dependencies() {
        let missing_version = Manifest::from_str(
            "[package]\nname = \"acme.app\"\nversion = \"1.0.0\"\nentry = \"main\"\n[dependencies]\n\"acme.http\" = { type = \"github\", repository = \"acme/packages\", revision = \"0123456789abcdef0123456789abcdef01234567\" }\n",
        )
        .expect_err("remote dependency requires a version");
        assert!(matches!(missing_version, ManifestError::Invalid { .. }));

        let mixed_source = Manifest::from_str(
            "[package]\nname = \"acme.app\"\nversion = \"1.0.0\"\nentry = \"main\"\n[dependencies]\n\"acme.http\" = { version = \"1.0.0\", type = \"github\", path = \"../http\", repository = \"acme/packages\", revision = \"0123456789abcdef0123456789abcdef01234567\" }\n",
        )
        .expect_err("remote dependency cannot also declare a path");
        assert!(matches!(mixed_source, ManifestError::Parse { .. }));
    }

    #[test]
    fn manifest_canonicalizes_dependencies_targets_and_capabilities() {
        let mut manifest =
            Manifest::new(coordinate("acme.root"), "1.0.0", "main").expect("manifest is valid");
        manifest.dependencies = vec![
            Dependency::new(coordinate("acme.zeta"), "../zeta"),
            Dependency::new(coordinate("acme.alpha"), "../alpha"),
        ];
        manifest.targets = vec!["native".to_string(), "js".to_string(), "js".to_string()];
        manifest.capabilities = vec![
            "clock.wall".to_string(),
            "clock".to_string(),
            "filesystem.read".to_string(),
        ];

        manifest.canonicalize().expect("manifest canonicalizes");
        assert_eq!(
            manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.name.as_str())
                .collect::<Vec<_>>(),
            vec!["acme.alpha", "acme.zeta"]
        );
        assert_eq!(manifest.targets, vec!["js", "native"]);
        assert_eq!(manifest.capabilities, vec!["clock", "filesystem.read"]);
    }

    #[test]
    fn manifest_toml_round_trip_is_canonical() {
        let mut manifest = Manifest::new(coordinate("acme.root"), "1.0.0", "src/main.aipo")
            .expect("manifest is valid");
        manifest.dependencies = vec![Dependency::new(coordinate("acme.dep"), "../dep")];
        manifest.capabilities = vec!["clock.wall".to_string()];
        let encoded = manifest.to_toml().expect("manifest serializes");
        assert!(encoded.contains("[package]"));

        let document: toml::Value = toml::from_str(&encoded).expect("canonical TOML parses");
        let package = document
            .get("package")
            .and_then(toml::Value::as_table)
            .expect("package table is present");
        assert_eq!(
            package.get("name").and_then(toml::Value::as_str),
            Some("acme.root")
        );
        assert_eq!(
            package.get("version").and_then(toml::Value::as_str),
            Some("1.0.0")
        );
        assert_eq!(
            package.get("entry").and_then(toml::Value::as_str),
            Some("src/main.aipo")
        );
        assert!(document.get("dependencies").is_some());
        assert!(document.get("targets").is_some());
        assert!(document.get("capabilities").is_some());
        assert!(package.get("dependencies").is_none());
        assert!(package.get("targets").is_none());
        assert!(package.get("capabilities").is_none());

        let decoded = Manifest::from_str(&encoded).expect("manifest parses");
        assert_eq!(
            decoded,
            manifest.canonicalized().expect("manifest canonicalizes")
        );
    }

    #[test]
    fn manifest_rejects_invalid_paths_versions_and_capabilities() {
        let mut manifest =
            Manifest::new(coordinate("acme.root"), "1.0.0", "main").expect("manifest is valid");
        manifest.entry = "/tmp/main.aipo".to_string();
        manifest.dependencies = vec![Dependency::new(coordinate("acme.dep"), "/dep")];
        manifest.capabilities = vec!["Clock".to_string()];
        let error = manifest.validate().expect_err("invalid manifest");
        assert!(matches!(error, ManifestError::Invalid { problems } if problems.len() >= 3));
    }

    #[test]
    fn manifest_accepts_semver_prerelease_and_build_metadata() {
        for version in [
            "0.0.0",
            "1.2.3-alpha.1",
            "1.2.3+build.001",
            "1.2.3-rc.1+build.5",
        ] {
            assert!(
                Manifest::new(coordinate("acme.root"), version, "main").is_ok(),
                "version should be valid: {version}"
            );
        }
    }

    #[test]
    fn manifest_rejects_non_exact_semver_versions() {
        for version in [
            "abc", "1.0", "1.0.0.0", "01.0.0", "1.0.0-01", "1.0.0-", "1.0.0+", " 1.0.0", "1.0.0 ",
            "1.0.0\n", "^1.0.0", "~1.0.0", ">=1.0.0",
        ] {
            assert!(
                Manifest::new(coordinate("acme.root"), version, "main").is_err(),
                "version should be invalid: {version}"
            );
        }

        let dependency = Dependency::new(coordinate("acme.dep"), "../dep").with_version("1.0");
        assert!(dependency.validate().is_err());
    }

    #[test]
    fn manifest_rejects_duplicate_dependency_coordinates() {
        let mut manifest =
            Manifest::new(coordinate("acme.root"), "1.0.0", "main").expect("manifest is valid");
        manifest.dependencies = vec![
            Dependency::new(coordinate("acme.dep"), "../one"),
            Dependency::new(coordinate("acme.dep"), "../two"),
        ];
        assert!(manifest.validate().is_err());
    }

    fn coordinate(value: &str) -> PackageId {
        PackageId::parse(value).expect("test coordinate")
    }
}
