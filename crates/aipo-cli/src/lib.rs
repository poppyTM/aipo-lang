//! `aipo-cli` implements the stable command surface documented in
//! `docs/reference/cli.md`:
//!
//! ```text
//! aipo run <path> [--wasm] [--engine=<vm|reg>] [--budget=<N>] [--max-instructions=<N>] [--package-cache <dir>] [--ahs=<file>] [--host=headless-test] [--message-format=<human|jsonl>]
//! aipo test [path] [--filter <pattern>] [--package-cache <dir>] [--message-format=<human|jsonl>]
//! aipo check <path> [--wasm] [--package-cache <dir>] [--ahs=<file>] [--host=headless-test] [--message-format=<human|jsonl>]
//! aipo build <path> [--target <js|wasm>] [--out <dir>] [--package-cache <dir>] [--message-format=<human|jsonl>]
//! aipo disasm <path> [--wasm] [--package-cache <dir>] [--message-format=<human|jsonl>]
//! aipo fmt <paths...> [--check]
//! aipo package lock <package-dir> [--fetch-github --cache <dir>] [--github-token-env <name>]
//! aipo package audit <package-dir>
//! aipo package cache verify <cache-dir>
//! aipo package cache prune <cache-dir> --lock <lockfile> [--apply]
//! aipo --version
//! aipo --help
//! ```
//!
//! Exit codes are part of the contract:
//!
//! - `0` — success (the program ran to completion, `check` found no error, or formatting
//!   either wrote the files or verified them unchanged);
//! - `1` — language/package failure (lexical/parse/semantic or package diagnostic, bytecode
//!   verification error, uncaught runtime fault, or a `--check` run that would rewrite a file);
//! - `2` — usage error (unknown command or flag, missing argument, unreadable file).
//!
//! The command surface is a thin orchestrator over the existing pipeline stages:
//! `aipo-source → aipo-syntax → aipo-hir → aipo-sema → aipo-ir → aipo-bytecode → aipo-vm`,
//! with `aipo-stdlib` registered before execution.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use aipo_bytecode::{RegCompiledModule, RegEmitter};
use aipo_diagnostics::{Diagnostic, DiagnosticCode, DiagnosticEmitter, MessageFormat, Severity};
use aipo_package::{
    CacheOnlyGitHubFetcher, CachedGraphError, GitHubCache, LOCK_FILE_NAME, LocalPackageResolver,
    Lockfile, MANIFEST_FILE_NAME, Manifest, PackagePathMap, PackageSource, ResolveError,
    ResolvedLocalPackages, ResolvedPackageGraph, resolve_mixed_package_graph,
};
#[cfg(feature = "github-http")]
use aipo_package::{
    CachedGitHubFetcher, GitHubFetchError, GitHubGraphError, GitHubHttpFetcher, GitHubToken,
    resolve_github_package_graph,
};
use aipo_runtime::NativeRegistry;
use aipo_sema::{HostFunction, HostParameter, PreludeSurface};
use aipo_source::{Source, SourceId, SourceMap};
use aipo_vm::{RegVm, Vm, VmError};
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

mod ahs;
mod headless;
mod lsp;
pub mod repl;
pub mod session;
mod tooling;
mod vendor;
pub use session::Session;
pub mod modules;

static NEXT_LOCK_TEMP: AtomicU64 = AtomicU64::new(0);

/// Loads a validated host description for use with [`compile_file`] or [`analyze_with_surface`].
pub use ahs::load as load_host_surface;

/// Redirects the standard library's `io` output away from the process streams.
///
/// Program output goes to standard output by default; an embedder (or an integration test)
/// can install a sink here to capture it.
pub use aipo_stdlib::io::set_output_sink;

/// Successful execution.
pub const EXIT_SUCCESS: u8 = 0;
/// Language failure: diagnostics, runtime fault, or `fmt --check` drift.
pub const EXIT_LANGUAGE_FAILURE: u8 = 1;
/// Usage failure: bad command line or unreadable input.
pub const EXIT_USAGE: u8 = 2;

/// Help text printed for `aipo --help`.
pub const USAGE: &str = "\
aipo — Aipo language toolchain

USAGE:
    aipo run <path> [--wasm] [--engine=<vm|reg>] [--budget=<N>] [--max-instructions=<N>] [--package-cache <dir>] [--message-format=<human|jsonl>] [--ahs=<file>] [--host=headless-test]
    aipo test [path] [--filter <pattern>] [--package-cache <dir>] [--message-format=<human|jsonl>]
    aipo check <path> [--wasm] [--package-cache <dir>] [--message-format=<human|jsonl>] [--ahs=<file>] [--host=headless-test]
    aipo build <path> [--target <js|wasm>] [--out <dir>] [--package-cache <dir>] [--message-format=<human|jsonl>]
    aipo disasm <path> [--wasm] [--package-cache <dir>] [--message-format=<human|jsonl>]
    aipo fmt <paths...> [--check]
    aipo new <directory>
    aipo watch <path>
    aipo profile <path> [--budget <instructions>] [--json]
    aipo debug <path>
    aipo plan <path>
    aipo lsp
    aipo package lock <package-dir> [--fetch-github --cache <dir>] [--github-token-env <name>]
    aipo package vendor <package-dir> --out <directory> [--cache <directory>]
    aipo package audit <package-dir>
    aipo package cache verify <cache-dir>
    aipo package cache prune <cache-dir> --lock <lockfile> [--apply]
    aipo --version
    aipo --help

COMMANDS:
    run      Compile and execute an Aipo source file (.aipo), bytecode file (.aibc), or WebAssembly module (.wasm)
    test     Discover and run isolated unit tests
    check    Run the frontend, semantic analysis and bytecode verification
    build    Emit a JavaScript bundle (default) or WebAssembly binary (.wasm)
    disasm   Disassemble a source file (.aipo), bytecode file (.aibc), or WebAssembly binary (.wasm)
    fmt      Format source files in place; --check reports drift without writing
    package  Create or audit a local package lockfile, or verify the package cache

FLAGS:
    --ahs=<file>   Host surface description (AHS) the run may reach, as data
    --host=headless-test   Opt-in VM conformance host (no capabilities granted)
    --engine=<vm|reg>   Execution engine for `run`: stack VM (default) or register VM
    --max-instructions=<N>   Maximum instruction or Wasm fuel budget (alias: --budget)

EXIT CODES:
    0  success
    1  language failure (diagnostics, runtime fault, or formatting drift)
    2  usage error (bad arguments or unreadable file)
";

#[cfg(feature = "github-http")]
const GITHUB_HTTP_USAGE: &str = "\n    aipo package fetch-github <owner/repository> <40-hex-commit> [subpath] --cache <dir> --out <dir> [--github-token-env <name>]\n";

/// Runs the CLI for an argument vector that excludes the program name.
pub fn run(args: &[String]) -> ExitCode {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    ExitCode::from(run_with(args, &mut out, &mut err))
}

fn usage_text() -> String {
    #[cfg(feature = "github-http")]
    {
        format!("{USAGE}{GITHUB_HTTP_USAGE}")
    }
    #[cfg(not(feature = "github-http"))]
    {
        USAGE.to_string()
    }
}

/// Runs the CLI with explicit output streams, returning the process exit code.
///
/// Keeping the streams injectable makes the whole command surface testable without
/// spawning a process.
pub fn run_with(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    if let Some(code) = tooling::dispatch(args, out, err) {
        return code;
    }
    let parsed = match Command::parse(args) {
        Ok(command) => command,
        Err(usage) => {
            let _ = writeln!(err, "error: {usage}");
            let _ = writeln!(err, "\n{}", usage_text());
            return EXIT_USAGE;
        }
    };

    match parsed {
        Command::Help => {
            let _ = write!(out, "{}", usage_text());
            EXIT_SUCCESS
        }
        Command::Version => {
            let _ = writeln!(out, "aipo {}", env!("CARGO_PKG_VERSION"));
            EXIT_SUCCESS
        }
        Command::Run {
            path,
            format,
            package_cache,
            wasm,
            reg,
            ahs,
            host,
            budget,
        } => {
            let surface = match execution_surface(ahs.as_deref(), host) {
                Ok(surface) => surface,
                Err(message) => {
                    let _ = writeln!(err, "error: {message}");
                    return EXIT_USAGE;
                }
            };
            execute(
                &path,
                format,
                package_cache.as_deref(),
                out,
                err,
                ExecutionOptions {
                    action: Action::Run,
                    wasm,
                    reg,
                    surface: surface.as_ref(),
                    host,
                    budget,
                },
            )
        }
        Command::Test {
            path,
            filter,
            format,
            package_cache,
        } => test_command(
            path.as_deref(),
            filter.as_deref(),
            format,
            package_cache.as_deref(),
            out,
            err,
        ),
        Command::Check {
            path,
            format,
            package_cache,
            wasm,
            ahs,
            host,
        } => {
            let surface = match execution_surface(ahs.as_deref(), host) {
                Ok(surface) => surface,
                Err(message) => {
                    let _ = writeln!(err, "error: {message}");
                    return EXIT_USAGE;
                }
            };
            execute(
                &path,
                format,
                package_cache.as_deref(),
                out,
                err,
                ExecutionOptions {
                    action: Action::Check,
                    wasm,
                    reg: false,
                    surface: surface.as_ref(),
                    host,
                    budget: None,
                },
            )
        }
        Command::Build {
            path,
            out_dir,
            target,
            format,
            package_cache,
        } => build_bundle(
            &path,
            out_dir.as_deref(),
            target,
            format,
            package_cache.as_deref(),
            out,
            err,
        ),
        Command::Disasm {
            path,
            format,
            package_cache,
            wasm,
        } => disassemble_command(&path, wasm, format, package_cache.as_deref(), out, err),
        Command::Fmt { paths, check } => format_files(&paths, check, out, err),
        Command::Package {
            operation,
            package_dir,
            fetch_github,
            cache_dir,
            github_token_env,
        } => package_command(
            operation,
            &package_dir,
            fetch_github,
            cache_dir.as_deref(),
            github_token_env.as_deref(),
            out,
            err,
        ),
        Command::PackageCacheVerify { cache_dir } => package_cache_verify(&cache_dir, out, err),
        Command::PackageCachePrune {
            cache_dir,
            lock_path,
            apply,
        } => package_cache_prune(&cache_dir, &lock_path, apply, out, err),
        #[cfg(feature = "github-http")]
        Command::PackageFetchGitHub {
            repository,
            revision,
            subpath,
            cache_dir,
            out_dir,
            github_token_env,
        } => package_fetch_github(
            GitHubFetchOptions {
                repository: &repository,
                revision: &revision,
                subpath: &subpath,
                cache_dir: &cache_dir,
                out_dir: &out_dir,
                github_token_env: github_token_env.as_deref(),
            },
            out,
            err,
        ),
    }
}

