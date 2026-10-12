//! Entry point for `aipo-sh`, the standalone lightweight Aipo shell and script runner (Marco 4 / ADP-014).
//!
//! Features:
//! - Persistent globals, multiline input, history and explicit completion
//! - Direct in-memory string evaluation (`aipo-sh -c "code"`)
//! - Script execution (`aipo-sh script.aipo`)
//! - Interactive REPL (`aipo-sh`)

#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let (args, reg) = split_engine_flag(raw_args);

    if args.is_empty() {
        return run_repl(reg);
    }

    match args[0].as_str() {
        "-h" | "--help" => {
            println!("aipo-sh — lightweight Aipo shell and script runner");
            println!();
            println!("USAGE:");
            println!("    aipo-sh <script.aipo> [args...]");
            println!("    aipo-sh -c <code>");
            println!("    aipo-sh [--engine=<vm|reg>]");
            println!("    aipo-sh --version");
            println!("    aipo-sh --help");
            println!();
            println!("FLAGS:");
            println!("    --engine=<vm|reg>   Execution engine: stack VM (default) or register VM");
            ExitCode::SUCCESS
        }
        "-v" | "--version" => {
            println!("aipo-sh {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "-c" => {
            if args.len() < 2 {
                eprintln!("error: -c requires a code string argument");
                return ExitCode::from(2);
            }
            let code = &args[1];
            let code_u8 = if reg {
                aipo_cli::eval_source_reg(code, &mut io::stdout(), &mut io::stderr())
            } else {
                aipo_cli::eval_source(code, &mut io::stdout(), &mut io::stderr())
            };
            ExitCode::from(code_u8)
        }
        _ => {
            let mut run_args = vec!["run".to_string()];
            if reg {
                run_args.push("--engine=reg".to_string());
            }
            run_args.extend(args);
            let code_u8 = aipo_cli::run_with(&run_args, &mut io::stdout(), &mut io::stderr());
            ExitCode::from(code_u8)
        }
    }
}

/// Extracts `--engine=<vm|reg>` / `--engine <vm|reg>` / `--reg` from shell args.
///
/// Returns the remaining args and `true` when the register VM was selected.
fn split_engine_flag(args: Vec<String>) -> (Vec<String>, bool) {
    let mut kept = Vec::with_capacity(args.len());
    let mut reg = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--reg" {
            reg = true;
        } else if let Some(value) = arg.strip_prefix("--engine=") {
            match value {
                "reg" => reg = true,
                "vm" | "bytecode" | "stack" => reg = false,
                _ => {
                    eprintln!("error: unrecognized --engine '{value}': expected 'vm' or 'reg'");
                    std::process::exit(2);
                }
            }
        } else if arg == "--engine" {
            index += 1;
            match args.get(index).map(String::as_str) {
                Some("reg") => reg = true,
                Some("vm" | "bytecode" | "stack") => reg = false,
                _ => {
                    eprintln!("error: --engine requires 'vm' or 'reg'");
                    std::process::exit(2);
                }
            }
        } else {
            kept.push(arg.clone());
        }
        index += 1;
    }
    (kept, reg)
}

fn run_repl(reg: bool) -> ExitCode {
    use std::io::IsTerminal;
    let interactive = io::stdin().is_terminal();
    if interactive {
        println!(
            "Aipo Shell v{} — persistent canonical session{}",
            env!("CARGO_PKG_VERSION"),
            if reg { " (reg profile)" } else { "" }
        );
        println!(":help for commands; exit to quit.");
    }
    let mut session = aipo_cli::Session::new();
    let mut history = aipo_cli::repl::History::load();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut code = String::new();
    let mut failed = false;
    loop {
        if interactive {
            let _ = write!(
                stdout,
                "{}",
                if code.is_empty() { "aipo> " } else { "....> " }
            );
            if stdout.flush().is_err() {
                return ExitCode::FAILURE;
            }
        }
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => {
                if !code.trim().is_empty() {
                    failed |= session.eval(
                        &code,
                        std::path::Path::new("<repl>"),
                        &mut stdout,
                        &mut stderr,
                    ) != 0;
                }
                break;
            }
            Ok(_) => {}
            Err(error) => {
                let _ = writeln!(stderr, "input error: {error}");
                return ExitCode::FAILURE;
            }
        }
        let trimmed = line.trim();
        if code.is_empty() {
            if matches!(trimmed, "exit" | "quit" | ":quit") {
                break;
            }
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with(':') {
                let (command, arg) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
                match command {
                    ":help" => {
                        let _ = writeln!(
                            stdout,
                            ":history, :complete PREFIX, :load PATH, :reload PATH, :reset, :quit"
                        );
                    }
                    ":history" => {
                        for (index, entry) in history.entries().iter().enumerate() {
                            let _ = writeln!(stdout, "{}: {}", index + 1, entry);
                        }
                    }
                    ":complete" => {
                        let _ = writeln!(stdout, "{}", session.complete(arg.trim()).join(" "));
                    }
                    ":reset" => session.reset(),
                    ":load" | ":reload" => match std::fs::read_to_string(arg.trim()) {
                        Ok(source) => {
                            let path = std::path::Path::new(arg.trim());
                            failed |= if command == ":reload" {
                                session.reload_with(&source, path, &mut stdout, &mut stderr, |_| {
                                    Ok(())
                                })
                            } else {
                                session.eval(&source, path, &mut stdout, &mut stderr)
                            } != 0;
                        }
                        Err(error) => {
                            failed = true;
                            let _ = writeln!(stderr, "{error}");
                        }
                    },
                    _ => {
                        failed = true;
                        let _ = writeln!(stderr, "unknown REPL command: {command}");
                    }
                }
                continue;
            }
        }
        code.push_str(&line);
        if !aipo_cli::repl::is_complete(&code) {
            continue;
        }
        history.push(code.trim_end().to_string());
        failed |= session.eval(
            &code,
            std::path::Path::new("<repl>"),
            &mut stdout,
            &mut stderr,
        ) != 0;
        code.clear();
    }
    if let Err(error) = history.save() {
        let _ = writeln!(stderr, "history: {error}");
    }
    if failed && !interactive {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
