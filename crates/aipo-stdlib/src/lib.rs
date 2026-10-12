//! Standard library for the Aipo programming language.
//!
//! Provides the canonical Prelude V1 and standard library modules (`math`, `string`, `io`,
//! `task`, `time`, `env`, `fs`), registering their native functions and module dictionaries into
//! the Aipo VM and NativeRegistry.
//!
//! Modules whose readings depend on the host (`time`, `env`, `fs`) are capability-gated: the function exists
//! and faults with `AIPO_RT_CAPABILITY_DENIED` until the host installs the service, so a denied
//! capability is never disguised as a missing or faked API.
//!
//! See `docs/stdlib/mvp-subset.md` for the closed V1 surface implemented here and
//! `docs/adp/ADP-001-byte-and-core-types-as-values.md` for the recorded representation gaps.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod binary;
pub mod bytes;
pub mod collections;
pub mod convert;
pub mod duration;
pub mod encoding;
pub mod env;
pub mod fs;
pub mod io;
pub mod json;
pub mod log;
pub mod math;
pub mod path;
pub mod prelude;
pub mod random;
#[cfg(feature = "regex")]
pub mod regex;
pub mod sh;
pub mod string;
pub mod task;
pub mod testing;
pub mod time;
pub mod url;

use aipo_host::Capability;
use aipo_runtime::{NativeFunctionMeta, NativeRegistry};
use aipo_vm::{TypeTag, Value, Vm, VmFault};

pub use fs::{MapFilesystemSource, fs_schema, install_filesystem_source};

/// Registers the canonical Prelude V1 and all standard library modules into the VM and NativeRegistry.
pub fn register_stdlib(vm: &mut Vm, registry: &mut NativeRegistry) {
    register_prelude(vm, registry);
    register_math(registry);
    register_string(registry);
    register_io(registry);
    register_task(registry);
    register_time(registry);
    env::register_natives(vm);
    register_env(registry);
    fs::register_natives(vm);
    fs::register_metadata(registry);
    register_random(registry);
    register_json(registry);
    register_encoding(registry);
    register_binary(registry);
    register_path(registry);
    register_sh(registry);
    register_url(registry);
    #[cfg(feature = "regex")]
    register_regex(registry);
    register_testing(registry);
    register_log(registry);
    register_modules(vm);
    register_methods(vm);
}

/// Binds a module-style function as a receiver-first method.
///
/// Canon spells the module forms with the receiver in the first position
/// (`string.contains(text, sub)`), so a method is just the same native with the
/// receiver prepended: `text.contains(sub)`. Generating the adapter keeps the two
/// spellings on exactly one implementation.
macro_rules! receiver_first {
    ($wrapper:ident, $target:path) => {
        fn $wrapper(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
            let mut all: Vec<Value> = Vec::with_capacity(args.len() + 1);
            all.push(receiver.clone());
            all.extend_from_slice(args);
            $target(&all)
        }
    };
}

receiver_first!(method_string_len, string::string_len);
receiver_first!(method_string_byte_len, string::string_byte_len);
receiver_first!(method_string_contains, string::string_contains);
receiver_first!(method_string_starts_with, string::string_starts_with);
receiver_first!(method_string_ends_with, string::string_ends_with);
receiver_first!(method_string_find, string::string_find);
receiver_first!(method_string_lower, string::string_lower);
receiver_first!(method_string_upper, string::string_upper);
receiver_first!(method_string_capitalize, string::string_capitalize);
receiver_first!(method_string_reverse, string::string_reverse);
receiver_first!(method_string_trim, string::string_trim);
receiver_first!(method_string_split, string::string_split);
receiver_first!(method_string_join, string::string_join);
receiver_first!(method_string_replace, string::string_replace);
receiver_first!(method_string_slice, string::string_slice);
receiver_first!(method_string_format, string::string_format);
receiver_first!(method_string_graphemes, string::string_graphemes);
receiver_first!(method_string_words, string::string_words);
receiver_first!(method_string_lines, string::string_lines);
receiver_first!(method_string_casefold, string::string_casefold);