/// Evaluates an in-memory Aipo source string directly (for REPL and `aipo-sh -c`).
///
/// Returns exit code `0` on success, `1` on error.
pub fn eval_source(source_text: &str, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let source = Source::new(SourceId::next(), "<eval>", source_text);
    let compiled = analyze_with_surface(&source, Path::new("<eval>"), None, None);
    let has_errors = compiled
        .diagnostics
        .iter()
        .any(|diag| diag.severity == Severity::Error);

    emit_diagnostics(
        MessageFormat::Human,
        &source,
        &compiled.diagnostics,
        out,
        err,
    );

    if has_errors {
        return EXIT_LANGUAGE_FAILURE;
    }

    match execute_module(&compiled.module) {
        Ok(()) => EXIT_SUCCESS,
        Err(error) => {
            let diagnostic = runtime_diagnostic(&source, &error);
            emit_diagnostics(
                MessageFormat::Human,
                &source,
                std::slice::from_ref(&diagnostic),
                out,
                err,
            );
            EXIT_LANGUAGE_FAILURE
        }
    }
}

/// Evaluates an in-memory Aipo source string on the register VM.
///
/// Mirrors [`eval_source`] (same frontend diagnostics and stdlib surface) but
/// lowers to 32-bit register bytecode and executes with [`RegVm`].
/// Used by `aipo-sh -c` and the REPL under `--engine=reg`.
///
/// Returns exit code `0` on success, `1` on error.
pub fn eval_source_reg(source_text: &str, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let source = Source::new(SourceId::next(), "<eval>", source_text);
    let (reg_module, diagnostics) =
        analyze_to_execution_plan(&source, Path::new("<eval>"), None, None);
    let has_errors = diagnostics
        .iter()
        .any(|diag| diag.severity == Severity::Error);

    emit_diagnostics(MessageFormat::Human, &source, &diagnostics, out, err);

    if has_errors {
        return EXIT_LANGUAGE_FAILURE;
    }

    let Some(reg_module) = reg_module else {
        return EXIT_LANGUAGE_FAILURE;
    };

    match execute_plan_with_host(&reg_module, None, None) {
        Ok(_) => EXIT_SUCCESS,
        Err(error) => {
            let diagnostic = runtime_diagnostic(&source, &error);
            emit_diagnostics(
                MessageFormat::Human,
                &source,
                std::slice::from_ref(&diagnostic),
                out,
                err,
            );
            EXIT_LANGUAGE_FAILURE
        }
    }
}

