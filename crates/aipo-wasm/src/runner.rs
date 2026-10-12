//! WebAssembly JIT runtime execution engine for Aipo programs using Wasmtime (ADP-013).

use std::io::Write;
#[cfg(feature = "wasmtime")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "wasmtime")]
use wasmtime::{Caller, Config, Engine, Linker, Module, Store};

/// Errors encountered during WebAssembly execution.
#[derive(Debug)]
pub enum WasmRuntimeError {
    /// Failure during module instantiation.
    Instantiation(String),
    /// Failure during function execution (trap or runtime fault).
    Execution(String),
    /// Exported symbol was not found in the module.
    MissingExport(String),
}

impl std::fmt::Display for WasmRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instantiation(msg) => write!(f, "Wasm instantiation error: {msg}"),
            Self::Execution(msg) => {
                if msg.contains("divide by zero") {
                    write!(
                        f,
                        "[AIPO_RT_DIV_ZERO] runtime fault [AIPO_RT_DIV_ZERO]: division by zero"
                    )
                } else if msg.contains("all fuel consumed by WebAssembly")
                    || msg.contains("out of fuel")
                {
                    write!(
                        f,
                        "[AIPO_RT_OVERFLOW] runtime fault [AIPO_RT_OVERFLOW]: execution budget exceeded: fuel limit reached"
                    )
                } else {
                    write!(f, "Wasm execution error: {msg}")
                }
            }
            Self::MissingExport(name) => write!(f, "Wasm missing export: `{name}`"),
        }
    }
}

impl std::error::Error for WasmRuntimeError {}

/// Disassembles WebAssembly binary bytes into the standard WebAssembly Text format (WAT).
///
/// # Errors
///
/// Returns an error string if the bytes are not valid WebAssembly binary format.
pub fn disassemble_wasm(wasm_bytes: &[u8]) -> Result<String, String> {
    wasmprinter::print_bytes(wasm_bytes).map_err(|e| e.to_string())
}

/// State held by the Wasmtime store during execution.
#[cfg(feature = "wasmtime")]
#[derive(Clone)]
struct HostState {
    output: Arc<Mutex<Vec<u8>>>,
}

#[cfg(feature = "wasmtime")]
impl HostState {
    fn write_str(&self, text: &str) {
        if let Ok(mut buf) = self.output.lock() {
            buf.extend_from_slice(text.as_bytes());
        }
    }
}

/// Executes a compiled WebAssembly module, routing host I/O (`print`, `println`) to `stdout`.
///
/// Looks for an entrypoint function in the following priority order:
/// 1. `__top_level__`
/// 2. `run`
/// 3. `main`
///
/// Executes a compiled WebAssembly module, routing host I/O (`print`, `println`) to `stdout`.
///
/// Looks for an entrypoint function in the following priority order:
/// 1. `__top_level__`
/// 2. `run`
/// 3. `main`
///
/// # Errors
///
/// Returns a [`WasmRuntimeError`] if instantiation fails or execution traps.
#[cfg(feature = "wasmtime")]
pub fn execute_wasm(wasm_bytes: &[u8], stdout: &mut dyn Write) -> Result<i64, WasmRuntimeError> {
    execute_wasm_with_fuel(wasm_bytes, stdout, None)
}

