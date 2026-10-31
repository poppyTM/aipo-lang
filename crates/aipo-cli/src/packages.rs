//! Package resolution, the fetch/lock pipeline and the package cache.
//!
//! These commands are the only place the CLI touches the network. Resolution is
//! pure: a package graph is computed from the lockfile and the manifests, then a
//! fetch materialises it. Nothing here writes a file it did not create, and no
//! output path is trusted before `safe_output_path` has confined it.

use super::NEXT_LOCK_TEMP;
use super::{CliError, Command, PackageOperation, report_cli_error};
use super::{EXIT_LANGUAGE_FAILURE, EXIT_SUCCESS, emit_diagnostics};
use aipo_diagnostics::{Diagnostic, DiagnosticCode, MessageFormat};
use aipo_package::*;
use aipo_source::{Source, SourceId};
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

pub(crate) fn parse_cache_verify(args: &[String]) -> Result<Command, String> {
    if args.len() != 3 {
        return Err("'package cache verify' requires exactly one cache directory".to_string());
    }
    if args[0] != "cache" || args[1] != "verify" {
        return Err("expected 'package cache verify'".to_string());
    }
    if args[2].is_empty() || args[2].starts_with('-') {
        return Err("'package cache verify' requires a non-empty directory".to_string());
    }
    Ok(Command::PackageCacheVerify {
        cache_dir: PathBuf::from(&args[2]),
    })
}

pub(crate) fn parse_cache_prune(args: &[String]) -> Result<Command, String> {
    if args.len() < 4 || args[0] != "cache" || args[1] != "prune" {
        return Err(
            "'package cache prune' requires <cache-dir> --lock <lockfile> [--apply]".to_string(),
        );
    }
    if args[2].is_empty() || args[2].starts_with('-') {
        return Err("'package cache prune' requires a non-empty cache directory".to_string());
    }
    let mut lock_path = None;
    let mut apply = false;
    let mut index = 3;
    while index < args.len() {
        match args[index].as_str() {
            "--lock" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--lock' requires a lockfile".to_string())?;
                if value.is_empty() || value.starts_with('-') {
                    return Err("'--lock' requires a lockfile path".to_string());
                }
                if lock_path.is_some() {
                    return Err("'--lock' was provided more than once".to_string());
                }
                lock_path = Some(PathBuf::from(value));
            }
            value if value.starts_with("--lock=") => {
                let value = value.trim_start_matches("--lock=");
                if value.is_empty() {
                    return Err("'--lock' requires a lockfile path".to_string());
                }
                if lock_path.is_some() {
                    return Err("'--lock' was provided more than once".to_string());
                }
                lock_path = Some(PathBuf::from(value));
            }
            "--apply" => {
                if apply {
                    return Err("'--apply' was provided more than once".to_string());
                }
                apply = true;
            }
            value if value.starts_with('-') => {
                return Err(format!("unrecognized cache prune flag '{value}'"));
            }
            value => return Err(format!("unexpected cache prune argument '{value}'")),
        }
        index += 1;
    }
    let lock_path = lock_path.ok_or_else(|| "'--lock' is required".to_string())?;
    Ok(Command::PackageCachePrune {
        cache_dir: PathBuf::from(&args[2]),
        lock_path,
        apply,
    })
}