/// Target output format for `aipo build`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTarget {
    /// Emit a JavaScript bundle (default: app.js, aipo-runtime.js, app.js.map).
    Js,
    /// Emit a WebAssembly binary (dist/app.wasm).
    Wasm,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Help,
    Version,
    Run {
        path: PathBuf,
        format: MessageFormat,
        package_cache: Option<PathBuf>,
        wasm: bool,
        /// `true` when `--engine=reg` selects the register VM.
        reg: bool,
        /// Host surface described as data (`--ahs=<file>`), when one was given.
        ahs: Option<PathBuf>,
        host: Option<HostProfile>,
        /// Execution budget (maximum instructions or Wasm fuel).
        budget: Option<u64>,
    },
    Test {
        path: Option<PathBuf>,
        filter: Option<String>,
        format: MessageFormat,
        package_cache: Option<PathBuf>,
    },
    Check {
        path: PathBuf,
        format: MessageFormat,
        package_cache: Option<PathBuf>,
        wasm: bool,
        /// Host surface described as data (`--ahs=<file>`), when one was given.
        ahs: Option<PathBuf>,
        host: Option<HostProfile>,
    },
    Build {
        path: PathBuf,
        out_dir: Option<PathBuf>,
        target: BuildTarget,
        format: MessageFormat,
        package_cache: Option<PathBuf>,
    },
    Disasm {
        path: PathBuf,
        format: MessageFormat,
        package_cache: Option<PathBuf>,
        wasm: bool,
    },
    Fmt {
        paths: Vec<PathBuf>,
        check: bool,
    },
    Package {
        operation: PackageOperation,
        package_dir: PathBuf,
        fetch_github: bool,
        cache_dir: Option<PathBuf>,
        github_token_env: Option<String>,
    },
    PackageCacheVerify {
        cache_dir: PathBuf,
    },
    PackageCachePrune {
        cache_dir: PathBuf,
        lock_path: PathBuf,
        apply: bool,
    },
    #[cfg(feature = "github-http")]
    PackageFetchGitHub {
        repository: String,
        revision: String,
        subpath: String,
        cache_dir: PathBuf,
        out_dir: PathBuf,
        github_token_env: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageOperation {
    Lock,
    Audit,
}

enum CliError {
    Usage(String),
    Diagnostic(Box<Diagnostic>),
}

impl CliError {
    fn diagnostic(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::Diagnostic(Box::new(Diagnostic::error(code, message)))
    }
}

fn cli_error_message(error: CliError) -> String {
    match error {
        CliError::Usage(message) => message,
        CliError::Diagnostic(diagnostic) => diagnostic.message,
    }
}

impl Command {
    fn parse(args: &[String]) -> Result<Self, String> {
        let Some(first) = args.first() else {
            return Err("missing command".to_string());
        };

        match first.as_str() {
            "--help" | "-h" | "help" => Ok(Self::Help),
            "--version" | "-V" | "version" => Ok(Self::Version),
            "run" | "check" | "disasm" => {
                let mut path = None;
                let mut format = MessageFormat::Human;
                let mut package_cache = None;
                let mut wasm = false;
                let mut reg = false;
                let mut engine_seen = false;
                let mut ahs = None;
                let mut host = None;
                let mut budget = None;
                let rest = &args[1..];
                let mut index = 0;
                while index < rest.len() {
                    let arg = &rest[index];
                    if let Some(value) = arg.strip_prefix("--message-format=") {
                        format = parse_format(value)?;
                    } else if arg == "--message-format" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--message-format requires a value".to_string())?;
                        format = parse_format(value)?;
                    } else if arg == "--wasm" {
                        wasm = true;
                    } else if arg == "--reg" {
                        if first != "run" {
                            return Err("--engine is only supported by run".to_string());
                        }
                        if engine_seen {
                            return Err("'--engine' was provided more than once".to_string());
                        }
                        engine_seen = true;
                        reg = true;
                    } else if let Some(value) = arg.strip_prefix("--engine=") {
                        if first != "run" {
                            return Err("--engine is only supported by run".to_string());
                        }
                        if engine_seen {
                            return Err("'--engine' was provided more than once".to_string());
                        }
                        engine_seen = true;
                        match value {
                            "reg" => reg = true,
                            "vm" | "bytecode" | "stack" => reg = false,
                            _ => {
                                return Err(format!(
                                    "unrecognized --engine '{value}': expected 'vm' or 'reg'"
                                ));
                            }
                        }
                    } else if arg == "--engine" {
                        if first != "run" {
                            return Err("--engine is only supported by run".to_string());
                        }
                        if engine_seen {
                            return Err("'--engine' was provided more than once".to_string());
                        }
                        engine_seen = true;
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--engine requires a value".to_string())?;
                        match value.as_str() {
                            "reg" => reg = true,
                            "vm" | "bytecode" | "stack" => reg = false,
                            _ => {
                                return Err(format!(
                                    "unrecognized --engine '{value}': expected 'vm' or 'reg'"
                                ));
                            }
                        }
                    } else if let Some(target) = arg.strip_prefix("--target=") {
                        match target {
                            "wasm" => wasm = true,
                            "vm" | "bytecode" => wasm = false,
                            other => {
                                return Err(format!(
                                    "unrecognized target '{other}': expected 'wasm' or 'bytecode'"
                                ));
                            }
                        }
                    } else if arg == "--target" || arg == "-t" {
                        index += 1;
                        let target = rest
                            .get(index)
                            .ok_or_else(|| format!("'{arg}' requires a target name"))?;
                        match target.as_str() {
                            "wasm" => wasm = true,
                            "vm" | "bytecode" => wasm = false,
                            other => {
                                return Err(format!(
                                    "unrecognized target '{other}': expected 'wasm' or 'bytecode'"
                                ));
                            }
                        }
                    } else if let Some(value) = arg.strip_prefix("--package-cache=") {
                        if value.is_empty() {
                            return Err(
                                "--package-cache requires a non-empty directory".to_string()
                            );
                        }
                        if package_cache.is_some() {
                            return Err("'--package-cache' was provided more than once".to_string());
                        }
                        package_cache = Some(PathBuf::from(value));
                    } else if arg == "--package-cache" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--package-cache requires a directory".to_string())?;
                        if value.is_empty() || value.starts_with('-') {
                            return Err(
                                "--package-cache requires a non-empty directory".to_string()
                            );
                        }
                        if package_cache.is_some() {
                            return Err("'--package-cache' was provided more than once".to_string());
                        }
                        package_cache = Some(PathBuf::from(value));
                    } else if arg == "--ahs" || arg.starts_with("--ahs=") {
                        if first == "disasm" {
                            return Err("--ahs is only supported by run and check".to_string());
                        }
                        if ahs.is_some() {
                            return Err("'--ahs' was provided more than once".to_string());
                        }
                        let value = if let Some(value) = arg.strip_prefix("--ahs=") {
                            value
                        } else {
                            index += 1;
                            rest.get(index)
                                .ok_or_else(|| "--ahs requires a file argument".to_string())?
                        };
                        if value.is_empty() || value.starts_with('-') {
                            return Err("--ahs requires a non-empty file argument".to_string());
                        }
                        ahs = Some(PathBuf::from(value));
                    } else if arg == "--host" || arg.starts_with("--host=") {
                        if first == "disasm" {
                            return Err("--host is only supported by run and check".to_string());
                        }
                        if host.is_some() {
                            return Err("'--host' was provided more than once".to_string());
                        }
                        let value = if let Some(value) = arg.strip_prefix("--host=") {
                            value
                        } else {
                            index += 1;
                            rest.get(index)
                                .ok_or_else(|| "--host requires a profile".to_string())?
                        };
                        host = Some(match value {
                            "headless-test" => HostProfile::HeadlessTest,
                            _ => {
                                return Err(format!(
                                    "unrecognized --host profile '{value}': expected 'headless-test'"
                                ));
                            }
                        });
                    } else if arg == "--budget"
                        || arg.starts_with("--budget=")
                        || arg == "--max-instructions"
                        || arg.starts_with("--max-instructions=")
                    {
                        let flag_name = if arg.starts_with("--budget") {
                            "--budget"
                        } else {
                            "--max-instructions"
                        };
                        if first != "run" {
                            return Err(format!("{flag_name} is only supported by run"));
                        }
                        if budget.is_some() {
                            return Err(format!("'{flag_name}' was provided more than once"));
                        }
                        let value = if let Some(val) = arg.strip_prefix("--budget=") {
                            val
                        } else if let Some(val) = arg.strip_prefix("--max-instructions=") {
                            val
                        } else {
                            index += 1;
                            let val = rest
                                .get(index)
                                .ok_or_else(|| format!("{flag_name} requires a value"))?;
                            val.as_str()
                        };
                        let parsed_budget = value.parse::<u64>().map_err(|_| {
                            format!("invalid budget '{value}': expected a positive integer")
                        })?;
                        budget = Some(parsed_budget);
                    } else if arg.starts_with('-') {
                        return Err(format!("unrecognized flag '{arg}'"));
                    } else if path.is_some() {
                        return Err(format!("unexpected argument '{arg}'"));
                    } else {
                        path = Some(PathBuf::from(arg));
                    }
                    index += 1;
                }

                let path = path.ok_or_else(|| format!("'{first}' requires a path argument"))?;
                if reg {
                    if wasm {
                        return Err("--engine=reg cannot be combined with --wasm".to_string());
                    }
                    if path.extension().and_then(|ext| ext.to_str()) != Some("aipo") {
                        return Err("--engine=reg requires a .aipo source file".to_string());
                    }
                }
                if host.is_some() {
                    if wasm || path.extension().and_then(|ext| ext.to_str()) != Some("aipo") {
                        return Err(
                            "--host requires a .aipo source file on the VM backend".to_string()
                        );
                    }
                    if ahs.is_some() {
                        return Err(
                            "--host supplies its own AHS and cannot be combined with --ahs"
                                .to_string(),
                        );
                    }
                }
                if first == "run" {
                    Ok(Self::Run {
                        path,
                        format,
                        package_cache,
                        wasm,
                        reg,
                        ahs,
                        host,
                        budget,
                    })
                } else if first == "check" {
                    Ok(Self::Check {
                        path,
                        format,
                        package_cache,
                        wasm,
                        ahs,
                        host,
                    })
                } else {
                    Ok(Self::Disasm {
                        path,
                        format,
                        package_cache,
                        wasm,
                    })
                }
            }
            "test" => {
                let mut path = None;
                let mut filter = None;
                let mut format = MessageFormat::Human;
                let mut package_cache = None;
                let rest = &args[1..];
                let mut index = 0;
                while index < rest.len() {
                    let arg = &rest[index];
                    if let Some(value) = arg.strip_prefix("--message-format=") {
                        format = parse_format(value)?;
                    } else if arg == "--message-format" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--message-format requires a value".to_string())?;
                        format = parse_format(value)?;
                    } else if let Some(value) = arg.strip_prefix("--filter=") {
                        if filter.is_some() {
                            return Err("'--filter' was provided more than once".to_string());
                        }
                        filter = Some(value.to_string());
                    } else if arg == "--filter" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--filter requires a pattern".to_string())?;
                        if filter.is_some() {
                            return Err("'--filter' was provided more than once".to_string());
                        }
                        filter = Some(value.to_string());
                    } else if let Some(value) = arg.strip_prefix("--package-cache=") {
                        if value.is_empty() {
                            return Err(
                                "--package-cache requires a non-empty directory".to_string()
                            );
                        }
                        if package_cache.is_some() {
                            return Err("'--package-cache' was provided more than once".to_string());
                        }
                        package_cache = Some(PathBuf::from(value));
                    } else if arg == "--package-cache" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--package-cache requires a directory".to_string())?;
                        if value.is_empty() || value.starts_with('-') {
                            return Err(
                                "--package-cache requires a non-empty directory".to_string()
                            );
                        }
                        if package_cache.is_some() {
                            return Err("'--package-cache' was provided more than once".to_string());
                        }
                        package_cache = Some(PathBuf::from(value));
                    } else if arg.starts_with('-') {
                        return Err(format!("unrecognized flag '{arg}'"));
                    } else if path.is_some() {
                        return Err(format!("unexpected argument '{arg}'"));
                    } else {
                        path = Some(PathBuf::from(arg));
                    }
                    index += 1;
                }
                Ok(Self::Test {
                    path,
                    filter,
                    format,
                    package_cache,
                })
            }
            "build" => {
                let mut path = None;
                let mut out_dir = None;
                let mut target = BuildTarget::Js;
                let mut format = MessageFormat::Human;
                let mut package_cache = None;
                let rest = &args[1..];
                let mut index = 0;
                while index < rest.len() {
                    let arg = &rest[index];
                    if let Some(value) = arg.strip_prefix("--message-format=") {
                        format = parse_format(value)?;
                    } else if arg == "--message-format" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--message-format requires a value".to_string())?;
                        format = parse_format(value)?;
                    } else if arg == "--wasm" {
                        target = BuildTarget::Wasm;
                    } else if let Some(t) = arg.strip_prefix("--target=") {
                        match t {
                            "wasm" => target = BuildTarget::Wasm,
                            "js" => target = BuildTarget::Js,
                            other => {
                                return Err(format!(
                                    "unrecognized build target '{other}': expected 'js' or 'wasm'"
                                ));
                            }
                        }
                    } else if arg == "--target" || arg == "-t" {
                        index += 1;
                        let t = rest
                            .get(index)
                            .ok_or_else(|| format!("'{arg}' requires a target name"))?;
                        match t.as_str() {
                            "wasm" => target = BuildTarget::Wasm,
                            "js" => target = BuildTarget::Js,
                            other => {
                                return Err(format!(
                                    "unrecognized build target '{other}': expected 'js' or 'wasm'"
                                ));
                            }
                        }
                    } else if arg == "--out" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "'--out' requires a directory argument".to_string())?;
                        out_dir = Some(PathBuf::from(value));
                    } else if let Some(value) = arg.strip_prefix("--out=") {
                        out_dir = Some(PathBuf::from(value));
                    } else if let Some(value) = arg.strip_prefix("--package-cache=") {
                        if value.is_empty() {
                            return Err(
                                "--package-cache requires a non-empty directory".to_string()
                            );
                        }
                        if package_cache.is_some() {
                            return Err("'--package-cache' was provided more than once".to_string());
                        }
                        package_cache = Some(PathBuf::from(value));
                    } else if arg == "--package-cache" {
                        index += 1;
                        let value = rest
                            .get(index)
                            .ok_or_else(|| "--package-cache requires a directory".to_string())?;
                        if value.is_empty() || value.starts_with('-') {
                            return Err(
                                "--package-cache requires a non-empty directory".to_string()
                            );
                        }
                        if package_cache.is_some() {
                            return Err("'--package-cache' was provided more than once".to_string());
                        }
                        package_cache = Some(PathBuf::from(value));
                    } else if arg.starts_with('-') {
                        return Err(format!("unrecognized flag '{arg}'"));
                    } else if path.is_some() {
                        return Err(format!("unexpected argument '{arg}'"));
                    } else {
                        path = Some(PathBuf::from(arg));
                    }
                    index += 1;
                }

                let path = path.ok_or_else(|| "'build' requires a path argument".to_string())?;
                Ok(Self::Build {
                    path,
                    out_dir,
                    target,
                    format,
                    package_cache,
                })
            }
            "package" => {
                if args.get(1).is_some_and(|operation| operation == "cache") {
                    return match args.get(2).map(String::as_str) {
                        Some("verify") => parse_cache_verify(&args[1..]),
                        Some("prune") => parse_cache_prune(&args[1..]),
                        _ => Err("'package cache' requires 'verify' or 'prune'".to_string()),
                    };
                }
                #[cfg(feature = "github-http")]
                if args
                    .get(1)
                    .is_some_and(|operation| operation == "fetch-github")
                {
                    return parse_fetch_github(&args[1..]);
                }
                if args.len() < 3 {
                    return Err(
                        "'package' requires 'lock' or 'audit' and a package directory".to_string(),
                    );
                }
                let operation = match args[1].as_str() {
                    "lock" => PackageOperation::Lock,
                    "audit" => PackageOperation::Audit,
                    other => {
                        return Err(format!(
                            "unrecognized package operation '{other}': expected 'lock' or 'audit'"
                        ));
                    }
                };
                if args[2].starts_with('-') {
                    return Err(format!("unrecognized flag '{}'", args[2]));
                }
                let mut fetch_github = false;
                let mut cache_dir = None;
                let mut github_token_env = None;
                let mut index = 3;
                while index < args.len() {
                    match args[index].as_str() {
                        "--fetch-github" => {
                            if operation != PackageOperation::Lock {
                                return Err("'--fetch-github' is only valid with 'package lock'"
                                    .to_string());
                            }
                            if fetch_github {
                                return Err(
                                    "'--fetch-github' was provided more than once".to_string()
                                );
                            }
                            fetch_github = true;
                        }
                        "--cache" => {
                            index += 1;
                            let value = args
                                .get(index)
                                .ok_or_else(|| "'--cache' requires a directory".to_string())?;
                            if value.is_empty() || value.starts_with('-') {
                                return Err("'--cache' requires a non-empty directory".to_string());
                            }
                            if cache_dir.is_some() {
                                return Err("'--cache' was provided more than once".to_string());
                            }
                            cache_dir = Some(PathBuf::from(value));
                        }
                        value if value.starts_with("--cache=") => {
                            let value = value.trim_start_matches("--cache=");
                            if value.is_empty() {
                                return Err("'--cache' requires a non-empty directory".to_string());
                            }
                            if cache_dir.is_some() {
                                return Err("'--cache' was provided more than once".to_string());
                            }
                            cache_dir = Some(PathBuf::from(value));
                        }
                        "--github-token-env" => {
                            index += 1;
                            let value = args.get(index).ok_or_else(|| {
                                "'--github-token-env' requires a variable name".to_string()
                            })?;
                            if !is_valid_github_token_env_name(value) {
                                return Err("'--github-token-env' requires a valid variable name"
                                    .to_string());
                            }
                            if github_token_env.is_some() {
                                return Err(
                                    "'--github-token-env' was provided more than once".to_string()
                                );
                            }
                            github_token_env = Some(value.clone());
                        }
                        value if value.starts_with("--github-token-env=") => {
                            let value = value.trim_start_matches("--github-token-env=");
                            if !is_valid_github_token_env_name(value) {
                                return Err("'--github-token-env' requires a valid variable name"
                                    .to_string());
                            }
                            if github_token_env.is_some() {
                                return Err(
                                    "'--github-token-env' was provided more than once".to_string()
                                );
                            }
                            github_token_env = Some(value.to_string());
                        }
                        value if value.starts_with('-') => {
                            return Err(format!("unrecognized package flag '{value}'"));
                        }
                        value => return Err(format!("unexpected package argument '{value}'")),
                    }
                    index += 1;
                }
                if fetch_github != cache_dir.is_some() {
                    return Err(
                        "'--fetch-github' and '--cache' must be provided together".to_string()
                    );
                }
                if github_token_env.is_some() && !fetch_github {
                    return Err("'--github-token-env' requires '--fetch-github'".to_string());
                }
                Ok(Self::Package {
                    operation,
                    package_dir: PathBuf::from(&args[2]),
                    fetch_github,
                    cache_dir,
                    github_token_env,
                })
            }
            "fmt" => {
                let mut paths = Vec::new();
                let mut check = false;
                for arg in &args[1..] {
                    if arg == "--check" {
                        check = true;
                    } else if arg.starts_with('-') {
                        return Err(format!("unrecognized flag '{arg}'"));
                    } else {
                        paths.push(PathBuf::from(arg));
                    }
                }
                if paths.is_empty() {
                    return Err("'fmt' requires at least one path argument".to_string());
                }
                Ok(Self::Fmt { paths, check })
            }
            other => Err(format!("unrecognized command '{other}'")),
        }
    }
}

