//! Value model for the Aipo VM runtime.

use crate::convert::TypeTag;
use crate::fault::VmFault;
use aipo_host::Handle;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
#[cfg(feature = "unicode")]
use unicode_normalization::UnicodeNormalization;

/// Maximum safe integer in Aipo: 2^53 - 1.
pub const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;
/// Minimum safe integer in Aipo: -(2^53 - 1).
pub const MIN_SAFE_INT: i64 = -9_007_199_254_740_991;

/// Reads one Unicode scalar without allocating a temporary character vector.
/// Negative indices count from the end; even `i64::MIN` fails without overflow.
pub(crate) fn string_char_at(text: &str, index: i64) -> Result<String, VmFault> {
    let character = if text.is_ascii() {
        let position = if index < 0 {
            usize::try_from(index.unsigned_abs())
                .ok()
                .and_then(|distance| text.len().checked_sub(distance))
        } else {
            usize::try_from(index).ok()
        };
        position
            .and_then(|position| text.as_bytes().get(position).copied())
            .map(char::from)
    } else if index < 0 {
        usize::try_from(index.unsigned_abs() - 1)
            .ok()
            .and_then(|distance| text.chars().nth_back(distance))
    } else {
        usize::try_from(index)
            .ok()
            .and_then(|position| text.chars().nth(position))
    };
    character
        .map(|character| character.to_string())
        .ok_or_else(|| VmFault::IndexOutOfRange {
            index,
            len: text.chars().count(),
        })
}

/// Validates that an integer is within the normative ±(2^53 - 1) boundary.
///
/// # Errors
/// Returns `VmFault::Overflow` if outside safe integer bounds.
pub fn check_safe_int(n: i64) -> Result<i64, VmFault> {
    if (MIN_SAFE_INT..=MAX_SAFE_INT).contains(&n) {
        Ok(n)
    } else {
        Err(VmFault::Overflow {
            details: format!("{n} exceeds integer range ±(2^53 - 1)"),
        })
    }
}

/// Validates that a float is finite (not NaN or Infinity).
///
/// # Errors
/// Returns `VmFault::NonFiniteFloat` if NaN or Infinite.
pub fn check_finite_float(f: f64) -> Result<f64, VmFault> {
    if f.is_finite() {
        Ok(f)
    } else {
        Err(VmFault::NonFiniteFloat)
    }
}

/// Recoverable failure payload (Model B).
#[derive(Debug, Clone, PartialEq)]
pub struct FailureValue {
    /// Human-readable explanation of the failure.
    pub message: String,
    /// Structured payload associated with the failure, or `Value::None` if plain.
    pub payload: Value,
}

impl FailureValue {
    /// Constructs a plain failure with no structured payload.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            payload: Value::None,
        }
    }

    /// Constructs a structured failure carrying a payload value.
    #[must_use]
    pub fn with_payload(message: impl Into<String>, payload: Value) -> Self {
        Self {
            message: message.into(),
            payload,
        }
    }
}

/// Instance of a user-defined struct.
#[derive(Debug, Clone, PartialEq)]
pub struct StructInstance {
    /// Name of the struct type.
    pub type_name: String,
    /// Ordered field name and value pairs.
    pub fields: Vec<(String, Value)>,
    /// Set of fields marked as `fixed` (immutable after initialization).
    pub fixed_fields: HashSet<String>,
    /// `true` while the instance is still being built by an `init` hook.
    ///
    /// Canon allows `fixed` fields to receive their value during construction — including from
    /// `init` — and forbids replacing them only after the instance is published. The flag is set
    /// when a deferred construction starts and cleared by [`StructInstance::seal`].
    pub under_construction: bool,
}

impl StructInstance {
    /// Retrieves a reference to a field's value.
    #[must_use]
    pub fn get_field(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    /// Mutates an existing field value, validating `fixed` constraints.
    ///
    /// # Errors
    /// Returns `VmFault::FixedFieldMutation` if the field is fixed,
    /// or `VmFault::NoSuchField` if the field does not exist.
    pub fn set_field(&mut self, name: &str, new_value: Value) -> Result<(), VmFault> {
        if !self.under_construction && self.fixed_fields.contains(name) {
            return Err(VmFault::FixedFieldMutation {
                type_name: self.type_name.clone(),
                field: name.to_string(),
            });
        }
        if let Some((_, v)) = self.fields.iter_mut().find(|(k, _)| k == name) {
            *v = new_value;
            Ok(())
        } else {
            Err(VmFault::NoSuchField {
                type_name: self.type_name.clone(),
                field: name.to_string(),
            })
        }
    }

    /// Updates a field already resolved to its layout slot.
    ///
    /// # Errors
    /// Returns the same fixed-field and missing-field faults as [`Self::set_field`] when the
    /// cached slot is not valid for this instance.
    pub fn set_field_at(
        &mut self,
        index: usize,
        field_name: &str,
        new_value: Value,
    ) -> Result<(), VmFault> {
        if !self.under_construction && self.fixed_fields.contains(field_name) {
            return Err(VmFault::FixedFieldMutation {
                type_name: self.type_name.clone(),
                field: field_name.to_string(),
            });
        }
        let Some((_, value)) = self.fields.get_mut(index) else {
            return Err(VmFault::NoSuchField {
                type_name: self.type_name.clone(),
                field: field_name.to_string(),
            });
        };
        *value = new_value;
        Ok(())
    }

    /// Restores a field to a journaled value during an invariant rollback.
    ///
    /// Canon returns the direct protected fields of the participating instances to their
    /// entry values when a boundary validation fails, so a rollback bypasses the `fixed`
    /// guard: it writes back the value the field already held.
    pub fn restore_field(&mut self, name: &str, value: Value) {
        if let Some((_, slot)) = self.fields.iter_mut().find(|(k, _)| k == name) {
            *slot = value;
        }
    }

    /// Publishes the instance once construction finishes.
    ///
    /// Canon's `init` runs with `self` still in construction, so `fixed` fields are mutable
    /// until this point. Sealing restores the declared `fixed` set and clears the flag, which
    /// makes every later assignment to those fields a [`VmFault::FixedFieldMutation`].
    pub fn seal(&mut self, fixed_fields: HashSet<String>) {
        self.fixed_fields = fixed_fields;
        self.under_construction = false;
    }
}

/// Ordered dictionary preserving insertion order.
///
/// Entries live in a `Vec` so iteration order is stable and structural equality
/// stays order-sensitive. A secondary index accelerates the overwhelmingly common
/// case — lookup and upsert by `String` key — while every other key category
/// keeps the exact linear-scan semantics. The index only ever points at the
/// *first* matching entry, mirroring what a scan would find.
#[derive(Clone, Default)]
pub struct DictMap {
    entries: Vec<(Value, Value)>,
    str_index: HashMap<String, usize>,
    revision: u64,
}

impl fmt::Debug for DictMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.entries)
    }
}

