//! Deterministic PRNG (`random`) module for Aipo.
//!
//! Provides a portable SplitMix64 deterministic pseudo-random number generator:
//! - `random.create(seed)`: creates an independent `Rng` instance
//! - `random.seed(seed)`: sets the seed of the default generator
//! - `random.int(min, max)`: random integer in `min..=max`
//! - `random.float()`: random float in `[0.0, 1.0)`
//! - `random.bool()`: random boolean
//! - `random.choice(list)`: random element from list
//! - `random.shuffle(list)`: returns a new list with elements shuffled

#![forbid(unsafe_code)]

use aipo_vm::{
    DictMap, FailureValue, StructInstance, Value, Vm, VmFault, check_finite_float, check_safe_int,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Mutex;

const SPLITMIX_INC: u64 = 0x9e37_79b9_7f4a_7c15;
const SPLITMIX_MUL1: u64 = 0xbf58_476d_1ce4_e5b9;
const SPLITMIX_MUL2: u64 = 0x94d0_49bb_1331_11eb;

static DEFAULT_SEED: Mutex<(i64, i64)> = Mutex::new((0, 0));

/// Resets the default PRNG seed and step count.
pub fn reset_default_seed(seed: i64) {
    let mut guard = DEFAULT_SEED.lock().unwrap_or_else(|e| e.into_inner());
    *guard = (seed, 0);
}

/// Advances SplitMix64 step given seed and step count.
#[must_use]
pub fn splitmix64_step(seed: i64, step: i64) -> u64 {
    let s = (seed as u64).wrapping_add((step as u64).wrapping_mul(SPLITMIX_INC));
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(SPLITMIX_MUL1);
    z = (z ^ (z >> 27)).wrapping_mul(SPLITMIX_MUL2);
    z ^ (z >> 31)
}

/// Advances an `Rng` struct receiver, returning the 64-bit random word.
fn next_word_from_rng(receiver: &Value) -> Result<u64, VmFault> {
    if let Value::Struct(inst) = receiver {
        let mut b = inst.borrow_mut();
        if b.type_name != "Rng" {
            return Err(VmFault::TypeMismatch {
                expected: "Rng struct instance".to_string(),
                actual: b.type_name.clone(),
            });
        }
        let seed = match b.get_field("seed") {
            Some(Value::Int(s)) => *s,
            _ => 0,
        };
        let step = match b.get_field("step") {
            Some(Value::Int(st)) => *st,
            _ => 0,
        };
        let word = splitmix64_step(seed, step);
        let next_step = step.wrapping_add(1) & 0x001F_FFFF_FFFF_FFFF;
        let _ = b.set_field("step", Value::Int(next_step));
        Ok(word)
    } else {
        Err(VmFault::TypeMismatch {
            expected: "Rng struct receiver".to_string(),
            actual: receiver.type_name().to_string(),
        })
    }
}

/// Advances the default process PRNG.
fn next_word_default() -> u64 {
    let mut guard = DEFAULT_SEED.lock().unwrap_or_else(|e| e.into_inner());
    let (seed, step) = *guard;
    let word = splitmix64_step(seed, step);
    let next_step = step.wrapping_add(1) & 0x001F_FFFF_FFFF_FFFF;
    *guard = (seed, next_step);
    word
}

/// Creates a new `Rng` struct instance.
#[must_use]
pub fn create_rng_instance(seed: i64) -> Value {
    let inst = StructInstance {
        type_name: "Rng".to_string(),
        fields: vec![
            ("seed".to_string(), Value::Int(seed)),
            ("step".to_string(), Value::Int(0)),
        ],
        fixed_fields: HashSet::new(),
        under_construction: false,
    };
    Value::Struct(Rc::new(RefCell::new(inst)))
}

fn generate_int(word: u64, min: i64, max: i64) -> Result<Value, VmFault> {
    if min > max {
        return Ok(Value::Failure(Rc::new(FailureValue::new(
            "random.int: min must be <= max".to_string(),
        ))));
    }
    if min == max {
        return check_safe_int(min).map(Value::Int);
    }
    let span = (max - min + 1) as u64;
    let n = (word % span) as i64;
    check_safe_int(min + n).map(Value::Int)
}

fn generate_float(word: u64) -> Result<Value, VmFault> {
    let bits = word >> 11;
    #[allow(clippy::cast_precision_loss)]
    let f = (bits as f64) * (1.0 / 9_007_199_254_740_992.0);
    check_finite_float(f).map(Value::Float)
}

fn generate_bool(word: u64) -> Value {
    Value::Bool((word & 1) == 1)
}

fn generate_choice(word: u64, list_val: &Value) -> Result<Value, VmFault> {
    if let Value::List(l) = list_val {
        let items = l.borrow();
        if items.is_empty() {
            return Ok(Value::Failure(Rc::new(FailureValue::new(
                "random.choice: cannot choose from empty list".to_string(),
            ))));
        }
        let idx = (word % items.len() as u64) as usize;
        Ok(items[idx].clone())
    } else {
        Err(VmFault::TypeMismatch {
            expected: "List".to_string(),
            actual: list_val.type_name().to_string(),
        })
    }
}

// Methods on Rng struct instance: receiver: &Value, args: &[Value]

/// Generates a random integer in `min..=max` using the `Rng` instance.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are invalid.
pub fn method_rng_int(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments (min, max)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let min = match &args[0] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int min".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let max = match &args[1] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int max".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let word = next_word_from_rng(receiver)?;
    generate_int(word, min, max)
}

/// Generates a random float in `[0.0, 1.0)` using the `Rng` instance.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are passed.
pub fn method_rng_float(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if !args.is_empty() {
        return Err(VmFault::TypeMismatch {
            expected: "0 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let word = next_word_from_rng(receiver)?;
    generate_float(word)
}

/// Generates a random boolean using the `Rng` instance.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are passed.
pub fn method_rng_bool(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if !args.is_empty() {
        return Err(VmFault::TypeMismatch {
            expected: "0 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let word = next_word_from_rng(receiver)?;
    Ok(generate_bool(word))
}

/// Selects a random element from a list using the `Rng` instance.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a list.
pub fn method_rng_choice(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (list)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let word = next_word_from_rng(receiver)?;
    generate_choice(word, &args[0])
}

/// Returns a new list with elements shuffled using the `Rng` instance.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a list.
pub fn method_rng_shuffle(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (list)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::List(l) = &args[0] {
        let mut items = l.borrow().to_vec();
        if items.len() > 1 {
            for i in (1..items.len()).rev() {
                let word = next_word_from_rng(receiver)?;
                let j = (word % (i as u64 + 1)) as usize;
                items.swap(i, j);
            }
        }
        Ok(Value::List(Rc::new(RefCell::new((items).into()))))
    } else {
        Err(VmFault::TypeMismatch {
            expected: "List".to_string(),
            actual: args[0].type_name().to_string(),
        })
    }
}

// Module-level functions on `random`

/// Creates an independent deterministic `Rng` instance.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if seed is not an Int.
pub fn random_create(args: &[Value]) -> Result<Value, VmFault> {
    let seed = match args.first() {
        Some(Value::Int(s)) => *s,
        Some(other) => {
            return Err(VmFault::TypeMismatch {
                expected: "Int seed".to_string(),
                actual: other.type_name().to_string(),
            });
        }
        None => 0,
    };
    Ok(create_rng_instance(seed))
}

/// Seeds the default PRNG generator.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if seed is not an Int.
pub fn random_seed(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let seed = match &args[0] {
        Value::Int(s) => *s,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int seed".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut guard = DEFAULT_SEED.lock().unwrap_or_else(|e| e.into_inner());
    *guard = (seed, 0);
    Ok(Value::None)
}

/// Generates a random integer in `min..=max` using the default generator.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are invalid.
pub fn random_int(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments (min, max)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let min = match &args[0] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int min".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let max = match &args[1] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int max".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let word = next_word_default();
    generate_int(word, min, max)
}

/// Generates a random float in `[0.0, 1.0)` using the default generator.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are passed.
pub fn random_float(args: &[Value]) -> Result<Value, VmFault> {
    if !args.is_empty() {
        return Err(VmFault::TypeMismatch {
            expected: "0 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let word = next_word_default();
    generate_float(word)
}

/// Generates a random boolean using the default generator.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are passed.
pub fn random_bool(args: &[Value]) -> Result<Value, VmFault> {
    if !args.is_empty() {
        return Err(VmFault::TypeMismatch {
            expected: "0 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let word = next_word_default();
    Ok(generate_bool(word))
}

/// Selects a random element from a list using the default generator.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a list.
pub fn random_choice(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (list)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let word = next_word_default();
    generate_choice(word, &args[0])
}

/// Returns a new list with elements shuffled using the default generator.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a list.
pub fn random_shuffle(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (list)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::List(l) = &args[0] {
        let mut items = l.borrow().to_vec();
        if items.len() > 1 {
            for i in (1..items.len()).rev() {
                let word = next_word_default();
                let j = (word % (i as u64 + 1)) as usize;
                items.swap(i, j);
            }
        }
        Ok(Value::List(Rc::new(RefCell::new((items).into()))))
    } else {
        Err(VmFault::TypeMismatch {
            expected: "List".to_string(),
            actual: args[0].type_name().to_string(),
        })
    }
}

/// Registers `Rng` struct native methods on the VM.
pub fn register_methods(vm: &mut Vm) {
    vm.register_method_native("Rng", "int", 2, method_rng_int);
    vm.register_method_native("Rng", "float", 0, method_rng_float);
    vm.register_method_native("Rng", "bool", 0, method_rng_bool);
    vm.register_method_native("Rng", "choice", 1, method_rng_choice);
    vm.register_method_native("Rng", "shuffle", 1, method_rng_shuffle);
}

/// Creates the canonical `random` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("create".to_string())),
            Value::native("random.create", 1, random_create),
        ),
        (
            Value::String(Rc::new("seed".to_string())),
            Value::native("random.seed", 1, random_seed),
        ),
        (
            Value::String(Rc::new("int".to_string())),
            Value::native("random.int", 2, random_int),
        ),
        (
            Value::String(Rc::new("float".to_string())),
            Value::native("random.float", 0, random_float),
        ),
        (
            Value::String(Rc::new("bool".to_string())),
            Value::native("random.bool", 0, random_bool),
        ),
        (
            Value::String(Rc::new("choice".to_string())),
            Value::native("random.choice", 1, random_choice),
        ),
        (
            Value::String(Rc::new("shuffle".to_string())),
            Value::native("random.shuffle", 1, random_shuffle),
        ),
    ];
    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
