use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aipo_testkit::pipeline;

const MANIFEST_JSON: &str = include_str!("../comparisons/manifest.json");
const REPORT_SCHEMA_VERSION: u32 = 5;
const MAX_COMPARE_N: u64 = 1_000_000;
const MAX_COMPARE_ROUNDS: usize = 31;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PROCESS_OUTPUT_BYTES: u64 = 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    workloads: Vec<WorkloadSpec>,
    languages: Vec<LanguageSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkloadSpec {
    id: String,
    description: String,
    n: u64,
    checksum: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LanguageSpec {
    id: String,
    label: String,
    kind: String,
    command: Option<String>,
    #[serde(default)]
    command_candidates: Vec<String>,
    #[serde(default)]
    program_args: bool,
    script: Option<String>,
    required: bool,
}

#[derive(Debug)]
struct RunOptions {
    rounds: usize,
    n_overrides: BTreeMap<String, u64>,
    json_path: Option<PathBuf>,
    aipo_bin: Option<PathBuf>,
    quick: bool,
    languages: Option<Vec<String>>,
    workloads: Option<Vec<String>>,
    resources: bool,
}

#[derive(Debug, Serialize)]
struct ComparisonReport {
    schema_version: u32,
    generated_unix_seconds: u64,
    manifest_schema: u32,
    environment: Environment,
    config: Config,
    results: Vec<ComparisonResult>,
    skipped: Vec<Skipped>,
    failures: Vec<Failure>,
}

#[derive(Debug, Serialize)]
struct Environment {
    os: String,
    arch: String,
    cpu_model: Option<String>,
    available_parallelism: Option<usize>,
    build_profile: String,
    debug_assertions: bool,
    rustc: String,
    git_commit: Option<String>,
    git_dirty: Option<bool>,
    runtimes: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct Config {
    rounds: usize,
    quick: bool,
    n_overrides: BTreeMap<String, u64>,
    aipo_bin: Option<String>,
    aipo_bin_profile: Option<String>,
    aipo_bin_sha256: Option<String>,
    benchmark_binary_sha256: Option<String>,
    languages: Option<Vec<String>>,
    workloads: Option<Vec<String>>,
    resources: bool,
    comparison_policy: String,
}

#[derive(Debug, Serialize)]
struct ComparisonResult {
    language_id: String,
    language: String,
    kind: String,
    runtime_version: String,
    workload: String,
    mode: String,
    n: u64,
    expected_checksum: String,
    observed_checksum: String,
    median_ns: u64,
    mad_ns: u64,
    min_ns: u64,
    p95_ns: u64,
    max_ns: u64,
    raw_samples_ns: Vec<u64>,
    operations: u64,
    operations_per_sec: f64,
    includes_startup: bool,
    includes_compile: bool,
    setup_ns: u64,
    runtime_setup_ns: Option<u64>,
    metrics: Option<MetricsReport>,
    resources: Option<ResourceMetricsReport>,
}

#[derive(Debug, Serialize)]
struct MetricsReport {
    instructions: u64,
    calls: u64,
    function_calls: u64,
    native_calls: u64,
    bound_method_calls: u64,
    field_lookups: u64,
    field_hits: u64,
    field_cache_hits: u64,
    field_cache_misses: u64,
    global_lookups: u64,
    global_hits: u64,
    constant_loads: u64,
    local_clones: u64,
    call_argument_copies: u64,
}

#[derive(Debug, Serialize)]
struct ResourceMetricsReport {
    peak_rss_bytes: Option<u64>,
    allocation_count: Option<u64>,
    allocated_bytes: Option<u64>,
    scope: String,
    method: String,
}

impl From<pipeline::VmMetrics> for MetricsReport {
    fn from(metrics: pipeline::VmMetrics) -> Self {
        Self {
            instructions: metrics.instructions,
            calls: metrics.calls,
            function_calls: metrics.function_calls,
            native_calls: metrics.native_calls,
            bound_method_calls: metrics.bound_method_calls,
            field_lookups: metrics.field_lookups,
            field_hits: metrics.field_hits,
            field_cache_hits: metrics.field_cache_hits,
            field_cache_misses: metrics.field_cache_misses,
            global_lookups: metrics.global_lookups,
            global_hits: metrics.global_hits,
            constant_loads: metrics.constant_loads,
            local_clones: metrics.local_clones,
            call_argument_copies: metrics.call_argument_copies,
        }
    }
}

#[derive(Debug, Serialize)]
struct Skipped {
    language_id: String,
    language: String,
    workload: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct Failure {
    language_id: String,
    language: String,
    workload: String,
    reason: String,
}

struct ExecutionConfig<'a> {
    root: &'a Path,
    aipo_bin: Option<&'a Path>,
    rounds: usize,
    collect_resources: bool,
}

struct PreparedInvocation {
    program: Option<PathBuf>,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    invocation: InvocationKind,
    cleanup: Option<PathBuf>,
    includes_startup: bool,
    includes_compile: bool,
    setup_ns: u64,
}

impl Drop for PreparedInvocation {
    fn drop(&mut self) {
        if let Some(path) = &self.cleanup {
            let _ = fs::remove_dir_all(path);
        }
    }
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
    peak_rss_bytes: Option<u64>,
}

struct PeakRssSampler {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl PeakRssSampler {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let thread_stop = Arc::clone(&stop);
        let thread_peak = Arc::clone(&peak);
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                if let Some(rss) = read_process_rss(std::process::id()) {
                    thread_peak.fetch_max(rss, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        Self {
            stop,
            peak,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> Option<u64> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let peak = self.peak.load(Ordering::Relaxed);
        (peak > 0).then_some(peak)
    }
}

impl Drop for PeakRssSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

enum InvocationKind {
    Process,
    AipoVm(aipo_bytecode::BytecodeModule),
}

struct ExecutionSample {
    elapsed: Duration,
    checksum: String,
    runtime_setup: Option<Duration>,
    metrics: Option<pipeline::VmMetrics>,
    resources: Option<ResourceMetricsReport>,
}

struct Measurement {
    raw_samples_ns: Vec<u64>,
    checksum: String,
    setup_ns: u64,
    runtime_setup_ns: Option<u64>,
    includes_startup: bool,
    includes_compile: bool,
    metrics: Option<pipeline::VmMetrics>,
    resources: Option<ResourceMetricsReport>,
}

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn is_requested(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--compare"
            || arg == "--compare-json"
            || arg == "--compare-runs"
            || arg == "--compare-n"
            || arg == "--compare-languages"
            || arg == "--compare-workloads"
            || arg == "--compare-resources"
            || arg == "--compare-quick"
            || arg == "--aipo-bin"
    })
}

pub(crate) fn is_worker_request(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--compare-worker")
}

pub(crate) fn run_rust_worker(args: &[String]) -> i32 {
    let Some(workload_index) = args.iter().position(|arg| arg == "--compare-worker") else {
        return 2;
    };
    let Some(workload) = args.get(workload_index + 1) else {
        return 2;
    };
    let Some(n) = args
        .get(workload_index + 2)
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return 2;
    };
    let checksum = match rust_checksum(workload, n) {
        Ok(checksum) => checksum,
        Err(_) => return 2,
    };
    println!("checksum:{checksum}");
    0
}

pub(crate) fn run(args: &[String]) -> i32 {
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!(
                "usage: aipo-bench --compare [--compare-json <path>] [--compare-runs <n>] [--compare-n <workload=N,...>] [--compare-languages <ids>] [--compare-workloads <ids>] [--compare-resources]"
            );
            return 2;
        }
    };
    let manifest: Manifest = match serde_json::from_str(MANIFEST_JSON) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("error: invalid comparison manifest: {error}");
            return 2;
        }
    };
    if let Err(error) = validate_manifest(&manifest) {
        eprintln!("error: invalid comparison manifest: {error}");
        return 2;
    }
    if let Err(error) =
        validate_language_selection(options.languages.as_deref(), &manifest.languages)
    {
        eprintln!("error: {error}");
        return 2;
    }
    if let Err(error) =
        validate_workload_selection(options.workloads.as_deref(), &manifest.workloads)
    {
        eprintln!("error: {error}");
        return 2;
    }
    for workload_id in options.n_overrides.keys() {
        if !manifest
            .workloads
            .iter()
            .any(|workload| workload.id == *workload_id)
        {
            eprintln!("error: unknown workload '{workload_id}' in --compare-n");
            return 2;
        }
    }
    for workload in &manifest.workloads {
        if !is_supported_workload(&workload.id) {
            eprintln!("error: unsupported workload '{}'", workload.id);
            return 2;
        }
        if let Err(error) = validate_n(&workload.id, workload.n) {
            eprintln!(
                "error: invalid manifest workload '{}': {error}",
                workload.id
            );
            return 2;
        }
        let expected = expected_checksum(&workload.id, workload.n);
        if workload.checksum != expected {
            eprintln!(
                "error: manifest checksum mismatch for {}: expected {}, got {}",
                workload.id, expected, workload.checksum
            );
            return 2;
        }
    }
    let aipo_bin = match resolve_aipo_bin(options.aipo_bin.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    let root = match create_temp_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };

    let runtimes = runtime_versions(&manifest.languages, aipo_bin.as_deref());
    let mut results = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    let aipo_bin_profile = aipo_bin.as_deref().map(binary_profile);
    let aipo_bin_sha256 = aipo_bin.as_deref().and_then(file_sha256);
    let benchmark_binary_sha256 = std::env::current_exe()
        .ok()
        .and_then(|path| file_sha256(&path));
    let selected_languages = options.languages.as_deref();
    let selected_workloads = options.workloads.as_deref();
    let workloads = manifest
        .workloads
        .iter()
        .filter(|workload| workload.id != "startup" || !options.quick)
        .filter(|workload| {
            selected_workloads.is_none_or(|selected| selected.iter().any(|id| id == &workload.id))
        })
        .collect::<Vec<_>>();

    println!("== cross-language comparison ==");
    println!(
        "rounds={} quick={} manifest_schema={} ",
        options.rounds, options.quick, manifest.schema_version
    );
    println!(
        "aipo_bin={} ",
        aipo_bin.as_ref().map_or_else(
            || "<in-process>".to_string(),
            |path| path.display().to_string()
        )
    );

    for workload in workloads {
        let n = workload_n(workload, &options.n_overrides);
        if let Err(error) = validate_n(&workload.id, n) {
            eprintln!("error: invalid workload '{}': {error}", workload.id);
            return 2;
        }
        let expected = expected_checksum(&workload.id, n);
        println!(
            "\n-- workload={} ({}) n={} expected={} --",
            workload.id, workload.description, n, expected
        );
        for language in &manifest.languages {
            if !language_selected(language, selected_languages) {
                continue;
            }
            let runtime = runtimes.get(&language.id).cloned().unwrap_or_default();
            let config = ExecutionConfig {
                root: &root.0,
                aipo_bin: aipo_bin.as_deref(),
                rounds: options.rounds,
                collect_resources: options.resources,
            };
            match execute_language(language, workload, n, &expected, &config) {
                Ok(measurement) => {
                    let (median_ns, mad_ns, min_ns, p95_ns, max_ns) =
                        summarize(&measurement.raw_samples_ns);
                    let operations = operation_count(&workload.id, n);
                    let operations_per_sec = operations as f64
                        / (median_ns as f64 / 1_000_000_000.0).max(f64::MIN_POSITIVE);
                    let record = ComparisonResult {
                        language_id: language.id.clone(),
                        language: language.label.clone(),
                        kind: language.kind.clone(),
                        runtime_version: runtime,
                        workload: workload.id.clone(),
                        mode: measurement_mode(language).to_string(),
                        n,
                        expected_checksum: expected.clone(),
                        observed_checksum: measurement.checksum,
                        median_ns,
                        mad_ns,
                        min_ns,
                        p95_ns,
                        max_ns,
                        raw_samples_ns: measurement.raw_samples_ns,
                        operations,
                        operations_per_sec,
                        includes_startup: measurement.includes_startup,
                        includes_compile: measurement.includes_compile,
                        setup_ns: measurement.setup_ns,
                        runtime_setup_ns: measurement.runtime_setup_ns,
                        metrics: measurement.metrics.map(MetricsReport::from),
                        resources: measurement.resources,
                    };
                    println!(
                        "{:<18} {:>12} MAD={:<10} p95={:<12} checksum={}",
                        record.language,
                        format_duration(Duration::from_nanos(record.median_ns)),
                        format_duration(Duration::from_nanos(record.mad_ns)),
                        format_duration(Duration::from_nanos(record.p95_ns)),
                        record.observed_checksum
                    );
                    results.push(record);
                }
                Err(error) => {
                    if language_available(language, aipo_bin.as_deref()) {
                        eprintln!("{}: {}", language.label, error);
                        failures.push(Failure {
                            language_id: language.id.clone(),
                            language: language.label.clone(),
                            workload: workload.id.clone(),
                            reason: error,
                        });
                    } else if language.required {
                        failures.push(Failure {
                            language_id: language.id.clone(),
                            language: language.label.clone(),
                            workload: workload.id.clone(),
                            reason: error,
                        });
                    } else {
                        skipped.push(Skipped {
                            language_id: language.id.clone(),
                            language: language.label.clone(),
                            workload: workload.id.clone(),
                            reason: error,
                        });
                    }
                }
            }
        }
    }

    let report = ComparisonReport {
        schema_version: REPORT_SCHEMA_VERSION,
        generated_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        manifest_schema: manifest.schema_version,
        environment: Environment {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_model: cpu_model(),
            available_parallelism: std::thread::available_parallelism().ok().map(|value| value.get()),
            build_profile: build_profile().to_string(),
            debug_assertions: cfg!(debug_assertions),
            rustc: command_version("rustc").unwrap_or_else(|| "unknown".to_string()),
            git_commit: command_output("git", &["rev-parse", "--short", "HEAD"]),
            git_dirty: command_output("git", &["status", "--porcelain"])
                .map(|status| !status.is_empty()),
            runtimes,
        },
        config: Config {
            rounds: options.rounds,
            quick: options.quick,
            n_overrides: options.n_overrides,
            aipo_bin: aipo_bin.map(|path| path.display().to_string()),
            aipo_bin_profile,
            aipo_bin_sha256,
            benchmark_binary_sha256,
            languages: options.languages,
            workloads: options.workloads,
            resources: options.resources,
            comparison_policy: "process timings include interpreter/runtime startup; Aipo VM in-process and Aipo->JavaScript are reported separately; Wren, Luau and PyPy are optional reference runtimes; Rust native is a control; peak RSS is sampled only with --compare-resources in a separate pass and allocation counters remain runtime-specific".to_string(),
        },
        results,
        skipped,
        failures,
    };

    if let Some(path) = options.json_path {
        if let Err(error) = write_report(&path, &report) {
            eprintln!("error: could not write comparison report: {error}");
            return 2;
        }
        println!("\ncomparison report written to {}", path.display());
    }
    println!(
        "\nsummary: {} results, {} skipped, {} failures",
        report.results.len(),
        report.skipped.len(),
        report.failures.len()
    );
    if report.failures.is_empty() { 0 } else { 1 }
}