impl PartialEq for DictMap {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl FromIterator<(Value, Value)> for DictMap {
    fn from_iter<I: IntoIterator<Item = (Value, Value)>>(iter: I) -> Self {
        Self::from_entries(iter.into_iter().collect())
    }
}

impl DictMap {
    /// Builds a map from entries in order, indexing the first occurrence of
    /// each `String` key.
    #[must_use]
    pub fn from_entries(entries: Vec<(Value, Value)>) -> Self {
        let mut map = Self {
            entries,
            str_index: HashMap::new(),
            revision: 0,
        };
        map.rebuild_index();
        map
    }

    /// Structural mutation revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in insertion order.
    #[must_use]
    pub fn entries(&self) -> &[(Value, Value)] {
        &self.entries
    }

    /// Looks up a key: `String` keys use the index, every other category scans,
    /// exactly as a full scan would.
    #[must_use]
    pub fn get(&self, key: &Value) -> Option<&Value> {
        if let Value::String(s) = key {
            if let Some(&pos) = self.str_index.get(s.as_str()) {
                return self.entries.get(pos).map(|(_, v)| v);
            }
            return None;
        }
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Updates the first matching entry or pushes a new one, preserving order.
    pub fn upsert(&mut self, key: Value, value: Value) {
        if let Value::String(s) = &key {
            if let Some(&pos) = self.str_index.get(s.as_str())
                && let Some((_, v)) = self.entries.get_mut(pos)
            {
                *v = value;
                return;
            }
        } else if let Some((_, v)) = self.entries.iter_mut().find(|(k, _)| k == &key) {
            *v = value;
            return;
        }
        let pos = self.entries.len();
        if let Value::String(s) = &key {
            self.str_index.entry(s.to_string()).or_insert(pos);
        }
        self.revision = self.revision.wrapping_add(1);
        self.entries.push((key, value));
    }

    /// Removes the first matching entry, reporting whether one existed.
    pub fn remove(&mut self, key: &Value) -> bool {
        let pos = self.position(key);
        match pos {
            Some(at) => {
                self.revision = self.revision.wrapping_add(1);
                self.entries.remove(at);
                self.rebuild_index();
                true
            }
            None => false,
        }
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        self.entries.clear();
        self.str_index.clear();
    }

    /// Values in insertion order.
    #[must_use]
    pub fn values(&self) -> Vec<Value> {
        self.entries.iter().map(|(_, v)| v.clone()).collect()
    }

    /// Keys in insertion order.
    #[must_use]
    pub fn keys(&self) -> Vec<Value> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    fn position(&self, key: &Value) -> Option<usize> {
        if let Value::String(s) = key {
            self.str_index.get(s.as_str()).copied()
        } else {
            self.entries.iter().position(|(k, _)| k == key)
        }
    }

    fn rebuild_index(&mut self) {
        self.str_index.clear();
        for (pos, (key, _)) in self.entries.iter().enumerate() {
            if let Value::String(s) = key {
                self.str_index.entry(s.to_string()).or_insert(pos);
            }
        }
    }
}

/// Identifier of a scheduler-managed task.
pub type TaskId = u64;

/// Identifier of a structured-concurrency group.
pub type GroupId = u64;

/// Materialized source of a lazy [`SequencePipeline`]: a snapshot taken by
/// `.lazy()`, so later mutations of the origin do not affect the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum SequenceSource {
    /// Elements of a list, in order.
    List(Vec<Value>),
    /// Values of a dict, in insertion order.
    Dict(Vec<Value>),
    /// Members of a set, in insertion order.
    Set(Vec<Value>),
    /// Integers of a half-open range.
    Range {
        /// Inclusive start.
        start: i64,
        /// Exclusive end.
        end: i64,
    },
}

/// One lazy pipeline stage. Stages holding callables (`Map`, `Filter`, …)
/// are driven by the VM, which can invoke Aipo closures; the pipeline itself
/// stays plain data so it remains target-neutral.
#[derive(Debug, Clone, PartialEq)]
pub enum SeqOp {
    /// Transform each element.
    Map(Value),
    /// Keep elements with a truthy predicate result.
    Filter(Value),
    /// Map each element to a list and concatenate.
    FlatMap(Value),
    /// Keep the first `n` elements.
    Take(i64),
    /// Drop the first `n` elements.
    Skip(i64),
    /// First element with a truthy predicate result (`none` if absent).
    Find(Value),
    /// Whether any element has a truthy predicate result.
    Any(Value),
    /// Whether every element has a truthy predicate result.
    All(Value),
    /// Count elements, or those with a truthy predicate result.
    Count(Option<Value>),
    /// Fold with an initial value.
    Reduce {
        /// Initial accumulator value.
        initial: Value,
        /// Binary folding function.
        func: Value,
    },
    /// Group elements by key into a `Dict` of lists.
    GroupBy(Value),
    /// Drop later duplicates, keeping first-occurrence order.
    Distinct,
    /// Pairwise zip with a snapshot taken at call time (stops at the shorter side).
    Zip(Vec<Value>),
    /// Concatenate a snapshot taken at call time after this pipeline.
    Chain(Vec<Value>),
    /// Non-overlapping chunks of `n` as lists.
    Chunk(i64),
    /// Sliding windows of `n` as lists.
    Window(i64),
    /// `[index, value]` pairs as two-element lists.
    Enumerate,
}

/// Lazy pipeline value: a snapshotted source plus unevaluated stages.
#[derive(Debug, Clone)]
pub struct SequencePipeline {
    /// Snapshotted source elements.
    pub source: SequenceSource,
    /// Unevaluated stages, in order.
    pub ops: Vec<SeqOp>,
    /// First-consumption snapshot of the pure prefix: `each` (Len + GetIndex
    /// per iteration) and repeated `collect` calls drain the same pipeline, so
    /// the memo keeps element callbacks running exactly once. Stages are
    /// strict at consumption; the memo never crosses pipeline construction
    /// (every op append builds a fresh pipeline).
    pub cached: RefCell<Option<Vec<Value>>>,
}

impl PartialEq for SequencePipeline {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.ops == other.ops
    }
}