/// Registers dot-call sugar for the core types so `text.upper()` and `list.len()` resolve.
///
/// Mutation during iteration is enforced by the VM, not here, so every mutating method
/// registered below is rejected with `MutationDuringIteration` while its receiver is
/// inside an active `each` or higher-order iteration.
fn register_methods(vm: &mut Vm) {
    vm.register_method_native("String", "len", 0, method_string_len);
    vm.register_method_native("String", "byte_len", 0, method_string_byte_len);
    vm.register_method_native("String", "contains", 1, method_string_contains);
    vm.register_method_native("String", "starts_with", 1, method_string_starts_with);
    vm.register_method_native("String", "ends_with", 1, method_string_ends_with);
    vm.register_method_native("String", "find", 1, method_string_find);
    vm.register_method_native("String", "lower", 0, method_string_lower);
    vm.register_method_native("String", "upper", 0, method_string_upper);
    vm.register_method_native("String", "capitalize", 0, method_string_capitalize);
    vm.register_method_native("String", "reverse", 0, method_string_reverse);
    vm.register_method_native("String", "trim", 0, method_string_trim);
    vm.register_method_native("String", "split", 1, method_string_split);
    vm.register_method_native("String", "join", 1, method_string_join);
    vm.register_method_native("String", "replace", 2, method_string_replace);
    vm.register_method_native("String", "slice", 2, method_string_slice);
    vm.register_method_native("String", "format", 1, method_string_format);
    vm.register_method_native("String", "graphemes", 0, method_string_graphemes);
    vm.register_method_native("String", "words", 0, method_string_words);
    vm.register_method_native("String", "lines", 0, method_string_lines);
    vm.register_method_native("String", "casefold", 0, method_string_casefold);

    collections::register_methods(vm);
    bytes::register_methods(vm);
    duration::register_methods(vm);
    time::register_methods(vm);
    random::register_methods(vm);
    #[cfg(feature = "regex")]
    regex::register_methods(vm);
}

pub(crate) fn native(
    name: &str,
    arity: usize,
    func: fn(&[Value]) -> Result<Value, aipo_vm::VmFault>,
) -> Value {
    Value::native(name, arity, func)
}

/// Registers Prelude V1 metadata (including core-type conversions) and globally available values.
fn register_prelude(vm: &mut Vm, registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "len",
        1,
        None,
        "Computes the length of a String, List, or Dict.",
    ));
    registry.register(NativeFunctionMeta::new(
        "copy",
        1,
        None,
        "Shallow copies a collection or struct.",
    ));
    registry.register(NativeFunctionMeta::new(
        "same",
        2,
        None,
        "Checks reference identity for references or equality for primitives.",
    ));
    registry.register(NativeFunctionMeta::new(
        "some",
        1,
        None,
        "Checks whether a value is not none.",
    ));
    registry.register(NativeFunctionMeta::new(
        "fail",
        1,
        None,
        "Creates a recoverable Failure value.",
    ));
    registry.register(NativeFunctionMeta::new(
        "Int",
        1,
        None,
        "Explicit conversion to Int (Float truncates toward zero, String is parsed).",
    ));
    registry.register(NativeFunctionMeta::new(
        "Float",
        1,
        None,
        "Explicit conversion to Float (Int widens, String is parsed).",
    ));
    registry.register(NativeFunctionMeta::new(
        "Byte",
        1,
        None,
        "Explicit, verified conversion to Byte in 0..=255.",
    ));
    registry.register(NativeFunctionMeta::new(
        "String",
        1,
        None,
        "Canonical textual conversion for String, Int, Float, Bool, and none.",
    ));

    vm.define_global("none", Value::None);
    vm.define_global("true", Value::Bool(true));
    vm.define_global("false", Value::Bool(false));

    vm.define_global("len", native("len", 1, prelude::native_len));
    vm.define_global("copy", native("copy", 1, prelude::native_copy));
    vm.define_global("same", native("same", 2, prelude::native_same));
    vm.define_global("some", native("some", 1, prelude::native_some));
    vm.define_global("fail", native("fail", 1, prelude::native_fail));

    // The conversion forms are first-class type values: `Int` is a value that can be
    // called, compared, passed around and used with `is`, all sharing one dispatch.
    vm.define_global("Int", Value::Type(TypeTag::Int));
    vm.define_global("Float", Value::Type(TypeTag::Float));
    vm.define_global("Byte", Value::Type(TypeTag::Byte));
    vm.define_global("String", Value::Type(TypeTag::String));
    vm.define_global("Bool", Value::Type(TypeTag::Bool));
    vm.define_global("List", Value::Type(TypeTag::List));
    vm.define_global("Dict", Value::Type(TypeTag::Dict));
    vm.define_global("Bytes", Value::Type(TypeTag::Bytes));
    vm.define_global("Range", Value::Type(TypeTag::Range));
    vm.define_global("Set", Value::Type(TypeTag::Set));
    vm.define_global("Duration", Value::Type(TypeTag::Duration));
}