fn parse_cache_verify(args: &[String]) -> Result<Command, String> {
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

fn parse_cache_prune(args: &[String]) -> Result<Command, String> {
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
fn parse_fetch_github(args: &[String]) -> Result<Command, String> {
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

fn is_valid_github_token_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn parse_format(value: &str) -> Result<MessageFormat, String> {
    match value {
        "human" => Ok(MessageFormat::Human),
        "jsonl" => Ok(MessageFormat::Jsonl),
        other => Err(format!(
            "invalid message format '{other}': expected 'human' or 'jsonl'"
        )),
    }
}

/// What a pipeline invocation should do after analysis succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Run,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostProfile {
    HeadlessTest,
}

struct ExecutionOptions<'a> {
    action: Action,
    wasm: bool,
    reg: bool,
    surface: Option<&'a PreludeSurface>,
    host: Option<HostProfile>,
    budget: Option<u64>,
}

fn execution_surface(
    path: Option<&Path>,
    host: Option<HostProfile>,
) -> Result<Option<PreludeSurface>, String> {
    match host {
        Some(HostProfile::HeadlessTest) => headless::surface().map(Some),
        None => path.map(ahs::load).transpose(),
    }
}

struct LoadedSource {
    source: Source,
    package_paths: Option<PackagePathMap>,
}

/// Result of compiling a source file down to verified bytecode.
pub struct Compiled {
    /// The compiled bytecode module.
    pub module: aipo_bytecode::BytecodeModule,
    /// Diagnostics accumulated during compilation.
    pub diagnostics: Vec<Diagnostic>,
}

/// Runs the frontend pipeline with an optional custom prelude surface and reports diagnostics.
pub fn analyze_with_surface(
    source: &Source,
    path: &Path,
    package_paths: Option<&PackagePathMap>,
    extra_surface: Option<&PreludeSurface>,
) -> Compiled {
    analyze_full(source, path, package_paths, extra_surface, true)
}

/// Runs the frontend pipeline, optionally skipping the Core IR optimization passes.
///
/// `optimize` runs [`aipo_ir::optimize`] between lowering and emission; passing
/// `false` keeps the raw lowered stream, which is what a differential conformance
/// run compares against.
pub fn analyze_full(
    source: &Source,
    path: &Path,
    package_paths: Option<&PackagePathMap>,
    extra_surface: Option<&PreludeSurface>,
    optimize: bool,
) -> Compiled {
    analyze_full_with_catalog(source, path, package_paths, extra_surface, optimize, None)
}

pub(crate) fn analyze_full_with_catalog(
    source: &Source,
    path: &Path,
    package_paths: Option<&PackagePathMap>,
    extra_surface: Option<&PreludeSurface>,
    optimize: bool,
    mut catalog: Option<&mut Vec<aipo_hir::HirItem>>,
) -> Compiled {
    let (program, mut diagnostics) = aipo_syntax::parse(source);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Compiled {
            module: aipo_bytecode::BytecodeModule::new(),
            diagnostics,
        };
    }

    // Every module reachable from this file is merged first, so semantic analysis sees one
    // program with the imported declarations already in place.
    let hir = aipo_hir::lower(program);
    let resolved = modules::resolve(path, hir, package_paths);
    diagnostics.extend(resolved.diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Compiled {
            module: aipo_bytecode::BytecodeModule::new(),
            diagnostics,
        };
    }
    let mut hir = resolved.program;
    if let Some(previous) = catalog.as_deref() {
        let new_keys: std::collections::HashSet<_> = hir
            .items
            .iter()
            .filter_map(crate::session::item_key)
            .collect();
        let mut items: Vec<_> = previous
            .iter()
            .filter(|item| {
                crate::session::item_key(item).is_some_and(|key| !new_keys.contains(&key))
            })
            .cloned()
            .collect();
        items.append(&mut hir.items);
        hir.items = items;
    }

    let mut surface = prelude_surface();
    if let Some(extra) = extra_surface {
        for (name, kind) in extra.iter() {
            match kind {
                aipo_sema::SymbolKind::Function { min_args, max_args } => {
                    surface.add_function(name, *min_args, *max_args);
                }
                _ => {
                    if extra.is_mutable(name) {
                        surface.add_mutable_variable(name);
                    } else {
                        surface.add_variable(name);
                    }
                }
            }
        }
        for (module, functions) in extra.host_modules() {
            surface.add_host_module(module, functions.clone());
        }
    }
    // Imported module names and aliases are ordinary globals to the checker.
    for name in &resolved.imported_names {
        surface.add_variable(name);
    }
    let (_, sema_diagnostics) = aipo_sema::check_with_prelude(source, &hir, &surface);
    diagnostics.extend(sema_diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Compiled {
            module: aipo_bytecode::BytecodeModule::new(),
            diagnostics,
        };
    }

    let ir = aipo_ir::lower_to_ir(&hir);
    let ir = if optimize { aipo_ir::optimize(&ir) } else { ir };
    match aipo_bytecode::compile(&ir) {
        Ok(module) => {
            if let Some(catalog) = catalog.as_mut() {
                **catalog = hir
                    .items
                    .iter()
                    .filter(|item| crate::session::item_key(item).is_some())
                    .cloned()
                    .collect();
            }
            Compiled {
                module,
                diagnostics,
            }
        }
        Err(errors) => {
            for error in errors {
                diagnostics.push(verification_diagnostic(&error));
            }
            Compiled {
                module: aipo_bytecode::BytecodeModule::new(),
                diagnostics,
            }
        }
    }
}

/// Runs the frontend pipeline and reports diagnostics.
///
/// `analyze` never executes the program: `check` stops here, while `run` continues with the
/// private module execution step.
pub fn analyze(source: &Source, path: &Path, package_paths: Option<&PackagePathMap>) -> Compiled {
    analyze_with_surface(source, path, package_paths, None)
}