fn parse_options(args: &[String]) -> Result<RunOptions, String> {
    let quick = args
        .iter()
        .any(|arg| arg == "--quick" || arg == "--compare-quick");
    let mut rounds = if quick { 3 } else { 5 };
    let mut n_overrides = BTreeMap::new();
    let mut json_path = None;
    let mut aipo_bin = None;
    let mut languages = None;
    let mut workloads = None;
    let mut resources = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--compare" | "--compare-quick" | "--quick" => {}
            "--compare-resources" => resources = true,
            "--compare-json" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--compare-json' requires a path".to_string())?;
                json_path = Some(PathBuf::from(value));
            }
            "--compare-runs" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--compare-runs' requires a positive integer".to_string())?;
                rounds = value
                    .parse::<usize>()
                    .ok()
                    .filter(|value| *value > 0 && *value <= MAX_COMPARE_ROUNDS)
                    .ok_or_else(|| {
                        format!(
                            "'--compare-runs' requires an integer between 1 and {MAX_COMPARE_ROUNDS}"
                        )
                    })?;
            }
            "--compare-n" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--compare-n' requires workload=N[,workload=N]".to_string())?;
                for item in value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                {
                    let (workload, n) = item.split_once('=').ok_or_else(|| {
                        "'--compare-n' entries must use workload=N syntax".to_string()
                    })?;
                    let n = n
                        .parse::<u64>()
                        .ok()
                        .filter(|value| *value > 0 && *value <= MAX_COMPARE_N)
                        .ok_or_else(|| {
                            format!(
                                "'--compare-n' requires workload=N with N between 1 and {MAX_COMPARE_N}"
                            )
                        })?;
                    if n_overrides.insert(workload.to_string(), n).is_some() {
                        return Err(format!("duplicate N override for workload '{workload}'"));
                    }
                }
                if n_overrides.is_empty() {
                    return Err("'--compare-n' requires at least one workload=N entry".to_string());
                }
            }
            "--aipo-bin" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "'--aipo-bin' requires a path".to_string())?;
                aipo_bin = Some(PathBuf::from(value));
            }
            "--compare-languages" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    "'--compare-languages' requires a comma-separated list".to_string()
                })?;
                let selected = value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if selected.is_empty() {
                    return Err("'--compare-languages' requires at least one language".to_string());
                }
                languages = Some(selected);
            }
            "--compare-workloads" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    "'--compare-workloads' requires a comma-separated list".to_string()
                })?;
                let selected = value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if selected.is_empty() {
                    return Err("'--compare-workloads' requires at least one workload".to_string());
                }
                workloads = Some(selected);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown comparison option '{other}'"));
            }
            other => return Err(format!("unexpected comparison argument '{other}'")),
        }
        index += 1;
    }
    Ok(RunOptions {
        rounds,
        n_overrides,
        json_path,
        aipo_bin,
        quick,
        languages,
        workloads,
        resources,
    })
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    let mut language_ids = BTreeSet::new();
    for language in &manifest.languages {
        if !language_ids.insert(language.id.as_str()) {
            return Err(format!("duplicate language id '{}'", language.id));
        }
        if !matches!(
            language.kind.as_str(),
            "aipo" | "aipo-reg" | "aipo-wasm" | "aipo-vm" | "aipo-js" | "script" | "self"
        ) {
            return Err(format!(
                "unsupported language kind '{}' for '{}'",
                language.kind, language.id
            ));
        }
        if language.kind == "script" {
            if language_commands(language).is_empty() {
                return Err(format!("script language '{}' has no command", language.id));
            }
            let script = language
                .script
                .as_deref()
                .ok_or_else(|| format!("script language '{}' has no script path", language.id))?;
            for workload in &manifest.workloads {
                let path = comparison_root().join(script.replace("{workload}", &workload.id));
                if !path.is_file() {
                    return Err(format!(
                        "missing script '{}' for language '{}'",
                        path.display(),
                        language.id
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_language_selection(
    selected: Option<&[String]>,
    languages: &[LanguageSpec],
) -> Result<(), String> {
    let Some(selected) = selected else {
        return Ok(());
    };
    if selected.is_empty() {
        return Err("at least one language must be selected".to_string());
    }
    let known = languages
        .iter()
        .map(|language| language.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for id in selected {
        if !known.contains(id.as_str()) {
            return Err(format!(
                "unknown language '{id}'; available languages: {}",
                known.iter().copied().collect::<Vec<_>>().join(", ")
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!("language '{id}' was selected more than once"));
        }
    }
    Ok(())
}

fn validate_workload_selection(
    selected: Option<&[String]>,
    workloads: &[WorkloadSpec],
) -> Result<(), String> {
    let Some(selected) = selected else {
        return Ok(());
    };
    if selected.is_empty() {
        return Err("at least one workload must be selected".to_string());
    }
    let known = workloads
        .iter()
        .map(|workload| workload.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for id in selected {
        if !known.contains(id.as_str()) {
            return Err(format!(
                "unknown workload '{id}'; available workloads: {}",
                known.iter().copied().collect::<Vec<_>>().join(", ")
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!("workload '{id}' was selected more than once"));
        }
    }
    Ok(())
}

fn is_supported_workload(workload: &str) -> bool {
    matches!(
        workload,
        "arithmetic" | "collections" | "fields" | "strings" | "recursion" | "startup"
    )
}

fn validate_n(workload: &str, n: u64) -> Result<(), String> {
    if n > MAX_COMPARE_N {
        return Err(format!(
            "n={n} exceeds the comparison limit of {MAX_COMPARE_N}"
        ));
    }
    if matches!(workload, "recursion" | "startup") {
        if n != 0 {
            return Err(format!("{workload} is a fixed workload and requires n=0"));
        }
    } else if n == 0 {
        return Err("n must be positive for this workload".to_string());
    }
    Ok(())
}

fn measurement_mode(language: &LanguageSpec) -> &'static str {
    if language.kind == "aipo-vm" {
        "in-process"
    } else {
        "process"
    }
}

fn workload_n(workload: &WorkloadSpec, overrides: &BTreeMap<String, u64>) -> u64 {
    if workload.n == 0 {
        0
    } else {
        overrides.get(&workload.id).copied().unwrap_or(workload.n)
    }
}

fn language_selected(language: &LanguageSpec, selected: Option<&[String]>) -> bool {
    selected.is_none_or(|values| values.iter().any(|value| value == &language.id))
}

fn language_commands(language: &LanguageSpec) -> Vec<&str> {
    let mut commands = Vec::new();
    if let Some(command) = language.command.as_deref() {
        commands.push(command);
    }
    commands.extend(language.command_candidates.iter().map(String::as_str));
    commands
}

fn resolve_language_command(language: &LanguageSpec) -> Option<(String, PathBuf)> {
    language_commands(language)
        .into_iter()
        .find_map(|command| command_path(command).map(|path| (command.to_string(), path)))
}

fn language_available(language: &LanguageSpec, aipo_bin: Option<&Path>) -> bool {
    match language.kind.as_str() {
        "aipo" | "aipo-reg" | "aipo-wasm" => aipo_bin.is_some_and(|path| path.is_file()),
        "aipo-js" => command_path("node").is_some(),
        "aipo-vm" | "self" => true,
        "script" => resolve_language_command(language).is_some(),
        _ => false,
    }
}

fn execute_language(
    language: &LanguageSpec,
    workload: &WorkloadSpec,
    n: u64,
    expected: &str,
    config: &ExecutionConfig<'_>,
) -> Result<Measurement, String> {
    if !language_available(language, config.aipo_bin) {
        return Err(format!("runtime unavailable ({})", language.kind));
    }
    let prepared = prepare_invocation(language, workload, n, config.root, config.aipo_bin)?;
    let _ = execute_once(&prepared, expected, false, false)?;
    let mut raw_samples_ns = Vec::with_capacity(config.rounds);
    let mut runtime_setup_ns = Vec::new();
    for _ in 0..config.rounds {
        let sample = execute_once(&prepared, expected, false, false)?;
        debug_assert_eq!(sample.checksum, expected);
        raw_samples_ns.push(duration_ns(sample.elapsed));
        if let Some(setup) = sample.runtime_setup {
            runtime_setup_ns.push(duration_ns(setup));
        }
    }
    let (metrics, resources) = if language.kind == "aipo-vm" {
        let sample = execute_once(&prepared, expected, true, config.collect_resources)?;
        let metrics = Some(
            sample
                .metrics
                .ok_or_else(|| "Aipo VM metrics were not collected".to_string())?,
        );
        (metrics, sample.resources)
    } else if config.collect_resources && matches!(&prepared.invocation, InvocationKind::Process) {
        let sample = execute_once(&prepared, expected, true, true)?;
        (None, sample.resources)
    } else {
        (None, None)
    };
    let runtime_setup_ns = (!runtime_setup_ns.is_empty()).then(|| summarize(&runtime_setup_ns).0);
    Ok(Measurement {
        checksum: expected.to_string(),
        raw_samples_ns,
        setup_ns: prepared.setup_ns,
        runtime_setup_ns,
        includes_startup: prepared.includes_startup,
        includes_compile: prepared.includes_compile,
        metrics,
        resources,
    })
}

fn emit_comparison_bundle(
    source: &str,
    module: &aipo_ir::CoreModule,
    tag: &str,
) -> Result<PathBuf, String> {
    let bundle = aipo_js::emit_js("comparison.aipo", source, module);
    let dir = create_temp_dir(&format!("aipo-testkit-{tag}"))?;
    let files = [
        ("package.json", "{\"type\": \"module\"}\n".to_string()),
        ("app.js", bundle.app_js),
        ("aipo-runtime.js", bundle.runtime_js),
        ("app.js.map", bundle.source_map),
    ];
    for (name, contents) in files {
        if let Err(error) = fs::write(dir.join(name), contents) {
            let _ = fs::remove_dir_all(&dir);
            return Err(format!("could not write comparison bundle: {error}"));
        }
    }
    Ok(dir)
}

fn prepare_invocation(
    language: &LanguageSpec,
    workload: &WorkloadSpec,
    n: u64,
    root: &Path,
    aipo_bin: Option<&Path>,
) -> Result<PreparedInvocation, String> {
    let setup_start = Instant::now();
    match language.kind.as_str() {
        "aipo" => {
            let aipo_bin = aipo_bin.ok_or_else(|| "Aipo binary is unavailable".to_string())?;
            let source = comparison_source(language, &workload.id, n)?;
            let path = root.join(format!("aipo-{}.aipo", workload.id));
            fs::write(&path, source).map_err(|error| error.to_string())?;
            Ok(PreparedInvocation {
                program: Some(aipo_bin.to_path_buf()),
                args: vec!["run".to_string(), path.display().to_string()],
                current_dir: None,
                invocation: InvocationKind::Process,
                cleanup: None,
                includes_startup: true,
                includes_compile: true,
                setup_ns: duration_ns(setup_start.elapsed()),
            })
        }
        "aipo-reg" => {
            let aipo_bin = aipo_bin.ok_or_else(|| "Aipo binary is unavailable".to_string())?;
            let source = comparison_source(language, &workload.id, n)?;
            let path = root.join(format!("aipo-reg-{}.aipo", workload.id));
            fs::write(&path, source).map_err(|error| error.to_string())?;
            Ok(PreparedInvocation {
                program: Some(aipo_bin.to_path_buf()),
                args: vec![
                    "run".to_string(),
                    path.display().to_string(),
                    "--engine=reg".to_string(),
                ],
                current_dir: None,
                invocation: InvocationKind::Process,
                cleanup: None,
                includes_startup: true,
                includes_compile: true,
                setup_ns: duration_ns(setup_start.elapsed()),
            })
        }
        "aipo-wasm" => {
            let aipo_bin = aipo_bin.ok_or_else(|| "Aipo binary is unavailable".to_string())?;
            let source = comparison_source(language, &workload.id, n)?;
            let path = root.join(format!("aipo-wasm-{}.aipo", workload.id));
            fs::write(&path, source).map_err(|error| error.to_string())?;
            Ok(PreparedInvocation {
                program: Some(aipo_bin.to_path_buf()),
                args: vec![
                    "run".to_string(),
                    path.display().to_string(),
                    "--wasm".to_string(),
                ],
                current_dir: None,
                invocation: InvocationKind::Process,
                cleanup: None,
                includes_startup: true,
                includes_compile: true,
                setup_ns: duration_ns(setup_start.elapsed()),
            })
        }
        "aipo-vm" => {
            let source = comparison_source(language, &workload.id, n)?;
            let (_, bytecode) = pipeline::compile_text("comparison.aipo", &source)
                .map_err(|error| format!("Aipo VM compile failed: {error:?}"))?;
            Ok(PreparedInvocation {
                program: None,
                args: Vec::new(),
                current_dir: None,
                invocation: InvocationKind::AipoVm(bytecode),
                cleanup: None,
                includes_startup: false,
                includes_compile: false,
                setup_ns: duration_ns(setup_start.elapsed()),
            })
        }
        "aipo-js" => {
            let node = command_path("node").ok_or_else(|| "Node is unavailable".to_string())?;
            let source = comparison_source(language, &workload.id, n)?;
            let (_, ir) = pipeline::lower_to_ir("comparison.aipo", &source)
                .map_err(|_| "Aipo JavaScript lowering failed".to_string())?;
            let tag = format!(
                "cross-{}-{}-{}",
                std::process::id(),
                workload.id,
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            );
            let bundle_dir = emit_comparison_bundle(&source, &ir, &tag)?;
            Ok(PreparedInvocation {
                program: Some(node),
                args: vec!["app.js".to_string()],
                current_dir: Some(bundle_dir.clone()),
                invocation: InvocationKind::Process,
                cleanup: Some(bundle_dir),
                includes_startup: true,
                includes_compile: false,
                setup_ns: duration_ns(setup_start.elapsed()),
            })
        }
        "script" => {
            let (_command, program) = resolve_language_command(language)
                .ok_or_else(|| "script command is unavailable".to_string())?;
            let script = language
                .script
                .as_deref()
                .ok_or_else(|| "script path is missing".to_string())?
                .replace("{workload}", &workload.id);
            let script = comparison_root().join(script);
            let mut args = vec![script.display().to_string()];
            if language.program_args {
                args.push("-a".to_string());
            }
            args.push(n.to_string());
            Ok(PreparedInvocation {
                program: Some(program),
                args,
                current_dir: None,
                invocation: InvocationKind::Process,
                cleanup: None,
                includes_startup: true,
                includes_compile: true,
                setup_ns: duration_ns(setup_start.elapsed()),
            })
        }
        "self" => {
            let program = std::env::current_exe().map_err(|error| error.to_string())?;
            Ok(PreparedInvocation {
                program: Some(program),
                args: vec![
                    "--compare-worker".to_string(),
                    workload.id.clone(),
                    n.to_string(),
                ],
                current_dir: None,
                invocation: InvocationKind::Process,
                cleanup: None,
                includes_startup: true,
                includes_compile: false,
                setup_ns: duration_ns(setup_start.elapsed()),
            })
        }
        other => Err(format!("unsupported language kind '{other}'")),
    }
}

fn run_process(
    program: &Path,
    args: &[String],
    current_dir: Option<&Path>,
    deadline: Duration,
    collect_metrics: bool,
) -> Result<ProcessOutput, String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }
    for variable in [
        "BASH_ENV",
        "CDPATH",
        "DYLD_INSERT_LIBRARIES",
        "ENV",
        "LD_PRELOAD",
        "NODE_OPTIONS",
        "PERL5OPT",
        "PYTHONPATH",
        "RUBYLIB",
        "RUBYOPT",
    ] {
        command.env_remove(variable);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start process '{}': {error}", program.display()))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("process stdout pipe is missing".to_string());
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("process stderr pipe is missing".to_string());
    };
    let stdout = spawn_reader(stdout, "stdout");
    let stderr = spawn_reader(stderr, "stderr");
    let mut peak_rss_bytes = None;
    if collect_metrics {
        update_peak_rss(&mut peak_rss_bytes, child.id());
    }
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if collect_metrics {
                    update_peak_rss(&mut peak_rss_bytes, child.id());
                }
                break status;
            }
            Ok(None) if start.elapsed() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "process '{}' exceeded the {}s deadline",
                    program.display(),
                    deadline.as_secs()
                ));
            }
            Ok(None) => {
                if collect_metrics {
                    update_peak_rss(&mut peak_rss_bytes, child.id());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "could not poll process '{}': {error}",
                    program.display()
                ));
            }
        }
    };
    let stdout = receive_reader(stdout, deadline.saturating_sub(start.elapsed()), "stdout")?;
    let stderr = receive_reader(stderr, deadline.saturating_sub(start.elapsed()), "stderr")?;
    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
        peak_rss_bytes,
    })
}

