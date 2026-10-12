//! Canonical `sh` system automation module for shell scripting and process control (Marco 4 / ADP-014).

#![forbid(unsafe_code)]

use aipo_vm::{DictMap, FailureValue, Value, VmFault};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

fn expect_string<'a>(arg: &'a Value, op: &str) -> Result<&'a str, VmFault> {
    match arg {
        Value::String(s) => Ok(s.as_str()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("String for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Executes an external system command: `sh.run(cmd: String, args: List[String] = []) -> Dict`.
///
/// Returns a Dictionary with:
/// - `"code"`: exit status code as integer (0 for success, non-zero for error, -1 if terminated by signal)
/// - `"stdout"`: standard output text captured
/// - `"stderr"`: standard error text captured
/// - `"ok"`: boolean (`code == 0`)
pub fn sh_run(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() || args.len() > 2 {
        return Err(VmFault::TypeMismatch {
            expected: "1 or 2 arguments for sh.run (cmd, args)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let cmd_name = expect_string(&args[0], "sh.run command")?;
    let mut command = Command::new(cmd_name);

    if args.len() == 2 {
        match &args[1] {
            Value::List(list) => {
                for item in list.borrow().iter() {
                    let s = expect_string(item, "sh.run argument item")?;
                    command.arg(s);
                }
            }
            Value::None => {}
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "List of strings for sh.run arguments".to_string(),
                    actual: other.type_name().to_string(),
                });
            }
        }
    }

    let output = match command.output() {
        Ok(out) => out,
        Err(err) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "failed to execute command '{cmd_name}': {err}"
            )))));
        }
    };

    let exit_code = output.status.code().unwrap_or(-1) as i64;
    let stdout_str = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr_str = String::from_utf8_lossy(&output.stderr).into_owned();
    let ok = output.status.success();

    let entries = vec![
        (
            Value::String(Rc::new("code".to_string())),
            Value::Int(exit_code),
        ),
        (
            Value::String(Rc::new("stdout".to_string())),
            Value::String(Rc::new(stdout_str)),
        ),
        (
            Value::String(Rc::new("stderr".to_string())),
            Value::String(Rc::new(stderr_str)),
        ),
        (Value::String(Rc::new("ok".to_string())), Value::Bool(ok)),
    ];

    Ok(Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(
        entries,
    )))))
}

/// Changes the current working directory: `sh.cd(path: String) -> Bool`.
pub fn sh_cd(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for sh.cd".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let path = expect_string(&args[0], "sh.cd")?;
    match std::env::set_current_dir(path) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(err) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "failed to change directory to '{path}': {err}"
        ))))),
    }
}

/// Returns the current working directory: `sh.pwd() -> String`.
pub fn sh_pwd(args: &[Value]) -> Result<Value, VmFault> {
    if !args.is_empty() {
        return Err(VmFault::TypeMismatch {
            expected: "0 arguments for sh.pwd".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match std::env::current_dir() {
        Ok(dir) => Ok(Value::String(Rc::new(dir.to_string_lossy().into_owned()))),
        Err(err) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "failed to get current directory: {err}"
        ))))),
    }
}

/// Retrieves an environment variable: `sh.env(key: String) -> String?`.
pub fn sh_env(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for sh.env".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let key = expect_string(&args[0], "sh.env")?;
    match std::env::var(key) {
        Ok(val) => Ok(Value::String(Rc::new(val))),
        Err(std::env::VarError::NotPresent) => Ok(Value::None),
        Err(err) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "failed to read environment variable '{key}': {err}"
        ))))),
    }
}

/// Looks up the path of an executable in PATH: `sh.which(cmd: String) -> String?`.
pub fn sh_which(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for sh.which".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let cmd = expect_string(&args[0], "sh.which")?;
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        let full = dir.join(cmd);
        if full.is_file() {
            return Ok(Value::String(Rc::new(full.to_string_lossy().into_owned())));
        }
    }
    Ok(Value::None)
}

/// Constructs the canonical `sh` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("run".to_string())),
            Value::native("sh.run", usize::MAX, sh_run),
        ),
        (
            Value::String(Rc::new("cd".to_string())),
            Value::native("sh.cd", 1, sh_cd),
        ),
        (
            Value::String(Rc::new("pwd".to_string())),
            Value::native("sh.pwd", 0, sh_pwd),
        ),
        (
            Value::String(Rc::new("env".to_string())),
            Value::native("sh.env", 1, sh_env),
        ),
        (
            Value::String(Rc::new("which".to_string())),
            Value::native("sh.which", 1, sh_which),
        ),
    ];
    let mut entries = entries;
    for (name, arity, function) in [
        (
            "spawn",
            1,
            sh_spawn as fn(&[Value]) -> Result<Value, VmFault>,
        ),
        ("wait", 1, sh_wait),
        ("kill", 1, sh_kill),
        ("jobs", 0, sh_jobs),
        ("pipeline", 1, sh_pipeline),
    ] {
        entries.push((
            Value::string(name),
            Value::native(format!("sh.{name}"), arity, function),
        ));
    }
    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}