/// Signature of a core-type conversion native.
pub type ConversionNative = fn(&[Value]) -> Result<Value, VmFault>;

/// Keeps the conversion natives reachable from Rust callers and tests.
#[must_use]
pub fn conversion_natives() -> [(&'static str, ConversionNative); 4] {
    [
        ("Int", convert::convert_int),
        ("Float", convert::convert_float),
        ("Byte", convert::convert_byte),
        ("String", convert::convert_string),
    ]
}

/// Registers `math` module metadata.
fn register_math(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "abs",
        1,
        Some("math"),
        "Computes absolute value.",
    ));
    registry.register(NativeFunctionMeta::new(
        "min",
        2,
        Some("math"),
        "Returns the minimum of two numbers.",
    ));
    registry.register(NativeFunctionMeta::new(
        "max",
        2,
        Some("math"),
        "Returns the maximum of two numbers.",
    ));
    registry.register(NativeFunctionMeta::new(
        "floor",
        1,
        Some("math"),
        "Floors number to integer.",
    ));
    registry.register(NativeFunctionMeta::new(
        "ceil",
        1,
        Some("math"),
        "Ceils number to integer.",
    ));
    registry.register(NativeFunctionMeta::new(
        "round",
        1,
        Some("math"),
        "Rounds number to nearest integer, half away from zero.",
    ));
    registry.register(NativeFunctionMeta::new(
        "truncate",
        1,
        Some("math"),
        "Truncates toward zero, returning Int when representable.",
    ));
    registry.register(NativeFunctionMeta::new(
        "sqrt",
        1,
        Some("math"),
        "Computes square root.",
    ));
    registry.register(NativeFunctionMeta::new(
        "pow",
        2,
        Some("math"),
        "Computes base raised to exponent.",
    ));
    registry.register(NativeFunctionMeta::new(
        "clamp",
        3,
        Some("math"),
        "Constrains a number to the inclusive [min, max] interval.",
    ));
    registry.register(NativeFunctionMeta::new(
        "sin",
        1,
        Some("math"),
        "Computes sine of angle in radians.",
    ));
    registry.register(NativeFunctionMeta::new(
        "cos",
        1,
        Some("math"),
        "Computes cosine of angle in radians.",
    ));
    registry.register(NativeFunctionMeta::new(
        "tan",
        1,
        Some("math"),
        "Computes tangent of angle in radians.",
    ));
    registry.register(NativeFunctionMeta::new(
        "asin",
        1,
        Some("math"),
        "Computes arc sine in radians.",
    ));
    registry.register(NativeFunctionMeta::new(
        "acos",
        1,
        Some("math"),
        "Computes arc cosine in radians.",
    ));
    registry.register(NativeFunctionMeta::new(
        "atan",
        1,
        Some("math"),
        "Computes arc tangent in radians.",
    ));
    registry.register(NativeFunctionMeta::new(
        "atan2",
        2,
        Some("math"),
        "Computes two-argument arc tangent.",
    ));
    registry.register(NativeFunctionMeta::new(
        "hypot",
        2,
        Some("math"),
        "Computes Euclidean distance sqrt(x^2 + y^2).",
    ));
    registry.register(NativeFunctionMeta::new(
        "log",
        1,
        Some("math"),
        "Computes natural logarithm (base e).",
    ));
    registry.register(NativeFunctionMeta::new(
        "log2",
        1,
        Some("math"),
        "Computes base-2 logarithm.",
    ));
    registry.register(NativeFunctionMeta::new(
        "log10",
        1,
        Some("math"),
        "Computes base-10 logarithm.",
    ));
    registry.register(NativeFunctionMeta::new(
        "exp",
        1,
        Some("math"),
        "Computes exponential e^x.",
    ));
    registry.register(NativeFunctionMeta::new(
        "sign",
        1,
        Some("math"),
        "Returns the sign of a number (-1, 0, or 1).",
    ));
    registry.register(NativeFunctionMeta::new(
        "rad",
        1,
        Some("math"),
        "Converts degrees to radians.",
    ));
    registry.register(NativeFunctionMeta::new(
        "deg",
        1,
        Some("math"),
        "Converts radians to degrees.",
    ));
}

