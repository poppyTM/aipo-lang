//! Developer commands using the same frontend and VM as ordinary execution.
use crate::{analyze, emit_diagnostics, register_module_symbols, standard_environment};
use aipo_diagnostics::{MessageFormat, Severity};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::Instant,
};

pub(crate) fn dispatch(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Option<u8> {
    if args.first().map(String::as_str) == Some("package")
        && args.get(1).map(String::as_str) == Some("vendor")
    {
        return Some(match crate::vendor::run(&args[2..], out) {
            Ok(code) => code,
            Err(error) => {
                let _ = writeln!(err, "error: {error}");
                2
            }
        });
    }
    let command = args.first()?.as_str();
    if !matches!(
        command,
        "new" | "watch" | "profile" | "debug" | "lsp" | "plan"
    ) {
        return None;
    }
    let result = match command {
        "new" if args.len() == 2 => create_project(Path::new(&args[1])),
        "profile" => profile(&args[1..], out, err),
        "debug" if args.len() == 2 => debug(Path::new(&args[1]), out, err),
        "watch" if args.len() == 2 => watch(Path::new(&args[1]), out, err),
        "plan" if args.len() == 2 => plan(Path::new(&args[1]), out, err),
        "lsp" if args.len() == 1 => {
            crate::lsp::serve(std::io::stdin().lock(), out).map_err(|e| e.to_string())
        }
        _ => Err(format!(
            "usage: aipo {command} {}",
            if command == "lsp" { "" } else { "<path>" }
        )),
    };
    Some(match result {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(err, "error: {message}");
            2
        }
    })
}
fn plan(path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    let loaded = crate::load_source_entry(path, None).map_err(crate::cli_error_message)?;
    let (plan, diagnostics) =
        crate::analyze_to_execution_plan(&loaded.source, path, loaded.package_paths.as_ref(), None);
    emit_diagnostics(MessageFormat::Human, &loaded.source, &diagnostics, out, err);
    let Some(plan) = plan else {
        return Ok(1);
    };
    let reason = match &plan {
        aipo_bytecode::ExecutionPlan::Register(_) => None,
        aipo_bytecode::ExecutionPlan::Canonical { reason, .. } => Some(reason),
    };
    writeln!(
        out,
        "{}",
        serde_json::json!({"engine":plan.engine(),"reason":reason})
    )
    .map_err(|error| error.to_string())?;
    Ok(0)
}
fn create_project(path: &Path) -> Result<u8, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("project path requires a name")?;
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
    {
        return Err("project name must use lowercase ASCII, digits, '-' or '_'".into());
    }
    let coordinate = format!("local.{}", name.replace('-', "_"));
    aipo_package::PackageId::parse(&coordinate).map_err(|e| e.to_string())?;
    std::fs::create_dir(path).map_err(|e| e.to_string())?;
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir(path.join("src"))?;
        std::fs::write(
            path.join("aipo.toml"),
            format!(
                "[package]\nname = \"{coordinate}\"\nversion = \"0.1.0\"\nentry = \"src/main.aipo\"\n"
            ),
        )?;
        std::fs::write(path.join("src/main.aipo"), "io.println(\"Olá, Aipo!\")\n")?;
        std::fs::write(
            path.join("README.md"),
            format!("# {name}\n\n```sh\naipo run src/main.aipo\naipo check src/main.aipo\n```\n"),
        )
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(path);
        return Err(error.to_string());
    }
    Ok(0)
}
fn compile(
    path: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Option<aipo_bytecode::BytecodeModule>, String> {
    let loaded = crate::load_source_entry(path, None).map_err(crate::cli_error_message)?;
    let source = loaded.source;
    let compiled = analyze(&source, path, loaded.package_paths.as_ref());
    emit_diagnostics(
        MessageFormat::Human,
        &source,
        &compiled.diagnostics,
        out,
        err,
    );
    Ok((!compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error))
    .then_some(compiled.module))
}
fn profile(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    if args.is_empty() || args.len() > 4 {
        return Err("usage: aipo profile <path> [--budget <instructions>] [--json]".into());
    }
    let mut budget = None;
    let mut json = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = true,
            "--budget" => {
                index += 1;
                budget = Some(
                    args.get(index)
                        .ok_or("missing budget")?
                        .parse::<u64>()
                        .map_err(|e| e.to_string())?,
                );
            }
            _ => return Err("unknown profile option".into()),
        }
        index += 1;
    }
    let Some(module) = compile(Path::new(&args[0]), out, err)? else {
        return Ok(1);
    };
    let (mut vm, _) = standard_environment();
    register_module_symbols(&mut vm, &module);
    vm.enable_metrics();
    vm.set_max_instructions(budget);
    let start = Instant::now();
    let result = vm.run(&module);
    let elapsed = start.elapsed();
    let metrics = vm.metrics();
    let report = serde_json::json!({"engine":"vm","elapsed_ns":elapsed.as_nanos().to_string(),"instructions":metrics.instructions,"calls":metrics.calls,"native_calls":metrics.native_calls,"field_cache_hits":metrics.field_cache_hits,"field_cache_misses":metrics.field_cache_misses,"ok":result.is_ok()});
    if json {
        writeln!(out, "{report}").map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{} instructions, {} calls, {:.3} ms",
            metrics.instructions,
            metrics.calls,
            elapsed.as_secs_f64() * 1000.0
        )
        .map_err(|e| e.to_string())?;
    }
    if let Err(error) = result {
        let _ = writeln!(err, "{error}");
        return Ok(1);
    }
    Ok(0)
}
fn debug(path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    let Some(module) = compile(path, out, err)? else {
        return Ok(1);
    };
    let (mut vm, _) = standard_environment();
    register_module_symbols(&mut vm, &module);
    vm.start_at(&module, 0).map_err(|e| e.to_string())?;
    let mut input = std::io::stdin().lock();
    let mut breaks = std::collections::HashSet::new();
    let mut running = false;
    loop {
        let span = module
            .spans
            .iter()
            .rev()
            .find(|(offset, _)| *offset <= vm.ip)
            .map(|(_, span)| *span);
        if !running {
            writeln!(out, "ip={} span={span:?} stack={:?}", vm.ip, vm.stack)
                .map_err(|e| e.to_string())?;
            write!(out, "debug> ").map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
            let mut command = String::new();
            if input.read_line(&mut command).map_err(|e| e.to_string())? == 0 {
                return Ok(0);
            }
            match command.trim() {
                "q" | "quit" => return Ok(0),
                "c" | "continue" => running = true,
                "s" | "step" => {}
                "globals" => {
                    let mut names: Vec<_> = vm.globals.keys().collect();
                    names.sort();
                    for name in names {
                        let _ = writeln!(out, "{name} = {:?}", vm.globals[name]);
                    }
                    continue;
                }
                command if command.starts_with("break ") => {
                    let offset = command[6..]
                        .trim()
                        .parse::<usize>()
                        .map_err(|e| e.to_string())?;
                    breaks.insert(offset);
                    continue;
                }
                _ => {
                    let _ = writeln!(
                        out,
                        "step (s), continue (c), break BYTE_OFFSET, globals, quit (q)"
                    );
                    continue;
                }
            }
        }
        match vm.debug_step(&module) {
            Ok(true) => {
                return match vm.take_completion() {
                    Some(Err(error)) => {
                        let _ = writeln!(err, "{error}");
                        Ok(1)
                    }
                    _ => Ok(0),
                };
            }
            Err(error) => {
                let _ = writeln!(err, "{error}");
                return Ok(1);
            }
            Ok(false) => {
                if breaks.contains(&vm.ip) {
                    running = false;
                }
            }
        }
    }
}
fn sources(root: &Path) -> Result<std::collections::BTreeMap<PathBuf, u64>, String> {
    use std::hash::{Hash, Hasher};
    let mut files = std::collections::BTreeMap::new();
    let mut dirs = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        if depth > 64 {
            return Err("watch directory depth exceeds 64".into());
        }
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "target" | "node_modules")
                ) {
                    dirs.push((path, depth + 1));
                }
            } else if matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("aipo" | "toml" | "lock" | "ahs")
            ) {
                let mut hasher = std::hash::DefaultHasher::new();
                std::fs::read(&path)
                    .map_err(|e| e.to_string())?
                    .hash(&mut hasher);
                files.insert(path, hasher.finish());
                if files.len() > 10_000 {
                    return Err("watch exceeds 10,000 files".into());
                }
            }
        }
    }
    Ok(files)
}
fn watch(path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let manifest = crate::find_ancestor_manifest(&path);
    let root = manifest
        .as_deref()
        .and_then(Path::parent)
        .or_else(|| path.parent())
        .ok_or("missing source directory")?;
    let mut state = sources(root)?;
    let mut session = crate::Session::new();
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    session.eval(&text, &path, out, err);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let next = match sources(root) {
            Ok(next) => next,
            Err(error) => {
                let _ = writeln!(err, "watch: {error}");
                continue;
            }
        };
        if next == state {
            continue;
        }
        state = next;
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                if session.reload_with(&text, &path, out, err, |_| Ok(())) == 0 {
                    let _ = writeln!(err, "reloaded generation {}", session.generation());
                }
            }
            Err(error) => {
                let _ = writeln!(err, "watch: {error}");
            }
        }
    }
}
