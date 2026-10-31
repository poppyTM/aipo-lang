//! Target-neutral Core IR data structures.

use aipo_ast::{BinaryOp, UnaryOp};
use aipo_source::SourceSpan;
use serde::{Deserialize, Serialize};

/// Target-neutral Core IR module representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoreModule {
    /// Top-level functions.
    pub functions: Vec<CoreFunction>,
    /// Top-level entrypoint script instructions.
    pub top_level: CoreFunction,
    /// Declared struct types, in declaration order.
    pub structs: Vec<CoreStruct>,
    /// Source span.
    pub span: SourceSpan,
}

/// A declared struct type carried by the module so the runtime can materialize
/// instances with their canonical field names and mutability markers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreStruct {
    /// Struct type name.
    pub name: String,
    /// Declared fields in canonical order: `(field name, is_fixed)`.
    pub fields: Vec<(String, bool)>,
}

/// A Core IR function definition.
///
/// Slot layout is `params` followed by `locals`: parameter `i` occupies slot `i` and
/// declared locals continue from `params.len()`. The emitter resolves `Load`/`Store`
/// against this layout before falling back to upvalues and globals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoreFunction {
    /// Function name (`__top_level__` for the module entry script).
    pub name: String,
    /// `true` for `async fn`: calling produces a `Task` instead of running.
    pub is_async: bool,
    /// Parameter names, in slot order.
    pub params: Vec<String>,
    /// Declared local names, in slot order after the parameters.
    pub locals: Vec<String>,
    /// Captured enclosing names, in upvalue index order.
    pub upvalues: Vec<String>,
    /// Sequential instructions in the function body.
    pub instructions: Vec<CoreInst>,
    /// Source span.
    pub span: SourceSpan,
}