impl SequencePipeline {
    /// Builds an unevaluated pipeline over a snapshotted source.
    #[must_use]
    pub fn new(source: SequenceSource, ops: Vec<SeqOp>) -> Self {
        Self {
            source,
            ops,
            cached: RefCell::new(None),
        }
    }
}

/// Half-open integer range data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeData {
    /// Inclusive start.
    pub start: i64,
    /// Exclusive end.
    pub end: i64,
}

impl RangeData {
    /// Constructs a new range.
    #[must_use]
    pub const fn new(start: i64, end: i64) -> Self {
        Self { start, end }
    }
}

/// Bound struct method metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct StructMethodData {
    /// Receiver struct instance.
    pub receiver: Rc<RefCell<StructInstance>>,
    /// Entry instruction pointer in the bytecode.
    pub entry_ip: u32,
    /// Total arity including receiver parameter.
    pub total_arity: u16,
    /// Whether the method is async.
    pub is_async: bool,
}

impl StructMethodData {
    /// Constructs new struct method metadata.
    #[must_use]
    pub fn new(
        receiver: Rc<RefCell<StructInstance>>,
        entry_ip: u32,
        total_arity: u16,
        is_async: bool,
    ) -> Self {
        Self {
            receiver,
            entry_ip,
            total_arity,
            is_async,
        }
    }
}

/// Dynamic value manipulated by the Aipo VM stack machine.
///
/// Sharing (`List`, `Dict`, struct instances, closure upvalue cells) is
/// `Rc<RefCell<…>>`: the interpreter is single-threaded by decision (see the
/// `aipo-vm` crate contract on threading), so `Value` is intentionally `!Send`.
/// A concurrent host must keep one VM per thread or replace this primitive first.
#[derive(Clone)]
pub enum Value {
    /// Absence of a value (`none`).
    None,
    /// Boolean value (`true` or `false`).
    Bool(bool),
    /// 64-bit integer within ±(2^53 - 1).
    Int(i64),
    /// Finite 64-bit floating-point number.
    Float(f64),
    /// Immutable UTF-8 string.
    String(Rc<String>),
    /// Ordered, mutable list of values.
    List(Rc<RefCell<crate::Collection<Value>>>),
    /// Ordered dictionary preserving insertion order.
    Dict(Rc<RefCell<DictMap>>),
    /// Instantiated user struct with field immutability tracking.
    Struct(Rc<RefCell<StructInstance>>),
    /// VM-managed bytecode function pointer.
    Function {
        /// Target entry point instruction offset.
        entry_ip: u32,
        /// Expected number of parameters.
        arity: u16,
        /// `true` for `async fn`: calling produces a `Task` instead of running.
        is_async: bool,
    },
    /// Closure capturing upvalues.
    Closure(Rc<ClosureData>),
    /// Native host function callable by the VM.
    Native(Rc<NativeData>),
    /// Compact integer in the canonical `Byte` range `0..=255`.
    Byte(u8),
    /// Managed mutable binary buffer (`Bytes`).
    Bytes(Rc<RefCell<crate::Collection<u8>>>),
    /// Fundamental type value usable as a runtime `is` target and, for the
    /// convertible core types, as a callable conversion.
    Type(TypeTag),
    /// User-declared struct or enum type value usable as a runtime `is` target.
    UserType(Rc<String>),
    /// Half-open integer range produced by `a..b`.
    Range(Rc<RangeData>),
    /// Method already bound to its receiver, produced by field access on a
    /// collection, string or struct instance.
    BoundMethod(Rc<BoundMethodData>),
    /// Struct method bound directly to an instance without heap allocation.
    StructMethod(Rc<StructMethodData>),
    /// Recoverable failure (Model B).
    Failure(Rc<FailureValue>),
    /// Insertion-ordered set of unique values (structural equality).
    Set(Rc<RefCell<crate::Collection<Value>>>),
    /// Lazy pipeline over a materialized source (see `SequencePipeline`).
    Sequence(Rc<SequencePipeline>),
    /// Future result produced by calling an `async fn`; driven by the scheduler.
    Task(TaskId),
    /// Structured-concurrency group: spawned tasks awaited together.
    Group(GroupId),
    /// Pure time span in seconds (finite float, may be negative as a value).
    Duration(f64),
    /// Reference to an object owned by the host, addressable only through [`Handle`].
    ///
    /// Canon keeps external identity with the host: a script may copy, pass and return the
    /// handle, but only the host resolves the slot behind it, and a released generation
    /// resolves to stale rather than to whatever reuses that slot (see `crate::host`).
    HostHandle(Handle),
    /// Internal sentinel standing for a defaulted parameter the caller omitted.
    ///
    /// Canon evaluates parameter defaults inside the callee, so the value travels from the
    /// call site to the callee prologue as a marker rather than as a value. No Aipo program
    /// can observe it: the prologue replaces every marker it can reach before the body runs.
    Unset,
}