fn parse_status_kib(status: &str, key: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let value = line.strip_prefix(key)?;
        value
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    })
}

fn parse_vm_rss(status: &str) -> Option<u64> {
    parse_status_kib(status, "VmRSS:")
}

fn parse_vm_hwm(status: &str) -> Option<u64> {
    parse_status_kib(status, "VmHWM:")
}

fn read_process_rss(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_vm_rss(&status)
}

fn read_process_peak_rss(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_vm_hwm(&status).or_else(|| parse_vm_rss(&status))
}

fn update_peak_rss(peak: &mut Option<u64>, pid: u32) {
    if let Some(current) = read_process_peak_rss(pid) {
        *peak = Some(peak.map_or(current, |previous| previous.max(current)));
    }
}

fn spawn_reader<R>(reader: R, label: &'static str) -> Receiver<Result<String, String>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let _reader_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = reader
            .take(MAX_PROCESS_OUTPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read process {label}: {error}"))
            .and_then(|_| {
                if bytes.len() as u64 > MAX_PROCESS_OUTPUT_BYTES {
                    Err(format!(
                        "process {label} exceeded the {MAX_PROCESS_OUTPUT_BYTES}-byte limit"
                    ))
                } else {
                    Ok(String::from_utf8_lossy(&bytes).into_owned())
                }
            });
        let _ = sender.send(result);
    });
    receiver
}

