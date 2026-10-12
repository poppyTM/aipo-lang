//! WebAssembly JIT runtime execution engine for Aipo programs using Wasmtime (ADP-013).

use std::io::Write;
#[cfg(feature = "wasmtime")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "wasmtime")]
use wasmtime::{Caller, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

/// Optional execution limits for a single Wasmtime invocation.
#[derive(Debug, Clone, Copy, Default)]
pub struct WasmExecutionOptions {
    /// Wasmtime instruction fuel; `None` preserves unlimited execution.
    /// Fuel does not bound native host work, memory, output or wall-clock time.
    pub fuel: Option<u64>,
    /// Maximum bytes in each linear memory; `None` preserves the old default.
    pub memory_bytes: Option<usize>,
    /// Maximum buffered host output; exceeding it traps before extending the buffer.
    pub output_bytes: Option<usize>,
}

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
struct HostState {
    output: Arc<Mutex<Vec<u8>>>,
    output_limit: Option<usize>,
    limits: StoreLimits,
}

#[cfg(feature = "wasmtime")]
impl HostState {
    fn write_str(&self, text: &str) -> wasmtime::Result<()> {
        let mut buf = self
            .output
            .lock()
            .map_err(|_| wasmtime::Error::msg("output buffer poisoned"))?;
        if self
            .output_limit
            .is_some_and(|limit| text.len() > limit.saturating_sub(buf.len()))
        {
            return Err(wasmtime::Error::msg("Aipo output budget exhausted"));
        }
        buf.extend_from_slice(text.as_bytes());
        Ok(())
    }
}

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
    execute_wasm_with_options(wasm_bytes, stdout, WasmExecutionOptions::default())
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
    execute_wasm_with_options(
        wasm_bytes,
        stdout,
        WasmExecutionOptions {
            fuel,
            ..Default::default()
        },
    )
}

/// Executes a module with optional instruction fuel and checked output delivery.
///
/// # Errors
/// Returns an error on invalid modules, exhausted fuel, missing entrypoints or writer errors.
#[cfg(feature = "wasmtime")]
pub fn execute_wasm_with_options(
    wasm_bytes: &[u8],
    stdout: &mut dyn Write,
    options: WasmExecutionOptions,
) -> Result<i64, WasmRuntimeError> {
    let mut runner = WasmRunner::new()?;
    runner.execute(wasm_bytes, stdout, options)
}