/// Executes a compiled WebAssembly module with optional execution fuel budget (S6 / ADP-003).
///
/// If `fuel` is `Some(limit)`, execution consumes fuel on each WebAssembly instruction and
/// traps with an out-of-fuel error if the budget is exhausted. Passing `None` runs with unlimited fuel.
///
/// # Errors
///
/// Returns a [`WasmRuntimeError`] if instantiation fails or execution traps.
#[cfg(feature = "wasmtime")]
pub fn execute_wasm_with_fuel(
    wasm_bytes: &[u8],
    stdout: &mut dyn Write,
    fuel: Option<u64>,
) -> Result<i64, WasmRuntimeError> {
    let mut config = Config::new();
    if fuel.is_some() {
        config.consume_fuel(true);
    }
    let engine =
        Engine::new(&config).map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;
    let module = Module::new(&engine, wasm_bytes)
        .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

    let output = Arc::new(Mutex::new(Vec::new()));
    let state = HostState {
        output: Arc::clone(&output),
    };
    let mut store = Store::new(&engine, state);
    if let Some(limit) = fuel {
        store
            .set_fuel(limit)
            .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;
    }
    let mut linker = Linker::new(&engine);

    // Register host I/O functions under module "aipo_host"
    linker
        .func_wrap(
            "aipo_host",
            "print_int",
            |caller: Caller<'_, HostState>, val: i64| {
                caller.data().write_str(&val.to_string());
            },
        )
        .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

    linker
        .func_wrap(
            "aipo_host",
            "print_float",
            |caller: Caller<'_, HostState>, val: f64| {
                if val.fract() == 0.0 && val.abs() < 1e15 {
                    caller.data().write_str(&format!("{val:.1}"));
                } else {
                    caller.data().write_str(&val.to_string());
                }
            },
        )
        .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

    linker
        .func_wrap(
            "aipo_host",
            "print_str",
            |mut caller: Caller<'_, HostState>, ptr: i32| {
                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };
                let data = mem.data(&caller);
                let ptr = ptr as usize;
                if ptr + 4 <= data.len() {
                    let len = u32::from_le_bytes(data[ptr..ptr + 4].try_into().unwrap_or([0; 4]))
                        as usize;
                    if ptr + 4 + len <= data.len() {
                        if let Ok(s) = std::str::from_utf8(&data[ptr + 4..ptr + 4 + len]) {
                            caller.data().write_str(s);
                        }
                    }
                }
            },
        )
        .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

    linker
        .func_wrap(
            "aipo_host",
            "print_bool",
            |caller: Caller<'_, HostState>, val: i32| {
                caller
                    .data()
                    .write_str(if val != 0 { "true" } else { "false" });
            },
        )
        .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

    linker
        .func_wrap("aipo_host", "println", |caller: Caller<'_, HostState>| {
            caller.data().write_str("\n");
        })
        .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

    let result = (|| -> Result<i64, WasmRuntimeError> {
        if let Ok(func) = instance.get_typed_func::<(), i64>(&mut store, "__top_level__") {
            return func
                .call(&mut store, ())
                .map_err(|e| WasmRuntimeError::Execution(format!("{e:#}")));
        }
        if let Ok(func) = instance.get_typed_func::<(), ()>(&mut store, "__top_level__") {
            func.call(&mut store, ())
                .map_err(|e| WasmRuntimeError::Execution(format!("{e:#}")))?;
            return Ok(0);
        }
        if let Ok(func) = instance.get_typed_func::<(), i64>(&mut store, "run") {
            return func
                .call(&mut store, ())
                .map_err(|e| WasmRuntimeError::Execution(format!("{e:#}")));
        }
        if let Ok(func) = instance.get_typed_func::<(), ()>(&mut store, "run") {
            func.call(&mut store, ())
                .map_err(|e| WasmRuntimeError::Execution(format!("{e:#}")))?;
            return Ok(0);
        }
        if let Ok(func) = instance.get_typed_func::<(), i64>(&mut store, "main") {
            return func
                .call(&mut store, ())
                .map_err(|e| WasmRuntimeError::Execution(format!("{e:#}")));
        }
        if let Ok(func) = instance.get_typed_func::<(), ()>(&mut store, "main") {
            func.call(&mut store, ())
                .map_err(|e| WasmRuntimeError::Execution(format!("{e:#}")))?;
            return Ok(0);
        }
        Ok(0)
    })();

    // Flush any buffered output to the destination writer
    if let Ok(buf) = output.lock() {
        if !buf.is_empty() {
            let _ = stdout.write_all(&buf);
            let _ = stdout.flush();
        }
    }

    result
}

/// Fallback execution function when `wasmtime` is disabled.
///
/// # Errors
/// Always returns [`WasmRuntimeError::Execution`] indicating that the execution engine is disabled.
#[cfg(not(feature = "wasmtime"))]
pub fn execute_wasm(_wasm_bytes: &[u8], _stdout: &mut dyn Write) -> Result<i64, WasmRuntimeError> {
    Err(WasmRuntimeError::Execution(
        "WebAssembly JIT execution is disabled in this build (wasmtime feature not enabled)"
            .to_string(),
    ))
}

/// Fallback execution function with fuel when `wasmtime` is disabled.
///
/// # Errors
/// Always returns [`WasmRuntimeError::Execution`] indicating that the execution engine is disabled.
#[cfg(not(feature = "wasmtime"))]
pub fn execute_wasm_with_fuel(
    wasm_bytes: &[u8],
    stdout: &mut dyn Write,
    _fuel: Option<u64>,
) -> Result<i64, WasmRuntimeError> {
    execute_wasm(wasm_bytes, stdout)
}