fn receive_reader(
    receiver: Receiver<Result<String, String>>,
    deadline: Duration,
    label: &str,
) -> Result<String, String> {
    receiver
        .recv_timeout(deadline)
        .map_err(|_| format!("process {label} did not close before the deadline"))?
}

fn execute_once(
    prepared: &PreparedInvocation,
    expected: &str,
    collect_metrics: bool,
    collect_resources: bool,
) -> Result<ExecutionSample, String> {
    match &prepared.invocation {
        InvocationKind::Process => {
            let program = prepared
                .program
                .as_ref()
                .ok_or_else(|| "process program is missing".to_string())?;
            let start = Instant::now();
            let output = run_process(
                program,
                &prepared.args,
                prepared.current_dir.as_deref(),
                SAMPLE_TIMEOUT,
                collect_metrics,
            )?;
            let elapsed = start.elapsed();
            if !output.status.success() {
                return Err(format!(
                    "process exited with {}: {}",
                    output.status,
                    output.stderr.trim()
                ));
            }
            let checksum = parse_checksum(&output.stdout, expected)?;
            let resources = collect_resources.then_some(ResourceMetricsReport {
                peak_rss_bytes: output.peak_rss_bytes,
                allocation_count: None,
                allocated_bytes: None,
                scope: "process".to_string(),
                method: "procfs-sampled".to_string(),
            });
            Ok(ExecutionSample {
                elapsed,
                checksum,
                runtime_setup: None,
                metrics: None,
                resources,
            })
        }
        InvocationKind::AipoVm(bytecode) => {
            let sampler = collect_resources.then(PeakRssSampler::start);
            let start = Instant::now();
            let report = if collect_metrics {
                pipeline::run_capture_split_with_metrics(bytecode)
            } else {
                pipeline::run_capture_split(bytecode)
            }
            .map_err(|error| error.to_string())?;
            let elapsed = start.elapsed();
            let checksum = parse_checksum(&report.stdout, expected)?;
            let resources = sampler.map(|sampler| ResourceMetricsReport {
                peak_rss_bytes: sampler.finish(),
                allocation_count: None,
                allocated_bytes: None,
                scope: "vm-process".to_string(),
                method: "procfs-sampled".to_string(),
            });
            Ok(ExecutionSample {
                elapsed,
                checksum,
                runtime_setup: Some(report.setup),
                metrics: collect_metrics.then_some(report.metrics),
                resources,
            })
        }
    }
}