/// Reusable engine and bounded compiled-module cache. Each invocation uses a fresh store.
#[cfg(feature = "wasmtime")]
pub struct WasmRunner {
    engine: Engine,
    cache: std::collections::VecDeque<(Vec<u8>, Module)>,
    cache_capacity: usize,
}
#[cfg(feature = "wasmtime")]
impl WasmRunner {
    /// Creates a fuel-capable engine, reusable across executions.
    pub fn new() -> Result<Self, WasmRuntimeError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine =
            Engine::new(&config).map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;
        Ok(Self {
            engine,
            cache: std::collections::VecDeque::new(),
            cache_capacity: 8,
        })
    }
    /// Sets the maximum number of compiled modules retained (zero disables caching).
    pub fn set_cache_capacity(&mut self, capacity: usize) {
        self.cache_capacity = capacity;
        while self.cache.len() > capacity {
            self.cache.pop_front();
        }
    }
    /// Executes with isolated memory, globals, fuel and output limits.
    pub fn execute(
        &mut self,
        wasm_bytes: &[u8],
        stdout: &mut dyn Write,
        options: WasmExecutionOptions,
    ) -> Result<i64, WasmRuntimeError> {
        let engine = &self.engine;
        let module =
            if let Some(index) = self.cache.iter().position(|(bytes, _)| bytes == wasm_bytes) {
                let entry = self.cache.remove(index).expect("known cache position");
                let module = entry.1.clone();
                self.cache.push_back(entry);
                module
            } else {
                let module = Module::new(engine, wasm_bytes)
                    .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;
                if self.cache_capacity > 0 {
                    if self.cache.len() >= self.cache_capacity {
                        self.cache.pop_front();
                    }
                    self.cache.push_back((wasm_bytes.to_vec(), module.clone()));
                }
                module
            };
        let output = Arc::new(Mutex::new(Vec::new()));
        let state = HostState {
            output: Arc::clone(&output),
            output_limit: options.output_bytes,
            limits: options.memory_bytes.map_or_else(
                || StoreLimitsBuilder::new().build(),
                |bytes| {
                    StoreLimitsBuilder::new()
                        .memory_size(bytes)
                        .trap_on_grow_failure(true)
                        .build()
                },
            ),
        };
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        {
            let fuel = options.fuel.unwrap_or(u64::MAX);
            store
                .set_fuel(fuel)
                .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;
        }
        let mut linker = Linker::new(engine);

        // Register host I/O functions under module "aipo_host"
        linker
            .func_wrap(
                "aipo_host",
                "print_int",
                |caller: Caller<'_, HostState>, val: i64| caller.data().write_str(&val.to_string()),
            )
            .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

        linker
            .func_wrap(
                "aipo_host",
                "print_float",
                |caller: Caller<'_, HostState>, val: f64| {
                    if val.fract() == 0.0 && val.abs() < 1e15 {
                        caller.data().write_str(&format!("{val:.1}"))
                    } else {
                        caller.data().write_str(&val.to_string())
                    }
                },
            )
            .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

        linker
            .func_wrap(
                "aipo_host",
                "print_str",
                |mut caller: Caller<'_, HostState>, ptr: i32| -> wasmtime::Result<()> {
                    let invalid = || wasmtime::Error::msg("invalid Aipo string pointer or UTF-8");
                    let mem = caller
                        .get_export("memory")
                        .and_then(|e| e.into_memory())
                        .ok_or_else(invalid)?;
                    let data = mem.data(&caller);
                    let ptr = usize::try_from(ptr).map_err(|_| invalid())?;
                    let start = ptr.checked_add(4).ok_or_else(invalid)?;
                    let header: [u8; 4] = data
                        .get(ptr..start)
                        .ok_or_else(invalid)?
                        .try_into()
                        .map_err(|_| invalid())?;
                    let len = u32::from_le_bytes(header) as usize;
                    let end = start.checked_add(len).ok_or_else(invalid)?;
                    let text = std::str::from_utf8(data.get(start..end).ok_or_else(invalid)?)
                        .map_err(|_| invalid())?;
                    caller.data().write_str(text)
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
                        .write_str(if val != 0 { "true" } else { "false" })
                },
            )
            .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

        linker
            .func_wrap("aipo_host", "println", |caller: Caller<'_, HostState>| {
                caller.data().write_str("\n")
            })
            .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;

        let result = (|| -> Result<i64, WasmRuntimeError> {
            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| WasmRuntimeError::Instantiation(e.to_string()))?;
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
            Err(WasmRuntimeError::MissingExport(
                "__top_level__, run or main with signature () -> i64 or () -> ()".to_string(),
            ))
        })();

        // Attempt to deliver output even after a guest trap. Preserve the original
        // execution error when execution and output delivery both fail.
        let output_result = (|| {
            let buf = output
                .lock()
                .map_err(|e| WasmRuntimeError::Execution(e.to_string()))?;
            stdout
                .write_all(&buf)
                .map_err(|e| WasmRuntimeError::Execution(e.to_string()))?;
            stdout
                .flush()
                .map_err(|e| WasmRuntimeError::Execution(e.to_string()))
        })();
        match result {
            Err(error) => Err(error),
            Ok(value) => output_result.map(|()| value),
        }
    }
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

/// Fallback for builds without the Wasmtime execution engine.
///
/// # Errors
/// Always reports that JIT execution is disabled, regardless of the requested fuel.
#[cfg(not(feature = "wasmtime"))]
pub fn execute_wasm_with_options(
    wasm_bytes: &[u8],
    stdout: &mut dyn Write,
    _options: WasmExecutionOptions,
) -> Result<i64, WasmRuntimeError> {
    execute_wasm(wasm_bytes, stdout)
}