/// Registers `string` module metadata.
fn register_string(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "len",
        1,
        Some("string"),
        "Computes string code point length.",
    ));
    registry.register(NativeFunctionMeta::new(
        "byte_len",
        1,
        Some("string"),
        "Computes the UTF-8 byte length of a string.",
    ));
    registry.register(NativeFunctionMeta::new(
        "contains",
        2,
        Some("string"),
        "Checks substring inclusion.",
    ));
    registry.register(NativeFunctionMeta::new(
        "starts_with",
        2,
        Some("string"),
        "Checks prefix match.",
    ));
    registry.register(NativeFunctionMeta::new(
        "ends_with",
        2,
        Some("string"),
        "Checks suffix match.",
    ));
    registry.register(NativeFunctionMeta::new(
        "find",
        2,
        Some("string"),
        "Returns the code point index of the first match, or none.",
    ));
    registry.register(NativeFunctionMeta::new(
        "lower",
        1,
        Some("string"),
        "Converts to lowercase.",
    ));
    registry.register(NativeFunctionMeta::new(
        "upper",
        1,
        Some("string"),
        "Converts to uppercase.",
    ));
    registry.register(NativeFunctionMeta::new(
        "capitalize",
        1,
        Some("string"),
        "Titlecases the first cased character and lowercases the rest.",
    ));
    registry.register(NativeFunctionMeta::new(
        "reverse",
        1,
        Some("string"),
        "Reverses extended grapheme clusters.",
    ));
    registry.register(NativeFunctionMeta::new(
        "trim",
        1,
        Some("string"),
        "Trims whitespace.",
    ));
    registry.register(NativeFunctionMeta::new(
        "split",
        2,
        Some("string"),
        "Splits string by a non-empty separator.",
    ));
    registry.register(NativeFunctionMeta::new(
        "join",
        2,
        Some("string"),
        "Joins a List of Strings with a separator.",
    ));
    registry.register(NativeFunctionMeta::new(
        "replace",
        3,
        Some("string"),
        "Replaces occurrences of a non-empty substring.",
    ));
    registry.register(NativeFunctionMeta::new(
        "slice",
        3,
        Some("string"),
        "Slices substring by character index range.",
    ));
    registry.register(NativeFunctionMeta::new(
        "format",
        2,
        Some("string"),
        "Substitutes named placeholders from a Dict into a runtime template.",
    ));
    // The catalog mirrors the `string` module dictionary the VM exposes; a member
    // present at runtime but missing here would make the frontend reject a valid
    // call, so the two lists are kept equal (`string::create_module`).
    registry.register(NativeFunctionMeta::new(
        "graphemes",
        1,
        Some("string"),
        "Splits into extended grapheme clusters as a List.",
    ));
    registry.register(NativeFunctionMeta::new(
        "words",
        1,
        Some("string"),
        "Splits on whitespace into a List of words.",
    ));
    registry.register(NativeFunctionMeta::new(
        "lines",
        1,
        Some("string"),
        "Splits into a List of lines.",
    ));
    registry.register(NativeFunctionMeta::new(
        "casefold",
        1,
        Some("string"),
        "Unicode case folding for comparison.",
    ));
}

/// Registers `io` module metadata.
///
/// `io` is the one module whose arity is *target-dependent*: on the VM the two
/// natives take exactly one value, while the WebAssembly/JavaScript host IO form is
/// the call itself (`io.print("a")`, `io.println()` to end a line — see
/// `tests/wasm_cli.rs`). The catalog is shared by every target, so these entries
/// record the permissive lower bound and leave the count to the target.
fn register_io(registry: &mut NativeRegistry) {
    registry.register(
        NativeFunctionMeta::new(
            "print",
            0,
            Some("io"),
            "Prints value without trailing newline.",
        )
        .with_variadic(),
    );
    registry.register(
        NativeFunctionMeta::new(
            "println",
            0,
            Some("io"),
            "Prints value with trailing newline.",
        )
        .with_variadic(),
    );
}