fn parse_checksum(stdout: &str, expected: &str) -> Result<String, String> {
    let line = stdout.trim();
    let checksum = line
        .strip_prefix("checksum:")
        .ok_or_else(|| format!("missing checksum output: {line}"))?;
    if checksum != expected {
        return Err(format!(
            "checksum mismatch: expected {expected}, got {checksum}"
        ));
    }
    Ok(checksum.to_string())
}

fn create_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    for _ in 0..8 {
        let token = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
            ^ NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{token}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "could not create comparison temp directory: {error}"
                ));
            }
        }
    }
    Err("could not allocate a unique comparison temp directory".to_string())
}

fn create_temp_root() -> Result<TempRoot, String> {
    create_temp_dir("aipo-cross-language").map(TempRoot)
}

fn comparison_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("comparisons")
}

fn comparison_source(language: &LanguageSpec, workload: &str, n: u64) -> Result<String, String> {
    let script = language
        .script
        .as_deref()
        .ok_or_else(|| format!("script path is missing for '{}'", language.id))?
        .replace("{workload}", workload);
    let path = comparison_root().join(script);
    let source =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(source.replace("__N__", &n.to_string()))
}

fn rust_checksum(workload: &str, n: u64) -> Result<String, String> {
    validate_n(workload, n)?;
    let checksum = match workload {
        "arithmetic" => arithmetic(n).to_string(),
        "collections" => collections(n).to_string(),
        "fields" => fields(n).to_string(),
        "strings" => strings(n).to_string(),
        "recursion" => recursion(24).to_string(),
        "startup" => "ready".to_string(),
        _ => return Err(format!("unsupported workload '{workload}'")),
    };
    Ok(checksum)
}