#[cfg(feature = "github-http")]
pub(crate) fn parse_fetch_github(args: &[String]) -> Result<Command, String> {
    if args.len() < 5 {
        return Err(
            "'package fetch-github' requires repository, revision, --cache and --out".to_string(),
        );
    }
    if args[0] != "fetch-github" {
        return Err("expected 'fetch-github' package operation".to_string());
    }
    let repository = args[1].clone();
    let revision = args[2].clone();
    if repository.starts_with('-') || revision.starts_with('-') {
        return Err("repository and revision cannot start with '-'".to_string());
    }
    let mut subpath = ".".to_string();
    let mut index = 3;
    if args.get(index).is_some_and(|value| !value.starts_with('-')) {
        subpath = args[index].clone();
        index += 1;
    }
    let mut cache_dir = None;
    let mut out_dir = None;
    let mut github_token_env = None;
    while index < args.len() {
        match args[index].as_str() {
            "--cache" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--cache' requires a directory".to_string())?;
                if cache_dir.is_some() {
                    return Err("'--cache' was provided more than once".to_string());
                }
                cache_dir = Some(PathBuf::from(value));
            }
            "--out" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--out' requires a directory".to_string())?;
                if out_dir.is_some() {
                    return Err("'--out' was provided more than once".to_string());
                }
                out_dir = Some(PathBuf::from(value));
            }
            "--github-token-env" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--github-token-env' requires a variable name".to_string())?;
                if !is_valid_github_token_env_name(value) {
                    return Err("'--github-token-env' requires a valid variable name".to_string());
                }
                if github_token_env.is_some() {
                    return Err("'--github-token-env' was provided more than once".to_string());
                }
                github_token_env = Some(value.clone());
            }
            value if value.starts_with("--github-token-env=") => {
                let value = value.trim_start_matches("--github-token-env=");
                if !is_valid_github_token_env_name(value) {
                    return Err("'--github-token-env' requires a valid variable name".to_string());
                }
                if github_token_env.is_some() {
                    return Err("'--github-token-env' was provided more than once".to_string());
                }
                github_token_env = Some(value.to_string());
            }
            value if value.starts_with('-') => {
                return Err(format!("unrecognized fetch-github flag '{value}'"));
            }
            value => return Err(format!("unexpected fetch-github argument '{value}'")),
        }
        index += 1;
    }
    Ok(Command::PackageFetchGitHub {
        repository,
        revision,
        subpath,
        cache_dir: cache_dir.ok_or_else(|| "'--cache' is required".to_string())?,
        out_dir: out_dir.ok_or_else(|| "'--out' is required".to_string())?,
        github_token_env,
    })
}

pub(crate) fn is_valid_github_token_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(crate) fn package_command(
    operation: PackageOperation,
    package_dir: &Path,
    fetch_github: bool,
    cache_dir: Option<&Path>,
    github_token_env: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if fetch_github {
        #[cfg(feature = "github-http")]
        {
            let cache_dir = cache_dir
                .ok_or_else(|| CliError::Usage("'--fetch-github' requires '--cache'".to_string()));
            let cache_dir = match cache_dir {
                Ok(path) => path,
                Err(error) => return report_cli_error(error, MessageFormat::Human, out, err),
            };
            return package_lock_with_github(package_dir, cache_dir, github_token_env, out, err);
        }
        #[cfg(not(feature = "github-http"))]
        {
            return report_cli_error(
                CliError::Usage("'--fetch-github' requires the 'github-http' feature".to_string()),
                MessageFormat::Human,
                out,
                err,
            );
        }
    }

    #[cfg(not(feature = "github-http"))]
    let _ = (cache_dir, github_token_env);

    let resolved = match resolve_local_package(package_dir, package_dir) {
        Ok(resolved) => resolved,
        Err(error) => return report_cli_error(error, MessageFormat::Human, out, err),
    };

    match operation {
        PackageOperation::Lock => {
            let lockfile = match generated_lockfile(&resolved.graph) {
                Ok(lockfile) => lockfile,
                Err(error) => return report_cli_error(error, MessageFormat::Human, out, err),
            };
            let contents = match lockfile.to_toml() {
                Ok(contents) => contents,
                Err(error) => {
                    return report_cli_error(
                        CliError::diagnostic(
                            DiagnosticCode::AIPO_PKG_LOCK_STALE,
                            format!("package lockfile could not be serialized: {error}"),
                        ),
                        MessageFormat::Human,
                        out,
                        err,
                    );
                }
            };
            if let Err(error) = write_lockfile(package_dir, &contents) {
                return report_cli_error(error, MessageFormat::Human, out, err);
            }
            let _ = writeln!(
                out,
                "locked {} at {}",
                resolved.graph.root,
                package_dir.join(LOCK_FILE_NAME).display()
            );
            EXIT_SUCCESS
        }
        PackageOperation::Audit => {
            let lock_path = package_dir.join(LOCK_FILE_NAME);
            if let Err(error) = validate_existing_lock(&lock_path, &resolved.graph, true) {
                return report_cli_error(error, MessageFormat::Human, out, err);
            }
            let _ = writeln!(out, "audit passed: {}", resolved.graph.root);
            EXIT_SUCCESS
        }
    }
}