/// Registers `task` module metadata.
fn register_task(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "spawn",
        2,
        Some("task"),
        "Spawns a new asynchronous task.",
    ));
    registry.register(NativeFunctionMeta::new(
        "sleep",
        1,
        Some("task"),
        "Suspends current task until virtual clock reaches deadline.",
    ));
    registry.register(NativeFunctionMeta::new(
        "all",
        1,
        Some("task"),
        "Awaits completion of all tasks in a list.",
    ));
    registry.register(NativeFunctionMeta::new(
        "race",
        1,
        Some("task"),
        "Races tasks, returning the first result.",
    ));
    registry.register(NativeFunctionMeta::new(
        "timeout",
        2,
        Some("task"),
        "Awaits task with a virtual tick deadline.",
    ));
    registry.register(NativeFunctionMeta::new(
        "cancel",
        1,
        Some("task"),
        "Cancels an active task.",
    ));
    registry.register(NativeFunctionMeta::new(
        "group",
        0,
        Some("task"),
        "Creates a structured-concurrency task group.",
    ));
}

/// Defines the standard module dictionaries as VM globals.
fn register_modules(vm: &mut Vm) {
    vm.define_global("math", math::create_module());
    vm.define_global("string", string::create_module());
    vm.define_global("io", io::create_module());
    vm.define_global("task", task::create_module());
    vm.define_global("time", time::create_module());
    vm.define_global("env", env::create_module());
    vm.define_global("fs", fs::create_module());
    vm.define_global("random", random::create_module());
    vm.define_global("json", json::create_module());
    vm.define_global("encoding", encoding::create_module());
    vm.define_global("binary", binary::create_module());
    vm.define_global("path", path::create_module());
    vm.define_global("url", url::create_module());
    #[cfg(feature = "regex")]
    vm.define_global("regex", regex::create_module());
    vm.define_global("expect", testing::create_expect_module());
    vm.define_global("testing", testing::create_module());
    vm.define_global("test", Value::native("test", 2, testing::test_declaration));
    vm.define_global("log", log::create_module());
    vm.define_global("sh", sh::create_module());
}

/// Registers `testing` module metadata.
fn register_testing(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "test",
        2,
        None,
        "Declares a unit test block with name and callback.",
    ));
    registry.register(NativeFunctionMeta::new(
        "test",
        2,
        Some("testing"),
        "Declares a unit test block with name and callback.",
    ));
    macro_rules! reg {
        ($name:expr, $arity:expr, $doc:expr) => {
            registry.register(NativeFunctionMeta::new($name, $arity, Some("expect"), $doc));
            registry.register(NativeFunctionMeta::new(
                $name,
                $arity,
                Some("testing"),
                $doc,
            ));
        };
    }
    reg!(
        "equal",
        2,
        "Asserts actual == expected, returning none or Failure."
    );
    reg!(
        "not_equal",
        2,
        "Asserts actual != expected, returning none or Failure."
    );
    reg!(
        "true",
        1,
        "Asserts value is true, returning none or Failure."
    );
    reg!(
        "false",
        1,
        "Asserts value is false, returning none or Failure."
    );
    reg!(
        "none",
        1,
        "Asserts value is none, returning none or Failure."
    );
    reg!(
        "some",
        1,
        "Asserts value is not none, returning none or Failure."
    );
    reg!(
        "failure",
        1,
        "Asserts value is a Failure, returning none or Failure."
    );
    reg!(
        "contains",
        2,
        "Asserts collection contains element, returning none or Failure."
    );
    registry.register(
        NativeFunctionMeta::new(
            "approx",
            2,
            Some("expect"),
            "Asserts floating point difference <= tolerance, returning none or Failure.",
        )
        .with_variadic(),
    );
    registry.register(
        NativeFunctionMeta::new(
            "approx",
            2,
            Some("testing"),
            "Asserts floating point difference <= tolerance, returning none or Failure.",
        )
        .with_variadic(),
    );
}

/// Registers `log` module metadata.
fn register_log(registry: &mut NativeRegistry) {
    macro_rules! reg {
        ($name:expr, $doc:expr) => {
            // `log.<nivel>(mensagem, [campos])`: o segundo argumento é opcional.
            registry.register(NativeFunctionMeta::new($name, 1, Some("log"), $doc).with_variadic());
        };
    }
    reg!("trace", "Emits structured log at TRACE level.");
    reg!("debug", "Emits structured log at DEBUG level.");
    reg!("info", "Emits structured log at INFO level.");
    reg!("warning", "Emits structured log at WARNING level.");
    reg!("error", "Emits structured log at ERROR level.");
}