/// Runs the frontend pipeline and lowers to a register-VM module.
///
/// Shares the parse/HIR/resolve/sema stages with [`analyze_full`], then emits
/// 32-bit register bytecode via [`RegEmitter::compile_module`] instead of the
/// stack bytecode. Returns `(None, diagnostics)` when the frontend reports errors.
pub fn analyze_to_reg_module(
    source: &Source,
    path: &Path,
    package_paths: Option<&PackagePathMap>,
    extra_surface: Option<&PreludeSurface>,
) -> (Option<RegCompiledModule>, Vec<Diagnostic>) {
    let (program, mut diagnostics) = aipo_syntax::parse(source);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let hir = aipo_hir::lower(program);
    let resolved = modules::resolve(path, hir, package_paths);
    diagnostics.extend(resolved.diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }
    let hir = resolved.program;

    let mut surface = prelude_surface();
    if let Some(extra) = extra_surface {
        for (name, kind) in extra.iter() {
            match kind {
                aipo_sema::SymbolKind::Function { min_args, max_args } => {
                    surface.add_function(name, *min_args, *max_args);
                }
                _ => {
                    if extra.is_mutable(name) {
                        surface.add_mutable_variable(name);
                    } else {
                        surface.add_variable(name);
                    }
                }
            }
        }
        for (module, functions) in extra.host_modules() {
            surface.add_host_module(module, functions.clone());
        }
    }
    for name in &resolved.imported_names {
        surface.add_variable(name);
    }
    let (_, sema_diagnostics) = aipo_sema::check_with_prelude(source, &hir, &surface);
    diagnostics.extend(sema_diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let ir = aipo_ir::lower_to_ir(&hir);
    let ir = aipo_ir::optimize(&ir);
    match RegEmitter::new().compile_module(&ir) {
        Ok(reg_module) => (Some(reg_module), diagnostics),
        Err(reason) => {
            let code = DiagnosticCode::AIPO_COMPILE_REG_UNSUPPORTED;
            let locale = aipo_diagnostics::Locale::detect();
            let help = match locale {
                aipo_diagnostics::Locale::En => "Run this program with --engine=vm.",
                aipo_diagnostics::Locale::PtBr => "Execute este programa com --engine=vm.",
            };
            diagnostics.push(
                Diagnostic::error(code, code.title(locale))
                    .with_note(reason)
                    .with_suggestion(aipo_diagnostics::Suggestion {
                        message: help.into(),
                        replacement: String::new(),
                        start: 0,
                        end: 0,
                    }),
            );
            (None, diagnostics)
        }
    }
}

/// Compiles an explicit execution plan with canonical fallback for shared VM services.
pub fn analyze_to_execution_plan(
    source: &Source,
    path: &Path,
    package_paths: Option<&PackagePathMap>,
    extra_surface: Option<&PreludeSurface>,
) -> (Option<aipo_bytecode::ExecutionPlan>, Vec<Diagnostic>) {
    let (program, mut diagnostics) = aipo_syntax::parse(source);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let hir = aipo_hir::lower(program);
    let resolved = modules::resolve(path, hir, package_paths);
    diagnostics.extend(resolved.diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }
    let hir = resolved.program;

    let mut surface = prelude_surface();
    if let Some(extra) = extra_surface {
        for (name, kind) in extra.iter() {
            match kind {
                aipo_sema::SymbolKind::Function { min_args, max_args } => {
                    surface.add_function(name, *min_args, *max_args);
                }
                _ => {
                    if extra.is_mutable(name) {
                        surface.add_mutable_variable(name);
                    } else {
                        surface.add_variable(name);
                    }
                }
            }
        }
        for (module, functions) in extra.host_modules() {
            surface.add_host_module(module, functions.clone());
        }
    }
    for name in &resolved.imported_names {
        surface.add_variable(name);
    }
    let (_, sema_diagnostics) = aipo_sema::check_with_prelude(source, &hir, &surface);
    diagnostics.extend(sema_diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let ir = aipo_ir::lower_to_ir(&hir);
    let ir = aipo_ir::optimize(&ir);
    match aipo_bytecode::compile_execution_plan(&ir) {
        Ok(plan) => (Some(plan), diagnostics),
        Err(errors) => {
            diagnostics.extend(errors.iter().map(|error| verification_diagnostic(error)));
            (None, diagnostics)
        }
    }
}

fn verification_diagnostic(reason: &str) -> Diagnostic {
    let fault = aipo_vm::VmFault::CorruptedBytecode {
        offset: 0,
        reason: reason.to_string(),
    };
    Diagnostic::error(fault.diagnostic_code(), fault.to_string())
}

fn report_cli_error(
    error: CliError,
    format: MessageFormat,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match error {
        CliError::Usage(message) => {
            let _ = writeln!(err, "error: {message}");
            EXIT_USAGE
        }
        CliError::Diagnostic(diagnostic) => {
            let source = Source::new(SourceId::next(), "<package>", "");
            emit_diagnostics(
                format,
                &source,
                std::slice::from_ref(diagnostic.as_ref()),
                out,
                err,
            );
            EXIT_LANGUAGE_FAILURE
        }
    }
}

fn package_command(
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

fn package_cache_prune(
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

fn package_cache_verify(cache_dir: &Path, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
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
fn package_lock_with_github(
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
fn package_fetch_github(
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
fn report_fetch_error(error: GitHubFetchError, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    report_cli_error(
        CliError::diagnostic(DiagnosticCode::AIPO_PKG_FETCH, error.to_string()),
        MessageFormat::Human,
        out,
        err,
    )
}

#[cfg(feature = "github-http")]
fn write_fetched_package(
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
fn prepare_output_dir(path: &Path) -> Result<(), CliError> {
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
fn safe_output_path(root: &Path, relative: &str) -> Result<PathBuf, CliError> {
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
fn write_new_output_file(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
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

fn resolve_local_package(
    package_root: &Path,
    context: &Path,
) -> Result<ResolvedLocalPackages, CliError> {
    LocalPackageResolver::new(package_root)
        .resolve_with_paths()
        .map_err(|error| classify_package_error(error, context))
}

fn classify_package_error(error: ResolveError, context: &Path) -> CliError {
    let code = match &error {
        ResolveError::Io { .. } => {
            return CliError::Usage(format!("{}: {error}", context.display()));
        }
        ResolveError::Lockfile(_) => DiagnosticCode::AIPO_PKG_LOCK_STALE,
        _ => DiagnosticCode::AIPO_PKG_RESOLUTION,
    };
    CliError::diagnostic(code, format!("{}: {error}", context.display()))
}

fn generated_lockfile(graph: &ResolvedPackageGraph) -> Result<Lockfile, CliError> {
    graph.to_lockfile().map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_LOCK_STALE,
            format!("resolved package graph could not produce a lockfile: {error}"),
        )
    })
}

fn validate_existing_lock(
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

fn write_lockfile(package_root: &Path, contents: &str) -> Result<(), CliError> {
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

/// Compiles an Aipo source file down to a WebAssembly module binary.
fn compile_to_wasm(
    source: &Source,
    path: &Path,
    package_paths: Option<&PackagePathMap>,
    extra_surface: Option<&PreludeSurface>,
) -> (Option<Vec<u8>>, Vec<Diagnostic>) {
    let (program, mut diagnostics) = aipo_syntax::parse(source);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let hir = aipo_hir::lower(program);
    let resolved = modules::resolve(path, hir, package_paths);
    diagnostics.extend(resolved.diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let mut surface = prelude_surface();
    if let Some(extra) = extra_surface {
        for (module, functions) in extra.host_modules() {
            surface.add_host_module(module, functions.clone());
        }
    }
    for name in &resolved.imported_names {
        surface.add_variable(name);
    }
    let (_, sema_diagnostics) = aipo_sema::check_with_prelude(source, &resolved.program, &surface);
    diagnostics.extend(sema_diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    #[cfg(feature = "wasm")]
    match aipo_wasm::compile_hir(&resolved.program) {
        Ok(bytes) => (Some(bytes), diagnostics),
        Err(err) => {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_COMPILE_WASM_UNSUPPORTED,
                    format!("wasm compilation failed: {err}"),
                )
                .with_primary_span(source, err.span())
                .with_note("use --engine=vm for the full language surface"),
            );
            (None, diagnostics)
        }
    }
    #[cfg(not(feature = "wasm"))]
    {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::AIPO_COMPILE_WASM_UNSUPPORTED,
                "WebAssembly compiler is disabled in this build (wasm feature not enabled)"
                    .to_string(),
            )
            .with_note("rebuild with the `wasm` feature or use --engine=vm"),
        );
        (None, diagnostics)
    }
}

fn execute(
    path: &Path,
    format: MessageFormat,
    package_cache: Option<&Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    options: ExecutionOptions<'_>,
) -> u8 {
    let ExecutionOptions {
        action,
        wasm,
        reg,
        surface,
        host,
        budget,
    } = options;
    if surface.is_some()
        && matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("wasm" | "aibc")
        )
    {
        let _ = writeln!(err, "error: --ahs requires an Aipo source file");
        return EXIT_USAGE;
    }
    // Pre-compiled `.wasm` files execute directly or check via disassembly.
    if path.extension().and_then(|ext| ext.to_str()) == Some("wasm") {
        if package_cache.is_some() {
            let _ = writeln!(
                err,
                "error: --package-cache is not supported for .wasm files"
            );
            return EXIT_USAGE;
        }
        return execute_wasm_file(path, action, budget, out, err);
    }

    // Pre-compiled `.aibc` files skip the frontend pipeline entirely.
    if path.extension().and_then(|ext| ext.to_str()) == Some("aibc") {
        if wasm {
            let _ = writeln!(err, "error: --wasm is not supported for .aibc files");
            return EXIT_USAGE;
        }
        if package_cache.is_some() {
            let _ = writeln!(
                err,
                "error: --package-cache is not supported for .aibc files"
            );
            return EXIT_USAGE;
        }
        if action == Action::Check {
            let _ = writeln!(err, "error: `check` is not supported for .aibc files");
            return EXIT_USAGE;
        }
        return execute_bytecode(path, format, budget, out, err);
    }

    if wasm {
        let LoadedSource {
            source,
            package_paths,
        } = match load_source_entry(path, package_cache) {
            Ok(loaded) => loaded,
            Err(error) => return report_cli_error(error, format, out, err),
        };

        let (wasm_bytes, diagnostics) =
            compile_to_wasm(&source, path, package_paths.as_ref(), surface);
        let has_errors = diagnostics
            .iter()
            .any(|diag| diag.severity == Severity::Error);

        emit_diagnostics(format, &source, &diagnostics, out, err);

        if has_errors {
            return EXIT_LANGUAGE_FAILURE;
        }
        if action == Action::Check {
            return EXIT_SUCCESS;
        }

        let Some(bytes) = wasm_bytes else {
            return EXIT_LANGUAGE_FAILURE;
        };

        #[cfg(feature = "wasm")]
        match aipo_wasm::execute_wasm_with_fuel(&bytes, out, budget) {
            Ok(_) => EXIT_SUCCESS,
            Err(error) => {
                let _ = writeln!(err, "error: {error}");
                EXIT_LANGUAGE_FAILURE
            }
        }
        #[cfg(not(feature = "wasm"))]
        {
            let _ = bytes;
            let _ = writeln!(
                err,
                "error: WebAssembly execution is disabled in this build"
            );
            EXIT_LANGUAGE_FAILURE
        }
    } else {
        let LoadedSource {
            source,
            package_paths,
        } = match load_source_entry(path, package_cache) {
            Ok(loaded) => loaded,
            Err(error) => return report_cli_error(error, format, out, err),
        };

        let compiled = analyze_with_surface(&source, path, package_paths.as_ref(), surface);
        let has_errors = compiled
            .diagnostics
            .iter()
            .any(|diag| diag.severity == Severity::Error);

        emit_diagnostics(format, &source, &compiled.diagnostics, out, err);

        if has_errors {
            return EXIT_LANGUAGE_FAILURE;
        }
        if action == Action::Check {
            return EXIT_SUCCESS;
        }

        if reg {
            let (reg_module, reg_diagnostics) = if host.is_some() {
                (
                    Some(aipo_bytecode::ExecutionPlan::Canonical {
                        module: Box::new(compiled.module.clone()),
                        reason: "host callbacks require the canonical host context".into(),
                    }),
                    Vec::new(),
                )
            } else {
                analyze_to_execution_plan(&source, path, package_paths.as_ref(), surface)
            };
            let reg_has_errors = reg_diagnostics
                .iter()
                .any(|diag| diag.severity == Severity::Error);
            emit_diagnostics(format, &source, &reg_diagnostics, out, err);
            if reg_has_errors {
                return EXIT_LANGUAGE_FAILURE;
            }
            let Some(reg_module) = reg_module else {
                return EXIT_LANGUAGE_FAILURE;
            };
            return match execute_plan_with_host(&reg_module, host, budget) {
                Ok(_) => EXIT_SUCCESS,
                Err(error) => {
                    let diagnostic = runtime_diagnostic(&source, &error);
                    emit_diagnostics(format, &source, std::slice::from_ref(&diagnostic), out, err);
                    EXIT_LANGUAGE_FAILURE
                }
            };
        }

        match execute_module_with_host(&compiled.module, host, budget) {
            Ok(()) => EXIT_SUCCESS,
            Err(error) => {
                let diagnostic = runtime_diagnostic(&source, &error);
                emit_diagnostics(format, &source, std::slice::from_ref(&diagnostic), out, err);
                EXIT_LANGUAGE_FAILURE
            }
        }
    }
}

/// Executes or checks a pre-compiled `.wasm` binary module.
#[cfg(feature = "wasm")]
fn execute_wasm_file(
    path: &Path,
    action: Action,
    budget: Option<u64>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = writeln!(err, "error: {}: {error}", path.display());
            return EXIT_USAGE;
        }
    };

    if action == Action::Check {
        match aipo_wasm::disassemble_wasm(&bytes) {
            Ok(_) => EXIT_SUCCESS,
            Err(error) => {
                let _ = writeln!(err, "error: malformed .wasm: {error}");
                EXIT_LANGUAGE_FAILURE
            }
        }
    } else {
        match aipo_wasm::execute_wasm_with_fuel(&bytes, out, budget) {
            Ok(_) => EXIT_SUCCESS,
            Err(error) => {
                let _ = writeln!(err, "error: {error}");
                EXIT_LANGUAGE_FAILURE
            }
        }
    }
}

#[cfg(not(feature = "wasm"))]
fn execute_wasm_file(
    _path: &Path,
    _action: Action,
    _budget: Option<u64>,
    _out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let _ = writeln!(
        err,
        "error: WebAssembly execution is disabled in this build"
    );
    EXIT_USAGE
}

/// Loads a pre-compiled `.aibc` file, verifies its bytecode, and executes it.
fn execute_bytecode(
    path: &Path,
    format: MessageFormat,
    budget: Option<u64>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = writeln!(err, "error: {}: {error}", path.display());
            return EXIT_USAGE;
        }
    };

    let module = match aipo_bytecode::BytecodeModule::from_bytes(&bytes) {
        Ok(module) => module,
        Err(error) => {
            let diagnostic = verification_diagnostic(&format!("malformed .aibc: {error}"));
            let source = Source::new(SourceId::next(), path.display().to_string(), "");
            emit_diagnostics(format, &source, std::slice::from_ref(&diagnostic), out, err);
            return EXIT_LANGUAGE_FAILURE;
        }
    };

    if let Err(errors) = aipo_bytecode::BytecodeVerifier::verify(&module) {
        let source = Source::new(SourceId::next(), path.display().to_string(), "");
        let diagnostics: Vec<Diagnostic> =
            errors.iter().map(|e| verification_diagnostic(e)).collect();
        emit_diagnostics(format, &source, &diagnostics, out, err);
        return EXIT_LANGUAGE_FAILURE;
    }

    match execute_module_with_host(&module, None, budget) {
        Ok(()) => EXIT_SUCCESS,
        Err(error) => {
            let source = Source::new(SourceId::next(), path.display().to_string(), "");
            let diagnostic = runtime_diagnostic(&source, &error);
            emit_diagnostics(format, &source, std::slice::from_ref(&diagnostic), out, err);
            EXIT_LANGUAGE_FAILURE
        }
    }
}

/// Disassembles a source file, bytecode file, or WebAssembly module and prints the listing.
fn disassemble_command(
    path: &Path,
    wasm: bool,
    format: MessageFormat,
    package_cache: Option<&Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if path.extension().and_then(|ext| ext.to_str()) == Some("wasm") {
        if package_cache.is_some() {
            let _ = writeln!(
                err,
                "error: --package-cache is not supported for .wasm files"
            );
            return EXIT_USAGE;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = writeln!(err, "error: {}: {error}", path.display());
                return EXIT_USAGE;
            }
        };

        #[cfg(feature = "wasm")]
        match aipo_wasm::disassemble_wasm(&bytes) {
            Ok(wat) => {
                let _ = write!(out, "{wat}");
                EXIT_SUCCESS
            }
            Err(error) => {
                let _ = writeln!(err, "error: malformed .wasm: {error}");
                EXIT_LANGUAGE_FAILURE
            }
        }
        #[cfg(not(feature = "wasm"))]
        {
            let _ = bytes;
            let _ = writeln!(
                err,
                "error: WebAssembly disassembler is disabled in this build"
            );
            EXIT_USAGE
        }
    } else if path.extension().and_then(|ext| ext.to_str()) == Some("aibc") {
        if wasm {
            let _ = writeln!(err, "error: --wasm is not supported for .aibc files");
            return EXIT_USAGE;
        }
        if package_cache.is_some() {
            let _ = writeln!(
                err,
                "error: --package-cache is not supported for .aibc files"
            );
            return EXIT_USAGE;
        }
        // For .aibc files: read bytes, deserialize, disassemble without source.
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = writeln!(err, "error: {}: {error}", path.display());
                return EXIT_USAGE;
            }
        };

        let module = match aipo_bytecode::BytecodeModule::from_bytes(&bytes) {
            Ok(module) => module,
            Err(error) => {
                let diagnostic = verification_diagnostic(&format!("malformed .aibc: {error}"));
                let source = Source::new(SourceId::next(), path.display().to_string(), "");
                emit_diagnostics(format, &source, std::slice::from_ref(&diagnostic), out, err);
                return EXIT_LANGUAGE_FAILURE;
            }
        };

        let listing = aipo_bytecode::disassemble(&module);
        let _ = write!(out, "{listing}");
        EXIT_SUCCESS
    } else if wasm {
        let LoadedSource {
            source,
            package_paths,
        } = match load_source_entry(path, package_cache) {
            Ok(loaded) => loaded,
            Err(error) => return report_cli_error(error, format, out, err),
        };
        let (wasm_bytes, diagnostics) =
            compile_to_wasm(&source, path, package_paths.as_ref(), None);
        let has_errors = diagnostics
            .iter()
            .any(|diag| diag.severity == Severity::Error);

        emit_diagnostics(format, &source, &diagnostics, out, err);

        if has_errors {
            return EXIT_LANGUAGE_FAILURE;
        }

        let Some(bytes) = wasm_bytes else {
            return EXIT_LANGUAGE_FAILURE;
        };

        #[cfg(feature = "wasm")]
        match aipo_wasm::disassemble_wasm(&bytes) {
            Ok(wat) => {
                let _ = write!(out, "{wat}");
                EXIT_SUCCESS
            }
            Err(error) => {
                let _ = writeln!(err, "error: disassembly failed: {error}");
                EXIT_LANGUAGE_FAILURE
            }
        }
        #[cfg(not(feature = "wasm"))]
        {
            let _ = bytes;
            let _ = writeln!(
                err,
                "error: WebAssembly disassembler is disabled in this build"
            );
            EXIT_USAGE
        }
    } else {
        // For .aipo files: compile first, then disassemble with source annotations.
        let LoadedSource {
            source,
            package_paths,
        } = match load_source_entry(path, package_cache) {
            Ok(loaded) => loaded,
            Err(error) => return report_cli_error(error, format, out, err),
        };
        let compiled = analyze(&source, path, package_paths.as_ref());
        let has_errors = compiled
            .diagnostics
            .iter()
            .any(|diag| diag.severity == Severity::Error);

        emit_diagnostics(format, &source, &compiled.diagnostics, out, err);

        if has_errors {
            return EXIT_LANGUAGE_FAILURE;
        }

        let listing = aipo_bytecode::disassemble_with_source(&compiled.module, &source);
        let _ = write!(out, "{listing}");
        EXIT_SUCCESS
    }
}