pub(crate) fn package_cache_prune(
    cache_dir: &Path,
    lock_path: &Path,
    apply: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let lock_bytes = match std::fs::read(lock_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return report_cli_error(
                CliError::diagnostic(
                    DiagnosticCode::AIPO_PKG_LOCK_STALE,
                    format!(
                        "lockfile '{}' is required for cache pruning",
                        lock_path.display()
                    ),
                ),
                MessageFormat::Human,
                out,
                err,
            );
        }
        Err(error) => {
            return report_cli_error(
                CliError::Usage(format!("{}: {error}", lock_path.display())),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    let lockfile = match Lockfile::from_bytes(&lock_bytes) {
        Ok(lockfile) => lockfile,
        Err(error) => {
            return report_cli_error(
                CliError::diagnostic(
                    DiagnosticCode::AIPO_PKG_LOCK_STALE,
                    format!("lockfile '{}' is invalid: {error}", lock_path.display()),
                ),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    let referenced = lockfile
        .packages
        .iter()
        .filter_map(|package| package.source.github_source())
        .map(|source| source.label())
        .collect::<BTreeSet<_>>();
    let cache = GitHubCache::new(cache_dir);
    let verification = match cache.verify() {
        Ok(verification) => verification,
        Err(error) => {
            return report_cli_error(
                CliError::diagnostic(DiagnosticCode::AIPO_PKG_FETCH, error.to_string()),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    if !verification.errors.is_empty() {
        let source = Source::new(SourceId::next(), "<package-cache>", "");
        let diagnostics = verification
            .errors
            .iter()
            .map(|error| Diagnostic::error(DiagnosticCode::AIPO_PKG_FETCH, error.to_string()))
            .collect::<Vec<_>>();
        emit_diagnostics(MessageFormat::Human, &source, &diagnostics, out, err);
        return EXIT_LANGUAGE_FAILURE;
    }
    let candidates = verification
        .sources
        .into_iter()
        .filter(|source| !referenced.contains(&source.label()))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        let _ = writeln!(out, "cache prune: 0 candidates");
        return EXIT_SUCCESS;
    }
    let mut removed = 0usize;
    for source in &candidates {
        if !apply {
            let _ = writeln!(out, "would remove {}", source.label());
            continue;
        }
        let package_source = PackageSource::GitHub {
            repository: source.repository.clone(),
            revision: source.revision.clone(),
            subpath: source.subpath.clone(),
        };
        match cache.remove_verified(&package_source) {
            Ok(true) => removed += 1,
            Ok(false) => {}
            Err(error) => {
                return report_cli_error(
                    CliError::diagnostic(DiagnosticCode::AIPO_PKG_FETCH, error.to_string()),
                    MessageFormat::Human,
                    out,
                    err,
                );
            }
        }
    }
    if apply {
        let _ = writeln!(out, "cache prune: removed {removed} candidates");
    } else {
        let _ = writeln!(
            out,
            "cache prune: {} candidates (dry-run)",
            candidates.len()
        );
    }
    EXIT_SUCCESS
}

#[cfg(feature = "github-http")]
fn github_token_from_env(name: Option<&str>) -> Result<Option<GitHubToken>, CliError> {
    let Some(name) = name else {
        return Ok(None);
    };
    let value = std::env::var(name).map_err(|error| match error {
        std::env::VarError::NotPresent => CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_FETCH,
            format!("GitHub token environment variable '{name}' is not set"),
        ),
        std::env::VarError::NotUnicode(_) => CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_FETCH,
            format!("GitHub token environment variable '{name}' is not valid Unicode"),
        ),
    })?;
    GitHubToken::new(&value).map(Some).map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_FETCH,
            format!("GitHub token from environment variable '{name}' is invalid: {error}"),
        )
    })
}

pub(crate) fn package_cache_verify(
    cache_dir: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let cache = GitHubCache::new(cache_dir);
    let verification = match cache.verify() {
        Ok(verification) => verification,
        Err(error) => {
            return report_cli_error(
                CliError::diagnostic(DiagnosticCode::AIPO_PKG_FETCH, error.to_string()),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    if verification.errors.is_empty() {
        let _ = writeln!(out, "cache verified: {} entries", verification.entries);
        return EXIT_SUCCESS;
    }
    let source = Source::new(SourceId::next(), "<package-cache>", "");
    let diagnostics = verification
        .errors
        .iter()
        .map(|error| Diagnostic::error(DiagnosticCode::AIPO_PKG_FETCH, error.to_string()))
        .collect::<Vec<_>>();
    emit_diagnostics(MessageFormat::Human, &source, &diagnostics, out, err);
    EXIT_LANGUAGE_FAILURE
}

#[cfg(feature = "github-http")]
pub(crate) fn package_lock_with_github(
    package_dir: &Path,
    cache_dir: &Path,
    github_token_env: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let discovered = match LocalPackageResolver::new(package_dir).discover_local() {
        Ok(discovered) => discovered,
        Err(error) => {
            return report_cli_error(
                classify_package_error(error, package_dir),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    let root_coordinate = discovered.root.clone();
    let root_input = match discovered.inputs.get(&root_coordinate) {
        Some(input) => input.clone(),
        None => {
            return report_cli_error(
                CliError::diagnostic(
                    DiagnosticCode::AIPO_PKG_RESOLUTION,
                    "local package discovery did not produce a root package".to_string(),
                ),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    let cache = GitHubCache::new(cache_dir);
    let token = match github_token_from_env(github_token_env) {
        Ok(token) => token,
        Err(error) => return report_cli_error(error, MessageFormat::Human, out, err),
    };
    let transport = match token {
        Some(token) => GitHubHttpFetcher::with_token(token),
        None => GitHubHttpFetcher::new(),
    };
    let fetcher = CachedGitHubFetcher::new(&cache, transport);
    let resolved = match resolve_mixed_package_graph(
        root_input,
        discovered.inputs.clone(),
        discovered.paths,
        &cache,
        &fetcher,
    ) {
        Ok(resolved) => resolved,
        Err(error) => {
            return report_cli_error(
                classify_cached_graph_error(error),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    if resolved.graph.root != root_coordinate {
        return report_cli_error(
            CliError::diagnostic(
                DiagnosticCode::AIPO_PKG_RESOLUTION,
                format!(
                    "resolved root '{}' does not match local root '{}'",
                    resolved.graph.root, root_coordinate
                ),
            ),
            MessageFormat::Human,
            out,
            err,
        );
    }
    let lockfile = match resolved.graph.to_lockfile() {
        Ok(lockfile) => lockfile,
        Err(error) => {
            return report_cli_error(
                CliError::diagnostic(
                    DiagnosticCode::AIPO_PKG_LOCK_STALE,
                    format!("mixed package graph could not be locked: {error}"),
                ),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    let contents = match lockfile.to_toml() {
        Ok(contents) => contents,
        Err(error) => {
            return report_cli_error(
                CliError::diagnostic(
                    DiagnosticCode::AIPO_PKG_LOCK_STALE,
                    format!("package lockfile could not be serialized: {error}"),
                ),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    if let Err(error) = write_lockfile(package_dir, &contents) {
        return report_cli_error(error, MessageFormat::Human, out, err);
    }
    let _ = writeln!(
        out,
        "locked {} at {}",
        resolved.graph.root,
        package_dir.join(LOCK_FILE_NAME).display()
    );
    EXIT_SUCCESS
}

#[cfg(feature = "github-http")]
struct GitHubFetchOptions<'a> {
    repository: &'a str,
    revision: &'a str,
    subpath: &'a str,
    cache_dir: &'a Path,
    out_dir: &'a Path,
    github_token_env: Option<&'a str>,
}

#[cfg(feature = "github-http")]
pub(crate) fn package_fetch_github(
    options: GitHubFetchOptions<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let GitHubFetchOptions {
        repository,
        revision,
        subpath,
        cache_dir,
        out_dir,
        github_token_env,
    } = options;
    let source = match PackageSource::github_at(repository, revision, subpath) {
        Ok(source) => source,
        Err(error) => {
            return report_cli_error(
                CliError::Usage(format!("invalid GitHub source: {error}")),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    let cache = GitHubCache::new(cache_dir);
    let token = match github_token_from_env(github_token_env) {
        Ok(token) => token,
        Err(error) => return report_cli_error(error, MessageFormat::Human, out, err),
    };
    let transport = match token {
        Some(token) => GitHubHttpFetcher::with_token(token),
        None => GitHubHttpFetcher::new(),
    };
    let fetcher = CachedGitHubFetcher::new(&cache, transport);
    let resolved = match resolve_github_package_graph(source.clone(), &fetcher) {
        Ok(resolved) => resolved,
        Err(GitHubGraphError::Fetch(error)) => return report_fetch_error(error, out, err),
        Err(GitHubGraphError::Resolve(error)) => {
            return report_cli_error(
                CliError::diagnostic(DiagnosticCode::AIPO_PKG_RESOLUTION, error.to_string()),
                MessageFormat::Human,
                out,
                err,
            );
        }
    };
    if let Err(error) = write_fetched_package(out_dir, &resolved) {
        return report_cli_error(error, MessageFormat::Human, out, err);
    }
    let _ = writeln!(
        out,
        "fetched {}@{} graph from {}",
        resolved.root.manifest.name,
        resolved.root.manifest.version,
        source.location()
    );
    EXIT_SUCCESS
}

#[cfg(feature = "github-http")]
pub(crate) fn report_fetch_error(
    error: GitHubFetchError,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    report_cli_error(
        CliError::diagnostic(DiagnosticCode::AIPO_PKG_FETCH, error.to_string()),
        MessageFormat::Human,
        out,
        err,
    )
}

#[cfg(feature = "github-http")]
pub(crate) fn write_fetched_package(
    output_dir: &Path,
    resolved: &aipo_package::GitHubPackageGraph,
) -> Result<(), CliError> {
    prepare_output_dir(output_dir)?;
    let manifest = resolved.root.manifest.to_toml().map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_FETCH,
            format!("fetched manifest could not be serialized: {error}"),
        )
    })?;
    let entry_path = safe_output_path(output_dir, &resolved.root.manifest.entry)?;
    let lock = resolved.graph.to_lockfile().map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_FETCH,
            format!("fetched package graph lockfile is invalid: {error}"),
        )
    })?;
    let lock = lock.to_toml().map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_FETCH,
            format!("fetched package lockfile could not be serialized: {error}"),
        )
    })?;
    write_new_output_file(&output_dir.join(MANIFEST_FILE_NAME), manifest.as_bytes())?;
    write_new_output_file(&entry_path, &resolved.root.entry_bytes)?;
    write_new_output_file(&output_dir.join(LOCK_FILE_NAME), lock.as_bytes())
}

#[cfg(feature = "github-http")]
pub(crate) fn prepare_output_dir(path: &Path) -> Result<(), CliError> {
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| CliError::Usage(format!("{}: {error}", path.display())))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CliError::Usage(format!(
                "{} is not a safe output directory",
                path.display()
            )));
        }
        if std::fs::read_dir(path)
            .map_err(|error| CliError::Usage(format!("{}: {error}", path.display())))?
            .next()
            .is_some()
        {
            return Err(CliError::Usage(format!("{} must be empty", path.display())));
        }
        return Ok(());
    }
    std::fs::create_dir_all(path)
        .map_err(|error| CliError::Usage(format!("{}: {error}", path.display())))
}

#[cfg(feature = "github-http")]
pub(crate) fn safe_output_path(root: &Path, relative: &str) -> Result<PathBuf, CliError> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative.chars().any(char::is_control)
    {
        return Err(CliError::Usage(format!(
            "unsafe package entry path '{relative}'"
        )));
    }
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                return Err(CliError::Usage(format!(
                    "unsafe package entry path '{relative}'"
                )));
            }
            segment => path.push(segment),
        }
    }
    Ok(path)
}

#[cfg(feature = "github-http")]
pub(crate) fn write_new_output_file(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| CliError::Usage(format!("{}: {error}", parent.display())))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| CliError::Usage(format!("{}: {error}", path.display())))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| CliError::Usage(format!("{}: {error}", path.display())))
}

pub(crate) fn resolve_local_package(
    package_root: &Path,
    context: &Path,
) -> Result<ResolvedLocalPackages, CliError> {
    LocalPackageResolver::new(package_root)
        .resolve_with_paths()
        .map_err(|error| classify_package_error(error, context))
}

pub(crate) fn classify_package_error(error: ResolveError, context: &Path) -> CliError {
    let code = match &error {
        ResolveError::Io { .. } => {
            return CliError::Usage(format!("{}: {error}", context.display()));
        }
        ResolveError::Lockfile(_) => DiagnosticCode::AIPO_PKG_LOCK_STALE,
        _ => DiagnosticCode::AIPO_PKG_RESOLUTION,
    };
    CliError::diagnostic(code, format!("{}: {error}", context.display()))
}

pub(crate) fn generated_lockfile(graph: &ResolvedPackageGraph) -> Result<Lockfile, CliError> {
    graph.to_lockfile().map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_LOCK_STALE,
            format!("resolved package graph could not produce a lockfile: {error}"),
        )
    })
}