/// Registers `binary` module metadata.
fn register_binary(registry: &mut NativeRegistry) {
    macro_rules! reg {
        ($name:expr, $arity:expr, $doc:expr) => {
            registry.register(NativeFunctionMeta::new($name, $arity, Some("binary"), $doc));
        };
    }
    reg!("read_i8", 2, "Reads a signed 8-bit integer at offset.");
    reg!("read_u8", 2, "Reads an unsigned 8-bit integer at offset.");
    reg!(
        "read_i16_le",
        2,
        "Reads a signed 16-bit integer (little-endian) at offset."
    );
    reg!(
        "read_i16_be",
        2,
        "Reads a signed 16-bit integer (big-endian) at offset."
    );
    reg!(
        "read_u16_le",
        2,
        "Reads an unsigned 16-bit integer (little-endian) at offset."
    );
    reg!(
        "read_u16_be",
        2,
        "Reads an unsigned 16-bit integer (big-endian) at offset."
    );
    reg!(
        "read_i32_le",
        2,
        "Reads a signed 32-bit integer (little-endian) at offset."
    );
    reg!(
        "read_i32_be",
        2,
        "Reads a signed 32-bit integer (big-endian) at offset."
    );
    reg!(
        "read_u32_le",
        2,
        "Reads an unsigned 32-bit integer (little-endian) at offset."
    );
    reg!(
        "read_u32_be",
        2,
        "Reads an unsigned 32-bit integer (big-endian) at offset."
    );
    reg!(
        "read_i64_le",
        2,
        "Reads a signed 64-bit integer (little-endian) at offset."
    );
    reg!(
        "read_i64_be",
        2,
        "Reads a signed 64-bit integer (big-endian) at offset."
    );
    reg!(
        "read_u64_le",
        2,
        "Reads an unsigned 64-bit integer (little-endian) at offset."
    );
    reg!(
        "read_u64_be",
        2,
        "Reads an unsigned 64-bit integer (big-endian) at offset."
    );
    reg!(
        "read_f32_le",
        2,
        "Reads a 32-bit float (little-endian) at offset."
    );
    reg!(
        "read_f32_be",
        2,
        "Reads a 32-bit float (big-endian) at offset."
    );
    reg!(
        "read_f64_le",
        2,
        "Reads a 64-bit float (little-endian) at offset."
    );
    reg!(
        "read_f64_be",
        2,
        "Reads a 64-bit float (big-endian) at offset."
    );

    reg!("write_i8", 3, "Writes a signed 8-bit integer at offset.");
    reg!("write_u8", 3, "Writes an unsigned 8-bit integer at offset.");
    reg!(
        "write_i16_le",
        3,
        "Writes a signed 16-bit integer (little-endian) at offset."
    );
    reg!(
        "write_i16_be",
        3,
        "Writes a signed 16-bit integer (big-endian) at offset."
    );
    reg!(
        "write_u16_le",
        3,
        "Writes an unsigned 16-bit integer (little-endian) at offset."
    );
    reg!(
        "write_u16_be",
        3,
        "Writes an unsigned 16-bit integer (big-endian) at offset."
    );
    reg!(
        "write_i32_le",
        3,
        "Writes a signed 32-bit integer (little-endian) at offset."
    );
    reg!(
        "write_i32_be",
        3,
        "Writes a signed 32-bit integer (big-endian) at offset."
    );
    reg!(
        "write_u32_le",
        3,
        "Writes an unsigned 32-bit integer (little-endian) at offset."
    );
    reg!(
        "write_u32_be",
        3,
        "Writes an unsigned 32-bit integer (big-endian) at offset."
    );
    reg!(
        "write_i64_le",
        3,
        "Writes a signed 64-bit integer (little-endian) at offset."
    );
    reg!(
        "write_i64_be",
        3,
        "Writes a signed 64-bit integer (big-endian) at offset."
    );
    reg!(
        "write_u64_le",
        3,
        "Writes an unsigned 64-bit integer (little-endian) at offset."
    );
    reg!(
        "write_u64_be",
        3,
        "Writes an unsigned 64-bit integer (big-endian) at offset."
    );
    reg!(
        "write_f32_le",
        3,
        "Writes a 32-bit float (little-endian) at offset."
    );
    reg!(
        "write_f32_be",
        3,
        "Writes a 32-bit float (big-endian) at offset."
    );
    reg!(
        "write_f64_le",
        3,
        "Writes a 64-bit float (little-endian) at offset."
    );
    reg!(
        "write_f64_be",
        3,
        "Writes a 64-bit float (big-endian) at offset."
    );

    reg!(
        "read_varint",
        2,
        "Reads an unsigned LEB128 varint returning [value, bytes_read]."
    );
    reg!(
        "write_varint",
        3,
        "Writes an unsigned LEB128 varint returning bytes_written."
    );
    reg!(
        "slice",
        3,
        "Slices a byte buffer with tolerant bounds returning new Bytes."
    );
}

