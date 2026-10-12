//! Canonical `io` module for Aipo.
//!
//! Provides text output functions:
//! - `print`: outputs text without trailing newline
//! - `println`: outputs text with trailing newline
//!
//! Includes a thread-safe test sink configuration hook for deterministic testing.

use aipo_vm::{DictMap, Value, VmFault};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex};

type DynWriter = Box<dyn Write + Send>;
type SafeSink = Arc<Mutex<Option<DynWriter>>>;

static OUTPUT_SINK: LazyLock<SafeSink> = LazyLock::new(|| Arc::new(Mutex::new(None)));

/// Configures a custom output sink (useful for unit tests and embedded runners).
pub fn set_output_sink(sink: Option<Box<dyn Write + Send>>) {
    if let Ok(mut guard) = OUTPUT_SINK.lock() {
        *guard = sink;
    }
}

/// Writes text to the configured output sink, or stdout if none is configured.
///
/// # Errors
/// Returns `VmFault::CorruptedBytecode` if an I/O error occurs during write.
pub fn write_output(text: &str, newline: bool) -> Result<(), VmFault> {
    if let Ok(mut guard) = OUTPUT_SINK.lock()
        && let Some(sink) = guard.as_mut()
    {
        if newline {
            writeln!(sink, "{text}").map_err(|e| VmFault::CorruptedBytecode {
                offset: 0,
                reason: format!("I/O write error: {e}"),
            })?;
        } else {
            write!(sink, "{text}").map_err(|e| VmFault::CorruptedBytecode {
                offset: 0,
                reason: format!("I/O write error: {e}"),
            })?;
        }
        return Ok(());
    }

    if newline {
        println!("{text}");
    } else {
        print!("{text}");
        let _ = std::io::stdout().flush();
    }
    Ok(())
}

fn value_to_text(val: &Value) -> String {
    match val {
        Value::String(s) => s.as_str().to_string(),
        other => other.to_string(),
    }
}

/// Prints value to standard output or configured sink without trailing newline.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument count is not 1.
pub fn io_print(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let text = value_to_text(&args[0]);
    write_output(&text, false)?;
    Ok(Value::None)
}

/// Prints value to standard output or configured sink with a trailing newline.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument count is not 1.
pub fn io_println(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let text = value_to_text(&args[0]);
    write_output(&text, true)?;
    Ok(Value::None)
}

/// Constructs the canonical `io` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("print".to_string())),
            Value::native("io.print", 1, io_print),
        ),
        (
            Value::String(Rc::new("println".to_string())),
            Value::native("io.println", 1, io_println),
        ),
    ];

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
