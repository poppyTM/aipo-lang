# Aipo — Diagnostics Catalog (Wave 1)

**Status:** normative
**Scope:** stable diagnostic codes for pipeline stages; JSONL shape
**Update Triggers:** new diagnostic; code changes are breaking for tooling

## Shape

```json
{
  "code": "AIPO_PARSE_UNCLOSED_BLOCK",
  "severity": "error",
  "message": "this block is not closed",
  "primary_span": {"file": "main.aipo", "start": 10, "end": 13, "line": 2, "column": 3},
  "notes": [],
  "suggestions": []
}
```

## Codes (initial)

### Compilation backend (AIPO_COMPILE_*)

| Code | Severity | Trigger |
| --- | --- | --- |
| AIPO_COMPILE_REG_UNSUPPORTED | error | Experimental register lowering cannot safely represent an IR operation, function or encoding operand |

O título está disponível em EN/PT-BR. A CLI inclui a razão técnica em uma nota e sugere `--engine=vm`. Não é uma nova regra de sintaxe; descreve uma limitação do backend. Veja o [guia de reimplementação](../development/runtime-hardening-guide.md).

### Source (AIPO_SRC_*)
| Code | Severity | Trigger |
|---|---|---|
| AIPO_SRC_INVALID_UTF8 | error | file bytes are not valid UTF-8 |

### Lexical (AIPO_LEX_*)
| Code | Severity | Trigger |
|---|---|---|
| AIPO_LEX_UNTERMINATED_STRING | error | string literal never closed |
| AIPO_LEX_UNKNOWN_ESCAPE | error | escape not in `\n \t \r \\ \" \u{...}` set |
| AIPO_LEX_INVALID_UNICODE_ESCAPE | error | `\u{...}` outside scalar range or surrogate |
| AIPO_LEX_INVALID_NUMBER | error | malformed numeric literal |
| AIPO_LEX_UNEXPECTED_CHARACTER | error | character that starts no token |

### Parse (AIPO_PARSE_*)
| Code | Severity | Trigger |
|---|---|---|
| AIPO_PARSE_UNCLOSED_BLOCK | error | block/paren/brace not closed before EOF |
| AIPO_PARSE_UNEXPECTED_TOKEN | error | token cannot start/continue a construct |
| AIPO_PARSE_MISSING_END | error | block opened but `end` missing |
| AIPO_PARSE_INVALID_TARGET | error | assignment target is not a mutable path |
| AIPO_PARSE_NESTING_TOO_DEEP | error | nesting exceeds the parser recursion bound (robustness limit, see ADP-005) |

### Semantic (AIPO_SEM_*)
| Code | Severity | Trigger |
|---|---|---|
| AIPO_SEM_UNKNOWN_NAME | error | name not resolvable in scope |
| AIPO_SEM_REDECLARED_IN_SCOPE | error | same name declared twice in one scope |
| AIPO_SEM_READONLY_MUTATION | error | mutation through `let`/read-only path |
| AIPO_SEM_FIXED_REASSIGN | error | assignment to `fixed` field after construction |
| AIPO_SEM_ARITY_MISMATCH | error | call argument count incompatible (known callee) |
| AIPO_SEM_NAMED_ARG_UNKNOWN | error | named argument not a parameter/field |
| AIPO_SEM_DUPLICATE_NAMED_ARG | error | same name supplied twice |
| AIPO_SEM_RETURN_VALUE_MISMATCH | error | value/no-result mixing in one function |
| AIPO_SEM_PATH_MISSING_RETURN_VALUE | error | value-producing path ends without value |
| AIPO_SEM_NON_BOOL_CONDITION | error | condition/`and`/`or`/`not` operand not provably Bool |
| AIPO_SEM_IMPORT_CYCLE | error | module import graph contains a cycle |
| AIPO_SEM_UNKNOWN_MODULE | error | import path not resolvable |
| AIPO_SEM_EXPORT_UNKNOWN | error | exported name does not exist |
| AIPO_SEM_CONTRACT_VIOLATION_STATIC | error | provable contract violation at call site |
| AIPO_SEM_AWAIT_IN_SUBEXPRESSION | error | explicit `await` outside a statement, initializer or return value (Wave 3) |
| AIPO_SEM_FORGOTTEN_TASK | error | known-`Task` value discarded without `await`, binding or combinator (Wave 3) |
| AIPO_SEM_NESTED_AWAIT_DO | error | `await do` nested inside another `await do` (Wave 3) |
| AIPO_SEM_PARAMETRIC_CONTRACT | error | parametric `Name[Args]` contract written before parametric contracts exist (Wave 3) |

### Package (AIPO_PKG_*)
| Code | Severity | Trigger |
|---|---|---|
| AIPO_PKG_RESOLUTION | error | manifest, local or explicit remote dependency graph, or capability resolution failed |
| AIPO_PKG_LOCK_STALE | error | lockfile required by audit is missing, or an existing lockfile is invalid or divergent from the resolved graph |
| AIPO_PKG_FETCH | error | public GitHub fetch or cache failed, was redirected, exceeded policy limits, or returned an invalid artifact |

### Runtime fault (AIPO_RT_*)
| Code | Severity | Trigger |
|---|---|---|
| AIPO_RT_OVERFLOW | fault | Int arithmetic leaves ±(2^53−1) |
| AIPO_RT_NON_FINITE_FLOAT | fault | Float result NaN/Infinity |
| AIPO_RT_DIV_ZERO | fault | `div` or `%` by zero |
| AIPO_RT_INDEX_OUT_OF_RANGE | fault | List/String index out of range |
| AIPO_RT_KEY_NOT_FOUND | fault | Dict strict access missing key |
| AIPO_RT_NOT_CALLABLE | fault | calling a non-function |
| AIPO_RT_MUTATION_DURING_ITERATION | fault | structural mutation of the iterated collection |
| AIPO_RT_TYPE_MISMATCH | fault | operator/condition/contract runtime violation |
| AIPO_RT_CANCELLED | fault | a cancelled `Task` is awaited or driven (Wave 3) |
| AIPO_RT_AWAIT_CYCLE | fault | a task awaits itself directly or transitively (Wave 3) |
| AIPO_RT_AWAIT_IN_CALLBACK | fault | `await`/`sleep`/join inside a synchronous `invoke` callback, which cannot suspend (Wave 3) |
| AIPO_RT_CAPABILITY_DENIED | fault | host operation attempted without the capability it requires (Wave 4) |
| AIPO_RT_STALE_HANDLE | fault | host handle addressed after its slot was released or reused (Wave 4) |
| AIPO_RT_SCOPE_ESCAPE | fault | scoped host binding reached a heap-publication point outside its scope (Wave 4). Enforced at `SetGlobal`, `Return`, `SetField`, `SetIndex`, `BuildList` and `BuildDict`, each of which names its site in the message; the check is skipped entirely while no host scope has closed. |
| AIPO_RT_MODULE_INIT_FAILED | fault | module top-level evaluation failed during initialization (maturity audit) |
| AIPO_RT_INVALID_SCHEMA | fault | host surface description is internally inconsistent (maturity audit) |

### Runtime failure (AIPO_RT_FAILURE_*)
| Code | Severity | Trigger |
|---|---|---|
| AIPO_RT_FAILURE_UNCAUGHT | error | Failure propagated to top level |

## Rules

- Codes are added; never repurposed. Removing a code is a breaking tooling change.
- Every code has at least one pass/fail fixture under `docs/conformance/fixtures/`.
- Machine output uses `--message-format=jsonl`; human output is default.