/// Registers `path` module metadata.
fn register_path(registry: &mut NativeRegistry) {
    registry.register(
        NativeFunctionMeta::new(
            "join",
            1,
            Some("path"),
            "Joins multiple path segments into a normalized path.",
        )
        .with_variadic(),
    );
    registry.register(NativeFunctionMeta::new(
        "normalize",
        1,
        Some("path"),
        "Logically normalizes a path resolving '.' and '..' segments.",
    ));
    registry.register(NativeFunctionMeta::new(
        "is_absolute",
        1,
        Some("path"),
        "Returns true if the path starts with a root separator or drive letter.",
    ));
    registry.register(
        NativeFunctionMeta::new(
            "basename",
            1,
            Some("path"),
            "Returns the final component of a path, optionally stripping an extension.",
        )
        .with_variadic(),
    );
    registry.register(NativeFunctionMeta::new(
        "dirname",
        1,
        Some("path"),
        "Returns the directory component of a path.",
    ));
    registry.register(NativeFunctionMeta::new(
        "ext",
        1,
        Some("path"),
        "Returns the extension portion of a path including the dot.",
    ));
}

/// Registers `sh` system automation module metadata.
fn register_sh(registry: &mut NativeRegistry) {
    registry.register(
        NativeFunctionMeta::new(
            "run",
            2,
            Some("sh"),
            "Executes a system process command, returning a dictionary with code, stdout, stderr, ok.",
        )
        .with_variadic(),
    );
    registry.register(NativeFunctionMeta::new(
        "cd",
        1,
        Some("sh"),
        "Changes current working directory.",
    ));
    registry.register(NativeFunctionMeta::new(
        "pwd",
        0,
        Some("sh"),
        "Returns current working directory.",
    ));
    registry.register(NativeFunctionMeta::new(
        "env",
        1,
        Some("sh"),
        "Retrieves an environment variable by name or none.",
    ));
    registry.register(NativeFunctionMeta::new(
        "which",
        1,
        Some("sh"),
        "Resolves executable path from PATH environment or none.",
    ));
}

/// Registers `encoding` module metadata.
fn register_encoding(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "base64_encode",
        1,
        Some("encoding"),
        "Encodes Bytes or String into standard Base64 string.",
    ));
    registry.register(NativeFunctionMeta::new(
        "base64_decode",
        1,
        Some("encoding"),
        "Decodes standard Base64 string into Bytes, returning Failure on error.",
    ));
    registry.register(NativeFunctionMeta::new(
        "base64url_encode",
        1,
        Some("encoding"),
        "Encodes Bytes or String into unpadded URL-safe Base64 string.",
    ));
    registry.register(NativeFunctionMeta::new(
        "base64url_decode",
        1,
        Some("encoding"),
        "Decodes URL-safe Base64 string into Bytes, returning Failure on error.",
    ));
    registry.register(NativeFunctionMeta::new(
        "hex_encode",
        1,
        Some("encoding"),
        "Encodes Bytes or String into lowercase hexadecimal string.",
    ));
    registry.register(NativeFunctionMeta::new(
        "hex_decode",
        1,
        Some("encoding"),
        "Decodes hexadecimal string into Bytes, returning Failure on error.",
    ));
    registry.register(NativeFunctionMeta::new(
        "utf8_encode",
        1,
        Some("encoding"),
        "Encodes String into UTF-8 Bytes.",
    ));
    registry.register(NativeFunctionMeta::new(
        "utf8_decode",
        1,
        Some("encoding"),
        "Decodes UTF-8 Bytes into String, returning Failure on error.",
    ));
}

/// Registers `json` module metadata.
fn register_json(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "parse",
        1,
        Some("json"),
        "Parses JSON text into an Aipo value, returning Failure on error.",
    ));
    registry.register(NativeFunctionMeta::new(
        "stringify",
        1,
        Some("json"),
        "Serializes an Aipo value to a JSON string (optional pretty = false).",
    ));
}