/// Emits a JavaScript bundle or WebAssembly binary for an entry file.
///
/// Diagnostics use the same frontend as `run`/`check`, so a program that fails
/// `check` fails `build` with the same codes and no files are written.
#[allow(clippy::needless_pass_by_value)]
fn build_bundle(
    path: &Path,
    out_dir: Option<&Path>,
    target: BuildTarget,
    format: MessageFormat,
    package_cache: Option<&Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let LoadedSource {
        source,
        package_paths,
    } = match load_source_entry(path, package_cache) {
        Ok(loaded) => loaded,
        Err(error) => return report_cli_error(error, format, out, err),
    };

    match target {
        BuildTarget::Wasm => {
            let (wasm_bytes, diagnostics) =
                compile_to_wasm(&source, path, package_paths.as_ref(), None);
            let has_errors = diagnostics
                .iter()
                .any(|diag| diag.severity == Severity::Error);

            emit_diagnostics(format, &source, &diagnostics, out, err);

            if has_errors {
                return EXIT_LANGUAGE_FAILURE;
            }

            let Some(bytes) = wasm_bytes else {
                return EXIT_LANGUAGE_FAILURE;
            };

            let dir: PathBuf = match out_dir {
                Some(dir) => dir.to_path_buf(),
                None => path
                    .parent()
                    .map_or_else(|| PathBuf::from("dist"), Path::to_path_buf)
                    .join("dist"),
            };
            if let Err(error) = std::fs::create_dir_all(&dir) {
                let _ = writeln!(err, "error: {}: {error}", dir.display());
                return EXIT_USAGE;
            }
            let out_file = dir.join("app.wasm");
            if let Err(error) = std::fs::write(&out_file, &bytes) {
                let _ = writeln!(err, "error: {}: {error}", out_file.display());
                return EXIT_USAGE;
            }
            let _ = writeln!(out, "built 1 file to {}", dir.display());
            EXIT_SUCCESS
        }
        BuildTarget::Js => {
            #[cfg(not(feature = "js"))]
            {
                let _ = writeln!(err, "error: JavaScript emitter is disabled in this build");
                EXIT_USAGE
            }
            #[cfg(feature = "js")]
            {
                let (program, mut diagnostics) = aipo_syntax::parse(&source);
                if !diagnostics.iter().any(|d| d.severity == Severity::Error) {
                    let hir = aipo_hir::lower(program);
                    let resolved = modules::resolve(path, hir, package_paths.as_ref());
                    diagnostics.extend(resolved.diagnostics);
                    if !diagnostics.iter().any(|d| d.severity == Severity::Error) {
                        let mut surface = prelude_surface();
                        for name in &resolved.imported_names {
                            surface.add_variable(name);
                        }
                        let (_, sema_diagnostics) =
                            aipo_sema::check_with_prelude(&source, &resolved.program, &surface);
                        diagnostics.extend(sema_diagnostics);
                        if !diagnostics.iter().any(|d| d.severity == Severity::Error) {
                            let ir = aipo_ir::lower_to_ir(&resolved.program);
                            let file_name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("main.aipo");
                            let bundle = aipo_js::emit_js(file_name, source.text(), &ir);
                            let dir: PathBuf = match out_dir {
                                Some(dir) => dir.to_path_buf(),
                                None => path
                                    .parent()
                                    .map_or_else(|| PathBuf::from("dist"), Path::to_path_buf)
                                    .join("dist"),
                            };
                            if let Err(error) = std::fs::create_dir_all(&dir) {
                                let _ = writeln!(err, "error: {}: {error}", dir.display());
                                return EXIT_USAGE;
                            }
                            for (name, contents) in [
                                ("app.js", bundle.app_js.as_str()),
                                ("aipo-runtime.js", bundle.runtime_js.as_str()),
                                ("app.js.map", bundle.source_map.as_str()),
                                ("package.json", "{\n  \"type\": \"module\"\n}\n"),
                            ] {
                                if let Err(error) = std::fs::write(dir.join(name), contents) {
                                    let _ = writeln!(
                                        err,
                                        "error: {}: {error}",
                                        dir.join(name).display()
                                    );
                                    return EXIT_USAGE;
                                }
                            }
                            let _ = writeln!(out, "built 4 files to {}", dir.display());
                            return EXIT_SUCCESS;
                        }
                    }
                }

                emit_diagnostics(format, &source, &diagnostics, out, err);
                EXIT_LANGUAGE_FAILURE
            }
        }
    }
}

/// Registers the structs and struct methods declared in a compiled module onto the VM.
pub fn register_module_symbols(vm: &mut Vm, module: &aipo_bytecode::BytecodeModule) {
    for decl in &module.structs {
        let fields: Vec<(&str, bool)> = decl
            .fields
            .iter()
            .map(|(name, fixed)| (name.as_str(), *fixed))
            .collect();
        vm.register_struct(decl.name.clone(), fields);
    }
    for function in &module.functions {
        if let Some((type_name, method)) = function.name.split_once('.') {
            vm.register_struct_method(
                type_name,
                method,
                function.entry_ip,
                function.params,
                function.is_async,
            );
        }
    }
}

/// Registers the standard library and runs a verified module.
///
/// # Errors
/// Returns the runtime error raised by the program.
fn execute_module(module: &aipo_bytecode::BytecodeModule) -> Result<(), VmError> {
    execute_module_with_host(module, None, None)
}

fn execute_module_with_host(
    module: &aipo_bytecode::BytecodeModule,
    host: Option<HostProfile>,
    budget: Option<u64>,
) -> Result<(), VmError> {
    let (mut vm, _) = standard_environment();
    if let Some(HostProfile::HeadlessTest) = host {
        headless::install(&mut vm);
    }
    vm.set_max_instructions(budget);
    register_module_symbols(&mut vm, module);
    vm.run(module).map(|_| ())
}

/// Registers the standard library and runs a register-VM module.
///
/// Globals (including stdlib natives) are bridged from a stack-VM standard
/// environment so both engines observe the same `io`, `sh`, `math`, … surface.
///
/// # Errors
/// Returns the runtime error raised by the program.
fn execute_plan_with_host(
    plan: &aipo_bytecode::ExecutionPlan,
    host: Option<HostProfile>,
    budget: Option<u64>,
) -> Result<aipo_vm::Value, VmError> {
    let (mut stack_vm, _) = standard_environment();
    if let Some(HostProfile::HeadlessTest) = host {
        headless::install(&mut stack_vm);
    }
    match plan {
        aipo_bytecode::ExecutionPlan::Canonical { module, .. } => {
            stack_vm.set_max_instructions(budget);
            register_module_symbols(&mut stack_vm, module);
            stack_vm.run(module)
        }
        aipo_bytecode::ExecutionPlan::Register(module) => {
            let mut reg_vm = RegVm::new();
            reg_vm.globals = stack_vm.globals.clone();
            reg_vm.method_natives = stack_vm.method_natives.clone();
            reg_vm.set_max_instructions(budget);
            let value = reg_vm.run_module(module).map_err(VmError::Fault)?;
            if let aipo_vm::Value::Failure(failure) = &value {
                return Err(VmError::UncaughtFailure(failure.message.clone()));
            }
            Ok(value)
        }
    }
}

fn collect_test_files(target: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    fn is_test_file(path: &Path) -> bool {
        if path.extension().and_then(|ext| ext.to_str()) != Some("aipo") {
            return false;
        }
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.ends_with("_test.aipo") || file_name.starts_with("test_") {
            return true;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            return content.contains("#!test");
        }
        false
    }

    fn scan_dir(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    scan_dir(&path, files);
                } else if is_test_file(&path) {
                    files.push(path);
                }
            }
        }
    }

    let mut files = Vec::new();
    if let Some(path) = target {
        if path.is_file() {
            files.push(path.to_path_buf());
        } else if path.is_dir() {
            scan_dir(path, &mut files);
        } else {
            return Err(format!("'{}' does not exist", path.display()));
        }
    } else {
        let tests_dir = Path::new("tests");
        if tests_dir.is_dir() {
            scan_dir(tests_dir, &mut files);
        }
        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_test_file(&path) {
                    files.push(path);
                }
            }
        }
    }

    files.sort();
    files.dedup();

    if files.is_empty() {
        return Err("no test files found".to_string());
    }
    Ok(files)
}