/// Native host function metadata and callback pointer.
#[derive(Debug, Clone)]
pub struct NativeData {
    /// Native function name.
    pub name: String,
    /// Expected argument count.
    pub arity: usize,
    /// Native function callback pointer.
    pub func: fn(&[Value]) -> Result<Value, VmFault>,
}

impl PartialEq for NativeData {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.arity == other.arity
    }
}

/// Closure metadata and captured upvalues allocated on the heap.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosureData {
    /// Entry instruction pointer.
    pub entry_ip: usize,
    /// Expected number of parameters.
    pub arity: usize,
    /// Captured upvalues.
    pub upvalues: Vec<Rc<RefCell<Value>>>,
    /// `true` for `async fn` closures: calling produces a `Task`.
    pub is_async: bool,
}

/// Method already bound to its receiver, produced by field access on a
/// collection, string or struct instance.
#[derive(Clone)]
pub struct BoundMethodData {
    /// Method name.
    pub name: String,
    /// Arguments expected after the receiver.
    pub arity: usize,
    /// Receiver value the method operates on.
    pub receiver: Value,
    /// How the method is executed.
    pub kind: MethodKind,
}

impl PartialEq for BoundMethodData {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.arity == other.arity && self.receiver == other.receiver
    }
}

impl fmt::Debug for BoundMethodData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<bound method {}>", self.name)
    }
}

/// Execution strategy of a bound method.
#[derive(Clone)]
pub enum MethodKind {
    /// Receiver-first native implementation (`fn(&Value, &[Value])`).
    Native(fn(&Value, &[Value]) -> Result<Value, VmFault>),
    /// User function defined in the module, called with the receiver as argument 0.
    Function {
        /// Bytecode entry offset of the method body.
        entry_ip: usize,
        /// Parameter count including the receiver.
        total_arity: usize,
        /// `true` for an `async fn` method: calling produces a `Task` instead of running.
        is_async: bool,
    },
    /// Higher-order method the VM executes itself so it can call back into Aipo code.
    HigherOrder,
}

impl Value {
    /// Constructs a shared string value.
    pub fn string(text: impl Into<String>) -> Self {
        Self::String(Rc::new(text.into()))
    }
    /// Constructs a list with revisioned storage.
    pub fn list(items: impl Into<crate::Collection<Value>>) -> Self {
        Self::List(Rc::new(RefCell::new(items.into())))
    }
    /// Constructs mutable bytes with revisioned storage.
    pub fn bytes(items: impl Into<crate::Collection<u8>>) -> Self {
        Self::Bytes(Rc::new(RefCell::new(items.into())))
    }
    /// Constructs an already deduplicated set with revisioned storage.
    pub fn set(items: impl Into<crate::Collection<Value>>) -> Self {
        Self::Set(Rc::new(RefCell::new(items.into())))
    }

    /// Revision of mutable collection structure, if this value owns one.
    pub fn structural_revision(&self) -> Option<u64> {
        match self {
            Self::List(items) | Self::Set(items) => Some(items.borrow().revision()),
            Self::Bytes(items) => Some(items.borrow().revision()),
            Self::Dict(items) => Some(items.borrow().revision()),
            _ => None,
        }
    }

    /// Constructs a new closure value.
    #[must_use]
    pub fn closure(
        entry_ip: usize,
        arity: usize,
        upvalues: Vec<Rc<RefCell<Value>>>,
        is_async: bool,
    ) -> Self {
        Self::Closure(Rc::new(ClosureData {
            entry_ip,
            arity,
            upvalues,
            is_async,
        }))
    }

    /// Constructs a new native function value.
    #[must_use]
    pub fn native(
        name: impl Into<String>,
        arity: usize,
        func: fn(&[Value]) -> Result<Value, VmFault>,
    ) -> Self {
        Self::Native(Rc::new(NativeData {
            name: name.into(),
            arity,
            func,
        }))
    }

    /// Constructs a new bound method value.
    #[must_use]
    pub fn bound_method(
        name: impl Into<String>,
        arity: usize,
        receiver: Value,
        kind: MethodKind,
    ) -> Self {
        Self::BoundMethod(Rc::new(BoundMethodData {
            name: name.into(),
            arity,
            receiver,
            kind,
        }))
    }

    /// Constructs a new range value.
    #[must_use]
    pub fn range(start: i64, end: i64) -> Self {
        Self::Range(Rc::new(RangeData::new(start, end)))
    }

    /// Constructs a new struct method value.
    #[must_use]
    pub fn struct_method(
        receiver: Rc<RefCell<StructInstance>>,
        entry_ip: u32,
        total_arity: u16,
        is_async: bool,
    ) -> Self {
        Self::StructMethod(Rc::new(StructMethodData::new(
            receiver,
            entry_ip,
            total_arity,
            is_async,
        )))
    }

    /// Constructs a plain failure value.
    #[must_use]
    pub fn failure(message: impl Into<String>) -> Self {
        Self::Failure(Rc::new(FailureValue::new(message)))
    }

    /// Constructs a structured failure value carrying a payload.
    #[must_use]
    pub fn failure_with_payload(message: impl Into<String>, payload: Value) -> Self {
        Self::Failure(Rc::new(FailureValue::with_payload(message, payload)))
    }