fn expected_checksum(workload: &str, n: u64) -> String {
    match workload {
        "arithmetic" => arithmetic(n).to_string(),
        "collections" => collections_checksum(n).to_string(),
        "fields" => fields(n).to_string(),
        "strings" => n
            .checked_mul(2)
            .map_or_else(|| "invalid".to_string(), |value| value.to_string()),
        "recursion" => recursion(24).to_string(),
        "startup" => "ready".to_string(),
        _ => "unknown".to_string(),
    }
}

fn operation_count(workload: &str, n: u64) -> u64 {
    match workload {
        "arithmetic" | "collections" | "fields" | "strings" => n,
        "recursion" | "startup" => 1,
        _ => 0,
    }
}

fn step(value: u64) -> i128 {
    i128::from(value) * 3 - 1
}

fn arithmetic(n: u64) -> i128 {
    let mut total = 0i128;
    for value in 0..n {
        total += step(value);
    }
    total
}

fn collections_checksum(n: u64) -> u128 {
    u128::from(n) * u128::from(n.saturating_sub(1)) / 2
}

fn collections(n: u64) -> u128 {
    let mut values = Vec::new();
    for value in 0..n {
        values.push(value);
    }
    values.into_iter().map(u128::from).sum()
}

fn fields(n: u64) -> u128 {
    let mut state = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut total = 0u128;
    for i in 0..n {
        let (_a, b, c, d, e, f) = state;
        let next_a = (b + i) % 1000;
        let next_b = (c + next_a) % 1000;
        let next_c = (d + next_b) % 1000;
        let next_d = (e + next_c) % 1000;
        let next_e = (f + next_d) % 1000;
        let next_f = (next_a + 1) % 1000;
        state = (next_a, next_b, next_c, next_d, next_e, next_f);
        total +=
            u128::from(next_a + 2 * next_b + 3 * next_c + 4 * next_d + 5 * next_e + 6 * next_f);
    }
    total
}