/// Target-neutral Core IR instruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreInst {
    /// Push literal constant.
    Constant(CoreConstant, SourceSpan),
    /// Load local/enclosing variable onto evaluation stack.
    Load(String, SourceSpan),
    /// Store top of stack into local variable.
    Store(String, SourceSpan),
    /// Binary operator evaluation.
    Binary(BinaryOp, SourceSpan),
    /// Unary prefix operator.
    Unary(UnaryOp, SourceSpan),
    /// Function call with argument count.
    Call {
        /// Number of arguments passed.
        arg_count: usize,
        /// Source span.
        span: SourceSpan,
    },
    /// Return top of stack or None.
    Return {
        /// Whether a value is returned.
        has_value: bool,
        /// Source span.
        span: SourceSpan,
    },
    /// Unconditional jump to an absolute instruction index (the bytecode emitter
    /// converts it to a relative offset).
    Jump(isize, SourceSpan),
    /// Jump to an absolute instruction index if the top of stack is false.
    JumpIfFalse(isize, SourceSpan),
    /// Pop top of stack and discard.
    Pop(SourceSpan),
    /// Duplicate top of stack.
    Dup(SourceSpan),
    /// Field get: `obj.field`.
    GetField(String, SourceSpan),
    /// Field set: `obj.field = val`.
    ///
    /// Pops the value and then the receiver; on success the stack is exactly as it was.
    /// A `Failure` on either side propagates through the handler stack instead of being
    /// left behind or turned into a type fault.
    SetField(String, SourceSpan),
    /// Index get: `coll[idx]`.
    GetIndex(SourceSpan),
    /// Deep-copy the struct on top of the stack, replacing it with the copy.
    ///
    /// Backs the functional update `base with { ... }`. A struct value is a shared
    /// `Rc<RefCell<..>>`, so `Dup` would alias the base and the update would mutate it;
    /// this makes the copy that keeps the base intact. Field values are carried over
    /// by reference, which matches every other copy in the VM (a `List` in two structs
    /// is still the same list until something writes to it).
    CloneStruct(SourceSpan),
    /// Index set: `coll[idx] = val`.
    ///
    /// Pops the value, the index and the collection; on success the stack is exactly as
    /// it was. A `Failure` in any of the three propagates like [`CoreInst::SetField`].
    SetIndex(SourceSpan),
    /// Tests whether the struct on top of the stack matches the variant name.
    IsVariant(String, SourceSpan),
    /// Construct list with count items.
    BuildList(usize, SourceSpan),
    /// Construct dictionary with count key-value pairs.
    BuildDict(usize, SourceSpan),
    /// Construct struct instance of type name with field count.
    BuildStruct {
        /// Struct type name.
        type_name: String,
        /// Number of initialized fields.
        field_count: usize,
        /// When set, the instance starts in construction: `fixed` fields stay mutable until
        /// [`CoreInst::SealStruct`] publishes it. Canon requires this whenever `init` runs,
        /// because `init` assigns fields — including `fixed` ones — before publication.
        defer_fixed: bool,
        /// Source span.
        span: SourceSpan,
    },
    /// Publish an instance built with `defer_fixed`: restores the declared `fixed` set and
    /// evaluates the type's `invariant()` hook at the end of construction.
    SealStruct(SourceSpan),
    /// Fail the construction when the top of stack is `false`, naming the type.
    ///
    /// Canon verifies `invariant()` after construction finishes, so this runs on the value
    /// returned by the type's invariant predicate and turns a violated invariant into a
    /// contract fault instead of a published instance.
    AssertInvariant {
        /// Struct type name, used for the fault message.
        type_name: String,
        /// Source span.
        span: SourceSpan,
    },
    /// Assert that the value on top of the stack satisfies a signature contract.
    ///
    /// Canon checks `name: Type`, `name!: Type`, `-> T` and `T?` at the call boundary: a
    /// violation discovered only at runtime is a contract fault, not a recoverable
    /// `Failure`, and is not capturable by `attempt`. `T?` accepts `none` in addition to
    /// `T`. The value stays on the stack, so the same instruction serves parameter checks
    /// (after `Load`) and return checks (before `Return`).
    AssertContract {
        /// Declared type or interface name the value must satisfy.
        type_name: String,
        /// `true` for the canonical `T?` form, which also accepts `none`.
        nullable: bool,
        /// Where the contract is written (`parameter \`x\``, `return`), for the fault message.
        position: String,
        /// Operations an interface contract requires, as `(name, declared arity)`.
        ///
        /// Canon makes interfaces structural: using one as a contract asks whether the value
        /// exposes the operations it declares. Empty for a core-type or `struct` contract.
        operations: Vec<(String, usize)>,
        /// Source span.
        span: SourceSpan,
    },
    /// Verify the invariant-protected mutations journaled in the current frame.
    ///
    /// Canon validates `invariant()` at stable mutable boundaries, not after each internal
    /// assignment, so a field assignment is provisional until a boundary commits it: on
    /// failure the direct journaled fields return to their entry values and the operation
    /// produces a recoverable `Failure`.
    CheckMutations(SourceSpan),
    /// Propagate an unhandled recoverable `Failure` at a statement boundary.
    ///
    /// Canon makes recoverable failures propagate automatically: a statement whose value is
    /// an unconsumed `Failure` ends the enclosing path, unwinding to the nearest `attempt`
    /// handler or terminating the program.
    PropagateFailure(SourceSpan),
    /// Push a function value for a declared function name.
    MakeFunction(String, SourceSpan),
    /// Push a closure value, popping one enclosing value per capture name first.
    ///
    /// `self_capture` marks the one capture that is the closure's own recursion handle:
    /// the emitter pushes the `Unset` sentinel for it instead of loading a binding, and
    /// the [`CoreInst::FillSelfCapture`] that follows rewrites that cell with the newly
    /// created closure. Canon lets a local function reference its own name, but captures
    /// snapshot values at creation time — before the closure exists.
    MakeClosure {
        /// Closure body function name.
        name: String,
        /// Captured enclosing names, in upvalue index order.
        upvalues: Vec<String>,
        /// The capture holding the closure's own recursion handle, if any.
        self_capture: Option<String>,
        /// Source span.
        span: SourceSpan,
    },
    /// Load a captured upvalue onto the stack.
    GetUpvalue(String, SourceSpan),
    /// Store the top of stack into a captured upvalue.
    SetUpvalue(String, SourceSpan),
    /// Replace a local slot with a shared cell seeded from its current value.
    ///
    /// Emitted immediately before [`CoreInst::MakeClosure`] for each captured local, so an
    /// assignment inside the closure and a read in the enclosing frame observe one storage
    /// location. Canon captures mutable bindings by reference; snapshotting a copy would let
    /// the closure's writes diverge from the enclosing frame's view.
    BoxLocal(String, SourceSpan),
    /// Fill a named upvalue of the closure being created with that same closure.
    ///
    /// Canon lets a local function reference its own name for recursion, but closure
    /// captures snapshot the value at creation time — before the closure exists. The
    /// builder therefore reserves a `Unset` placeholder slot for the self capture and
    /// this instruction, emitted right after [`CoreInst::MakeClosure`], rewrites the
    /// placeholder in the newly created closure's cell with the closure itself.
    /// Reading it later yields the recursion handle without changing capture semantics.
    FillSelfCapture {
        /// Name of the self capture upvalue being filled.
        name: String,
        /// Source span.
        span: SourceSpan,
    },
    /// Build a half-open range value from two Ints.
    Range(SourceSpan),
    /// Push the length of a String, List, or Dict.
    Len(SourceSpan),
    /// Runtime type test against a core-type value.
    TypeIs(SourceSpan),
    /// Runtime nullable type test against a core-type value (`is ...?`).
    TypeIsNullable(SourceSpan),
    /// Register the collection on top of the stack as actively iterated.
    IterGuard(SourceSpan),
    /// Read one `each` binding at an ordinal position: pops the index and then the
    /// collection, pushes the requested projection (auditoria IR-10).
    IterAt(IterMode, SourceSpan),
    /// Unregister the innermost active iteration.
    IterGuardEnd(SourceSpan),
    /// Failure propagation.
    Fail(SourceSpan),
    /// Register a failure recovery handler targeting an absolute instruction index.
    PushHandler(isize, SourceSpan),
    /// Unregister the topmost failure recovery handler.
    PopHandler(SourceSpan),
    /// Drive the `Task` on top of the stack to its value.
    ///
    /// Canon suspends only at explicit `await` positions: the scheduler runs the
    /// awaited task (depth-first, deterministic order) and pushes its result.
    /// Awaiting a ready task is immediate; a failed one propagates `Failure`;
    /// a cancelled one is a fault, never capturable.
    Await(SourceSpan),
    /// Push the omitted-argument sentinel used for a defaulted parameter the caller skipped.
    ///
    /// Canon evaluates parameter defaults inside the callee and allows a default to reference
    /// earlier parameters, so the call site passes a marker instead of a value; the callee
    /// prologue replaces it with the evaluated default.
    PushUnset(SourceSpan),
    /// Jump to `target` when the local in `slot` holds a caller-provided value.
    ///
    /// The default prologue is a sequence of these guards: a parameter the caller supplied
    /// jumps over its default, and an omitted one falls through and evaluates it.
    JumpIfSetLocal {
        /// Frame slot of the parameter being checked.
        slot: usize,
        /// Absolute instruction index to jump to when the parameter was provided.
        target: isize,
        /// Source span.
        span: SourceSpan,
    },
}
/// Which `each` binding `CoreInst::IterAt` projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IterMode {
    /// The natural one-name binding: the key of a `Dict`, the element otherwise.
    Primary,
    /// The two-name first binding: the key of a `Dict`, the positional Int otherwise.
    Key,
    /// The two-name second binding: the value of a `Dict`, the element otherwise.
    Value,
}

/// Target-neutral literal constant value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreConstant {
    /// Absence of value `none`.
    None,
    /// Boolean `true` or `false`.
    Bool(bool),
    /// Exact 64-bit signed integer.
    Int(i64),
    /// 64-bit IEEE floating-point number.
    Float(f64),
    /// Immutable UTF-8 NFC normalized string.
    String(String),
}
