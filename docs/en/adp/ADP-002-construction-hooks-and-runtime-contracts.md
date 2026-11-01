# ADP-002 — Construction hooks, invariants and runtime contracts

**Status:** resolved — G1, G2 (verification at construction) and G4 closed by `P00-G13`; G2b (mutation points) and G3 (runtime contracts, including structural interface conformance) closed by `P00-G14` and corrected/certified by `P00-G15`
**Origin:** Slice S11 (`P00-G12`), `docs/conformance/` corpus
**Resolved in:** `docs/evidence/P00-G13-construction-hooks-and-bytes.md` and
`docs/evidence/P00-G14-runtime-contract-enforcement.md`
**Authority:** subordinate to `docs/canon/Aipo V1 — Language Reference…`,
`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…` and
`docs/canon/Interlúdio — Chamadas, Construção, Impl e Módulos…`
**Complements:** `docs/conformance/README.md`, `docs/evidence/P00-G12-conformance-and-mvp-gate.md`

The no-invention policy (`prumo.json` → `risk`) requires that every gap between the canon and the
implementation be **recorded**, not silenced. Slice S11 assembled the conformance corpus
and, by exercising the MVP cut, found four verifiable gaps. Each one below has a
minimal program that demonstrates it today, the behavior the canon requires, and the question that needs
to be closed before implementing. None of them invalidates what the corpus already covers; all are
listed as unmet criteria in the gate's evidence record.

## G1 — `init` is not executed on `Type{...}` construction

**The canon requires.** `Type{...}` uses the `init` when it exists, and `init` does not produce a value
directly — the construction produces the new instance
(`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…`, lines 46–49). Parameter defaults of
`init` ergonomically define optional construction values.

**Current behavior.** The `init` hook is parsed, analyzed and compiled as a function
(`Type.init`), but construction never invokes it: `BuildStruct` only initializes the fields
named in the literal. Demonstration:

```text
struct P
    name
end

impl P
    init(self!, name)
        self.name = name + "!"
    end
end

io.println(P{name = "z"}.name)   // today: "z"  · canon: "z!"
io.println(P{}.name)             // today: "none" · canon: "anon" with init(self!, name = "anon")
```

**Question.** How does construction start calling `init` without losing the canon's automatic
construction (line 49, used when the `struct` does not declare `init`): does `init` become a call target
of `BuildStruct` itself, or is construction rewritten in HIR into a `Type.init` call
followed by returning the instance? The answer must also say who evaluates the `init` parameter
defaults — the callee-side prologue already implemented (S11) or a construction-specific path.