fn strings(n: u64) -> u64 {
    let mut value = String::new();
    for _ in 0..n {
        value.push_str("ab");
    }
    value.len() as u64
}

fn recursion(n: u32) -> u128 {
    match n {
        0 => 0,
        1 => 1,
        _ => recursion(n - 1) + recursion(n - 2),
    }
}

fn summarize(samples: &[u64]) -> (u64, u64, u64, u64, u64) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let mut deviations: Vec<u64> = sorted
        .iter()
        .map(|sample| sample.abs_diff(median))
        .collect();
    deviations.sort_unstable();
    let mad = deviations[deviations.len() / 2];
    let p95_index = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    (
        median,
        mad,
        sorted[0],
        sorted[p95_index],
        sorted[sorted.len() - 1],
    )
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    if nanos >= 1_000_000 {
        format!("{:.2}ms", nanos as f64 / 1_000_000.0)
    } else if nanos >= 1_000 {
        format!("{:.2}us", nanos as f64 / 1_000.0)
    } else {
        format!("{nanos}ns")
    }
}

fn binary_profile(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.contains("/release/") {
        "release".to_string()
    } else if text.contains("/debug/") {
        "debug".to_string()
    } else {
        "custom-or-unknown".to_string()
    }
}

fn file_sha256(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    Some(hex)
}

fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn resolve_aipo_bin(explicit: Option<&Path>) -> Result<Option<PathBuf>, String> {
    if let Some(path) = explicit {
        return if path.is_file() {
            Ok(Some(path.to_path_buf()))
        } else {
            Err(format!("Aipo binary does not exist: {}", path.display()))
        };
    }
    if let Some(path) = std::env::var_os("AIPO_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(Some(path));
        }
        return Err(format!(
            "AIPO_BIN does not point to a file: {}",
            path.display()
        ));
    }
    let profile = build_profile();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let candidates = [
        PathBuf::from("target").join(profile).join("aipo"),
        root.join("target").join(profile).join("aipo"),
    ];
    Ok(candidates.into_iter().find(|path| path.is_file()))
}

fn version_args(command: &str) -> &'static [&'static str] {
    if command.ends_with("omnilua") || command == "lua" || command == "luajit" {
        &["-v"]
    } else if command == "luau" {
        &["-h"]
    } else {
        &["--version"]
    }
}

fn command_path(command: &str) -> Option<PathBuf> {
    let args = version_args(command)
        .iter()
        .map(|argument| (*argument).to_string())
        .collect::<Vec<_>>();
    let output = run_process(Path::new(command), &args, None, DISCOVERY_TIMEOUT, false).ok()?;
    output.status.success().then(|| PathBuf::from(command))
}

fn command_version(command: &str) -> Option<String> {
    let args = version_args(command)
        .iter()
        .map(|argument| (*argument).to_string())
        .collect::<Vec<_>>();
    let output = run_process(Path::new(command), &args, None, DISCOVERY_TIMEOUT, false).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = if output.stdout.trim().is_empty() {
        output.stderr.trim()
    } else {
        output.stdout.trim()
    };
    if command == "luau" {
        return Some("luau CLI (no --version flag)".to_string());
    }
    text.lines().next().map(str::to_string)
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let args = args
        .iter()
        .map(|argument| (*argument).to_string())
        .collect::<Vec<_>>();
    let output = run_process(Path::new(command), &args, None, DISCOVERY_TIMEOUT, false).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(output.stdout.trim().to_string())
}

fn runtime_versions(
    languages: &[LanguageSpec],
    aipo_bin: Option<&Path>,
) -> BTreeMap<String, String> {
    let mut versions = BTreeMap::new();
    for language in languages {
        let version = match language.kind.as_str() {
            "aipo" | "aipo-reg" | "aipo-wasm" => aipo_bin
                .and_then(|path| path.to_str().and_then(command_version))
                .unwrap_or_else(|| "unavailable".to_string()),
            "aipo-js" => command_version("node").unwrap_or_else(|| "unavailable".to_string()),
            "aipo-vm" => "in-process".to_string(),
            "self" => command_version("rustc").unwrap_or_else(|| "rustc unknown".to_string()),
            "script" => resolve_language_command(language)
                .and_then(|(command, _)| command_version(&command))
                .unwrap_or_else(|| "unavailable".to_string()),
            _ => "unknown".to_string(),
        };
        versions.insert(language.id.clone(), version);
    }
    versions
}