pub(crate) fn validate_existing_lock(
    lock_path: &Path,
    graph: &ResolvedPackageGraph,
    required: bool,
) -> Result<(), CliError> {
    let bytes = match std::fs::read(lock_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if required {
                return Err(CliError::diagnostic(
                    DiagnosticCode::AIPO_PKG_LOCK_STALE,
                    format!("lockfile '{}' is missing", lock_path.display()),
                ));
            }
            return Ok(());
        }
        Err(error) => {
            return Err(CliError::Usage(format!("{}: {error}", lock_path.display())));
        }
    };

    let actual = Lockfile::from_bytes(&bytes).map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_LOCK_STALE,
            format!("lockfile '{}' is invalid: {error}", lock_path.display()),
        )
    })?;
    let expected = generated_lockfile(graph)?;
    if actual != expected {
        return Err(CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_LOCK_STALE,
            format!(
                "lockfile '{}' is stale: it does not match the resolved local package graph",
                lock_path.display()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn write_lockfile(package_root: &Path, contents: &str) -> Result<(), CliError> {
    let lock_path = package_root.join(LOCK_FILE_NAME);
    let temp_path = package_root.join(format!(
        ".aipo.lock.tmp-{}-{}",
        std::process::id(),
        NEXT_LOCK_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(CliError::Usage(format!("{}: {error}", temp_path.display())));
    }
    if let Err(error) = std::fs::rename(&temp_path, &lock_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(CliError::Usage(format!("{}: {error}", lock_path.display())));
    }
    Ok(())
}