**Resolution (`P00-G13`).** Construction calls `Type.init` explicitly. The parser injects the
implicit `self!` receiver when the author does not declare it (the canon writes `init(id, ...)` without
`self`), so all subsequent stages see the same shape of a function with a receiver. The
Core IR builder emits, in this order: `BuildStruct` (with `defer_fixed`), `MakeFunction
Type.init`, the receiver argument, the arguments aligned to the `init` declaration,
`Call(1 + n)`, `Pop` of the construction copy and — after the `invariant()` check —
`SealStruct`. **Defaults continue to be evaluated by the existing callee-side prologue**
(each default is an expression in the function's own prologue, guarded by `JumpIfSetLocal`), which
preserves the canon rule that a default is re-evaluated on every call and can reference earlier
parameters. The implicit return of `init` becomes the instance itself (the hook
"does not produce a value directly"; construction produces the new instance), so that the value of
`Type{...}` is the published instance. `fixed` stops being immutable during construction: the
`StructInstance` carries `under_construction`, and `set_field` only rejects a `fixed` field after
`SealStruct`. See `docs/conformance/programs/13_init_and_invariant.aipo`.

## G2 — `invariant()` is not evaluated at runtime

**The canon requires.** The `invariant()` hook declares conditions that must remain true; its
lines are conceptually equivalent to conditions combined with `and`
(`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…`, lines 50–56).

**Current behavior.** The hook is parsed and type-checked, and the instance is published without
any condition being evaluated. Demonstration:

```text
struct Rn
    lo
    hi
end

impl Rn
    init(self!, lo, hi)
        self.lo = lo
        self.hi = hi
    end

    invariant()
        self.lo < self.hi
    end
end

io.println(String(Rn{lo = 2, hi = 1}.hi))   // today: 1 · canon: contract failure
```

**Question.** Where is the invariant evaluated: at the end of `init` (once per construction), or on
every instance publication? And what is the error model — recoverable `Failure` or fault? The
`StructInvariant` hook (`crates/aipo-vm::InvariantValidator`) already exists and accepts a
validation closure, but no path feeds it from bytecode: it remains to define how
the compiled conditions become that validator.

**Partial resolution (`P00-G13`) — verification at construction.** The hook is lowered to a
function `<Type>.invariant(self) -> Bool` whose lines are combined with `and` (exactly the
canon's "conceptually `and`"). Construction emits, after `init` (or right after
`BuildStruct` when there is no `init`), a call to the predicate followed by the new opcode
`AssertInvariant`, which consumes the `Bool` and fails the contract with `VmFault::InvariantViolation`
when it is `false`. The channel is **fault**, as decided by the MVP cut in
`docs/waves/wave-1-mvp.md` itself, which classifies "contract violations at runtime" as a runtime
fault, and consistent with the error model already documented in `docs/stdlib/mvp-subset.md`.
`BuildStruct` stops evaluating the invariant when construction is deferred (`defer_fixed`):
`SealStruct` is what publishes the instance, so the invariant observes the fields already assigned
by `init`. Extending the invariant to the **later mutation points** (canon:
`candidate -> provisional application -> verification -> commit`) was left as **G2b** and closed
by `P00-G14` — see the section *G2b — `invariant()` at mutation points* below.

## G3 — Signature contracts are not verified at runtime

**The canon requires.** Optional contracts appear on parameters and returns of functions/signatures and
are verified at the call boundary ("runtime check at boundaries", MVP cut in
`docs/waves/wave-1-mvp.md`).

**Current behavior.** Annotations are parsed and part of the semantic analysis uses them, but no
type-check opcode is emitted: the value crosses the boundary unchecked.

```text
fn f(x: Int)
    return x
end

io.println(String(f("no")))   // today: "no" · canon: contract error at the boundary
```

**Question.** `TypeIs` already exists as an opcode (a type test against a type value). Should the contract
reuse `TypeIs` with a per-parameter type table compiled into the function prologue
(following the S11 defaults-prologue pattern), or does it require a dedicated opcode that also
carries the parameter name for the message? And `T?` — is `none` accepted and does it not take part in the
check?

**Resolution (`P00-G14`).** A dedicated opcode (`AssertContract`), emitted in the callee prologue for
parameters and before each `return expr` for returns — see the section *G3 — signature contracts
at runtime* below, which also answers why `TypeIs` was not reused.

## G4 — `Bytes` has no construction form

**The canon requires.** The MVP cut excludes "`Bytes` packing APIs beyond construction and
indexing", which implies that construction and indexing are **inside** the scope
(`docs/waves/wave-1-mvp.md`).

**Current behavior.** `Bytes` exists as a type value and as a value (`Value::Bytes`); indexing
and `len` work, but there is no callable conversion: `Bytes([1, 2, 3])` fails at runtime with
`AIPO_RT_NOT_CALLABLE` (`Bytes (no conversion form in V1)`). Originally recorded as Q2 of
`docs/adp/ADP-001-byte-and-core-types-as-values.md`; this ADP reaffirms it as a gate criterion.

**Question.** What is the canonical way to construct `Bytes` — `Bytes(list_of_ints)`,
`Bytes(string)`, or both? ADP-001 Q2 remains open and is what needs to be closed first.

**Resolution (`P00-G13`).** The canon answers: Language Reference §6 shows `let data = Bytes(32)`
and describes `Bytes` as a "mutable, managed binary block". The canonical form is
**`Bytes(count: Int)`**, which allocates `count` zeroed bytes; byte indexing produces `Byte`
(`Value::Bytes` + `Value::Byte`, both already existing) and `len` reports the count. `Bytes(list)`
and `Bytes(string)` are **not** canonical forms and remain out. The upper allocation limit is
a provisional implementation decision (`BYTES_MAX_ALLOCATION`, 64 MiB) so that a size
coming from source code cannot exhaust memory; above the limit the result is a recoverable `Failure`.
The packing API (`read_i32`/`write_f32`/…) remains outside the MVP cut.

## Consequence for the gate

The items below were criteria of the MVP cut (`docs/waves/wave-1-mvp.md`) that the corpus could not
certify; that is why the MVP gate was recorded as **partial** in
`docs/evidence/P00-G12-conformance-and-mvp-gate.md`. `P00-G13` closed three of them and `P00-G14`
closed the remaining two, and the corpus certifies all of them:

| Gap | State | Certification |
|---|---|---|
| G1 — `init` on `Type{...}` construction | **closed** (`P00-G13`) | `docs/conformance/programs/13_init_and_invariant.aipo` |
| G2 — `invariant()` verified at the end of construction | **closed** (`P00-G13`) | `programs/13_init_and_invariant.aipo` + `diagnostics/11_runtime_invariant_violation.aipo` |
| G4 — canonical `Bytes` construction form | **closed** (`P00-G13`) | `programs/12_bytes.aipo` |
| G2b — `invariant()` at mutation boundaries | **closed** (`P00-G14`) | `programs/15_invariant_on_mutation.aipo` + `diagnostics/14_runtime_invariant_mutation_uncaught.aipo` |
| G3 — signature contracts at runtime | **closed** (`P00-G14`) | `programs/14_signature_contracts.aipo` + `diagnostics/12_runtime_contract_violation.aipo`, `13_runtime_return_contract.aipo` |

With the five items closed, the MVP-cut criteria that depended on these gaps are
certified by the corpus. What remains outside the MVP is still what is declared in
`docs/waves/wave-1-mvp.md` (JS backend, `Bytes` packing APIs, `Set`, `Sequence`, LSP,
REPL). Structural interface conformance at runtime, previously listed here as a residual limit,
was closed and certified by `P00-G15`.

## G2b — `invariant()` at mutation points — closed by `P00-G14`

**The canon requires.** "A field update subject to an invariant follows
`candidate -> provisional application -> verification -> commit`; on a recoverable failure, the previous
value is preserved" (`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…`). The
`Interlúdio — Chamadas, Construção, Impl e Módulos` closes the model in §3: "`invariant()` is
verified after automatic construction, after `init()` completes successfully, and at the successful
end of a boundary that received mutable capability over the instance"; "Validation
occurs in a stable state, not after each internal assignment"; "When validation fails, protected
direct fields of the instance return to the entry state and the operation produces a
`Failure`".

**Resolution.** Three decisions, all taken from the canon:

1. **Error channel — recoverable `Failure`.** §3 literally says "the operation produces a
   `Failure`", in contrast to the signature contract, which Language Reference §4 classifies
   as a *contract fault* that `attempt` cannot catch.
2. **Timing — stable mutable boundary, not each assignment.** The VM keeps a *journal*: an
   assignment to a protected field of a **published** instance is provisional — the first write per
   frame/statement stores the entry value — and verification happens at the end of the boundary.
   That way the canon still allows a temporarily invalid state inside an operation.
3. **How the predicate is reached.** The module already registers the compiled hook as the
   `Type.invariant` function, so `Vm::run` resolves the `entry_ip` by type and invokes the predicate
   itself: there is no second expression evaluator, and mutation verification and construction
   verification use the same compiled code.

Boundaries are emitted by the builder as `CoreInst::CheckMutations` (opcode `CheckMutations`): the
return of each function/method, including the implicit epilogue, and each mutation statement of the
entry script — which has no enclosing operation to defer to, and whose `attempt` needs to
catch the failure of the statement itself.

When validation fails, the VM restores the direct fields of **all** participating
instances (not just the one that failed, as §3 requires when an operation receives multiple
mutable instances) and propagates the `Failure` through model B, so that `attempt ... failed`
catches it and the entry value remains observable. `fixed` remains a mutation error: the
rollback writes back the value the field already had, so immutability is not loosened.

Certification: `docs/conformance/programs/15_invariant_on_mutation.aipo` (commit, rollback via
method, rollback via direct statement, rollback of two instances in the same operation) and
`docs/conformance/diagnostics/14_runtime_invariant_mutation_uncaught.aipo` (the uncaught `Failure`
terminates the program like any recoverable failure). At the VM level,
`test_guarded_mutation_rolls_back_at_boundary` (`crates/aipo-vm/tests/data_and_errors.rs`)
exercises the same rule with a host validator.

**Note on the original criterion.** The `P00-G14` goal described verification "on `SetField`". The
implementation kept `SetField` as the guarded mutation point (it is what decides whether the
assignment is provisional and records the entry value), but moved the **verification** to the
boundary, because the canon forbids validating "after each internal assignment". Verifying at
assignment would reject canonical programs that pass through a transient state inside an operation.

## G3 — signature contracts at runtime — closed by `P00-G14`

**The canon requires.** `name: Type` constrains a parameter, `name!: Type` grants mutation and constrains the
value, `-> T` constrains the result, `T?` means exactly `T` or `none`, and the simple
`Function` contract only verifies that the value is callable (`docs/canon/Aipo V1 — Language
Reference…` §4 and §7). The check happens at the call boundary, and "a violation discovered
only at runtime is a programming **contract fault** and is not catchable by `attempt`" (§4).

**Resolution.** The parser and HIR already carried `TypeAnnotation` (name + `?`) on parameters and
returns, but no runtime path used it. Now Core IR emits
`AssertContract { type_name, nullable, position, operations }` and the `AssertContract` opcode carries
a `u16` with the contract name, a `u8` with the nullability flag, a `u16` with the position, and the list of
required operations (a `u8` with the count, then `u16`+`u8` per operation; empty for a
core-type or `struct` contract):

- **parameters**, in the callee's own prologue and after the defaults prologue — so the value
  of a default is also verified, and `init`, methods, functions and closures get the check through
  the same path;
- **returns**, immediately before each `return expr`, on the value already pushed (the
  instruction inspects the top without consuming it, so it serves both cases).

A missing value (`Unset`, an omitted parameter with no default) and a `Failure` (`return fail(...)`)
are not violations: the first is a missing-argument error and the second ends the path without
needing to satisfy the return contract. `none` only satisfies a contract written with `?`. The names
`Int`/`Float`/`Byte`/`String`/`Bool`/`List`/`Dict`/`Bytes`/`Range` are checked against `TypeTag`,
`Function` requires a callable value, and a name that the module declares as a `struct` is compared with
`StructInstance::type_name`.

**Interface as contract — structural conformance.** When the contract name is an interface,
`AssertContract` also carries the operations it declares (a `u16` with the name and a `u8` with the
arity of each), because the canon keeps interfaces **structural**: using one as a contract
asks whether the value exposes the declared operations. The arity compared is the one **visible at the call site**
— the receiver is not an argument there, which is how the arities of native methods and of
`BoundMethod` are already declared. A value that does not expose the operation (or exposes a different arity) is a
*contract fault*, and the message names the declared type of the `struct` rather than the generic runtime
kind. `none` satisfies `T?` without exposing anything, which is exactly the meaning of `T?`.

The first published version of this check contained a real defect, found and fixed by
`P00-G15`: the builder counted the interface arity **including the receiver** (`fn draw(self)` =
1) while the runtime compared the visible arity (0), so the contract also rejected
conforming values. The convention was unified in the builder, which now emits the visible arity.

**Question answered.** `TypeIs` was not reused. It tests a value against a *type value* that
the program pushes at run time, whereas the contract is a promise of the signature that
needs to travel in the bytecode together with the parameter name for the failure message. The relevant
reuse is the category table (`TypeTag`), shared by both.

Certification: `docs/conformance/programs/14_signature_contracts.aipo` (parameter, verified
default, `T?`, `Function`, `struct` contract, mutable receiver and return) and
`docs/conformance/diagnostics/12_runtime_contract_violation.aipo` /
`13_runtime_return_contract.aipo` (fault not catchable by `attempt`). Structural interface
conformance: `docs/conformance/programs/16_interface_contracts.aipo` (conforming value, `satisfy`,
`T?` with `none`, operation with an argument) and `docs/conformance/diagnostics/17` and `18`
(missing operation and mismatched arity), plus the tests
`test_interface_contract_accepts_a_conforming_operation` and
`test_interface_contract_names_the_failing_struct` in `crates/aipo-vm/tests/data_and_errors.rs`.