/// Registers `random` module metadata.
fn register_random(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "create",
        1,
        Some("random"),
        "Creates a deterministic Rng generator instance.",
    ));
    registry.register(NativeFunctionMeta::new(
        "seed",
        1,
        Some("random"),
        "Seeds the default generator.",
    ));
    registry.register(NativeFunctionMeta::new(
        "int",
        2,
        Some("random"),
        "Generates a random integer in inclusive [min, max].",
    ));
    registry.register(NativeFunctionMeta::new(
        "float",
        0,
        Some("random"),
        "Generates a random float in [0.0, 1.0).",
    ));
    registry.register(NativeFunctionMeta::new(
        "bool",
        0,
        Some("random"),
        "Generates a random boolean.",
    ));
    registry.register(NativeFunctionMeta::new(
        "choice",
        1,
        Some("random"),
        "Selects a random element from a list.",
    ));
    registry.register(NativeFunctionMeta::new(
        "shuffle",
        1,
        Some("random"),
        "Returns a new list with elements shuffled.",
    ));
}

/// Registers `time` module metadata.
///
/// Both functions are capability-gated at call time, so they are always present as natives and
/// a denied profile reports the denial instead of an undefined name.
fn register_time(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "now",
        0,
        Some("time"),
        "Reads the wall clock (capability `clock.wall`).",
    ));
    registry.register(NativeFunctionMeta::new(
        "monotonic",
        0,
        Some("time"),
        "Reads the monotonic clock (capability `clock.monotonic`).",
    ));
    registry.register(NativeFunctionMeta::new(
        "date",
        3,
        Some("time"),
        "Creates a Date struct instance (year, month, day).",
    ));
    registry.register(
        NativeFunctionMeta::new(
            "time_of_day",
            2,
            Some("time"),
            "Creates a TimeOfDay struct instance (hour, minute, [second, millisecond]).",
        )
        .with_variadic(),
    );
    registry.register(
        NativeFunctionMeta::new(
            "date_time",
            2,
            Some("time"),
            "Creates a DateTime struct instance from date/time components or objects.",
        )
        .with_variadic(),
    );
    registry.register(NativeFunctionMeta::new(
        "parse_date",
        1,
        Some("time"),
        "Parses a date string in YYYY-MM-DD format into a Date struct.",
    ));
    registry.register(NativeFunctionMeta::new(
        "parse_time",
        1,
        Some("time"),
        "Parses a time string in HH:MM[:SS[.sss]] format into a TimeOfDay struct.",
    ));
    registry.register(NativeFunctionMeta::new(
        "parse_iso",
        1,
        Some("time"),
        "Parses an ISO 8601 string into a DateTime struct.",
    ));
    registry.register(NativeFunctionMeta::new(
        "parse_datetime",
        1,
        Some("time"),
        "Parses an ISO 8601 string into a DateTime struct.",
    ));
    registry.register(NativeFunctionMeta::new(
        "duration",
        1,
        Some("time"),
        "Creates a Duration value with given seconds.",
    ));
}

/// Registers `env` module metadata.
fn register_env(registry: &mut NativeRegistry) {
    registry.register(
        NativeFunctionMeta::new(
            "get",
            1,
            Some("env"),
            "Reads an environment value, or none when the name is absent.",
        )
        .with_capabilities([Capability::ENV_READ]),
    );
    registry.register(
        NativeFunctionMeta::new(
            "has",
            1,
            Some("env"),
            "Checks whether an environment name is present.",
        )
        .with_capabilities([Capability::ENV_READ]),
    );
}

/// Registers `url` module metadata.
fn register_url(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "parse",
        1,
        Some("url"),
        "Parses a URL string into components according to WHATWG model.",
    ));
}

/// Registers `regex` module metadata.
#[cfg(feature = "regex")]
fn register_regex(registry: &mut NativeRegistry) {
    registry.register(NativeFunctionMeta::new(
        "compile",
        1,
        Some("regex"),
        "Compiles a regular expression string into a Pattern struct.",
    ));
    registry.register(NativeFunctionMeta::new(
        "is_match",
        2,
        Some("regex"),
        "Tests if a regex pattern matches text.",
    ));
    registry.register(NativeFunctionMeta::new(
        "replace",
        3,
        Some("regex"),
        "Replaces regex matches in text with replacement string.",
    ));
}
