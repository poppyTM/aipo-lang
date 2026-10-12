//! Canonical `log` structured logging module for Aipo.
//!
//! Provides level-based structured logging:
//! - `log.trace(message, [fields])`
//! - `log.debug(message, [fields])`
//! - `log.info(message, [fields])`
//! - `log.warning(message, [fields])`
//! - `log.error(message, [fields])`
//!
//! Sinks can be redirected or captured for testing.

#![forbid(unsafe_code)]

use aipo_vm::{DictMap, Value, VmFault};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex};

type DynLogWriter = Box<dyn Write + Send>;
type SafeLogSink = Arc<Mutex<Option<DynLogWriter>>>;

static LOG_SINK: LazyLock<SafeLogSink> = LazyLock::new(|| Arc::new(Mutex::new(None)));

/// Configures a custom sink for `log` output.
pub fn set_log_sink(sink: Option<Box<dyn Write + Send>>) {
    if let Ok(mut guard) = LOG_SINK.lock() {
        *guard = sink;
    }
}

fn emit_log(level: &str, args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() || args.len() > 2 {
        return Err(VmFault::TypeMismatch {
            expected: "1 or 2 arguments (message, [fields]) for log".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let msg = match &args[0] {
        Value::String(s) => s.as_str().to_string(),
        other => other.to_string(),
    };

    let line = if args.len() == 2 {
        let fields_str = match &args[1] {
            Value::Dict(_) => args[1].to_string(),
            other => format!("{{{other}}}"),
        };
        format!("[{level}] {msg} {fields_str}")
    } else {
        format!("[{level}] {msg}")
    };

    if let Ok(mut guard) = LOG_SINK.lock()
        && let Some(sink) = guard.as_mut()
    {
        let _ = writeln!(sink, "{line}");
        return Ok(Value::None);
    }

    crate::io::write_output(&line, true)?;
    Ok(Value::None)
}

/// `log.trace(message, [fields])`
pub fn log_trace(args: &[Value]) -> Result<Value, VmFault> {
    emit_log("TRACE", args)
}

/// `log.debug(message, [fields])`
pub fn log_debug(args: &[Value]) -> Result<Value, VmFault> {
    emit_log("DEBUG", args)
}

/// `log.info(message, [fields])`
pub fn log_info(args: &[Value]) -> Result<Value, VmFault> {
    emit_log("INFO", args)
}

/// `log.warning(message, [fields])`
pub fn log_warning(args: &[Value]) -> Result<Value, VmFault> {
    emit_log("WARNING", args)
}

/// `log.error(message, [fields])`
pub fn log_error(args: &[Value]) -> Result<Value, VmFault> {
    emit_log("ERROR", args)
}

/// Constructs the canonical `log` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let mut entries = Vec::new();
    macro_rules! reg {
        ($name:expr, $func:expr) => {
            entries.push((
                Value::String(Rc::new($name.to_string())),
                Value::native(concat!("log.", $name), usize::MAX, $func),
            ));
        };
    }

    reg!("trace", log_trace);
    reg!("debug", log_debug);
    reg!("info", log_info);
    reg!("warning", log_warning);
    reg!("error", log_error);

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