#[derive(Default)]
struct Jobs {
    next: u64,
    children: std::collections::BTreeMap<u64, std::process::Child>,
}
impl Drop for Jobs {
    fn drop(&mut self) {
        for child in self.children.values_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
thread_local! { static JOBS:RefCell<Jobs>=RefCell::new(Jobs::default()); }
fn command_spec(value: &Value) -> Result<Command, VmFault> {
    let Value::List(parts) = value else {
        return Err(VmFault::TypeMismatch {
            expected: "List[String] command".into(),
            actual: value.type_name().into(),
        });
    };
    let parts = parts.borrow();
    let Some(program) = parts.first() else {
        return Err(VmFault::TypeMismatch {
            expected: "nonempty command".into(),
            actual: "empty List".into(),
        });
    };
    let mut command = Command::new(expect_string(program, "process program")?);
    for arg in &parts[1..] {
        command.arg(expect_string(arg, "process argument")?);
    }
    Ok(command)
}
fn failure(error: impl std::fmt::Display) -> Value {
    Value::failure(error.to_string())
}
fn job_id(args: &[Value]) -> Result<u64, VmFault> {
    match args {
        [Value::Int(id)] if *id > 0 => Ok(*id as u64),
        _ => Err(VmFault::TypeMismatch {
            expected: "one positive Int job id".into(),
            actual: format!("{} arguments", args.len()),
        }),
    }
}
/// Starts an owned background job from `[program, args...]`, inheriting host streams.
/// At most 128 jobs are retained; the owning thread reaps them on shutdown.
pub fn sh_spawn(args: &[Value]) -> Result<Value, VmFault> {
    let [spec] = args else {
        return Err(VmFault::TypeMismatch {
            expected: "one command List".into(),
            actual: format!("{} arguments", args.len()),
        });
    };
    let mut command = command_spec(spec)?;
    JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        if jobs.children.len() >= 128 {
            return Ok(failure("background job limit of 128 reached"));
        }
        let Some(id) = jobs
            .next
            .checked_add(1)
            .filter(|id| *id <= aipo_vm::value::MAX_SAFE_INT as u64)
        else {
            return Ok(failure("job id space exhausted"));
        };
        match command.spawn() {
            Ok(child) => {
                jobs.next = id;
                jobs.children.insert(id, child);
                Ok(Value::Int(id as i64))
            }
            Err(error) => Ok(failure(error)),
        }
    })
}
/// Waits for and removes a background job, returning its portable exit code.
pub fn sh_wait(args: &[Value]) -> Result<Value, VmFault> {
    let id = job_id(args)?;
    let child = JOBS.with(|jobs| jobs.borrow_mut().children.remove(&id));
    let Some(mut child) = child else {
        return Ok(failure("unknown job id"));
    };
    match child.wait() {
        Ok(status) => Ok(Value::Int(status.code().unwrap_or(-1) as i64)),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Ok(failure(error))
        }
    }
}
/// Terminates and reaps an owned job using the platform's Child::kill operation.
pub fn sh_kill(args: &[Value]) -> Result<Value, VmFault> {
    let id = job_id(args)?;
    let child = JOBS.with(|jobs| jobs.borrow_mut().children.remove(&id));
    let Some(mut child) = child else {
        return Ok(failure("unknown job id"));
    };
    let result = child.kill().and_then(|()| child.wait().map(|_| ()));
    Ok(match result {
        Ok(()) => Value::None,
        Err(error) => failure(error),
    })
}
/// Lists owned jobs as `{id, pid, running}` dictionaries.
pub fn sh_jobs(args: &[Value]) -> Result<Value, VmFault> {
    if !args.is_empty() {
        return Err(VmFault::TypeMismatch {
            expected: "no arguments".into(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let values = JOBS.with(|jobs| {
        jobs.borrow_mut()
            .children
            .iter_mut()
            .map(|(id, child)| {
                let entries = vec![
                    (Value::string("id"), Value::Int(*id as i64)),
                    (Value::string("pid"), Value::Int(child.id() as i64)),
                    (
                        Value::string("running"),
                        Value::Bool(matches!(child.try_wait(), Ok(None))),
                    ),
                ];
                Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
            })
            .collect::<Vec<_>>()
    });
    Ok(Value::List(Rc::new(RefCell::new(values.into()))))
}
/// Connects OS pipes between command lists, without shell interpolation.
/// Intermediate stderr and final stdout/stderr inherit host streams. Returns the
/// final stage's status; every child is reaped, including partial-spawn failures.
pub fn sh_pipeline(args: &[Value]) -> Result<Value, VmFault> {
    let [Value::List(specs)] = args else {
        return Err(VmFault::TypeMismatch {
            expected: "List[List[String]]".into(),
            actual: "invalid pipeline arguments".into(),
        });
    };
    let specs = specs.borrow();
    if specs.is_empty() || specs.len() > 128 {
        return Ok(failure("pipeline requires 1..128 commands"));
    }
    let commands = specs
        .iter()
        .map(command_spec)
        .collect::<Result<Vec<_>, _>>()?;
    struct Children(Vec<std::process::Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut children = Children(Vec::new());
    let mut previous = None;
    for (index, mut command) in commands.into_iter().enumerate() {
        if let Some(stdout) = previous.take() {
            command.stdin(std::process::Stdio::from(stdout));
        }
        if index + 1 < specs.len() {
            command.stdout(std::process::Stdio::piped());
        }
        match command.spawn() {
            Ok(mut child) => {
                previous = child.stdout.take();
                children.0.push(child);
            }
            Err(error) => return Ok(failure(error)),
        }
    }
    let mut final_status = 0;
    for child in &mut children.0 {
        match child.wait() {
            Ok(status) => final_status = status.code().unwrap_or(-1),
            Err(error) => return Ok(failure(error)),
        }
    }
    children.0.clear();
    Ok(Value::Int(final_status as i64))
}
