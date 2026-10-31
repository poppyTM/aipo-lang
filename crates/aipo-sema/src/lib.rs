//! `aipo-sema` implements semantic analysis, lexical scope resolution,
//! mutability path checking, and contract verification for Aipo.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod analyzer;
pub mod prelude;
pub mod symbol;

pub(crate) mod flow;

pub use analyzer::{LIST_MEMBERS, STRING_MEMBERS, SemanticAnalyzer, SemanticFacts};
pub use prelude::{HostFunction, HostParameter, PreludeSurface};
pub use symbol::{MethodSignature, Mutability, Scope, ScopeTree, Symbol, SymbolKind};

use aipo_diagnostics::Diagnostic;
use aipo_hir::HirProgram;
use aipo_source::Source;

/// Runs semantic analysis on an `HirProgram` with only the language's own surface.
#[must_use]
pub fn check(source: &Source, program: &HirProgram) -> (SemanticFacts, Vec<Diagnostic>) {
    let analyzer = SemanticAnalyzer::new(source);
    analyzer.analyze(program)
}

/// Runs semantic analysis with an explicit global surface.
///
/// Embedders that register the standard library pass the surface derived from their
/// registration, so semantic analysis accepts exactly the names the VM can execute.
#[must_use]
pub fn check_with_prelude(
    source: &Source,
    program: &HirProgram,
    surface: &PreludeSurface,
) -> (SemanticFacts, Vec<Diagnostic>) {
    let analyzer = SemanticAnalyzer::with_prelude(source, surface);
    analyzer.analyze(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipo_diagnostics::DiagnosticCode;
    use aipo_hir::lower;
    use aipo_source::{Source, SourceId};
    use aipo_syntax::parse;

    fn analyze_source(text: &str) -> Vec<Diagnostic> {
        let src = Source::new(SourceId::next(), "test.aipo", text);
        let (ast, diags) = parse(&src);
        assert!(diags.is_empty(), "parse diags: {:?}", diags);
        let hir = lower(ast);
        let (_, sema_diags) = check(&src, &hir);
        sema_diags
    }

    #[test]
    fn test_valid_program_passes() {
        let diags = analyze_source("let x = 1\nvar y = x + 2\ny = 5");
        assert!(diags.is_empty(), "expected 0 diags, found: {:?}", diags);
    }

    #[test]
    fn test_unknown_variable() {
        let diags = analyze_source("let x = unknown_name + 1");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
    }

    #[test]
    fn test_redeclaration_in_same_scope() {
        let diags = analyze_source("let x = 1\nlet x = 2");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE);
    }

    #[test]
    fn test_shadowing_in_nested_scope() {
        let diags = analyze_source("let x = 1\nif true {\n  let x = 2\n}");
        assert!(
            diags.is_empty(),
            "expected shadowing to succeed, found: {:?}",
            diags
        );
    }

    #[test]
    fn test_immutable_mutation() {
        let diags = analyze_source("let x = 10\nx = 20");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_READONLY_MUTATION);
    }

    #[test]
    fn test_arity_mismatch() {
        let diags = analyze_source("fn add(a, b) {\n  return a + b\n}\nadd(1)");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_ARITY_MISMATCH);
    }

    #[test]
    fn test_satisfy_interface_success() {
        let code = "interface Greeter {\n  fn greet(name)\n}\n\n#!satisfies Greeter\nstruct Bot {\n}\n\nBot:greet(name) {\n    return name\n}";
        let diags = analyze_source(code);
        assert!(
            diags.is_empty(),
            "expected valid satisfy, found: {:?}",
            diags
        );
    }

    #[test]
    fn test_satisfy_missing_method() {
        let code =
            "interface Greeter {\n  fn greet(name)\n}\n\n#!satisfies Greeter\nstruct Bot {\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
    }

    /// Canon allows one `await` in statement, initializer and return position; a nested
    /// one cannot suspend the enclosing path, so it is reported instead of guessed.
    #[test]
    fn test_await_in_subexpression_is_reported() {
        let code = "async fn worker() {\n  return 21\n}\n\nfn consume(value) {\n  return value\n}\n\nfn caller() {\n  let t = worker()\n  return consume(await t)\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_AWAIT_IN_SUBEXPRESSION
        );
    }

    /// A `Task` that no statement observes was started and can never be joined, which is
    /// canon's forgotten-task diagnostic (never silent).
    #[test]
    fn test_forgotten_task_is_reported() {
        let code = "async fn worker() {\n  return 1\n}\n\nfn caller() {\n  worker()\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_FORGOTTEN_TASK);
    }

    /// Binding the handle instead of discarding it is the canonical way to keep the task
    /// observable, so it must stay silent.
    #[test]
    fn test_bound_task_is_not_reported() {
        let code = "async fn worker() {\n  return 1\n}\n\nfn caller() {\n  let t = worker()\n  return t\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// `await do` is already a sequential-await block: an inner one adds no suspension
    /// point, so canon asks for a diagnostic.
    #[test]
    fn test_nested_await_do_is_reported() {
        let code = "async fn one() {\n  return 1\n}\n\nasync fn outer() {\n  await do {\n    await do {\n      let value = await one()\n    }\n  }\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_NESTED_AWAIT_DO);
    }

    /// The single-block form is the sugar canon sanctions, so it must analyze cleanly.
    #[test]
    fn test_await_do_accepts_sequential_awaits() {
        let code = "async fn one() {\n  return 1\n}\n\nasync fn outer() {\n  await do {\n    let value = await one()\n    return value\n  }\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// ADP-006 §G: a `Task` only comes from an `async fn` call or a combinator, so a literal
    /// operand is provable before execution and reported instead of left to the runtime fault.
    #[test]
    fn test_await_of_a_literal_is_reported_statically() {
        let diags = analyze_source("let value = await 41");
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC
        );
    }

    /// Awaiting a handle from an `async fn` call is the sanctioned shape and stays silent.
    #[test]
    fn test_await_of_a_call_is_not_reported() {
        let code =
            "async fn worker() {\n  return 1\n}\n\nfn caller() {\n  return await worker()\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    #[test]
    fn test_assignment_await_not_reported() {
        let code = "async fn worker() {\n  return 1\n}\n\nfn caller() {\n  var x = 0\n  x = await worker()\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    #[test]
    fn test_immutable_receiver_mutation() {
        let code =
            "struct Point {\n  var x\n  var y\n}\n\nPoint:set_x(self, val) {\n    self.x = val\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_READONLY_MUTATION);
    }

    #[test]
    fn test_mutable_path_mutation_allowed() {
        let code = "var items = [1, 2, 3]\nitems[0] = 10";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    #[test]
    fn test_fixed_field_reassignment_reported() {
        let code = "struct User {\n  fixed id\n  var name\n}\n\nUser:change_id(var self, new_id) {\n    self.id = new_id\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_FIXED_REASSIGN);
    }

    #[test]
    fn test_async_method_forgotten_task_reported() {
        let code = "struct Service {\n}\n\nasync Service:fetch(self) {\n    return 42\n}\n\nlet s = Service{}\ns.fetch()";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_FORGOTTEN_TASK);
    }

    #[test]
    fn test_immutable_binding_field_mutation_allowed() {
        let code = "struct Point {\n  var x\n  var y\n}\n\nlet p = Point{x: 1, y: 2}\np.x = 10";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    #[test]
    fn test_immutable_binding_index_mutation_allowed() {
        let code = "let items = [1, 2, 3]\nitems[0] = 99";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    #[test]
    fn test_fixed_field_instance_mutation_reported() {
        let code = "struct User {\n  fixed id\n  name\n}\n\nvar u = User{id = 1, name = \"Alice\"}\nu.id = 2";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_FIXED_REASSIGN);
    }

    #[test]
    fn test_satisfy_return_type_mismatch() {
        let code = "interface Getter {\n  fn get() -> Int\n}\n\n#!satisfies Getter\nstruct Boxed {\n}\n\nBoxed:get() -> String {\n    return \"hi\"\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC
        );
    }

    #[test]
    fn test_satisfy_receiver_mutability_mismatch() {
        let code = "interface Mutator {\n  fn mutate(var self)\n}\n\n#!satisfies Mutator\nstruct State {}\n\nState:mutate(self) {}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC
        );
    }

    #[test]
    fn test_satisfy_async_mismatch() {
        let code = "interface Fetcher {\n  async fn fetch()\n}\n\n#!satisfies Fetcher\nstruct Service {\n}\n\nService:fetch() {\n    return 1\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC
        );
    }

    #[test]
    fn test_destructuring_list_valid() {
        let code = "let [a, b] = [1, 2]";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    #[test]
    fn test_modern_struct_immutable_by_default() {
        let code = "struct User {\n  id\n  var name\n}\n\nvar u = User{ id: 1, name: \"Alice\" }\nu.id = 2";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_FIXED_REASSIGN);
    }

    #[test]
    fn test_modern_interface_and_var_self() {
        let code = "interface Counter {\n  fn increment(var self)\n}\n\n#!satisfies Counter\nstruct Ticker {\n  var count\n}\n\nTicker:increment(var self) {\n    self.count += 1\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    // --- control-flow analysis ---

    /// A `return <value>` next to a bare `return` leaves the caller unable to know
    /// which shape it received, so it is reported instead of left to runtime.
    #[test]
    fn test_mixed_return_shapes_are_reported() {
        let code = "fn pick(flag: Bool) -> Int {\n  if flag {\n    return 1\n  }\n  return\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_RETURN_VALUE_MISMATCH
        );
    }

    /// Every `return` carrying a value is the ordinary shape and must stay silent.
    #[test]
    fn test_uniform_value_returns_are_accepted() {
        let code = "fn pick(flag: Bool) -> Int {\n  if flag {\n    return 1\n  }\n  return 2\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// Every bare `return` is consistent for a function with no value contract.
    #[test]
    fn test_uniform_void_returns_are_accepted() {
        let code = "fn stop(flag: Bool) {\n  if flag {\n    return\n  }\n  return\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// A function that promises a value but can reach its end without producing
    /// one would hand the caller `none`, so the path is reported.
    #[test]
    fn test_declared_contract_with_falling_path_is_reported() {
        let code = "fn maybe(flag: Bool) -> Int {\n  if flag {\n    return 1\n  }\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_PATH_MISSING_RETURN_VALUE
        );
    }

    /// When every path returns a value, no path falls off the end and the check
    /// must stay silent.
    #[test]
    fn test_total_returns_are_not_reported_as_missing() {
        let code = "fn total(flag: Bool) -> Int {\n  if flag {\n    return 1\n  } else {\n    return 2\n  }\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// `match` with an `else` that returns on every arm is total.
    #[test]
    fn test_match_with_total_arms_is_accepted() {
        let code = "fn classify(v: Int) -> Int {\n  match v {\n    when 1\n      return 10\n    else\n      return 0\n  }\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// `attempt` diverges only when the body *and* the handler both diverge: a
    /// transaction that returns a value in both arms honours a value contract.
    #[test]
    fn test_attempt_with_total_arms_is_accepted() {
        let code = "fn transfer(source: Int) -> Bool {\n  attempt {\n    return true\n  } failed err {\n    return false\n  }\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// Completing the body normally falls through to the statement after the
    /// `attempt`, so a handler that does not diverge still leaves a falling path.
    #[test]
    fn test_attempt_with_falling_handler_is_reported() {
        let code = "fn transfer(source: Int) -> Bool {\n  var visto = \"\"\n  attempt {\n    return true\n  } failed err {\n    visto = err.message\n  }\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_PATH_MISSING_RETURN_VALUE
        );
    }

    /// `match` without an `else` can fall out of the statement, so a value
    /// contract is not honoured on that path.
    #[test]
    fn test_match_without_else_can_fall_through() {
        let code = "fn classify(v: Int) -> Int {\n  match v {\n    when 1\n      return 10\n  }\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_PATH_MISSING_RETURN_VALUE
        );
    }

    /// `loop` without a reachable `break` never completes, so nothing falls off
    /// the end and the missing-value path must not be reported.
    #[test]
    fn test_infinite_loop_is_not_a_missing_value_path() {
        let code = "fn forever() -> Int {\n  loop {\n    return 1\n  }\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// `fail` ends the path like `return`, so it satisfies a value contract.
    #[test]
    fn test_fail_ends_the_path() {
        let code = "fn checked(v: Int) -> Int {\n  if v < 0 {\n    fail (\"negative\")\n  }\n  return v\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// `break` exits only the loop, so the function can still fall off the end.
    #[test]
    fn test_break_does_not_end_the_function_path() {
        let code = "fn first(limit: Int) -> Int {\n  var total = 0\n  var i = 0\n  loop {\n    if i >= limit {\n      break\n    }\n    total = total + i\n    i = i + 1\n  }\n  return total\n}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    // --- non-Bool conditions ---

    /// An integer literal is provably not a `Bool`, so the condition is reported
    /// before execution rather than faulting at runtime.
    #[test]
    fn test_integer_literal_condition_is_reported() {
        let diags = analyze_source("if 42 {\n  let x = 1\n}");
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_NON_BOOL_CONDITION);
    }

    /// A `while` with a literal condition has the same problem.
    #[test]
    fn test_while_literal_condition_is_reported() {
        let diags = analyze_source("while 0 {\n  break\n}");
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_NON_BOOL_CONDITION);
    }

    /// `and` and `or` short-circuit on a `Bool`; a literal operand is reported.
    #[test]
    fn test_logical_operator_literal_operands_are_reported() {
        let diags = analyze_source("let flag = 1 and 2");
        assert_eq!(diags.len(), 2, "found: {diags:?}");
        assert!(
            diags
                .iter()
                .all(|d| d.code == DiagnosticCode::AIPO_SEM_NON_BOOL_CONDITION)
        );
    }

    /// `not` also requires a `Bool`.
    #[test]
    fn test_not_of_literal_is_reported() {
        let diags = analyze_source("let flag = not 7");
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_NON_BOOL_CONDITION);
    }

    /// A `Bool` literal is the canonical condition and must stay silent.
    #[test]
    fn test_bool_literal_condition_is_accepted() {
        let diags = analyze_source("if true {\n  let x = 1\n}");
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// A condition whose type is not statically known is left to the runtime,
    /// so a dynamic program keeps working.
    #[test]
    fn test_dynamic_condition_is_accepted() {
        let diags = analyze_source("var ready = 1\nif ready {\n  let x = 1\n}");
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// The diagnostic carries a replacement, so an editor can offer a quick fix.
    #[test]
    fn test_non_bool_condition_carries_a_suggestion() {
        let diags = analyze_source("if 42 {\n  let x = 1\n}");
        assert_eq!(diags[0].suggestions.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].suggestions[0].replacement, "42 != 0");
    }

    // --- struct construction ---

    /// A field the struct does not declare is a typo, caught at the call site.
    #[test]
    fn test_unknown_struct_field_is_reported() {
        let code = "struct Point {\n  x\n  y\n}\n\nlet p = Point{x: 1, z: 3}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
    }

    /// A declared field supplied exactly once is the ordinary shape.
    #[test]
    fn test_struct_with_all_fields_is_accepted() {
        let code = "struct Point {\n  x\n  y\n}\n\nlet p = Point{x: 1, y: 2}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// A literal that provably breaks a declared field contract is reported
    /// before execution, exactly like a parameter contract.
    #[test]
    fn test_struct_field_contract_literal_violation_is_reported() {
        let code = "struct User {\n  id: Int\n}\n\nlet u = User{id: \"x\"}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(
            diags[0].code,
            DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC
        );
    }

    /// A literal satisfying the field contract stays silent; non-literals are
    /// left to the runtime, mirroring parameter contracts.
    #[test]
    fn test_struct_field_contract_match_is_accepted() {
        let code = "struct User {\n  id: Int\n}\n\nlet u = User{id: 1}";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    // --- enum declaration and construction ---

    /// Declaring the same variant twice is a redeclaration, not a merge.
    #[test]
    fn test_enum_duplicate_variant_is_reported() {
        let code = "enum Estado {\n  Ativo,\n  Ativo,\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE);
    }

    /// A match arm naming a variant the enum does not declare is a typo,
    /// reported even when `else` would otherwise silence exhaustiveness.
    #[test]
    fn test_enum_unknown_variant_in_match_is_reported() {
        let code = "enum Estado {\n  Ativo,\n  Inativo,\n}\n\nfn f(s) {\n  match s {\n    when Estado.Ativo {\n      return 1\n    }\n    when Estado.Inexistente {\n      return 2\n    }\n    else {\n      return 0\n    }\n  }\n}";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
    }

    /// Calling a variant the enum does not declare is reported at the call site.
    #[test]
    fn test_enum_unknown_variant_call_is_reported() {
        let code = "enum Resultado {\n  Sucesso(valor: Int),\n}\n\nlet r = Resultado.Falha(1)";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
    }

    /// A tuple variant called with the wrong payload count is an arity error
    /// before execution, not a runtime fault.
    #[test]
    fn test_enum_tuple_variant_arity_mismatch_is_reported() {
        let code = "enum Resultado {\n  Sucesso(valor: Int),\n}\n\nlet r = Resultado.Sucesso(1, 2)";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_ARITY_MISMATCH);
    }

    /// Only tuple variants are callable: a unit variant takes no call.
    #[test]
    fn test_enum_unit_variant_call_is_reported() {
        let code = "enum Estado {\n  Ativo,\n  Inativo,\n}\n\nlet s = Estado.Ativo()";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_ARITY_MISMATCH);
    }

    /// The exact-arity tuple construction stays silent.
    #[test]
    fn test_enum_tuple_variant_exact_arity_is_accepted() {
        let code = "enum Resultado {\n  Sucesso(valor: Int),\n}\n\nlet r = Resultado.Sucesso(1)";
        let diags = analyze_source(code);
        assert!(diags.is_empty(), "found: {diags:?}");
    }

    /// Accessing a non-existent member on a known collection type is caught statically (D3).
    #[test]
    fn test_unknown_collection_member_d3() {
        let code = "var l = [1, 2, 3]\nl.pop()";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
    }

    #[test]
    fn test_hook_name_warning() {
        let code = "struct Point { var x }\nPoint:constructor(var self, x) { self.x = x }\nPoint:validar() -> Bool { return self.x > 0 }";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 2, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_NOME_DE_HOOK);
        assert_eq!(diags[1].code, DiagnosticCode::AIPO_SEM_NOME_DE_HOOK);
    }

    #[test]
    fn test_empty_invariant_warning() {
        let code = "struct Point { x }\nPoint:invariant { }";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_HOOK_VAZIO);
    }

    #[test]
    fn test_duplicate_hooks_error() {
        let code = "struct Point { x }\nPoint:init(x) { self.x = x }\nPoint:init(x, y) { self.x = x }\nPoint:invariant { self.x > 0 }\nPoint:invariant { self.x < 100 }";
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 2, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_HOOK_DUPLICADO);
        assert_eq!(diags[1].code, DiagnosticCode::AIPO_SEM_HOOK_DUPLICADO);
    }

    #[test]
    fn test_flow_narrowing_is_struct() {
        let code = r#"
struct Point {
    x
    y
}
fn process(val) {
    if val is Point {
        let a = val.x
        let b = val.bad_field
    }
}
"#;
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
        assert!(
            diags[0]
                .message
                .contains("struct 'Point' has no member 'bad_field'")
        );
    }

    #[test]
    fn test_flow_narrowing_is_list() {
        let code = r#"
fn f(l) {
    if l is List {
        l.add(1)
        l.pop()
    }
}
"#;
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
        assert!(diags[0].message.contains("type 'List' has no member 'pop'"));
    }

    #[test]
    fn test_flow_narrowing_negative_in_else() {
        let code = r#"
struct Point {
    x
    y
}
fn f(val) {
    if not (val is Point) {
        return 0
    } else {
        let b = val.bad_field
        return 1
    }
}
"#;
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
        assert!(
            diags[0]
                .message
                .contains("struct 'Point' has no member 'bad_field'")
        );
    }

    #[test]
    fn test_flow_narrowing_nullable_parameter() {
        let code = r#"
struct Point {
    x
    y
}
fn f(p: Point?) {
    if p != none {
        let b = p.bad_field
    }
}
"#;
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
        assert!(
            diags[0]
                .message
                .contains("struct 'Point' has no member 'bad_field'")
        );
    }

    #[test]
    fn test_flow_narrowing_match_variant() {
        let code = r#"
enum State {
    Loading,
    Ready { count: Int },
}
fn check(s: State) {
    match s {
        when State.Ready { count } {
            s.bad_field
        }
        else { }
    }
}
"#;
        let diags = analyze_source(code);
        assert_eq!(diags.len(), 1, "found: {diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::AIPO_SEM_UNKNOWN_NAME);
        assert!(
            diags[0]
                .message
                .contains("struct 'State.Ready' has no member 'bad_field'")
        );
    }
}