fn test_command(
    path: Option<&Path>,
    filter: Option<&str>,
    format: MessageFormat,
    package_cache: Option<&Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let test_files = match collect_test_files(path) {
        Ok(files) => files,
        Err(msg) => {
            let _ = writeln!(err, "error: {msg}");
            return EXIT_USAGE;
        }
    };

    let suite_start = std::time::Instant::now();
    let mut suite_total_passed = 0;
    let mut suite_total_failed = 0;
    let mut suite_total_skipped = 0;

    for test_file in &test_files {
        let LoadedSource {
            source,
            package_paths,
        } = match load_source_entry(test_file, package_cache) {
            Ok(loaded) => loaded,
            Err(error) => return report_cli_error(error, format, out, err),
        };

        let compiled = analyze(&source, test_file, package_paths.as_ref());
        let has_errors = compiled
            .diagnostics
            .iter()
            .any(|diag| diag.severity == Severity::Error);

        if has_errors {
            emit_diagnostics(format, &source, &compiled.diagnostics, out, err);
            return EXIT_LANGUAGE_FAILURE;
        }

        let (mut vm, _) = standard_environment();
        register_module_symbols(&mut vm, &compiled.module);
        let discovered = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        vm.set_test_mode(aipo_vm::TestMode::Discover(discovered.clone()));
        let _ = vm.run(&compiled.module);

        #[derive(Clone)]
        enum TestTarget {
            Legacy(String),
            Directive {
                display_name: String,
                fn_name: String,
            },
        }

        let (parsed_program, _) = aipo_syntax::parse(&source);
        let mut all_tests: Vec<TestTarget> = discovered
            .borrow()
            .iter()
            .cloned()
            .map(TestTarget::Legacy)
            .collect();
        for item in &parsed_program.items {
            if let aipo_ast::Item::Fn(f) = item {
                for dir in &f.directives {
                    if dir.name.name == "test" {
                        let display_name = match &dir.argument {
                            Some(arg) => {
                                let trimmed = arg.trim();
                                if trimmed.starts_with('(')
                                    && trimmed.ends_with(')')
                                    && trimmed.len() >= 2
                                {
                                    let inner = trimmed[1..trimmed.len() - 1].trim();
                                    if inner.starts_with('"')
                                        && inner.ends_with('"')
                                        && inner.len() >= 2
                                    {
                                        inner[1..inner.len() - 1].to_string()
                                    } else {
                                        inner.to_string()
                                    }
                                } else if trimmed.starts_with('"')
                                    && trimmed.ends_with('"')
                                    && trimmed.len() >= 2
                                {
                                    trimmed[1..trimmed.len() - 1].to_string()
                                } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                                    format!("{}[{}]", f.name.name, &trimmed[1..trimmed.len() - 1])
                                } else {
                                    format!("{}[{}]", f.name.name, trimmed)
                                }
                            }
                            None => f.name.name.clone(),
                        };
                        all_tests.push(TestTarget::Directive {
                            display_name,
                            fn_name: f.name.name.clone(),
                        });
                    }
                }
            }
        }

        let mut tests_to_run = Vec::new();
        for test in all_tests {
            let matches_filter = match filter {
                Some(f) => match &test {
                    TestTarget::Legacy(name) => name.contains(f),
                    TestTarget::Directive {
                        display_name,
                        fn_name,
                    } => display_name.contains(f) || fn_name.contains(f),
                },
                None => true,
            };
            if !matches_filter {
                suite_total_skipped += 1;
                continue;
            }
            tests_to_run.push(test);
        }

        if format == MessageFormat::Human {
            let _ = writeln!(
                out,
                "running {} test{} in {}",
                tests_to_run.len(),
                if tests_to_run.len() == 1 { "" } else { "s" },
                test_file.display()
            );
        } else {
            let _ = writeln!(
                out,
                r#"{{"type":"suite_start","file":"{}","total":{}}}"#,
                test_file.display(),
                tests_to_run.len()
            );
        }

        for test_target in tests_to_run {
            let test_name = match &test_target {
                TestTarget::Legacy(name) => name.clone(),
                TestTarget::Directive { display_name, .. } => display_name.clone(),
            };
            if format == MessageFormat::Jsonl {
                let _ = writeln!(out, r#"{{"type":"test_start","name":"{}"}}"#, test_name);
            }

            let start = std::time::Instant::now();
            aipo_stdlib::time::install_clock(Box::new(aipo_stdlib::time::DeterministicClock {
                seconds: 0.0,
            }));
            aipo_stdlib::random::reset_default_seed(0x1234_5678);

            let (mut vm, _) = standard_environment();
            register_module_symbols(&mut vm, &compiled.module);

            let (run_result, ran) = match &test_target {
                TestTarget::Legacy(name) => {
                    let ran_flag = std::rc::Rc::new(std::cell::Cell::new(false));
                    vm.set_test_mode(aipo_vm::TestMode::Execute {
                        target: name.clone(),
                        ran: ran_flag.clone(),
                    });
                    let res = vm.run(&compiled.module);
                    (res, ran_flag.get())
                }
                TestTarget::Directive { fn_name, .. } => {
                    let top_res = vm.run(&compiled.module);
                    if let Err(err) = top_res {
                        (Err(err), true)
                    } else if let Some(func) = compiled
                        .module
                        .functions
                        .iter()
                        .find(|f| f.name == *fn_name)
                    {
                        let callee = aipo_vm::Value::Function {
                            entry_ip: func.entry_ip as u32,
                            arity: func.params as u16,
                            is_async: func.is_async,
                        };
                        match vm.invoke(&compiled.module, callee, &[]) {
                            Ok(val) => {
                                if let aipo_vm::Value::Failure(f) = val {
                                    (
                                        Err(aipo_vm::VmError::UncaughtFailure(f.message.clone())),
                                        true,
                                    )
                                } else {
                                    (Ok(aipo_vm::Value::None), true)
                                }
                            }
                            Err(err) => (Err(err), true),
                        }
                    } else {
                        (Ok(aipo_vm::Value::None), false)
                    }
                }
            };

            let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

            match run_result {
                Ok(_) if ran => {
                    suite_total_passed += 1;
                    if format == MessageFormat::Human {
                        let _ = writeln!(out, "test {test_name} ... ok ({duration_ms:.2}ms)");
                    } else {
                        let _ = writeln!(
                            out,
                            r#"{{"type":"test_pass","name":"{}","duration_ms":{:.2}}}"#,
                            test_name, duration_ms
                        );
                    }
                }
                Err(VmError::UncaughtFailure(msg)) => {
                    suite_total_failed += 1;
                    if format == MessageFormat::Human {
                        let _ = writeln!(out, "test {test_name} ... FAILED ({duration_ms:.2}ms)");
                        let _ = writeln!(out, "    {msg}");
                    } else {
                        let escaped = msg.replace('"', "\\\"").replace('\n', " ");
                        let _ = writeln!(
                            out,
                            r#"{{"type":"test_fail","name":"{}","duration_ms":{:.2},"error":"{}"}}"#,
                            test_name, duration_ms, escaped
                        );
                    }
                }
                Err(err) => {
                    suite_total_failed += 1;
                    let msg = err.to_string();
                    if format == MessageFormat::Human {
                        let _ = writeln!(out, "test {test_name} ... FAILED ({duration_ms:.2}ms)");
                        let _ = writeln!(out, "    {msg}");
                    } else {
                        let escaped = msg.replace('"', "\\\"").replace('\n', " ");
                        let _ = writeln!(
                            out,
                            r#"{{"type":"test_fail","name":"{}","duration_ms":{:.2},"error":"{}"}}"#,
                            test_name, duration_ms, escaped
                        );
                    }
                }
                Ok(_) => {
                    suite_total_failed += 1;
                    if format == MessageFormat::Human {
                        let _ = writeln!(out, "test {test_name} ... FAILED ({duration_ms:.2}ms)");
                        let _ = writeln!(out, "    test was not executed");
                    } else {
                        let _ = writeln!(
                            out,
                            r#"{{"type":"test_fail","name":"{}","duration_ms":{:.2},"error":"test was not executed"}}"#,
                            test_name, duration_ms
                        );
                    }
                }
            }
        }
    }

    aipo_stdlib::time::install_clock(Box::new(aipo_stdlib::time::SystemClock));

    let total_duration = suite_start.elapsed().as_secs_f64();
    if format == MessageFormat::Human {
        let status = if suite_total_failed == 0 {
            "ok"
        } else {
            "FAILED"
        };
        let _ = writeln!(
            out,
            "\ntest result: {status}. {} passed; {} failed; {} skipped; finished in {:.2}s\n",
            suite_total_passed, suite_total_failed, suite_total_skipped, total_duration
        );
    } else {
        let _ = writeln!(
            out,
            r#"{{"type":"suite_finish","passed":{},"failed":{},"skipped":{},"duration_s":{:.2}}}"#,
            suite_total_passed, suite_total_failed, suite_total_skipped, total_duration
        );
    }

    if suite_total_failed > 0 {
        EXIT_LANGUAGE_FAILURE
    } else {
        EXIT_SUCCESS
    }
}

/// Builds a VM with the standard library registered, plus the registry of its metadata.
pub fn standard_environment() -> (Vm, NativeRegistry) {
    install_host_services();
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    aipo_stdlib::register_stdlib(&mut vm, &mut registry);
    (vm, registry)
}

/// Installs the host services the CLI profile grants.
///
/// The CLI is a real host, so `clock` is granted: `time.now` and `time.monotonic` read the
/// operating system's clocks. This is the one place that decision is made, and a deterministic
/// profile (replay, tests, the Poppy demo) replaces it by calling `install_clock` with its own
/// source, or `revoke_clock` to deny the capability outright. Nothing in the language changes
/// between the two: a denied reading faults with `AIPO_RT_CAPABILITY_DENIED` either way.
fn install_host_services() {
    aipo_stdlib::time::install_clock(Box::new(aipo_stdlib::time::SystemClock));
}