    /// Returns the language-level type name for diagnostics.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bool(_) => "Bool",
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::String(_) => "String",
            Self::List(_) => "List",
            Self::Dict(_) => "Dict",
            Self::Struct(_) => "struct",
            Self::Function { .. }
            | Self::Closure(_)
            | Self::Native(_)
            | Self::BoundMethod(_)
            | Self::StructMethod(_) => "Function",
            Self::Byte(_) => "Byte",
            Self::Bytes(_) => "Bytes",
            Self::Type(_) | Self::UserType(_) => "Type",
            Self::Range(_) => "Range",
            Self::Failure(_) => "Failure",
            Self::Set(_) => "Set",
            Self::Sequence(_) => "Sequence",
            Self::Task(_) => "Task",
            Self::Group(_) => "Group",
            Self::HostHandle(_) => "Handle",
            Self::Duration(_) => "Duration",
            // Internal sentinel: never observable from Aipo, so the name only ever
            // appears in an invariant-violation message.
            Self::Unset => "<omitted argument>",
        }
    }

    /// Numeric projection used for sort-key comparison; `None` for non-numeric values.
    #[must_use]
    pub(crate) fn numeric_key(&self) -> Option<f64> {
        match self {
            Self::Int(n) =>
            {
                #[allow(clippy::cast_precision_loss)]
                Some(*n as f64)
            }
            Self::Byte(b) => Some(f64::from(*b)),
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Widens `Byte` to `Int` so numeric operators can treat it as an integer.
    #[must_use]
    fn numeric(&self) -> Self {
        match self {
            Self::Byte(b) => Self::Int(i64::from(*b)),
            other => other.clone(),
        }
    }

    /// Returns `true` when either operand is a `Byte`, so operators can widen first.
    fn needs_byte_widening(&self, other: &Self) -> bool {
        matches!(self, Self::Byte(_)) || matches!(other, Self::Byte(_))
    }

    /// Checks if value is a recoverable failure.
    #[must_use]
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure(_))
    }

    /// Returns `true` when both values share the same heap allocation.
    ///
    /// `List`, `Dict`, `Struct`, `Set`, `Bytes`, `Closure` and `Sequence` are
    /// reference-counted, so pointer identity is what tells "the same collection"
    /// from "an equal-looking copy". Scalars are compared by value, because a
    /// scalar can never be stored into itself and so can never form a cycle.
    ///
    /// This is the primitive behind the self-reference guard: assigning a
    /// container into itself would create a cycle the reference counter can never
    /// reclaim, so the assignment is refused instead.
    #[must_use]
    pub fn is_same_allocation(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::List(left), Self::List(right)) => Rc::ptr_eq(left, right),
            (Self::Dict(left), Self::Dict(right)) => Rc::ptr_eq(left, right),
            (Self::Struct(left), Self::Struct(right)) => Rc::ptr_eq(left, right),
            (Self::Set(left), Self::Set(right)) => Rc::ptr_eq(left, right),
            (Self::Bytes(left), Self::Bytes(right)) => Rc::ptr_eq(left, right),
            (Self::Sequence(left), Self::Sequence(right)) => Rc::ptr_eq(left, right),
            (Self::Closure(left), Self::Closure(right)) => Rc::ptr_eq(left, right),
            (Self::String(left), Self::String(right)) => Rc::ptr_eq(left, right),
            _ => false,
        }
    }

    /// Extracts boolean value, returning `TypeMismatch` if not a Bool.
    ///
    /// # Errors
    /// Returns `VmFault::TypeMismatch` if self is not `Value::Bool`.
    pub fn as_bool(&self) -> Result<bool, VmFault> {
        match self {
            Self::Bool(b) => Ok(*b),
            other => Err(VmFault::TypeMismatch {
                expected: "Bool".to_string(),
                actual: other.type_name().to_string(),
            }),
        }
    }

    /// Binary addition `+`.
    ///
    /// # Errors
    /// Returns `VmFault` on overflow, non-finite float, or incompatible operand types.
    pub fn add(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().add(&other.numeric());
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                let res = a.checked_add(*b).ok_or_else(|| VmFault::Overflow {
                    details: format!("{a} + {b} exceeded i64 representation"),
                })?;
                Ok(Self::Int(check_safe_int(res)?))
            }
            (Self::Float(a), Self::Float(b)) => {
                let res = check_finite_float(a + b)?;
                Ok(Self::Float(res))
            }
            (Self::Int(a), Self::Float(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let res = check_finite_float(*a as f64 + b)?;
                Ok(Self::Float(res))
            }
            (Self::Float(a), Self::Int(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let res = check_finite_float(a + *b as f64)?;
                Ok(Self::Float(res))
            }
            (Self::String(a), Self::String(b)) => {
                let combined = if a.is_ascii() && b.is_ascii() {
                    let mut combined = String::with_capacity(a.len() + b.len());
                    combined.push_str(a);
                    combined.push_str(b);
                    combined
                } else {
                    #[cfg(feature = "unicode")]
                    {
                        a.chars().chain(b.chars()).nfc().collect()
                    }
                    #[cfg(not(feature = "unicode"))]
                    {
                        let mut combined = String::with_capacity(a.len() + b.len());
                        combined.push_str(a);
                        combined.push_str(b);
                        combined
                    }
                };
                Ok(Self::String(Rc::new(combined)))
            }
            (Self::List(a), Self::List(b)) => {
                let mut items = a.borrow().clone();
                items.extend(b.borrow().iter().cloned());
                Ok(Self::List(Rc::new(RefCell::new(items))))
            }
            (Self::Duration(a), Self::Duration(b)) => {
                let res = check_finite_float(a + b)?;
                Ok(Self::Duration(res))
            }
            _ => Err(VmFault::TypeMismatch {
                expected: "Int, Float, String, List, or Duration".to_string(),
                actual: format!("{} and {}", self.type_name(), other.type_name()),
            }),
        }
    }

    /// Binary subtraction `-`.
    ///
    /// # Errors
    /// Returns `VmFault` on overflow, non-finite float, or incompatible operand types.
    pub fn sub(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().sub(&other.numeric());
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                let res = a.checked_sub(*b).ok_or_else(|| VmFault::Overflow {
                    details: format!("{a} - {b} exceeded i64 representation"),
                })?;
                Ok(Self::Int(check_safe_int(res)?))
            }
            (Self::Float(a), Self::Float(b)) => {
                let res = check_finite_float(a - b)?;
                Ok(Self::Float(res))
            }
            (Self::Int(a), Self::Float(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let res = check_finite_float(*a as f64 - b)?;
                Ok(Self::Float(res))
            }
            (Self::Float(a), Self::Int(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let res = check_finite_float(a - *b as f64)?;
                Ok(Self::Float(res))
            }
            (Self::Duration(a), Self::Duration(b)) => {
                let res = check_finite_float(a - b)?;
                Ok(Self::Duration(res))
            }
            _ => Err(VmFault::TypeMismatch {
                expected: "Int, Float, or Duration".to_string(),
                actual: format!("{} and {}", self.type_name(), other.type_name()),
            }),
        }
    }

    /// Binary multiplication `*`.
    ///
    /// # Errors
    /// Returns `VmFault` on overflow, non-finite float, or incompatible operand types.
    pub fn mul(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().mul(&other.numeric());
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                let res = a.checked_mul(*b).ok_or_else(|| VmFault::Overflow {
                    details: format!("{a} * {b} exceeded i64 representation"),
                })?;
                Ok(Self::Int(check_safe_int(res)?))
            }
            (Self::Float(a), Self::Float(b)) => {
                let res = check_finite_float(a * b)?;
                Ok(Self::Float(res))
            }
            (Self::Int(a), Self::Float(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let res = check_finite_float(*a as f64 * b)?;
                Ok(Self::Float(res))
            }
            (Self::Float(a), Self::Int(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let res = check_finite_float(a * *b as f64)?;
                Ok(Self::Float(res))
            }
            _ => Err(VmFault::TypeMismatch {
                expected: "Int or Float".to_string(),
                actual: format!("{} and {}", self.type_name(), other.type_name()),
            }),
        }
    }

    /// Binary float division `/`. Always produces a Float.
    ///
    /// # Errors
    /// Returns `VmFault::DivisionByZero` or `VmFault::NonFiniteFloat`.
    pub fn div(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().div(&other.numeric());
        }

        #[allow(clippy::cast_precision_loss)]
        let (a, b) = match (self, other) {
            (Self::Int(a), Self::Int(b)) => (*a as f64, *b as f64),
            (Self::Float(a), Self::Float(b)) => (*a, *b),
            (Self::Int(a), Self::Float(b)) => (*a as f64, *b),
            (Self::Float(a), Self::Int(b)) => (*a, *b as f64),
            _ => {
                return Err(VmFault::TypeMismatch {
                    expected: "Int or Float".to_string(),
                    actual: format!("{} and {}", self.type_name(), other.type_name()),
                });
            }
        };

        if b == 0.0 {
            return Err(VmFault::DivisionByZero);
        }

        let res = check_finite_float(a / b)?;
        Ok(Self::Float(res))
    }

    /// Binary integer truncated division `div`.
    ///
    /// # Errors
    /// Returns `VmFault::DivisionByZero` if divisor is 0, or `VmFault::Overflow`.
    pub fn int_div(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().int_div(&other.numeric());
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                if *b == 0 {
                    return Err(VmFault::DivisionByZero);
                }
                let res = a.checked_div(*b).ok_or_else(|| VmFault::Overflow {
                    details: format!("{a} div {b} overflowed"),
                })?;
                Ok(Self::Int(check_safe_int(res)?))
            }
            _ => Err(VmFault::TypeMismatch {
                expected: "Int".to_string(),
                actual: format!("{} and {}", self.type_name(), other.type_name()),
            }),
        }
    }

    /// Binary modulo `%`.
    ///
    /// # Errors
    /// Returns `VmFault::DivisionByZero` if divisor is 0.
    pub fn modulo(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().modulo(&other.numeric());
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                if *b == 0 {
                    return Err(VmFault::DivisionByZero);
                }
                let res = a.checked_rem(*b).ok_or_else(|| VmFault::Overflow {
                    details: format!("{a} % {b} overflowed"),
                })?;
                Ok(Self::Int(check_safe_int(res)?))
            }
            _ => Err(VmFault::TypeMismatch {
                expected: "Int".to_string(),
                actual: format!("{} and {}", self.type_name(), other.type_name()),
            }),
        }
    }

    /// Unary numeric negation `-`.
    ///
    /// # Errors
    /// Returns `VmFault::Overflow` if integer negates beyond safe bounds.
    pub fn negate(&self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if matches!(self, Self::Byte(_)) {
            return self.numeric().negate();
        }

        match self {
            Self::Int(a) => {
                let res = a.checked_neg().ok_or_else(|| VmFault::Overflow {
                    details: format!("negation of {a} overflowed"),
                })?;
                Ok(Self::Int(check_safe_int(res)?))
            }
            Self::Float(a) => Ok(Self::Float(check_finite_float(-a)?)),
            Self::Duration(a) => Ok(Self::Duration(check_finite_float(-a)?)),
            _ => Err(VmFault::TypeMismatch {
                expected: "Int, Float, or Duration".to_string(),
                actual: self.type_name().to_string(),
            }),
        }
    }

    /// Logical NOT `not`.
    ///
    /// # Errors
    /// Returns `VmFault::TypeMismatch` if operand is not Bool.
    pub fn not(&self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        let b = self.as_bool()?;
        Ok(Self::Bool(!b))
    }

    /// Equality comparison `==`.
    ///
    /// # Errors
    /// Propagates failure if either operand is Failure.
    pub fn equal(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        Ok(Self::Bool(self == other))
    }

    /// Inequality comparison `!=`.
    ///
    /// # Errors
    /// Propagates failure if either operand is Failure.
    pub fn not_equal(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        Ok(Self::Bool(self != other))
    }

    /// Relational comparison `<`.
    ///
    /// # Errors
    /// Returns `VmFault::TypeMismatch` if operands cannot be ordered.
    pub fn less(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().less(&other.numeric());
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Bool(a < b)),
            (Self::Float(a), Self::Float(b)) => Ok(Self::Bool(a < b)),
            #[allow(clippy::cast_precision_loss)]
            (Self::Int(a), Self::Float(b)) => Ok(Self::Bool((*a as f64) < *b)),
            #[allow(clippy::cast_precision_loss)]
            (Self::Float(a), Self::Int(b)) => Ok(Self::Bool(*a < (*b as f64))),
            (Self::String(a), Self::String(b)) => Ok(Self::Bool(a < b)),
            (Self::Duration(a), Self::Duration(b)) => Ok(Self::Bool(a < b)),
            _ => Err(VmFault::TypeMismatch {
                expected: "comparable Int, Float, or String".to_string(),
                actual: format!("{} and {}", self.type_name(), other.type_name()),
            }),
        }
    }

    /// Relational comparison `<=`.
    ///
    /// # Errors
    /// Returns `VmFault::TypeMismatch` if operands cannot be ordered.
    pub fn less_equal(&self, other: &Self) -> Result<Self, VmFault> {
        if self.is_failure() {
            return Ok(self.clone());
        }
        if other.is_failure() {
            return Ok(other.clone());
        }
        if self.needs_byte_widening(other) {
            return self.numeric().less_equal(&other.numeric());
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Bool(a <= b)),
            (Self::Float(a), Self::Float(b)) => Ok(Self::Bool(a <= b)),
            #[allow(clippy::cast_precision_loss)]
            (Self::Int(a), Self::Float(b)) => Ok(Self::Bool((*a as f64) <= *b)),
            #[allow(clippy::cast_precision_loss)]
            (Self::Float(a), Self::Int(b)) => Ok(Self::Bool(*a <= (*b as f64))),
            (Self::String(a), Self::String(b)) => Ok(Self::Bool(a <= b)),
            (Self::Duration(a), Self::Duration(b)) => Ok(Self::Bool(a <= b)),
            _ => Err(VmFault::TypeMismatch {
                expected: "comparable Int, Float, or String".to_string(),
                actual: format!("{} and {}", self.type_name(), other.type_name()),
            }),
        }
    }

    /// Relational comparison `>`.
    ///
    /// # Errors
    /// Returns `VmFault::TypeMismatch` if operands cannot be ordered.
    pub fn greater(&self, other: &Self) -> Result<Self, VmFault> {
        other.less(self)
    }

    /// Relational comparison `>=`.
    ///
    /// # Errors
    /// Returns `VmFault::TypeMismatch` if operands cannot be ordered.
    pub fn greater_equal(&self, other: &Self) -> Result<Self, VmFault> {
        other.less_equal(self)
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            #[allow(clippy::cast_precision_loss)]
            (Self::Int(a), Self::Float(b)) => (*a as f64) == *b,
            #[allow(clippy::cast_precision_loss)]
            (Self::Float(a), Self::Int(b)) => *a == (*b as f64),
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Byte(a), Self::Byte(b)) => a == b,
            (Self::Byte(a), Self::Int(b)) => i64::from(*a) == *b,
            (Self::Int(a), Self::Byte(b)) => *a == i64::from(*b),
            #[allow(clippy::cast_precision_loss)]
            (Self::Byte(a), Self::Float(b)) => f64::from(*a) == *b,
            #[allow(clippy::cast_precision_loss)]
            (Self::Float(a), Self::Byte(b)) => *a == f64::from(*b),
            (Self::Bytes(a), Self::Bytes(b)) => *a.borrow() == *b.borrow(),
            (Self::Type(a), Self::Type(b)) => a == b,
            (Self::UserType(a), Self::UserType(b)) => a == b,
            (Self::Range(a), Self::Range(b)) => a == b,
            (Self::Set(a), Self::Set(b)) => *a.borrow() == *b.borrow(),
            (Self::Sequence(a), Self::Sequence(b)) => Rc::ptr_eq(a, b),
            (Self::Task(a), Self::Task(b)) => a == b,
            (Self::Group(a), Self::Group(b)) => a == b,
            (Self::HostHandle(a), Self::HostHandle(b)) => a == b,
            (Self::Duration(a), Self::Duration(b)) => a == b,
            (Self::BoundMethod(a), Self::BoundMethod(b)) => a == b,
            (Self::StructMethod(m1), Self::StructMethod(m2)) => {
                Rc::ptr_eq(&m1.receiver, &m2.receiver)
                    && m1.entry_ip == m2.entry_ip
                    && m1.total_arity == m2.total_arity
                    && m1.is_async == m2.is_async
            }
            (Self::List(a), Self::List(b)) => *a.borrow() == *b.borrow(),
            (Self::Dict(a), Self::Dict(b)) => *a.borrow() == *b.borrow(),
            (Self::Struct(a), Self::Struct(b)) => {
                let a = a.borrow();
                let b = b.borrow();
                a.type_name == b.type_name && a.fields == b.fields
            }
            (Self::Native(n1), Self::Native(n2)) => n1.name == n2.name && n1.arity == n2.arity,
            (Self::Failure(a), Self::Failure(b)) => a.message == b.message,
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(n) => write!(f, "{n:?}"),
            Self::String(s) => write!(f, "{s:?}"),
            Self::List(l) => write!(f, "{:?}", l.borrow()),
            Self::Dict(d) => write!(f, "{:?}", d.borrow()),
            Self::Set(items) => write!(f, "{:?}", items.borrow()),
            Self::Sequence(pipeline) => write!(f, "<sequence {:?}>", pipeline.source),
            Self::Task(id) => write!(f, "<task #{id}>"),
            Self::Group(id) => write!(f, "<group #{id}>"),
            Self::Duration(seconds) => write!(f, "Duration({seconds:?})"),
            Self::Struct(s) => {
                let s = s.borrow();
                write!(f, "{}{:?}", s.type_name, s.fields)
            }
            Self::Function {
                entry_ip, arity, ..
            } => {
                write!(f, "<fn@{entry_ip} arity={arity}>")
            }
            Self::Closure(c) => {
                write!(f, "<closure@{} arity={}>", c.entry_ip, c.arity)
            }
            Self::Native(native) => {
                write!(f, "<native fn {} arity={}>", native.name, native.arity)
            }
            Self::Byte(b) => write!(f, "{b}"),
            Self::Bytes(b) => write!(f, "Bytes({} bytes)", b.borrow().len()),
            Self::Type(tag) => write!(f, "<type {}>", tag.name()),
            Self::UserType(name) => write!(f, "<type {name}>"),
            Self::Range(r) => write!(f, "{}..{}", r.start, r.end),
            Self::BoundMethod(b) => {
                write!(f, "<method {} arity={}>", b.name, b.arity)
            }
            Self::StructMethod(m) => {
                write!(
                    f,
                    "<struct method @{} arity={}>",
                    m.entry_ip,
                    m.total_arity.saturating_sub(1)
                )
            }
            Self::Failure(err) => write!(f, "failure({:?})", err.message),
            Self::HostHandle(handle) => write!(f, "HostHandle({handle})"),
            Self::Unset => write!(f, "<unset>"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{n:.1}")
                } else {
                    write!(f, "{n}")
                }
            }
            Self::String(s) => write!(f, "{s}"),
            Self::List(l) => {
                let l = l.borrow();
                write!(f, "[")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Self::Dict(d) => {
                let d = d.borrow();
                write!(f, "#{{")?;
                for (i, (k, v)) in d.entries().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Self::Struct(s) => {
                let s = s.borrow();
                write!(f, "{}{{", s.type_name)?;
                for (i, (k, v)) in s.fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Self::Function { entry_ip, .. } => write!(f, "<fn@{entry_ip}>"),
            Self::Closure(c) => write!(f, "<closure@{}>", c.entry_ip),
            Self::Native(native) => write!(f, "<fn {}>", native.name),
            Self::StructMethod(m) => write!(f, "<method@{}>", m.entry_ip),
            Self::Byte(b) => write!(f, "{b}"),
            Self::Bytes(_) => write!(f, "<bytes>"),
            Self::Type(tag) => write!(f, "{}", tag.name()),
            Self::UserType(name) => write!(f, "{name}"),
            Self::Range(r) => write!(f, "{}..{}", r.start, r.end),
            Self::Set(items) => {
                let items = items.borrow();
                write!(f, "{{")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "}}")
            }
            Self::Sequence(_) => write!(f, "<sequence>"),
            Self::Task(id) => write!(f, "<task #{id}>"),
            Self::Group(id) => write!(f, "<group #{id}>"),
            Self::Duration(seconds) => {
                if seconds.fract() == 0.0 {
                    write!(f, "{}s", *seconds as i64)
                } else {
                    write!(f, "{seconds}s")
                }
            }
            Self::BoundMethod(b) => write!(f, "<fn {}>", b.name),
            Self::Failure(err) => write!(f, "fail(\"{}\")", err.message),
            Self::HostHandle(handle) => write!(f, "<{handle}>"),
            Self::Unset => write!(f, "<unset>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_key(name: &str) -> Value {
        Value::String(Rc::new(name.to_string()))
    }

    #[test]
    fn test_dict_map_string_lookup_uses_index() {
        let map = DictMap::from_entries(vec![
            (str_key("b"), Value::Int(2)),
            (str_key("a"), Value::Int(1)),
        ]);
        assert_eq!(map.get(&str_key("a")), Some(&Value::Int(1)));
        assert_eq!(map.get(&str_key("missing")), None);
        assert_eq!(map.keys(), vec![str_key("b"), str_key("a")]);
    }

    #[test]
    fn test_dict_map_non_string_keys_scan() {
        let map = DictMap::from_entries(vec![
            (Value::Int(1), str_key("one")),
            (
                Value::List(Rc::new(RefCell::new((vec![Value::Int(2)]).into()))),
                str_key("two"),
            ),
        ]);
        assert_eq!(map.get(&Value::Int(1)), Some(&str_key("one")));
        assert_eq!(map.get(&Value::Int(9)), None);
    }

    #[test]
    fn test_dict_map_upsert_preserves_first_match_and_order() {
        let mut map = DictMap::from_entries(vec![(str_key("a"), Value::Int(1))]);
        map.upsert(str_key("b"), Value::Int(2));
        map.upsert(str_key("a"), Value::Int(10));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&str_key("a")), Some(&Value::Int(10)));
        assert_eq!(map.keys(), vec![str_key("a"), str_key("b")]);
        map.upsert(Value::Int(7), Value::Int(70));
        assert_eq!(map.get(&Value::Int(7)), Some(&Value::Int(70)));
    }

    #[test]
    fn test_dict_map_remove_rebuilds_index() {
        let mut map = DictMap::from_entries(vec![
            (str_key("a"), Value::Int(1)),
            (str_key("b"), Value::Int(2)),
            (str_key("c"), Value::Int(3)),
        ]);
        assert!(map.remove(&str_key("b")));
        assert!(!map.remove(&str_key("b")));
        assert_eq!(map.get(&str_key("c")), Some(&Value::Int(3)));
        assert_eq!(map.keys(), vec![str_key("a"), str_key("c")]);
        map.clear();
        assert!(map.is_empty());
        assert_eq!(map.get(&str_key("a")), None);
    }

    #[test]
    fn test_dict_map_equality_ignores_index() {
        let first = DictMap::from_entries(vec![(str_key("a"), Value::Int(1))]);
        let mut second = DictMap::from_entries(vec![(str_key("a"), Value::Int(1))]);
        second.upsert(str_key("a"), Value::Int(1));
        assert_eq!(first, second);
    }
}