fn cpu_model() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim().to_string())
    })
}

fn write_report(path: &Path, report: &ComparisonReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(report).map_err(|error| error.to_string())?;
    fs::write(path, format!("{text}\n")).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_checksums_match_algorithms() {
        let manifest: Manifest = serde_json::from_str(MANIFEST_JSON).expect("manifest parses");
        for workload in manifest.workloads {
            assert_eq!(
                workload.checksum,
                expected_checksum(&workload.id, workload.n),
                "{} checksum",
                workload.id
            );
        }
    }

    #[test]
    fn summarize_uses_median_mad_and_extremes() {
        let (median, mad, minimum, p95, maximum) = summarize(&[4, 1, 3, 2]);
        assert_eq!((median, mad, minimum, p95, maximum), (3, 1, 1, 4, 4));
    }

    #[test]
    fn checksum_validation_rejects_wrong_output() {
        assert_eq!(
            parse_checksum("checksum:42\n", "42").expect("checksum parses"),
            "42"
        );
        assert!(parse_checksum("checksum:41\n", "42").is_err());
        assert!(parse_checksum("ready\n", "ready").is_err());
    }

    #[test]
    fn rust_worker_executes_the_same_work() {
        assert_eq!(rust_checksum("collections", 5), Ok("10".to_string()));
        assert_eq!(rust_checksum("fields", 5), Ok("1841".to_string()));
        assert_eq!(rust_checksum("strings", 5), Ok("10".to_string()));
    }

    #[test]
    fn file_hash_preserves_canonical_sha256_and_missing_file_behavior() {
        let path = std::env::temp_dir().join(format!("aipo-bench-hash-{}", std::process::id()));
        fs::write(&path, b"abc").expect("write hash fixture");
        assert_eq!(
            file_sha256(&path).as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        fs::remove_file(&path).expect("remove hash fixture");
        assert_eq!(file_sha256(&path), None);
    }

    #[test]
    fn input_limits_reject_overflow_and_invalid_shapes() {
        assert!(validate_n("arithmetic", MAX_COMPARE_N + 1).is_err());
        assert!(validate_n("strings", 0).is_err());
        assert!(validate_n("startup", 1).is_err());
    }

    #[test]
    fn language_selection_rejects_empty_unknown_and_duplicates() {
        let languages = vec![LanguageSpec {
            id: "aipo-vm".to_string(),
            label: "Aipo VM".to_string(),
            kind: "aipo-vm".to_string(),
            command: None,
            command_candidates: Vec::new(),
            program_args: false,
            script: None,
            required: true,
        }];
        assert!(validate_language_selection(Some(&[]), &languages).is_err());
        assert!(validate_language_selection(Some(&["unknown".to_string()]), &languages).is_err());
        assert!(
            validate_language_selection(
                Some(&["aipo-vm".to_string(), "aipo-vm".to_string()]),
                &languages,
            )
            .is_err()
        );
        assert!(validate_language_selection(Some(&["aipo-vm".to_string()]), &languages).is_ok());
    }

    #[test]
    fn workload_selection_rejects_empty_unknown_and_duplicates() {
        let workloads = vec![
            WorkloadSpec {
                id: "arithmetic".to_string(),
                description: "arithmetic".to_string(),
                n: 1,
                checksum: "1".to_string(),
            },
            WorkloadSpec {
                id: "collections".to_string(),
                description: "collections".to_string(),
                n: 1,
                checksum: "0".to_string(),
            },
        ];
        assert!(validate_workload_selection(Some(&[]), &workloads).is_err());
        assert!(validate_workload_selection(Some(&["unknown".to_string()]), &workloads).is_err());
        assert!(
            validate_workload_selection(
                Some(&["arithmetic".to_string(), "arithmetic".to_string()]),
                &workloads,
            )
            .is_err()
        );
        assert!(
            validate_workload_selection(Some(&["collections".to_string()]), &workloads).is_ok()
        );
    }

    #[test]
    fn resource_metrics_are_opt_in() {
        let options = parse_options(&["--compare".to_string(), "--compare-resources".to_string()])
            .expect("resource option parses");
        assert!(options.resources);
    }

    #[test]
    fn comparison_options_reject_unbounded_rounds_and_input() {
        let rounds = vec![
            "--compare".to_string(),
            "--compare-runs".to_string(),
            (MAX_COMPARE_ROUNDS + 1).to_string(),
        ];
        assert!(parse_options(&rounds).is_err());
        let input = vec![
            "--compare".to_string(),
            "--compare-n".to_string(),
            format!("strings={}", MAX_COMPARE_N + 1),
        ];
        assert!(parse_options(&input).is_err());
    }

    #[test]
    fn manifest_contains_first_wave_runtimes_and_fixtures() {
        let manifest: Manifest = serde_json::from_str(MANIFEST_JSON).expect("manifest parses");
        validate_manifest(&manifest).expect("manifest is valid");
        let ids = manifest
            .languages
            .iter()
            .map(|language| language.id.as_str())
            .collect::<BTreeSet<_>>();
        for expected in ["wren", "luau", "pypy"] {
            assert!(ids.contains(expected), "missing runtime {expected}");
        }
    }

    #[test]
    fn field_cache_records_repeated_struct_slot_hits() {
        let source = include_str!("../comparisons/aipo/fields.aipo").replace("__N__", "2");
        let (_, bytecode) =
            pipeline::compile_text("fields.aipo", &source).expect("fields compiles");
        let report = pipeline::run_capture_split_with_metrics(&bytecode).expect("fields runs");
        assert_eq!(report.stdout.trim(), "checksum:38");
        assert!(report.metrics.field_cache_hits > 0);
        assert!(report.metrics.field_cache_misses > 0);
    }

    #[test]
    fn rss_parser_accepts_linux_status_and_rejects_malformed_values() {
        let status = "Name:\tbench\nVmRSS:\t  1234 kB\nVmHWM:\t 2345 kB\nVmPeak:\t9999 kB\n";
        assert_eq!(parse_vm_rss(status), Some(1234 * 1024));
        assert_eq!(parse_vm_hwm(status), Some(2345 * 1024));
        assert_eq!(parse_vm_rss("VmRSS:\tnot-a-number kB\n"), None);
        assert_eq!(parse_vm_rss("Name:\tbench\n"), None);
    }
}