/// Derives the semantic surface from the standard library registration.
///
/// `check` and `run` therefore agree for everything the standard library registers: a
/// global is accepted exactly when the VM that will execute the program can resolve it,
/// and each module's members carry their cataloged arity so a call can be checked.
///
/// The one class this cannot cover is a name that only [`PreludeSurface::fundamental`]
/// declares — `Range` is in that surface but is not a runtime global, so it survives
/// `check` and faults as an undefined global. Closing that gap means either binding
/// `Range` in the VM or dropping it from the language surface, which is an ADP.
pub fn prelude_surface() -> PreludeSurface {
    let (vm, registry) = standard_environment();
    let mut surface = PreludeSurface::fundamental();

    for name in vm.globals.keys() {
        match registry.get(None, name) {
            Some(meta) => surface.add_function(name, meta.arity, meta.arity),
            None => surface.add_variable(name),
        }
    }

    // A module global also carries the members of the dictionary the VM exposes.
    // Without this, `check` accepts *any* `modulo.membro` — a typo like
    // `math.nope(1.0)` passes the frontend and only faults at runtime with
    // `AIPO_RT_KEY_NOT_FOUND`, and a member that exists in the documentation but
    // not in the module is indistinguishable from a real one until execution.
    //
    // Arity comes from the same catalog the VM is built from. Parameter types are
    // not cataloged, so every parameter carries the permissive `Value?` contract:
    // arity and member existence are checked, argument *types* are not invented.
    //
    // An entry whose native leaves arity unchecked (`Value::native(.., usize::MAX, ..)`)
    // accepts an open tail, so its cataloged arity is a lower bound.
    for module in registry.list_modules() {
        let functions: HashMap<String, HostFunction> = registry
            .list_module(&module)
            .into_iter()
            .map(|meta| {
                let params = (0..meta.arity)
                    .map(|index| HostParameter {
                        name: format!("arg{index}"),
                        ty: "Value".to_string(),
                        nullable: true,
                        optional: false,
                    })
                    .collect();
                (
                    meta.name.clone(),
                    HostFunction {
                        params,
                        variadic: meta.is_variadic,
                    },
                )
            })
            .collect();
        if !functions.is_empty() {
            surface.add_host_module(&module, functions);
        }
    }

    // Built-in receiver types expose methods the same way a module exposes members:
    // through a registration the runtime dispatches. Deriving the member set from that
    // registration makes a typo like `xs.nope()` a frontend error instead of a runtime
    // fault, and keeps a type with no hand-written list from accepting any name.
    for type_name in vm.registered_method_types() {
        surface.add_type_members(&type_name, vm.registered_methods(&type_name));
    }
    // Higher-order `List` methods are dispatched by the VM rather than registered as
    // natives, so they need to be added to the catalog explicitly or the member check
    // would reject a method the runtime supports.
    let higher_order = aipo_vm::Vm::LIST_HIGHER_ORDER_METHODS
        .iter()
        .copied()
        .chain(std::iter::once(aipo_vm::Vm::LIST_FOLD_METHOD))
        .map(str::to_string);
    surface.add_type_members("List", higher_order);

    surface
}

fn runtime_diagnostic(source: &Source, error: &VmError) -> Diagnostic {
    Diagnostic::error(error.diagnostic_code(), error.to_string())
        .with_note(format!("while running {}", source.name()))
}

/// Emits diagnostics in the requested format.
///
/// Human diagnostics go to standard error, matching compiler convention; JSONL keeps the
/// schema documented in `docs/reference/cli.md`.
pub fn emit_diagnostics(
    format: MessageFormat,
    source: &Source,
    diagnostics: &[Diagnostic],
    out: &mut dyn Write,
    err: &mut dyn Write,
) {
    if diagnostics.is_empty() {
        return;
    }

    let mut map = SourceMap::new();
    map.add_source(source.name(), source.text());
    let emitter = DiagnosticEmitter::new(format, Some(&map));
    let _ = emitter.emit_all(err, diagnostics);
    let _ = out.flush();
}

/// Compiles an Aipo source file down to verified bytecode, resolving local packages and imports.
///
/// # Errors
/// Returns the source and diagnostics if resolution, parsing, semantic analysis, or bytecode verification fails.
pub fn compile_file(
    path: &Path,
    extra_surface: Option<&PreludeSurface>,
) -> Result<(Source, aipo_bytecode::BytecodeModule), (Source, Vec<Diagnostic>)> {
    let loaded = match load_source_entry(path, None) {
        Ok(l) => l,
        Err(err) => {
            let src = Source::new(SourceId::next(), path.display().to_string(), "");
            let diag = match err {
                CliError::Usage(msg) => Diagnostic::error(DiagnosticCode::AIPO_PKG_RESOLUTION, msg),
                CliError::Diagnostic(d) => *d,
            };
            return Err((src, vec![diag]));
        }
    };

    let compiled = analyze_with_surface(
        &loaded.source,
        path,
        loaded.package_paths.as_ref(),
        extra_surface,
    );
    if compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        Err((loaded.source, compiled.diagnostics))
    } else {
        Ok((loaded.source, compiled.module))
    }
}

/// Formats files in place, or verifies that they are already canonical.
#[cfg(feature = "formatter")]
fn format_files(paths: &[PathBuf], check: bool, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let mut exit = EXIT_SUCCESS;
    let mut changed = 0usize;

    for path in paths {
        let text = match read_source(path) {
            Ok(text) => text,
            Err(message) => {
                let _ = writeln!(err, "error: {message}");
                return EXIT_USAGE;
            }
        };

        let formatted = match aipo_formatter::format_text(&path.display().to_string(), &text) {
            Ok(formatted) => formatted,
            Err(error) => {
                let _ = writeln!(err, "error: {}: {error}", path.display());
                exit = EXIT_LANGUAGE_FAILURE;
                continue;
            }
        };

        if formatted == text {
            continue;
        }

        changed += 1;
        if check {
            let _ = writeln!(err, "would reformat {}", path.display());
            exit = EXIT_LANGUAGE_FAILURE;
        } else if let Err(error) = std::fs::write(path, &formatted) {
            let _ = writeln!(err, "error: {}: {error}", path.display());
            exit = EXIT_USAGE;
        }
    }

    if !check {
        let verb = if changed == 1 { "file" } else { "files" };
        let _ = writeln!(out, "formatted {changed} {verb}");
    }
    exit
}

#[cfg(not(feature = "formatter"))]
fn format_files(_paths: &[PathBuf], _check: bool, _out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let _ = writeln!(err, "error: code formatter is disabled in this build");
    EXIT_USAGE
}

fn load_source_entry(path: &Path, package_cache: Option<&Path>) -> Result<LoadedSource, CliError> {
    let text = read_source(path).map_err(CliError::Usage)?;
    let package_paths = resolve_local_package_paths(path, package_cache, Some(text.as_bytes()))?;
    let source = Source::new(SourceId::next(), path.display().to_string(), &text);
    Ok(LoadedSource {
        source,
        package_paths,
    })
}

fn resolve_local_package_paths(
    entry: &Path,
    package_cache: Option<&Path>,
    entry_bytes: Option<&[u8]>,
) -> Result<Option<PackagePathMap>, CliError> {
    let Some(manifest_path) = find_ancestor_manifest(entry) else {
        if package_cache.is_some() {
            return Err(CliError::Usage(
                "--package-cache requires a nearby aipo.toml".to_string(),
            ));
        }
        return Ok(None);
    };
    let package_root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    if let Some(cache_dir) = package_cache {
        return resolve_cached_package_paths(
            entry,
            &manifest_path,
            package_root,
            cache_dir,
            entry_bytes,
        )
        .map(Some);
    }
    let resolved = resolve_local_package(package_root, &manifest_path)?;
    let lock_path = package_root.join(LOCK_FILE_NAME);
    validate_existing_lock(&lock_path, &resolved.graph, false)?;
    Ok(Some(resolved.paths))
}

fn resolve_cached_package_paths(
    entry: &Path,
    manifest_path: &Path,
    package_root: &Path,
    cache_dir: &Path,
    entry_bytes: Option<&[u8]>,
) -> Result<PackagePathMap, CliError> {
    let lock_path = package_root.join(LOCK_FILE_NAME);
    let lock_bytes = match std::fs::read(&lock_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(CliError::diagnostic(
                DiagnosticCode::AIPO_PKG_LOCK_STALE,
                format!(
                    "lockfile '{}' is required with --package-cache",
                    lock_path.display()
                ),
            ));
        }
        Err(error) => return Err(CliError::Usage(format!("{}: {error}", lock_path.display()))),
    };
    let lockfile = Lockfile::from_bytes(&lock_bytes).map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_LOCK_STALE,
            format!("lockfile '{}' is invalid: {error}", lock_path.display()),
        )
    })?;
    let manifest_bytes = std::fs::read(manifest_path)
        .map_err(|error| CliError::Usage(format!("{}: {error}", manifest_path.display())))?;
    let manifest = Manifest::from_bytes(&manifest_bytes).map_err(|error| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_RESOLUTION,
            format!("{}: {error}", manifest_path.display()),
        )
    })?;
    let locked_root = lockfile.package(manifest.coordinate()).ok_or_else(|| {
        CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_LOCK_STALE,
            format!(
                "lockfile '{}' does not contain root package '{}'",
                lock_path.display(),
                manifest.name
            ),
        )
    })?;
    if matches!(
        &locked_root.source,
        aipo_package::PackageSource::GitHub { .. }
    ) && manifest
        .dependencies
        .iter()
        .any(|dependency| matches!(&dependency.source, aipo_package::PackageSource::Path { .. }))
    {
        return Err(CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_RESOLUTION,
            "a remote package root cannot declare local path dependencies",
        ));
    }
    let discovered = LocalPackageResolver::new(package_root)
        .discover_local()
        .map_err(|error| classify_package_error(error, manifest_path))?;
    let entry_bytes = entry_bytes.ok_or_else(|| {
        CliError::Usage("cached package resolution requires entry bytes".to_string())
    })?;
    let root_coordinate = manifest.name.clone();
    let root_input = discovered
        .inputs
        .get(&root_coordinate)
        .cloned()
        .ok_or_else(|| {
            CliError::diagnostic(
                DiagnosticCode::AIPO_PKG_RESOLUTION,
                format!("root package '{}' was not discovered", root_coordinate),
            )
        })?;
    let expected_entry = discovered
        .paths
        .get(&root_coordinate)
        .cloned()
        .ok_or_else(|| {
            CliError::diagnostic(
                DiagnosticCode::AIPO_PKG_RESOLUTION,
                format!("root package '{}' has no entry path", root_coordinate),
            )
        })?;
    let actual_entry = std::fs::canonicalize(entry)
        .map_err(|error| CliError::Usage(format!("{}: {error}", entry.display())))?;
    if std::fs::canonicalize(&expected_entry).ok().as_ref() != Some(&actual_entry)
        || root_input.entry_bytes != entry_bytes
    {
        return Err(CliError::diagnostic(
            DiagnosticCode::AIPO_PKG_LOCK_STALE,
            format!(
                "entry '{}' does not match the verified package manifest and entry",
                entry.display()
            ),
        ));
    }
    let root_input = root_input.with_source(locked_root.source.clone());
    let mut local_inputs = discovered.inputs;
    local_inputs.insert(root_coordinate.clone(), root_input.clone());
    let cache = GitHubCache::new(cache_dir);
    let cache_only = CacheOnlyGitHubFetcher::new(&cache);
    let resolved = resolve_mixed_package_graph(
        root_input,
        local_inputs,
        discovered.paths,
        &cache,
        &cache_only,
    )
    .map_err(classify_cached_graph_error)?;
    validate_existing_lock(&lock_path, &resolved.graph, true)?;
    Ok(resolved.paths)
}

fn classify_cached_graph_error(error: CachedGraphError) -> CliError {
    match error {
        CachedGraphError::Cache(_) | CachedGraphError::Fetch(_) => {
            CliError::diagnostic(DiagnosticCode::AIPO_PKG_FETCH, error.to_string())
        }
        CachedGraphError::Resolve(_) => {
            CliError::diagnostic(DiagnosticCode::AIPO_PKG_RESOLUTION, error.to_string())
        }
    }
}

fn find_ancestor_manifest(entry: &Path) -> Option<PathBuf> {
    entry
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .ancestors()
        .map(|ancestor| ancestor.join(MANIFEST_FILE_NAME))
        .find(|manifest| manifest.is_file())
}

fn read_source(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}
