//! Lowers an `HirProgram` into `CoreModule`.
//!
//! The builder performs lexical scope analysis so the emitter can resolve every
//! `Load`/`Store` to a local slot, a captured upvalue or a global:
//!
//! - parameters occupy slots `0..params.len()`;
//! - declared locals continue from `params.len()` and are never freed, so block
//!   scopes only change name visibility;
//! - a nested function captures the enclosing names it references (snapshot capture,
//!   which also gives canonical per-iteration capture inside loops);
//! - every control-flow form in HIR is lowered: `if`/`elif`/`else`, `match`, `loop`,
//!   `while`, `repeat`, `each`, `break`, `continue`, `attempt`/`failed`, `and`/`or`
//!   short-circuiting, ranges and pipelines.
//!
//! Hidden locals used by the lowering (`__t…`) are ordinary slots and cannot collide
//! with user identifiers in practice.

use crate::ir::*;
use aipo_ast::{BinaryOp, Literal, TypeAnnotation};
use aipo_hir::*;
use aipo_lexer::{parse_float_literal, parse_int_literal};
use aipo_source::SourceSpan;
use std::collections::{HashMap, HashSet};

/// Sentinel used when an integer literal does not fit in `i64`.
///
/// Such a literal is lexically well formed but outside `Int`'s `±(2^53 - 1)` range, so it
/// must never be coerced silently (the old `unwrap_or(0)` produced a bogus `0`). `i64::MAX`
/// is itself outside that range, so the constant faults with `AIPO_RT_OVERFLOW` when it is
/// loaded — on both backends, by construction.
const OUT_OF_RANGE_INT: i64 = i64::MAX;

/// Sentinel used when a float literal cannot be parsed at all.
///
/// `NaN` is not a legal `Float`, so the constant faults with `AIPO_RT_NON_FINITE_FLOAT`
/// instead of silently becoming `0.0`.
const MALFORMED_FLOAT: f64 = f64::NAN;

/// Loop bookkeeping for `break`/`continue` patching.
#[derive(Default)]
struct LoopCtx {
    /// Instruction index the `continue` statement must jump to, when already known.
    continue_target: Option<usize>,
    /// Indices of `Jump` instructions emitted by `break`.
    break_jumps: Vec<usize>,
    /// Indices of `Jump` or `JumpIfFalse` instructions emitted by `continue`.
    continue_jumps: Vec<usize>,
    /// Active failure handlers when the loop started; a `break`/`continue` emitted with
    /// more than this open inside the body pops the difference first.
    handler_depth_at_entry: usize,
    /// Active iteration guards when the loop started (auditoria IR-8/IR-12).
    iter_depth_at_entry: usize,
}

/// Per-function lowering state.
struct FnCtx {
    params: Vec<String>,
    locals: Vec<String>,
    upvalues: Vec<String>,
    /// Block scopes; `scopes[0]` is the function root and is never popped.
    scopes: Vec<Vec<(String, usize)>>,
    loops: Vec<LoopCtx>,
    /// `true` for the module entry script, whose root-scope declarations are
    /// module-scoped globals rather than frame slots.
    module_scope: bool,
    /// Failure handlers open at this point of the frame, lexically counted.
    handler_depth: usize,
    /// Iteration guards open at this point of the frame, lexically counted.
    iter_depth: usize,
    /// Return contract of the function being lowered, checked at every `return`.
    return_contract: Option<TypeAnnotation>,
    /// Span of the last `Type.field = ...` assignment in this frame, if any.
    ///
    /// Canon validates `invariant()` at stable mutable boundaries rather than after each
    /// internal assignment, so the boundary marker is emitted once per frame (and per
    /// top-level statement) using this span.
    mutations: Option<SourceSpan>,
}

impl FnCtx {
    fn new(params: Vec<String>) -> Self {
        Self {
            params,
            locals: Vec::new(),
            upvalues: Vec::new(),
            scopes: vec![Vec::new()],
            loops: Vec::new(),
            module_scope: false,
            handler_depth: 0,
            iter_depth: 0,
            return_contract: None,
            mutations: None,
        }
    }

    /// `true` when declarations land in the module root scope.
    ///
    /// Module-scoped bindings must stay globals: a function declared in the module
    /// resolves them by name (the emitter falls back to `Slot::Global`), and canon
    /// requires module bindings to be visible to every function of that module.
    /// Declarations in nested blocks stay frame slots, so per-iteration capture and
    /// block scoping keep working exactly as in a function body.
    fn is_module_root(&self) -> bool {
        self.module_scope && self.scopes.len() == 1
    }

    /// Declares a module-level or frame-level binding, honoring module scope.
    fn declare_binding(&mut self, name: &str) {
        if !self.is_module_root() {
            self.declare(name);
        }
    }

    fn lookup(&self, name: &str) -> Option<Access> {
        if let Some(idx) = self.params.iter().position(|p| p == name) {
            return Some(Access::Local(idx));
        }
        for scope in self.scopes.iter().rev() {
            if let Some((_, slot)) = scope.iter().rev().find(|(n, _)| n == name) {
                return Some(Access::Local(*slot));
            }
        }
        self.upvalues
            .iter()
            .position(|u| u == name)
            .map(Access::Upvalue)
    }

    fn declare(&mut self, name: &str) -> usize {
        let slot = self.params.len() + self.locals.len();
        self.locals.push(name.to_string());
        self.scopes
            .last_mut()
            .expect("function root scope always exists")
            .push((name.to_string(), slot));
        slot
    }

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
}

/// Result of resolving a name in a lexical scope.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Access {
    /// Parameter or local slot of the current frame.
    Local(usize),
    /// Captured upvalue index of the current closure.
    Upvalue(usize),
}

/// Builder that transforms HIR nodes into sequential Core IR instructions.
#[derive(Default)]
pub struct IrBuilder {
    functions: Vec<CoreFunction>,
    fn_stack: Vec<FnCtx>,
    closure_counter: usize,
    hidden_counter: usize,
    /// Declared struct fields by type name, in canonical declaration order, carrying each
    /// field's `fixed` marker and its optional default expression.
    structs: Vec<CoreStruct>,
    struct_fields: HashMap<String, Vec<(String, bool, Option<HirExpr>)>>,
    /// Parameter layout of every declared function, used to place call arguments.
    signatures: HashMap<String, DeclaredSignature>,
    /// Declared `init` hooks by struct type name.
    ///
    /// Canon makes `init` the construction signature: `Type{...}` aligns its arguments with
    /// these parameters and calls the hook, so the construction surface stays the same while
    /// the parameter names and defaults come from `init`.
    init_hooks: HashMap<String, DeclaredSignature>,
    /// Operations declared by each interface, as `(name, caller-visible arity)`.
    ///
    /// Canon makes an interface a structural contract, so a signature that names one promises
    /// the value exposes these operations; the runtime check needs the list. The arity is the one
    /// a call site sees — the receiver is not an argument there — which is also how native
    /// method and bound-method arities are declared, so both sides of the runtime check agree.
    interfaces: HashMap<String, Vec<(String, usize)>>,
    /// Struct types that declare an `invariant()` hook.
    ///
    /// Canon evaluates the hook at the end of every construction, so the call site needs to
    /// know the type has one before it emits the check.
    invariant_hooks: HashSet<String>,
    /// Names of declared `async fn` functions (plain and `Type.method`): calling
    /// one produces a `Task`. Collected before any body is lowered so forward
    /// references resolve, backing the known-Task analysis.
    async_fns: HashSet<String>,
    enum_unit_variants: HashMap<String, HashSet<String>>,
    enum_variant_fields: HashMap<String, Vec<String>>,
}

/// Parameter layout of a declared function.
///
/// Canon orders named arguments by the declaration, not by source order, and evaluates a
/// parameter default inside the callee, so the call site needs the declaration's parameter
/// names and the boundary between required and defaulted parameters.
struct DeclaredSignature {
    /// Parameter layout, in declaration order.
    params: Vec<DeclaredParam>,
    /// How many leading parameters have no default.
    required: usize,
}

/// One declared parameter of a function, `init` hook or method.
///
/// Call alignment reads the name and the span; the parameter's optional contract is checked
/// by the callee's own prologue, which is the runtime call boundary canon asks for.
#[derive(Clone)]
struct DeclaredParam {
    /// Parameter name, used to align named arguments with the declaration.
    name: String,
    /// Declaration span, reported when the caller omits the parameter.
    span: SourceSpan,
}

/// Builds the declaration layout entry for one parameter.
fn declared_param(param: &HirParam) -> DeclaredParam {
    DeclaredParam {
        name: param.name.clone(),
        span: param.span,
    }
}

/// Where one call argument comes from once the call has been aligned with its declaration.
enum ResolvedArgument<'a> {
    /// A provided argument expression, pushed in declaration order.
    Provided(&'a HirExpr),
    /// A parameter the caller skipped, filled by the callee's default prologue.
    Omitted(SourceSpan),
}

impl IrBuilder {
    /// Creates a new IR builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Lowers an `HirProgram` into a `CoreModule`.
    pub fn build(mut self, program: &HirProgram) -> CoreModule {
        // Signatures are collected before any body is lowered so a call can be aligned with a
        // declaration that appears later in the file. `init` hooks are collected the same way:
        // canon makes the construction signature come from `init`, so `Type{...}` must see the
        // hook no matter where in the file it is declared.
        for item in &program.items {
            if let HirItem::Fn(f) = item {
                if f.is_async {
                    self.async_fns.insert(f.name.clone());
                }
                self.signatures.insert(
                    f.name.clone(),
                    DeclaredSignature {
                        params: f.params.iter().map(declared_param).collect(),
                        required: f.params.iter().filter(|p| p.default.is_none()).count(),
                    },
                );
            }
            if let HirItem::Interface(interface) = item {
                self.interfaces.insert(
                    interface.name.clone(),
                    interface
                        .methods
                        .iter()
                        .map(|method| {
                            // The receiver is not an argument at a call site, and `dot-call`
                            // bindings report their arity the same way.
                            let arguments = method.params.iter().filter(|p| !p.is_self).count();
                            (method.name.clone(), arguments)
                        })
                        .collect(),
                );
            }
            if let HirItem::Impl(imp) = item {
                if imp.invariant.is_some() {
                    self.invariant_hooks.insert(imp.target.clone());
                }
                for method in imp.methods.iter().filter(|m| m.is_async) {
                    self.async_fns
                        .insert(format!("{}.{}", imp.target, method.name));
                }
                if let Some(init) = &imp.init {
                    // Canon's construction surface is `Type{...}`: the implicit `self` receiver
                    // is not part of it, so only the declared parameters are mapped.
                    let declared: Vec<&HirParam> =
                        init.params.iter().filter(|p| !p.is_self).collect();
                    self.init_hooks.insert(
                        imp.target.clone(),
                        DeclaredSignature {
                            params: declared.iter().map(|p| declared_param(p)).collect(),
                            required: declared.iter().filter(|p| p.default.is_none()).count(),
                        },
                    );
                }
            }
        }

        for item in &program.items {
            match item {
                HirItem::Fn(f) => {
                    let func = self.build_function(f.name.clone(), f);
                    self.functions.push(func);
                }
                HirItem::Struct(s) => {
                    let declared: Vec<(String, bool, Option<HirExpr>)> = s
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.is_fixed, field.default.clone()))
                        .collect();
                    let fields: Vec<(String, bool)> = declared
                        .iter()
                        .map(|(name, fixed, _)| (name.clone(), *fixed))
                        .collect();
                    self.struct_fields.insert(s.name.clone(), declared);
                    self.structs.push(CoreStruct {
                        name: s.name.clone(),
                        fields,
                    });
                }
                HirItem::Enum(e) => {
                    for v in &e.variants {
                        let full_name = format!("{}.{}", e.name, v.name);
                        match &v.payload {
                            aipo_hir::HirEnumVariantPayload::Unit => {
                                self.enum_unit_variants
                                    .entry(e.name.clone())
                                    .or_default()
                                    .insert(v.name.clone());
                                self.structs.push(CoreStruct {
                                    name: full_name.clone(),
                                    fields: Vec::new(),
                                });
                                self.struct_fields.insert(full_name, Vec::new());
                            }
                            aipo_hir::HirEnumVariantPayload::Tuple(fields) => {
                                let field_names: Vec<String> = fields
                                    .iter()
                                    .enumerate()
                                    .map(|(idx, f)| {
                                        f.name.clone().unwrap_or_else(|| idx.to_string())
                                    })
                                    .collect();
                                let struct_f: Vec<(String, bool)> =
                                    field_names.iter().map(|n| (n.clone(), true)).collect();
                                let declared: Vec<(String, bool, Option<HirExpr>)> = field_names
                                    .iter()
                                    .map(|n| (n.clone(), true, None))
                                    .collect();
                                self.enum_variant_fields
                                    .insert(full_name.clone(), field_names.clone());
                                self.struct_fields.insert(full_name.clone(), declared);
                                self.structs.push(CoreStruct {
                                    name: full_name.clone(),
                                    fields: struct_f,
                                });

                                // Constructor function `Estado.Desligado(...)`
                                self.fn_stack.push(FnCtx::new(field_names.clone()));
                                let mut instrs = Vec::new();
                                for name in &field_names {
                                    instrs.push(CoreInst::Load(name.clone(), v.span));
                                }
                                instrs.push(CoreInst::BuildStruct {
                                    type_name: full_name.clone(),
                                    field_count: field_names.len(),
                                    defer_fixed: false,
                                    span: v.span,
                                });
                                if self.invariant_hooks.contains(&e.name) {
                                    self.build_invariant_check(&e.name, v.span, &mut instrs);
                                }
                                instrs.push(CoreInst::Return {
                                    has_value: true,
                                    span: v.span,
                                });
                                let ctx = self.fn_stack.pop().expect("fn context present");
                                self.functions.push(CoreFunction {
                                    name: full_name,
                                    is_async: false,
                                    params: ctx.params,
                                    locals: ctx.locals,
                                    upvalues: ctx.upvalues,
                                    instructions: instrs,
                                    span: v.span,
                                });
                            }
                            aipo_hir::HirEnumVariantPayload::Struct(fields) => {
                                let field_names: Vec<String> =
                                    fields.iter().map(|f| f.name.clone()).collect();
                                let struct_f: Vec<(String, bool)> =
                                    field_names.iter().map(|n| (n.clone(), true)).collect();
                                let declared: Vec<(String, bool, Option<HirExpr>)> = field_names
                                    .iter()
                                    .map(|n| (n.clone(), true, None))
                                    .collect();
                                self.enum_variant_fields
                                    .insert(full_name.clone(), field_names);
                                self.struct_fields.insert(full_name.clone(), declared);
                                self.structs.push(CoreStruct {
                                    name: full_name,
                                    fields: struct_f,
                                });
                            }
                        }
                    }
                }
                HirItem::Impl(imp) => {
                    if let Some(init) = &imp.init {
                        let name = format!("{}.init", imp.target);
                        let func = self.build_init_function(name, init);
                        self.functions.push(func);
                    }
                    if let Some(invariant) = &imp.invariant {
                        let name = format!("{}.invariant", imp.target);
                        let func = self.build_function(name, invariant);
                        self.functions.push(func);
                    }
                    for method in &imp.methods {
                        let name = format!("{}.{}", imp.target, method.name);
                        let func = self.build_function(name, method);
                        self.functions.push(func);
                    }
                }
                HirItem::Batch(binding) => {
                    // Batch association promotes free functions to methods: each listed function
                    // is copied as `Tipo.fn`. Semáfora guarantees the source declares `self` as
                    // its first parameter (SYNTAX.md §5.3), so no receiver is ever injected here
                    // and the mutability the author wrote (`self` vs `var self`) is exactly what
                    // the promoted method gets. The original stays callable on its own; only the
                    // binding is new.
                    for function_name in &binding.functions {
                        let Some((_, source)) = program.items.iter().find_map(|item| match item {
                            HirItem::Fn(f) if &f.name == function_name => Some(((), f)),
                            _ => None,
                        }) else {
                            continue;
                        };
                        if !source.params.first().is_some_and(|p| p.is_self) {
                            continue;
                        }
                        let promoted = aipo_hir::HirFunctionDecl {
                            directives: source.directives.clone(),
                            name: format!("{}.{}", binding.target, function_name),
                            is_async: source.is_async,
                            params: source.params.clone(),
                            return_type: source.return_type.clone(),
                            body: source.body.clone(),
                            span: source.span,
                        };
                        let name = promoted.name.clone();
                        let built = self.build_function(name, &promoted);
                        self.functions.push(built);
                    }
                }
                HirItem::Interface(_) | HirItem::Import(_) | HirItem::Export(_) => {}
            }
        }

        let mut module_ctx = FnCtx::new(Vec::new());
        module_ctx.module_scope = true;
        self.fn_stack.push(module_ctx);
        let mut top_level_insts = Vec::new();
        for stmt in &program.statements {
            self.build_stmt(stmt, &mut top_level_insts);
        }
        top_level_insts.push(CoreInst::Return {
            has_value: false,
            span: program.span,
        });
        let top_ctx = self
            .fn_stack
            .pop()
            .expect("top-level function context is always present");

        let top_level = CoreFunction {
            name: "__top_level__".to_string(),
            is_async: false,
            params: top_ctx.params,
            locals: top_ctx.locals,
            upvalues: top_ctx.upvalues,
            instructions: top_level_insts,
            span: program.span,
        };

        CoreModule {
            functions: self.functions,
            top_level,
            structs: self.structs,
            span: program.span,
        }
    }

    fn build_function(&mut self, name: String, f: &HirFunctionDecl) -> CoreFunction {
        let params: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        self.fn_stack.push(FnCtx::new(params));
        self.fn_stack
            .last_mut()
            .expect("function context is always active")
            .return_contract = f.return_type.clone();
        let mut instructions = Vec::new();
        self.build_default_prologue(&f.params, &mut instructions);
        self.build_contract_prologue(&f.params, &mut instructions);
        for stmt in &f.body {
            self.build_stmt(stmt, &mut instructions);
        }
        self.build_mutation_boundary(&mut instructions);
        instructions.push(CoreInst::Return {
            has_value: false,
            span: f.span,
        });
        let ctx = self
            .fn_stack
            .pop()
            .expect("function context is always present");

        CoreFunction {
            name,
            is_async: f.is_async,
            params: ctx.params,
            locals: ctx.locals,
            upvalues: ctx.upvalues,
            instructions,
            span: f.span,
        }
    }

    /// Builds a struct's `init` hook.
    ///
    /// The hook has the same shape as any function — `self` is parameter 0, followed by the
    /// declared parameters (the parser injects the implicit receiver). Canon adds one rule:
    /// `init` does not produce a value directly, the construction produces the new instance, so
    /// the generated epilogue returns the instance it was handed and the call site uses that
    /// result as the value of `Type{...}`.
    fn build_init_function(&mut self, name: String, f: &HirFunctionDecl) -> CoreFunction {
        let mut function = self.build_function(name, f);
        if let Some(last) = function.instructions.last_mut() {
            *last = CoreInst::Load("self".to_string(), f.span);
        }
        function.instructions.push(CoreInst::Return {
            has_value: true,
            span: f.span,
        });
        function
    }

    /// Emits the call that runs a type's `init` hook inside `Type{...}`.
    ///
    /// Argument placement mirrors a normal call because canon keeps the construction surface
    /// unchanged: positional arguments follow the declaration, named arguments are matched by
    /// parameter name, and a skipped parameter with a default is left to the callee's prologue
    /// through [`CoreInst::PushUnset`]. Slot numbers are offset by one because the implicit
    /// `self` receiver occupies slot 0.
    fn build_init_call(
        &mut self,
        type_name: &str,
        fields: &[(Option<String>, HirExpr)],
        signature: &DeclaredSignature,
        span: SourceSpan,
        out: &mut Vec<CoreInst>,
    ) {
        let receiver = self.hidden_local("init_self");
        let receiver_name = self.hidden_name(receiver);

        out.push(CoreInst::Store(receiver_name.clone(), span));
        out.push(CoreInst::Load(receiver_name.clone(), span));
        out.push(CoreInst::MakeFunction(format!("{type_name}.init"), span));
        out.push(CoreInst::Load(receiver_name, span));

        match self.resolve_init_arguments(fields, signature) {
            Some(resolved) => {
                for (index, argument) in resolved.iter().enumerate() {
                    match argument {
                        Some(expression) => self.build_expr(expression, out),
                        // `Unset` marks the argument position as "not provided"; because the
                        // receiver is pushed first, declared parameter `index` sits at arg
                        // position `index + 1`, which is where this instruction lands.
                        None => {
                            let span = signature.params[index].span;
                            out.push(CoreInst::PushUnset(span));
                        }
                    }
                }
                out.push(CoreInst::Call {
                    arg_count: resolved.len() + 1,
                    span,
                });
            }
            None => {
                for (_, expression) in fields {
                    self.build_expr(expression, out);
                }
                out.push(CoreInst::Call {
                    arg_count: fields.len() + 1,
                    span,
                });
            }
        }

        // Discard the construction-time copy; the hook returned the same instance.
        out.push(CoreInst::Pop(span));
    }

    /// Emits the `invariant()` check for a freshly built instance.
    ///
    /// Canon makes the hook a read-only predicate over `self` whose lines are combined by
    /// `and`, so the check is a normal call to the generated `<Type>.invariant` predicate whose
    /// result [`CoreInst::AssertInvariant`] turns into a contract fault when it is `false`.
    fn build_invariant_check(
        &mut self,
        type_name: &str,
        span: SourceSpan,
        out: &mut Vec<CoreInst>,
    ) {
        let holder = self.hidden_local("invariant_self");
        let holder_name = self.hidden_name(holder);

        out.push(CoreInst::Store(holder_name.clone(), span));
        out.push(CoreInst::Load(holder_name.clone(), span));
        out.push(CoreInst::MakeFunction(
            format!("{type_name}.invariant"),
            span,
        ));
        out.push(CoreInst::Load(holder_name, span));
        out.push(CoreInst::Call { arg_count: 1, span });
        out.push(CoreInst::AssertInvariant {
            type_name: type_name.to_string(),
            span,
        });
    }

    /// Aligns a construction literal with an `init` declaration.
    ///
    /// Returns one entry per declared parameter, `None` meaning "not written, use the default".
    /// Returns `None` overall when alignment is unsafe — too many arguments, an unknown field
    /// name or a missing required parameter — so the call keeps its source order and the
    /// runtime arity check reports the mismatch instead of silently dropping an argument.
    fn resolve_init_arguments<'a>(
        &self,
        fields: &'a [(Option<String>, HirExpr)],
        signature: &DeclaredSignature,
    ) -> Option<Vec<Option<&'a HirExpr>>> {
        let positional = fields.iter().take_while(|(name, _)| name.is_none()).count();
        if positional > signature.params.len() {
            return None;
        }

        let mut resolved: Vec<Option<&'a HirExpr>> = Vec::with_capacity(signature.params.len());
        let mut consumed = 0usize;
        for (index, param) in signature.params.iter().enumerate() {
            if index < positional {
                resolved.push(Some(&fields[index].1));
                consumed += 1;
                continue;
            }
            match fields
                .iter()
                .skip(positional)
                .find(|(name, _)| name.as_deref() == Some(param.name.as_str()))
            {
                Some((_, expression)) => {
                    resolved.push(Some(expression));
                    consumed += 1;
                }
                None if index < signature.required => return None,
                None => resolved.push(None),
            }
        }

        (consumed == fields.len()).then_some(resolved)
    }

    /// Emits the prologue that fills parameters the caller omitted with their defaults.
    ///
    /// Canon evaluates a default at every call, inside the callee, and allows it to reference
    /// earlier parameters — so the default expression is compiled into the function prologue,
    /// where earlier parameters are already bound as frame slots. Each default is guarded by
    /// [`CoreInst::JumpIfSetLocal`]: a parameter the caller supplied jumps over its own
    /// default, and an omitted one falls through and evaluates it. A guard therefore targets
    /// the instruction *after its own* default block, never a shared exit — otherwise a
    /// supplied leading parameter would skip the defaults of the parameters that follow it.
    fn build_default_prologue(&mut self, params: &[HirParam], out: &mut Vec<CoreInst>) {
        for (slot, param) in params.iter().enumerate() {
            let Some(default) = &param.default else {
                continue;
            };
            let guard = out.len();
            out.push(CoreInst::JumpIfSetLocal {
                slot,
                target: 0,
                span: param.span,
            });
            self.build_expr(default, out);
            out.push(CoreInst::Store(param.name.clone(), param.span));

            let after_default = out.len() as isize;
            if let CoreInst::JumpIfSetLocal { target, .. } = &mut out[guard] {
                *target = after_default;
            }
        }
    }

    /// Emits the runtime check for every declared parameter contract.
    ///
    /// Canon checks `name: Type` and `name!: Type` at the call boundary, so the prologue loads
    /// each annotated parameter after the defaults have filled the omitted ones. A parameter the
    /// caller omitted and no default supplied keeps the sentinel, which the check ignores: the
    /// missing argument is reported as an arity/name error, not as a type violation.
    fn build_contract_prologue(&mut self, params: &[HirParam], out: &mut Vec<CoreInst>) {
        for param in params {
            let Some(contract) = &param.type_annotation else {
                continue;
            };
            out.push(CoreInst::Load(param.name.clone(), param.span));
            out.push(CoreInst::AssertContract {
                type_name: contract.name.clone(),
                nullable: contract.is_nullable,
                position: format!("parameter `{}`", param.name),
                operations: self.interface_operations(&contract.name),
                span: contract.span,
            });
            out.push(CoreInst::Pop(param.span));
        }
    }

    /// Emits the contract check for the value a `return` is about to produce.
    ///
    /// The value is already on top of the stack, and [`CoreInst::AssertContract`] peeks at it,
    /// so the check does not disturb the value the `Return` consumes next.
    fn build_return_contract(&mut self, span: SourceSpan, out: &mut Vec<CoreInst>) {
        let Some(contract) = self
            .fn_stack
            .last()
            .and_then(|ctx| ctx.return_contract.clone())
        else {
            return;
        };
        let operations = self.interface_operations(&contract.name);
        out.push(CoreInst::AssertContract {
            type_name: contract.name,
            nullable: contract.is_nullable,
            position: "return".to_string(),
            operations,
            span,
        });
    }

    /// Operations an interface contract requires, in declaration order.
    fn interface_operations(&self, name: &str) -> Vec<(String, usize)> {
        self.interfaces.get(name).cloned().unwrap_or_default()
    }

    /// Emits the stable-boundary verification for the frame's provisional mutations.
    ///
    /// Canon validates `invariant()` at stable mutable boundaries rather than after each
    /// internal assignment, so the assignment did not check anything itself: the boundary
    /// verifies every journaled instance, publishes the new state or rolls it back.
    fn build_mutation_boundary(&mut self, out: &mut Vec<CoreInst>) {
        let span = self
            .fn_stack
            .last_mut()
            .and_then(|ctx| ctx.mutations.take());
        if let Some(span) = span {
            out.push(CoreInst::CheckMutations(span));
        }
    }

    /// Records that the current frame assigned a struct field.
    fn mark_mutation(&mut self, span: SourceSpan) {
        if let Some(ctx) = self.fn_stack.last_mut() {
            ctx.mutations = Some(span);
        }
    }

    /// Records a field assignment and closes a stable boundary when the entry script made it.
    ///
    /// The module entry script has no enclosing operation, so each mutation statement is its
    /// own boundary — including one nested in an `if` or an `attempt`, which must be able to
    /// catch the failure of the statement that violated the invariant. Inside a function or
    /// method the boundary is the frame itself, because canon allows a transiently invalid
    /// state while an operation is still running.
    fn mark_field_mutation(&mut self, span: SourceSpan, out: &mut Vec<CoreInst>) {
        self.mark_mutation(span);
        if self.fn_stack.last().is_some_and(|ctx| ctx.module_scope) {
            self.build_mutation_boundary(out);
        }
    }

    /// Places a call's arguments in declaration order when the callee is a declared function.
    ///
    /// Returns `None` when the call cannot be aligned safely, in which case argument order is
    /// left exactly as written and the runtime arity check reports the mismatch. Aligning is
    /// only safe when every provided argument is consumed: a call that passes too many
    /// arguments, names a parameter that does not exist, or omits a required parameter must
    /// keep failing instead of being silently rearranged.
    fn resolve_arguments<'a>(
        &self,
        callee: &HirExpr,
        args: &'a [HirCallArg],
    ) -> Option<Vec<ResolvedArgument<'a>>> {
        let HirExpr::Identifier(name, _) = callee else {
            return None;
        };
        let signature = self.signatures.get(name)?;
        let positional = args.iter().take_while(|arg| arg.name.is_none()).count();
        // Positional arguments follow the declaration once it is reached.
        if positional > signature.params.len() {
            return None;
        }

        // Detect a trailing unnamed argument (from a trailing block `call(...) do { ... }`)
        // after named arguments.
        let has_trailing_block =
            positional < args.len() && args.last().is_some_and(|arg| arg.name.is_none());
        let trailing_arg = if has_trailing_block {
            Some(&args[args.len() - 1])
        } else {
            None
        };
        let named_args_end = if has_trailing_block {
            args.len() - 1
        } else {
            args.len()
        };
        let named_args = &args[positional..named_args_end];

        // The trailing block targets the last parameter if not already provided positionally.
        let trailing_param_idx = if has_trailing_block && signature.params.len() > positional {
            Some(signature.params.len() - 1)
        } else {
            None
        };

        let mut resolved: Vec<ResolvedArgument<'a>> = Vec::with_capacity(signature.params.len());
        let mut consumed = 0usize;
        for (index, param) in signature.params.iter().enumerate() {
            if index < positional {
                resolved.push(ResolvedArgument::Provided(&args[index].value));
                consumed += 1;
                continue;
            }
            if Some(index) == trailing_param_idx {
                if let Some(tb) = trailing_arg {
                    // Reject if a named argument also targeted this parameter.
                    if named_args
                        .iter()
                        .any(|arg| arg.name.as_deref() == Some(param.name.as_str()))
                    {
                        return None;
                    }
                    resolved.push(ResolvedArgument::Provided(&tb.value));
                    consumed += 1;
                    continue;
                }
            }
            match named_args
                .iter()
                .find(|arg| arg.name.as_deref() == Some(param.name.as_str()))
            {
                Some(named) => {
                    resolved.push(ResolvedArgument::Provided(&named.value));
                    consumed += 1;
                }
                None if index < signature.required => return None,
                None => resolved.push(ResolvedArgument::Omitted(param.span)),
            }
        }

        (consumed == args.len()).then_some(resolved)
    }

    /// Builds an anonymous function body and returns its generated function name.
    ///
    /// Captures are registered before the body is lowered so that intermediate
    /// closures expose the value transitively to deeper closures.
    fn build_closure(
        &mut self,
        f: &HirFunctionExpr,
        captures: &[String],
        self_capture: Option<&str>,
    ) -> String {
        let name = format!("__closure_{}", self.closure_counter);
        self.closure_counter += 1;

        let params: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        self.fn_stack.push(FnCtx::new(params));
        self.fn_stack
            .last_mut()
            .expect("closure context is always active")
            .return_contract = f.return_type.clone();
        let target = self.fn_stack.len() - 1;
        for capture in captures {
            // The self capture is the recursion handle this very closure will fill with
            // itself: it is never looked up in enclosing scopes, only registered as the
            // closure's own upvalue (the `Unset` placeholder) so body references resolve
            // to it.
            if Some(capture.as_str()) == self_capture {
                if !self.fn_stack[target].upvalues.iter().any(|u| u == capture) {
                    self.fn_stack[target].upvalues.push(capture.clone());
                }
                continue;
            }
            self.ensure_capture(target, capture);
        }
        let mut instructions = Vec::new();
        self.build_default_prologue(&f.params, &mut instructions);
        self.build_contract_prologue(&f.params, &mut instructions);
        for stmt in &f.body {
            self.build_stmt(stmt, &mut instructions);
        }
        self.build_mutation_boundary(&mut instructions);
        instructions.push(CoreInst::Return {
            has_value: false,
            span: f.span,
        });
        let ctx = self
            .fn_stack
            .pop()
            .expect("closure context is always present");

        self.functions.push(CoreFunction {
            name: name.clone(),
            is_async: f.is_async,
            params: ctx.params,
            locals: ctx.locals,
            upvalues: ctx.upvalues,
            instructions,
            span: f.span,
        });

        name
    }

    /// Checks whether an identifier name is referenced inside a function body.
    fn references_name(&self, f: &HirFunctionExpr, name: &str) -> bool {
        let mut free = Vec::new();
        let mut seen = HashSet::new();
        for stmt in &f.body {
            collect_free_stmt(stmt, &mut seen, &mut free);
        }
        free.iter().any(|n| n == name)
    }

    /// Returns the names referenced by a closure body that resolve to an enclosing scope.
    fn enclosing_captures(&mut self, f: &HirFunctionExpr) -> Vec<String> {
        let mut free = Vec::new();
        let mut seen = HashSet::new();
        for stmt in &f.body {
            collect_free_stmt(stmt, &mut seen, &mut free);
        }
        // `build_closure` pushes the closure's own context *after* this call, so every
        // context currently on the stack is an enclosing one.
        let outermost = self.fn_stack.len();
        free.retain(|n| {
            (0..outermost)
                .rev()
                .any(|i| self.fn_stack[i].lookup(n).is_some())
        });
        free
    }

    /// Ensures `name` is available as an upvalue of the function context at `target`.
    ///
    /// Walks outwards one function at a time so intermediate closures capture the value
    /// transitively before the innermost one does.
    fn ensure_capture(&mut self, target: usize, name: &str) -> bool {
        if target == 0 {
            return false;
        }
        let enclosing = target - 1;
        let resolvable =
            self.fn_stack[enclosing].lookup(name).is_some() || self.ensure_capture(enclosing, name);
        if !resolvable {
            return false;
        }
        if !self.fn_stack[target].upvalues.iter().any(|u| u == name) {
            self.fn_stack[target].upvalues.push(name.to_string());
        }
        true
    }

    fn hidden_local(&mut self, purpose: &str) -> usize {
        // Skip names the user already bound in this frame: a hidden local that collides
        // with a real identifier would shadow it and miscompile the user's reference
        // (auditoria IR-18).
        loop {
            let name = format!("__t_{purpose}_{}", self.hidden_counter);
            self.hidden_counter += 1;
            let ctx = self
                .fn_stack
                .last()
                .expect("a function context is always active");
            if ctx.lookup(&name).is_none() {
                return self
                    .fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .declare(&name);
            }
        }
    }

    fn current_loop_mut(&mut self) -> Option<&mut LoopCtx> {
        self.fn_stack
            .last_mut()
            .expect("a function context is always active")
            .loops
            .last_mut()
    }

    /// Builds an expression that must yield a usable value at this point.
    ///
    /// Canon propagates recoverable failures automatically, so an unconsumed `Failure`
    /// ends the enclosing path. `or_else` consumes the failure it inspects, and its
    /// fallback propagates its own failure, so one check after the whole expression is
    /// still correct and catches a fallback that failed too.
    fn build_value(&mut self, expr: &HirExpr, out: &mut Vec<CoreInst>) {
        self.build_expr(expr, out);
        out.push(CoreInst::PropagateFailure(expr.span()));
    }

    /// Builds a value used as a condition, target or count.
    ///
    /// Canon propagates a recoverable `Failure` instead of letting a type test turn it
    /// into an uncatchable fault, so every control-flow condition goes through the
    /// statement-boundary check first (auditoria IR-9).
    fn build_condition(&mut self, expr: &HirExpr, out: &mut Vec<CoreInst>) {
        self.build_expr(expr, out);
        out.push(CoreInst::PropagateFailure(expr.span()));
    }

    fn build_stmt(&mut self, stmt: &HirStmt, out: &mut Vec<CoreInst>) {
        match stmt {
            HirStmt::FnDecl(f) => {
                // Canon: a local function is created when execution reaches its declaration,
                // captures enclosing lexical bindings automatically, and keeps its own name
                // visible inside its body for recursion. Captures snapshot values, so the
                // self handle cannot be captured before the closure exists: the body gets a
                // reserved `Unset` placeholder upvalue named after the function itself that
                // `FillSelfCapture` rewrites with the newly created closure.
                let self_capture = f.name.clone();
                let fn_expr = HirFunctionExpr {
                    is_async: f.is_async,
                    params: f.params.to_vec(),
                    return_type: f.return_type.clone(),
                    body: f.body.clone(),
                    span: f.span,
                };
                let captures = self.enclosing_captures(&fn_expr);
                let is_self_recursive = self.references_name(&fn_expr, &self_capture);
                if is_self_recursive {
                    let captures = if captures.iter().any(|c| c == &self_capture) {
                        captures
                    } else {
                        let mut with_self = vec![self_capture.clone()];
                        with_self.extend(captures);
                        with_self
                    };
                    let name = self.build_closure(&fn_expr, &captures, Some(&self_capture));
                    out.push(CoreInst::MakeClosure {
                        name,
                        upvalues: captures,
                        self_capture: Some(self_capture.clone()),
                        span: f.span,
                    });
                    out.push(CoreInst::FillSelfCapture {
                        name: self_capture,
                        span: f.span,
                    });
                } else {
                    let name = self.build_closure(&fn_expr, &captures, None);
                    out.push(CoreInst::MakeClosure {
                        name,
                        upvalues: captures,
                        self_capture: None,
                        span: f.span,
                    });
                }
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .declare_binding(&f.name);
                out.push(CoreInst::Store(f.name.clone(), f.span));
            }
            HirStmt::Let(name, expr, span) | HirStmt::Var(name, expr, span) => {
                self.build_value(expr, out);
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .declare_binding(name);
                out.push(CoreInst::Store(name.clone(), *span));
            }
            HirStmt::Assign(target, value, span) => match target {
                HirExpr::Identifier(name, id_span) => {
                    self.build_value(value, out);
                    out.push(CoreInst::Store(name.clone(), *id_span));
                }
                HirExpr::Dot(base, field, dot_span) => {
                    // `SetField` pops the new value and then the receiver, so the receiver
                    // must be evaluated onto the stack first.
                    self.build_expr(base, out);
                    self.build_value(value, out);
                    out.push(CoreInst::SetField(field.clone(), *dot_span));
                    self.mark_field_mutation(*dot_span, out);
                }
                HirExpr::QuestionDot(..) => {
                    // `a?.b = v` is not a canonical assignment target: sema rejects it in
                    // `check_assignment_target`, and `CompoundAssign` has no arm for it.
                    // Emitting `Fail` keeps the three layers consistent instead of giving
                    // `=` a different safe-navigation story than `+=` (auditoria IR-15).
                    self.build_value(value, out);
                    out.push(CoreInst::Fail(*span));
                }
                HirExpr::Index(base, idx, idx_span) => {
                    self.build_expr(base, out);
                    self.build_expr(idx, out);
                    self.build_value(value, out);
                    out.push(CoreInst::SetIndex(*idx_span));
                }
                _ => {
                    // Invalid targets are rejected statically by
                    // `sema::check_assignment_target`; reaching IR means a broken
                    // frontend invariant. Fail loudly instead of silently dropping
                    // the assignment (auditoria IR-2).
                    self.build_value(value, out);
                    out.push(CoreInst::Fail(*span));
                }
            },
            HirStmt::CompoundAssign(op, target, value, span) => match target {
                HirExpr::Identifier(name, id_span) => {
                    self.build_expr(target, out);
                    self.build_value(value, out);
                    out.push(CoreInst::Binary(*op, *span));
                    out.push(CoreInst::PropagateFailure(*span));
                    out.push(CoreInst::Store(name.clone(), *id_span));
                }
                HirExpr::Dot(base, field, dot_span) => {
                    self.build_expr(base, out);
                    out.push(CoreInst::Dup(*dot_span));
                    out.push(CoreInst::GetField(field.clone(), *dot_span));
                    self.build_value(value, out);
                    out.push(CoreInst::Binary(*op, *span));
                    out.push(CoreInst::PropagateFailure(*span));
                    out.push(CoreInst::SetField(field.clone(), *dot_span));
                    self.mark_field_mutation(*dot_span, out);
                }
                HirExpr::Index(base, idx, idx_span) => {
                    // Save base and index into hidden locals to avoid double evaluation.
                    // Without this, expressions with side effects (e.g. `list[f()] += 1`)
                    // would execute the side effect twice.
                    let base_slot = self.hidden_local("cai_base");
                    let idx_slot = self.hidden_local("cai_idx");
                    let base_name = self.hidden_name(base_slot);
                    let idx_name = self.hidden_name(idx_slot);

                    self.build_expr(base, out);
                    out.push(CoreInst::Store(base_name.clone(), *idx_span));
                    self.build_expr(idx, out);
                    out.push(CoreInst::Store(idx_name.clone(), *idx_span));

                    // Stack for SetIndex: [base, idx, new_value]
                    out.push(CoreInst::Load(base_name.clone(), *idx_span));
                    out.push(CoreInst::Load(idx_name.clone(), *idx_span));

                    // Stack for GetIndex to compute old value: [base, idx, base, idx]
                    out.push(CoreInst::Load(base_name, *idx_span));
                    out.push(CoreInst::Load(idx_name, *idx_span));
                    out.push(CoreInst::GetIndex(*idx_span));
                    self.build_value(value, out);
                    out.push(CoreInst::Binary(*op, *span));
                    out.push(CoreInst::PropagateFailure(*span));
                    out.push(CoreInst::SetIndex(*idx_span));
                }
                _ => {
                    // Same invariant as `Assign` above (auditoria IR-2).
                    self.build_value(value, out);
                    out.push(CoreInst::Fail(*span));
                }
            },
            HirStmt::If(s) => {
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .push_scope();
                let mut next_test_jumps: Vec<usize> = Vec::new();
                let mut end_jumps: Vec<usize> = Vec::new();

                self.build_condition(&s.condition, out);
                next_test_jumps.push(out.len());
                out.push(CoreInst::JumpIfFalse(0, s.span));
                self.build_block(&s.then_branch, out);
                end_jumps.push(out.len());
                out.push(CoreInst::Jump(0, s.span));

                for (cond, body) in &s.elif_branches {
                    let here = out.len();
                    out[next_test_jumps.remove(0)] = CoreInst::JumpIfFalse(here as isize, s.span);
                    self.build_condition(cond, out);
                    next_test_jumps.push(out.len());
                    out.push(CoreInst::JumpIfFalse(0, s.span));
                    self.build_block(body, out);
                    end_jumps.push(out.len());
                    out.push(CoreInst::Jump(0, s.span));
                }

                let else_target = out.len();
                if let Some(else_branch) = &s.else_branch {
                    for jump in next_test_jumps {
                        out[jump] = CoreInst::JumpIfFalse(else_target as isize, s.span);
                    }
                    self.build_block(else_branch, out);
                } else {
                    let after_else = else_target as isize;
                    for jump in next_test_jumps {
                        out[jump] = CoreInst::JumpIfFalse(after_else, s.span);
                    }
                }

                let end = out.len() as isize;
                for jump in end_jumps {
                    out[jump] = CoreInst::Jump(end, s.span);
                }

                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .pop_scope();
            }
            HirStmt::Match(s) => {
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .push_scope();
                let target_slot = self.hidden_local("match");
                let target_name = self.hidden_name(target_slot);
                self.build_condition(&s.target, out);
                out.push(CoreInst::Store(target_name.clone(), s.span));

                let mut end_jumps: Vec<usize> = Vec::new();
                let mut fail_jumps: Vec<usize> = Vec::new();
                let mut arm_starts: Vec<usize> = Vec::new();

                for arm in &s.when_arms {
                    // Remember where this arm's tests begin so a previous arm that matched
                    // nothing can fall through to them instead of skipping straight to `else`.
                    arm_starts.push(out.len());
                    let mut test_jumps: Vec<Option<usize>> = Vec::new();
                    let mut matched_jumps: Vec<usize> = Vec::new();
                    let mut test_starts: Vec<usize> = Vec::new();

                    // A destructuring pattern is a shape match: it cannot fail, so it emits no
                    // test. Its bindings are established once the arm is selected, together
                    // with the other destructuring names of the same arm.
                    let mut bound_fields: Vec<(String, String)> = Vec::new();
                    for pattern in &arm.patterns {
                        test_starts.push(out.len());
                        match pattern {
                            HirMatchPattern::Value(pattern) => {
                                out.push(CoreInst::Load(target_name.clone(), s.span));
                                self.build_expr(pattern, out);
                                out.push(CoreInst::Binary(BinaryOp::Equal, s.span));
                                let jump = out.len();
                                out.push(CoreInst::JumpIfFalse(0, s.span));
                                test_jumps.push(Some(jump));
                                matched_jumps.push(out.len());
                                out.push(CoreInst::Jump(0, s.span));
                            }
                            HirMatchPattern::Destructure(fields) => {
                                for f in fields {
                                    bound_fields.push((f.clone(), f.clone()));
                                }
                                test_jumps.push(None);
                                // It cannot fail, so it jumps straight to the body where
                                // its bindings are established. Without this the arm is
                                // unreachable and the `else` branch always runs.
                                matched_jumps.push(out.len());
                                out.push(CoreInst::Jump(0, s.span));
                            }
                            HirMatchPattern::Variant {
                                enum_name,
                                variant_name,
                                payload,
                                span,
                            } => {
                                let full_variant = if let Some(e) = enum_name {
                                    format!("{e}.{variant_name}")
                                } else {
                                    variant_name.clone()
                                };
                                out.push(CoreInst::Load(target_name.clone(), *span));
                                out.push(CoreInst::IsVariant(full_variant.clone(), *span));
                                let jump = out.len();
                                out.push(CoreInst::JumpIfFalse(0, *span));
                                test_jumps.push(Some(jump));
                                matched_jumps.push(out.len());
                                out.push(CoreInst::Jump(0, *span));

                                match payload {
                                    HirVariantPatternPayload::Unit => {}
                                    HirVariantPatternPayload::Tuple(ids) => {
                                        let field_names = self
                                            .enum_variant_fields
                                            .get(&full_variant)
                                            .cloned()
                                            .unwrap_or_default();
                                        for (idx, bound_name) in ids.iter().enumerate() {
                                            let field_key = field_names
                                                .get(idx)
                                                .cloned()
                                                .unwrap_or_else(|| idx.to_string());
                                            bound_fields.push((bound_name.clone(), field_key));
                                        }
                                    }
                                    HirVariantPatternPayload::Struct(ids) => {
                                        for bound_name in ids {
                                            bound_fields
                                                .push((bound_name.clone(), bound_name.clone()));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // No pattern matched: continue with the next arm.
                    let fail = out.len();
                    out.push(CoreInst::Jump(0, s.span));
                    fail_jumps.push(fail);

                    for (idx, jump) in test_jumps.iter().enumerate() {
                        let Some(jump) = jump else { continue };
                        let next = test_starts.get(idx + 1).copied().unwrap_or(fail);
                        out[*jump] = CoreInst::JumpIfFalse(next as isize, s.span);
                    }

                    let body_start = out.len();
                    for jump in matched_jumps {
                        out[jump] = CoreInst::Jump(body_start as isize, s.span);
                    }

                    // The selected arm binds its destructured fields before the guard runs, so
                    // the guard can test them. `GetField` on a missing field is the same
                    // runtime fault as `value.field`, which is what makes this a shape match.
                    for (local_name, field_name) in &bound_fields {
                        self.fn_stack
                            .last_mut()
                            .expect("a function context is always active")
                            .declare_binding(local_name);
                        out.push(CoreInst::Load(target_name.clone(), s.span));
                        out.push(CoreInst::GetField(field_name.clone(), s.span));
                        out.push(CoreInst::Store(local_name.clone(), s.span));
                    }

                    // A `false` guard rejects the arm and continues with the next one.
                    if let Some(guard) = &arm.guard {
                        self.build_condition(guard, out);
                        let guard_jump = out.len();
                        out.push(CoreInst::JumpIfFalse(fail as isize, s.span));
                        let _ = guard_jump;
                    }

                    self.build_block(&arm.body, out);
                    end_jumps.push(out.len());
                    out.push(CoreInst::Jump(0, s.span));
                }

                let else_target = out.len();
                // An arm that matched nothing continues with the *next* arm; only the last
                // arm falls through to the `else` body (or past the statement).
                for (idx, jump) in fail_jumps.into_iter().enumerate() {
                    let target = arm_starts.get(idx + 1).copied().unwrap_or(else_target);
                    out[jump] = CoreInst::Jump(target as isize, s.span);
                }
                if let Some(else_arm) = &s.else_arm {
                    self.build_block(else_arm, out);
                }

                let end = out.len() as isize;
                for jump in end_jumps {
                    out[jump] = CoreInst::Jump(end, s.span);
                }

                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .pop_scope();
            }
            HirStmt::Loop(body, span) => {
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .push_scope();
                let loop_start = out.len();
                self.push_loop_ctx(Some(loop_start));
                self.build_block(body, out);
                out.push(CoreInst::Jump(loop_start as isize, *span));
                let end = out.len();
                self.finish_loop_ctx(out, end);
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .pop_scope();
            }
            HirStmt::While(cond, body, span) => {
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .push_scope();
                let loop_start = out.len();
                self.build_condition(cond, out);
                let exit_jump = out.len();
                out.push(CoreInst::JumpIfFalse(0, *span));
                self.push_loop_ctx(Some(loop_start));
                self.build_block(body, out);
                out.push(CoreInst::Jump(loop_start as isize, *span));
                let end = out.len() as isize;
                out[exit_jump] = CoreInst::JumpIfFalse(end, *span);
                let end_idx = out.len();
                self.finish_loop_ctx(out, end_idx);
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .pop_scope();
            }
            HirStmt::Repeat(count, index_name, body, span) => {
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .push_scope();
                let count_slot = self.hidden_local("repeat_count");
                let index_slot = self.hidden_local("repeat_index");
                self.build_condition(count, out);
                out.push(CoreInst::Store(self.hidden_name(count_slot), *span));
                out.push(CoreInst::Constant(CoreConstant::Int(0), *span));
                out.push(CoreInst::Store(self.hidden_name(index_slot), *span));

                let loop_start = out.len();
                out.push(CoreInst::Load(self.hidden_name(index_slot), *span));
                out.push(CoreInst::Load(self.hidden_name(count_slot), *span));
                out.push(CoreInst::Binary(BinaryOp::Less, *span));
                let exit_jump = out.len();
                out.push(CoreInst::JumpIfFalse(0, *span));

                if let Some(name) = index_name {
                    self.fn_stack
                        .last_mut()
                        .expect("a function context is always active")
                        .declare(name);
                    out.push(CoreInst::Load(self.hidden_name(index_slot), *span));
                    out.push(CoreInst::Store(name.clone(), *span));
                }

                self.push_loop_ctx(None);
                self.build_block(body, out);
                // `continue` lands on the increment step.
                let increment_start = out.len();
                out.push(CoreInst::Load(self.hidden_name(index_slot), *span));
                out.push(CoreInst::Constant(CoreConstant::Int(1), *span));
                out.push(CoreInst::Binary(BinaryOp::Add, *span));
                out.push(CoreInst::Store(self.hidden_name(index_slot), *span));
                out.push(CoreInst::Jump(loop_start as isize, *span));

                let end = out.len() as isize;
                out[exit_jump] = CoreInst::JumpIfFalse(end, *span);
                let end_idx = out.len();
                self.patch_continue_jumps(out, increment_start);
                self.finish_loop_ctx(out, end_idx);
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .pop_scope();
            }
            HirStmt::Each(names, iterable, body, span) => {
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .push_scope();
                let coll_slot = self.hidden_local("iter_coll");
                let index_slot = self.hidden_local("iter_index");
                let name_ref = self.hidden_name(coll_slot);

                self.build_condition(iterable, out);
                out.push(CoreInst::Store(name_ref.clone(), *span));
                out.push(CoreInst::Load(name_ref.clone(), *span));
                out.push(CoreInst::IterGuard(*span));
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .iter_depth += 1;
                out.push(CoreInst::Constant(CoreConstant::Int(0), *span));
                out.push(CoreInst::Store(self.hidden_name(index_slot), *span));

                let loop_start = out.len();
                out.push(CoreInst::Load(self.hidden_name(index_slot), *span));
                out.push(CoreInst::Load(name_ref.clone(), *span));
                out.push(CoreInst::Len(*span));
                out.push(CoreInst::Binary(BinaryOp::Less, *span));
                let exit_jump = out.len();
                out.push(CoreInst::JumpIfFalse(0, *span));

                let index_name = self.hidden_name(index_slot);
                match names.len() {
                    0 => {
                        // No binding to produce: iterating for the body's effects must
                        // not read the collection (auditoria IR-18).
                    }
                    1 => {
                        let value = names[0].clone();
                        self.fn_stack
                            .last_mut()
                            .expect("a function context is always active")
                            .declare(&value);
                        // One name binds the natural element: the key of a Dict, the
                        // element of anything else (auditoria IR-10).
                        out.push(CoreInst::Load(name_ref.clone(), *span));
                        out.push(CoreInst::Load(index_name.clone(), *span));
                        out.push(CoreInst::IterAt(IterMode::Primary, *span));
                        out.push(CoreInst::Store(value, *span));
                    }
                    _ => {
                        let key_binding = names[0].clone();
                        let value_binding = names[1].clone();
                        for name in [&key_binding, &value_binding] {
                            self.fn_stack
                                .last_mut()
                                .expect("a function context is always active")
                                .declare(name);
                        }
                        // First name: the positional index for a List, the key for a
                        // Dict. Second name: read through the collection, so a Dict
                        // value stays live and a List element is the element.
                        out.push(CoreInst::Load(name_ref.clone(), *span));
                        out.push(CoreInst::Load(index_name.clone(), *span));
                        out.push(CoreInst::IterAt(IterMode::Key, *span));
                        out.push(CoreInst::Store(key_binding.clone(), *span));
                        out.push(CoreInst::Load(name_ref.clone(), *span));
                        out.push(CoreInst::Load(key_binding, *span));
                        out.push(CoreInst::GetIndex(*span));
                        out.push(CoreInst::Store(value_binding, *span));
                    }
                }

                self.push_loop_ctx(None);
                self.build_block(body, out);
                let increment_start = out.len();
                out.push(CoreInst::Load(index_name, *span));
                out.push(CoreInst::Constant(CoreConstant::Int(1), *span));
                out.push(CoreInst::Binary(BinaryOp::Add, *span));
                out.push(CoreInst::Store(self.hidden_name(index_slot), *span));
                out.push(CoreInst::Jump(loop_start as isize, *span));

                let end = out.len() as isize;
                out[exit_jump] = CoreInst::JumpIfFalse(end, *span);
                let end_idx = out.len();
                self.patch_continue_jumps(out, increment_start);
                self.finish_loop_ctx(out, end_idx);
                out.push(CoreInst::IterGuardEnd(*span));
                let ctx = self
                    .fn_stack
                    .last_mut()
                    .expect("a function context is always active");
                ctx.iter_depth = ctx.iter_depth.saturating_sub(1);
                ctx.pop_scope();
            }
            HirStmt::Break(span) => {
                self.emit_loop_unwind(out, *span);
                let jump = out.len();
                out.push(CoreInst::Jump(0, *span));
                if let Some(ctx) = self.current_loop_mut() {
                    ctx.break_jumps.push(jump);
                }
            }
            HirStmt::Continue(span) => {
                self.emit_loop_unwind(out, *span);
                let jump = out.len();
                out.push(CoreInst::Jump(0, *span));
                if let Some(ctx) = self.current_loop_mut() {
                    if let Some(target) = ctx.continue_target {
                        out[jump] = CoreInst::Jump(target as isize, *span);
                    } else {
                        ctx.continue_jumps.push(jump);
                    }
                }
            }
            HirStmt::Return(expr, span) => {
                if let Some(e) = expr {
                    // `return fail(...)` is the canonical way to end a path in failure, and a
                    // returned `Failure` propagates to the caller's statement boundary.
                    self.build_value(e, out);
                    self.build_return_contract(*span, out);
                    // A returning operation is a stable mutable boundary, so its provisional
                    // mutations are verified before the frame ends.
                    self.build_mutation_boundary(out);
                    self.emit_frame_unwind(out, *span);
                    out.push(CoreInst::Return {
                        has_value: true,
                        span: *span,
                    });
                } else {
                    // A bare `return` yields `none`, and a non-nullable return contract
                    // rejects it: emit the value and the check so the violation surfaces
                    // as a contract fault instead of a silent `none` (auditoria IR-17).
                    if self
                        .fn_stack
                        .last()
                        .is_some_and(|ctx| ctx.return_contract.is_some())
                    {
                        out.push(CoreInst::Constant(CoreConstant::None, *span));
                        self.build_return_contract(*span, out);
                        // `AssertContract` inspects in place; the valueless `Return`
                        // supplies its own `none`, so drop the checked value.
                        out.push(CoreInst::Pop(*span));
                    }
                    self.build_mutation_boundary(out);
                    self.emit_frame_unwind(out, *span);
                    out.push(CoreInst::Return {
                        has_value: false,
                        span: *span,
                    });
                }
            }
            HirStmt::Fail(expr, span) => {
                self.build_expr(expr, out);
                out.push(CoreInst::Fail(*span));
            }
            HirStmt::Attempt(s) => {
                let push_handler_idx = out.len();
                out.push(CoreInst::PushHandler(0, s.span));
                self.fn_stack
                    .last_mut()
                    .expect("a function context is always active")
                    .handler_depth += 1;

                for st in &s.body {
                    self.build_stmt(st, out);
                }

                out.push(CoreInst::PopHandler(s.span));
                // The handler is closed here on the success path and popped by
                // `handle_failure` on the failure path, so the handler body runs with
                // the depth it had before the `attempt`.
                let ctx = self
                    .fn_stack
                    .last_mut()
                    .expect("a function context is always active");
                ctx.handler_depth = ctx.handler_depth.saturating_sub(1);
                let jump_end_idx = out.len();
                out.push(CoreInst::Jump(0, s.span));

                let handler_target = out.len() as isize;
                out[push_handler_idx] = CoreInst::PushHandler(handler_target, s.span);

                if let Some(err_name) = &s.error_binding {
                    self.fn_stack
                        .last_mut()
                        .expect("a function context is always active")
                        .declare(err_name);
                    out.push(CoreInst::Store(err_name.clone(), s.span));
                } else {
                    out.push(CoreInst::Pop(s.span));
                }

                for st in &s.handler {
                    self.build_stmt(st, out);
                }

                let end_target = out.len() as isize;
                out[jump_end_idx] = CoreInst::Jump(end_target, s.span);
            }
            HirStmt::AwaitDo(body, span) => {
                for stmt in &Self::desugar_await_do(body, &self.async_fns) {
                    self.build_stmt(stmt, out);
                }
                let _ = span;
            }
            HirStmt::Expr(expr) => {
                self.build_value(expr, out);
                out.push(CoreInst::Pop(expr.span()));
            }
        }
    }

    /// Desugars `await do ... end` into explicit awaits (canon: sugar for
    /// sequential awaits, never a second async machine). Statements whose whole
    /// value is a known `Task` are awaited; `if`/`each`/`match` bodies recurse;
    /// function, lambda and trailing-callback bodies are never entered.
    fn desugar_await_do(body: &[HirStmt], async_fns: &HashSet<String>) -> Vec<HirStmt> {
        fn is_task_producing(expr: &HirExpr, async_fns: &HashSet<String>) -> bool {
            match expr {
                HirExpr::Call(callee, _, _) => match &**callee {
                    HirExpr::Identifier(name, _) => async_fns.contains(name),
                    HirExpr::Dot(target, member, _) => {
                        if let HirExpr::Identifier(name, _) = &**target {
                            if name == "task" {
                                return matches!(member.as_str(), "spawn" | "all" | "race");
                            }
                            // Async methods are registered as `Type.method`; a receiver
                            // written as the type name is the resolvable form
                            // (auditoria IR-13).
                            return async_fns.contains(&format!("{name}.{member}"));
                        }
                        false
                    }
                    _ => false,
                },
                _ => false,
            }
        }
        fn await_expr(expr: HirExpr) -> HirExpr {
            let span = expr.span();
            HirExpr::Await(Box::new(expr), span)
        }
        fn desugar_block(body: &[HirStmt], async_fns: &HashSet<String>) -> Vec<HirStmt> {
            body.iter()
                .map(|stmt| desugar_stmt(stmt, async_fns))
                .collect()
        }
        fn desugar_stmt(stmt: &HirStmt, async_fns: &HashSet<String>) -> HirStmt {
            match stmt {
                HirStmt::Let(name, expr, span) if is_task_producing(expr, async_fns) => {
                    HirStmt::Let(name.clone(), await_expr(expr.clone()), *span)
                }
                HirStmt::Var(name, expr, span) if is_task_producing(expr, async_fns) => {
                    HirStmt::Var(name.clone(), await_expr(expr.clone()), *span)
                }
                HirStmt::Expr(expr) if is_task_producing(expr, async_fns) => {
                    HirStmt::Expr(await_expr(expr.clone()))
                }
                HirStmt::If(s) => HirStmt::If(HirIfStmt {
                    condition: s.condition.clone(),
                    then_branch: desugar_block(&s.then_branch, async_fns),
                    elif_branches: s
                        .elif_branches
                        .iter()
                        .map(|(cond, body)| (cond.clone(), desugar_block(body, async_fns)))
                        .collect(),
                    else_branch: s
                        .else_branch
                        .as_ref()
                        .map(|body| desugar_block(body, async_fns)),
                    span: s.span,
                }),
                HirStmt::Match(s) => HirStmt::Match(HirMatchStmt {
                    target: s.target.clone(),
                    when_arms: s
                        .when_arms
                        .iter()
                        .map(|arm| HirMatchArm {
                            patterns: arm.patterns.clone(),
                            guard: arm.guard.clone(),
                            body: desugar_block(&arm.body, async_fns),
                            span: arm.span,
                        })
                        .collect(),
                    else_arm: s
                        .else_arm
                        .as_ref()
                        .map(|body| desugar_block(body, async_fns)),
                    span: s.span,
                }),
                HirStmt::Each(vars, iter, body, span) => HirStmt::Each(
                    vars.clone(),
                    iter.clone(),
                    desugar_block(body, async_fns),
                    *span,
                ),
                // Every block-bearing form recurses; a task produced as a whole
                // statement value is awaited (auditoria IR-13).
                HirStmt::While(cond, body, span) => {
                    HirStmt::While(cond.clone(), desugar_block(body, async_fns), *span)
                }
                HirStmt::Loop(body, span) => HirStmt::Loop(desugar_block(body, async_fns), *span),
                HirStmt::Repeat(count, index, body, span) => HirStmt::Repeat(
                    count.clone(),
                    index.clone(),
                    desugar_block(body, async_fns),
                    *span,
                ),
                HirStmt::Attempt(s) => HirStmt::Attempt(HirAttemptStmt {
                    body: desugar_block(&s.body, async_fns),
                    error_binding: s.error_binding.clone(),
                    handler: desugar_block(&s.handler, async_fns),
                    span: s.span,
                }),
                HirStmt::Assign(target, value, span) if is_task_producing(value, async_fns) => {
                    HirStmt::Assign(target.clone(), await_expr(value.clone()), *span)
                }
                HirStmt::CompoundAssign(op, target, value, span)
                    if is_task_producing(value, async_fns) =>
                {
                    HirStmt::CompoundAssign(*op, target.clone(), await_expr(value.clone()), *span)
                }
                HirStmt::Return(Some(expr), span) if is_task_producing(expr, async_fns) => {
                    HirStmt::Return(Some(await_expr(expr.clone())), *span)
                }
                HirStmt::Fail(expr, span) if is_task_producing(expr, async_fns) => {
                    HirStmt::Fail(await_expr(expr.clone()), *span)
                }
                other => other.clone(),
            }
        }
        desugar_block(body, async_fns)
    }

    fn build_block(&mut self, stmts: &[HirStmt], out: &mut Vec<CoreInst>) {
        self.fn_stack
            .last_mut()
            .expect("a function context is always active")
            .push_scope();
        for stmt in stmts {
            self.build_stmt(stmt, out);
        }
        self.fn_stack
            .last_mut()
            .expect("a function context is always active")
            .pop_scope();
    }

    fn hidden_name(&self, slot: usize) -> String {
        let ctx = self.fn_stack.last().expect("function context");
        if slot < ctx.params.len() {
            return ctx.params[slot].clone();
        }
        ctx.locals
            .get(slot - ctx.params.len())
            .cloned()
            .unwrap_or_else(|| format!("__t_slot_{slot}"))
    }

    fn push_loop_ctx(&mut self, continue_target: Option<usize>) {
        let ctx = self
            .fn_stack
            .last_mut()
            .expect("a function context is always active");
        let handler_depth_at_entry = ctx.handler_depth;
        let iter_depth_at_entry = ctx.iter_depth;
        ctx.loops.push(LoopCtx {
            continue_target,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            handler_depth_at_entry,
            iter_depth_at_entry,
        });
    }

    /// Pops failure handlers and iteration guards opened inside the current loop body.
    ///
    /// A `break`/`continue` leaving the body must undo them: jumping out of an `attempt`
    /// or an `each` without their lexical `PopHandler`/`IterGuardEnd` would leave stale
    /// entries behind (auditoria IR-8/IR-12).
    fn emit_loop_unwind(&mut self, out: &mut Vec<CoreInst>, span: SourceSpan) {
        let Some(ctx) = self.fn_stack.last() else {
            return;
        };
        let Some(loop_ctx) = ctx.loops.last() else {
            return;
        };
        let handlers = ctx
            .handler_depth
            .saturating_sub(loop_ctx.handler_depth_at_entry);
        let guards = ctx.iter_depth.saturating_sub(loop_ctx.iter_depth_at_entry);
        for _ in 0..handlers {
            out.push(CoreInst::PopHandler(span));
        }
        for _ in 0..guards {
            out.push(CoreInst::IterGuardEnd(span));
        }
    }

    /// Pops every failure handler and iteration guard the frame opened.
    ///
    /// `return` ends the frame without running the lexical cleanup of each `attempt` and
    /// `each`, and the VM keeps handlers and guards machine-wide, so the frame must undo
    /// them explicitly (auditoria IR-8/IR-12).
    fn emit_frame_unwind(&mut self, out: &mut Vec<CoreInst>, span: SourceSpan) {
        let Some(ctx) = self.fn_stack.last() else {
            return;
        };
        for _ in 0..ctx.handler_depth {
            out.push(CoreInst::PopHandler(span));
        }
        for _ in 0..ctx.iter_depth {
            out.push(CoreInst::IterGuardEnd(span));
        }
    }

    fn patch_continue_jumps(&mut self, out: &mut [CoreInst], target: usize) {
        if let Some(ctx) = self.current_loop_mut() {
            let jumps = std::mem::take(&mut ctx.continue_jumps);
            for jump in jumps {
                let span = match &out[jump] {
                    CoreInst::Jump(_, span) | CoreInst::JumpIfFalse(_, span) => *span,
                    _ => continue,
                };
                out[jump] = CoreInst::Jump(target as isize, span);
            }
        }
    }

    fn finish_loop_ctx(&mut self, out: &mut [CoreInst], end: usize) {
        let Some(ctx) = self
            .fn_stack
            .last_mut()
            .expect("a function context is always active")
            .loops
            .pop()
        else {
            return;
        };
        for jump in ctx.break_jumps {
            let span = match &out[jump] {
                CoreInst::Jump(_, span) => *span,
                _ => continue,
            };
            out[jump] = CoreInst::Jump(end as isize, span);
        }
        for jump in ctx.continue_jumps {
            let span = match &out[jump] {
                CoreInst::Jump(_, span) => *span,
                _ => continue,
            };
            out[jump] = CoreInst::Jump(end as isize, span);
        }
    }

    /// Lowers `base?.member` and its call form with existing instructions.
    ///
    /// The receiver is stored once; the expression is `none` when it holds `none`, and the
    /// access (plus any arguments) only runs otherwise. A `Failure` receiver propagates
    /// instead of activating the safe path.
    fn build_question_dot(
        &mut self,
        base: &HirExpr,
        member: &str,
        span: SourceSpan,
        call: Option<(&[HirCallArg], SourceSpan)>,
        out: &mut Vec<CoreInst>,
    ) {
        let slot = self.hidden_local("safe_base");
        let name = self.hidden_name(slot);

        self.build_expr(base, out);
        out.push(CoreInst::Store(name.clone(), span));
        out.push(CoreInst::Load(name.clone(), span));
        out.push(CoreInst::Constant(CoreConstant::None, span));
        out.push(CoreInst::Binary(BinaryOp::Equal, span));
        out.push(CoreInst::PropagateFailure(span));
        let jump_false = out.len();
        out.push(CoreInst::JumpIfFalse(0, span));

        // Receiver is `none`: the whole expression is `none`.
        out.push(CoreInst::Constant(CoreConstant::None, span));
        let jump_end = out.len();
        out.push(CoreInst::Jump(0, span));

        let access_target = out.len() as isize;
        out[jump_false] = CoreInst::JumpIfFalse(access_target, span);
        out.push(CoreInst::Load(name, span));
        out.push(CoreInst::GetField(member.to_string(), span));
        if let Some((args, call_span)) = call {
            for arg in args {
                self.build_expr(&arg.value, out);
            }
            out.push(CoreInst::Call {
                arg_count: args.len(),
                span: call_span,
            });
        }
        let end_target = out.len() as isize;
        out[jump_end] = CoreInst::Jump(end_target, span);
    }

    fn build_expr(&mut self, expr: &HirExpr, out: &mut Vec<CoreInst>) {
        match expr {
            HirExpr::Literal(lit, span) => {
                let const_val = match lit {
                    Literal::None => CoreConstant::None,
                    Literal::Bool(b) => CoreConstant::Bool(*b),
                    Literal::Int(raw) => {
                        // Literal text is classified once, in `aipo-lexer`: the builder parses
                        // exactly what the lexer accepted, so what lexes and what executes
                        // cannot drift apart.
                        let val = parse_int_literal(raw).unwrap_or(OUT_OF_RANGE_INT);
                        CoreConstant::Int(val)
                    }
                    Literal::Float(raw) => {
                        let val = parse_float_literal(raw).unwrap_or(MALFORMED_FLOAT);
                        CoreConstant::Float(val)
                    }
                    Literal::String(s, _) => CoreConstant::String(s.clone()),
                };
                out.push(CoreInst::Constant(const_val, *span));
            }
            HirExpr::Identifier(name, span) => {
                out.push(CoreInst::Load(name.clone(), *span));
            }
            HirExpr::Unary(op, inner, span) => {
                self.build_expr(inner, out);
                out.push(CoreInst::Unary(*op, *span));
            }
            HirExpr::Binary(op, left, right, span) => match op {
                BinaryOp::And => {
                    self.build_expr(left, out);
                    out.push(CoreInst::Dup(*span));
                    let jump_false = out.len();
                    out.push(CoreInst::JumpIfFalse(0, *span));
                    out.push(CoreInst::Pop(*span));
                    self.build_expr(right, out);
                    let end = out.len() as isize;
                    out[jump_false] = CoreInst::JumpIfFalse(end, *span);
                }
                BinaryOp::Or => {
                    self.build_expr(left, out);
                    out.push(CoreInst::Dup(*span));
                    let jump_false = out.len();
                    out.push(CoreInst::JumpIfFalse(0, *span));
                    let jump_end = out.len();
                    out.push(CoreInst::Jump(0, *span));
                    let next = out.len() as isize;
                    out[jump_false] = CoreInst::JumpIfFalse(next, *span);
                    out.push(CoreInst::Pop(*span));
                    self.build_expr(right, out);
                    let end = out.len() as isize;
                    out[jump_end] = CoreInst::Jump(end, *span);
                }
                BinaryOp::Range => {
                    self.build_expr(left, out);
                    self.build_expr(right, out);
                    out.push(CoreInst::Range(*span));
                }
                BinaryOp::Is => {
                    self.build_expr(left, out);
                    self.build_expr(right, out);
                    out.push(CoreInst::TypeIs(*span));
                }
                BinaryOp::IsNullable => {
                    self.build_expr(left, out);
                    self.build_expr(right, out);
                    out.push(CoreInst::TypeIsNullable(*span));
                }
                BinaryOp::Pipeline => {
                    // `a |> f` is `f(a)`, and source order evaluates the argument before
                    // the callee. The value is parked in a hidden local so the callee can
                    // be built first on the stack without reordering effects
                    // (auditoria IR-16).
                    let slot = self.hidden_local("pip_arg");
                    let name = self.hidden_name(slot);
                    self.build_expr(left, out);
                    out.push(CoreInst::Store(name.clone(), *span));
                    self.build_expr(right, out);
                    out.push(CoreInst::Load(name, *span));
                    out.push(CoreInst::Call {
                        arg_count: 1,
                        span: *span,
                    });
                }
                _ => {
                    self.build_expr(left, out);
                    self.build_expr(right, out);
                    out.push(CoreInst::Binary(*op, *span));
                }
            },
            HirExpr::Call(callee, args, span) => {
                if let HirExpr::QuestionDot(base, member, member_span) = &**callee {
                    // `service?.load(args)`: the receiver is evaluated once; if it is
                    // `none` neither the access nor the arguments run (auditoria IR-5).
                    self.build_question_dot(base, member, *member_span, Some((args, *span)), out);
                    return;
                }
                if let HirExpr::Dot(base, member, dot_span) = &**callee {
                    if let HirExpr::Identifier(namespace, _) = &**base {
                        let qualified = format!("{namespace}.{member}");
                        if self.functions.iter().any(|f| f.name == qualified) {
                            out.push(CoreInst::Load(qualified, *dot_span));
                            for arg in args {
                                self.build_expr(&arg.value, out);
                            }
                            out.push(CoreInst::Call {
                                arg_count: args.len(),
                                span: *span,
                            });
                            return;
                        }
                    }
                }
                // Canonical evaluation order: callee first, then arguments left to right, once.
                self.build_expr(callee, out);
                match self.resolve_arguments(callee, args) {
                    Some(resolved) => {
                        for argument in &resolved {
                            match argument {
                                ResolvedArgument::Provided(value) => self.build_expr(value, out),
                                ResolvedArgument::Omitted(at) => {
                                    out.push(CoreInst::PushUnset(*at));
                                }
                            }
                        }
                        out.push(CoreInst::Call {
                            arg_count: resolved.len(),
                            span: *span,
                        });
                    }
                    None => {
                        for arg in args {
                            self.build_expr(&arg.value, out);
                        }
                        out.push(CoreInst::Call {
                            arg_count: args.len(),
                            span: *span,
                        });
                    }
                }
            }
            HirExpr::Dot(base, member, span) => {
                if let HirExpr::Identifier(name, _) = &**base {
                    if self
                        .enum_unit_variants
                        .get(name)
                        .is_some_and(|set| set.contains(member))
                    {
                        out.push(CoreInst::BuildStruct {
                            type_name: format!("{name}.{member}"),
                            field_count: 0,
                            defer_fixed: false,
                            span: *span,
                        });
                        if self.invariant_hooks.contains(name) {
                            self.build_invariant_check(name, *span, out);
                        }
                        return;
                    }
                }
                self.build_expr(base, out);
                out.push(CoreInst::GetField(member.clone(), *span));
            }
            HirExpr::QuestionDot(base, member, span) => {
                // Canon: if the receiver is `none`, the access does not happen and the
                // expression is `none` (Language Reference §143). Desugared with existing
                // instructions so both backends share the semantics (auditoria IR-5).
                self.build_question_dot(base, member, *span, None, out);
            }
            HirExpr::Index(base, idx, span) => {
                self.build_expr(base, out);
                self.build_expr(idx, out);
                out.push(CoreInst::GetIndex(*span));
            }
            HirExpr::List(items, span) => {
                for i in items {
                    self.build_expr(i, out);
                }
                out.push(CoreInst::BuildList(items.len(), *span));
            }
            HirExpr::Dict(pairs, span) => {
                for (k, v) in pairs {
                    self.build_expr(k, out);
                    self.build_expr(v, out);
                }
                out.push(CoreInst::BuildDict(pairs.len(), *span));
            }
            HirExpr::Construct(type_name, fields, span) => {
                // Instances carry their values in *declaration* order, which is the order the
                // runtime zips against the registered struct layout. A literal may name its
                // fields in any order, so reorder here instead of trusting the source order.
                // Every declared field keeps its position: the literal's value when written,
                // otherwise the field's declared default, otherwise `none`. Positions must
                // never shift, or a later field's value would land in an earlier slot.
                let ordered: Vec<HirExpr> = match self.struct_fields.get(type_name) {
                    Some(declared) => declared
                        .iter()
                        .map(|(name, _, default)| {
                            fields
                                .iter()
                                .find(|(written, _)| written.as_deref() == Some(name.as_str()))
                                .map(|(_, expr)| expr.clone())
                                .or_else(|| default.clone())
                                .unwrap_or(HirExpr::Literal(Literal::None, *span))
                        })
                        .collect(),
                    None => fields.iter().map(|(_, expr)| expr.clone()).collect(),
                };
                for expr in &ordered {
                    self.build_expr(expr, out);
                }
                // canon: `Type{...}` uses `init` when the type declares one. The instance is
                // built first (so `fixed` fields can still be assigned), then `init` runs with
                // it as `self`, then `SealStruct` publishes it and evaluates `invariant()`.
                let init_hook = self
                    .init_hooks
                    .get(type_name)
                    .map(|signature| DeclaredSignature {
                        params: signature.params.clone(),
                        required: signature.required,
                    });
                let has_init = init_hook.is_some();
                out.push(CoreInst::BuildStruct {
                    type_name: type_name.clone(),
                    field_count: ordered.len(),
                    defer_fixed: has_init,
                    span: *span,
                });
                if let Some(signature) = init_hook {
                    self.build_init_call(type_name, fields, &signature, *span, out);
                }
                // canon: `invariant()` is verified at the end of construction, so it observes
                // exactly what `init` assigned (the field assignments above have already run).
                let inv_target = if self.invariant_hooks.contains(type_name) {
                    Some(type_name.as_str())
                } else {
                    type_name.split_once('.').and_then(|(parent, _)| {
                        self.invariant_hooks.contains(parent).then_some(parent)
                    })
                };
                if let Some(target) = inv_target {
                    self.build_invariant_check(target, *span, out);
                }
                if has_init {
                    out.push(CoreInst::SealStruct(*span));
                }
            }
            HirExpr::If(cond, then_b, else_b, span) => {
                self.build_condition(cond, out);
                let jump_false_idx = out.len();
                out.push(CoreInst::JumpIfFalse(0, *span));

                self.build_expr(then_b, out);
                let jump_end_idx = out.len();
                out.push(CoreInst::Jump(0, *span));

                let else_offset = out.len() as isize;
                out[jump_false_idx] = CoreInst::JumpIfFalse(else_offset, *span);

                self.build_expr(else_b, out);
                let end_offset = out.len() as isize;
                out[jump_end_idx] = CoreInst::Jump(end_offset, *span);
            }
            HirExpr::OrElse(left, right, span) => {
                // Canon: `or_else` is lazy — the fallback runs only when the left side
                // ends in a `Failure`, and a runtime fault never activates it. The
                // handler is the same mechanism `attempt` uses: the left side's own
                // propagation check jumps here with the failure on the stack, which the
                // fallback drops before producing its value (auditoria IR-6).
                let push_handler_idx = out.len();
                out.push(CoreInst::PushHandler(0, *span));
                self.build_value(left, out);
                out.push(CoreInst::PopHandler(*span));
                let jump_end_idx = out.len();
                out.push(CoreInst::Jump(0, *span));

                let fallback_target = out.len() as isize;
                out[push_handler_idx] = CoreInst::PushHandler(fallback_target, *span);
                out.push(CoreInst::Pop(*span));
                // A failed fallback propagates normally; `build_value` emits that check,
                // so a double failure reaches the enclosing handler (auditoria IR-7).
                self.build_value(right, out);

                let end_target = out.len() as isize;
                out[jump_end_idx] = CoreInst::Jump(end_target, *span);
            }
            HirExpr::Await(inner, span) => {
                self.build_expr(inner, out);
                out.push(CoreInst::Await(*span));
            }
            HirExpr::Try(inner, span) => {
                // `expr?` evaluates the operand and then runs the same check the
                // VM already performs at every statement boundary: an unconsumed
                // `Failure` unwinds to the nearest handler or caller, while any
                // other value stays on the stack for the enclosing expression.
                self.build_expr(inner, out);
                out.push(CoreInst::PropagateFailure(*span));
            }
            HirExpr::With(base, updates, span) => {
                // Functional update: clone the base, then overwrite just the named fields.
                // `CloneStruct` is what makes this non-destructive — structs are shared
                // `Rc<RefCell<..>>`, so assigning through the base would mutate it.
                self.build_expr(base, out);
                out.push(CoreInst::CloneStruct(*span));
                for (name, value) in updates {
                    // `SetField` pops the value then the receiver, so the receiver is
                    // duplicated back onto the stack for the next override.
                    out.push(CoreInst::Dup(*span));
                    self.build_value(value, out);
                    out.push(CoreInst::SetField(name.clone(), *span));
                }
                self.mark_field_mutation(*span, out);
            }
            HirExpr::Fn(f) => {
                let captures = self.enclosing_captures(f);
                let name = self.build_closure(f, &captures, None);
                out.push(CoreInst::MakeClosure {
                    name,
                    upvalues: captures,
                    self_capture: None,
                    span: f.span,
                });
            }
        }
    }
}

fn collect_free_stmt(stmt: &HirStmt, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    match stmt {
        HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => {
            collect_free_expr(expr, seen, out);
            seen.insert(name.clone());
        }
        HirStmt::Assign(target, value, _) | HirStmt::CompoundAssign(_, target, value, _) => {
            collect_free_expr(target, seen, out);
            collect_free_expr(value, seen, out);
        }
        HirStmt::If(s) => {
            collect_free_expr(&s.condition, seen, out);
            for stmt in &s.then_branch {
                collect_free_stmt(stmt, seen, out);
            }
            for (cond, body) in &s.elif_branches {
                collect_free_expr(cond, seen, out);
                for stmt in body {
                    collect_free_stmt(stmt, seen, out);
                }
            }
            if let Some(else_branch) = &s.else_branch {
                for stmt in else_branch {
                    collect_free_stmt(stmt, seen, out);
                }
            }
        }
        HirStmt::Match(s) => {
            collect_free_expr(&s.target, seen, out);
            for arm in &s.when_arms {
                for pattern in &arm.patterns {
                    if let HirMatchPattern::Value(expr) = pattern {
                        collect_free_expr(expr, seen, out);
                    }
                }
                if let Some(guard) = &arm.guard {
                    collect_free_expr(guard, seen, out);
                }
                for stmt in &arm.body {
                    collect_free_stmt(stmt, seen, out);
                }
            }
            if let Some(else_arm) = &s.else_arm {
                for stmt in else_arm {
                    collect_free_stmt(stmt, seen, out);
                }
            }
        }
        HirStmt::Loop(body, _) => {
            for stmt in body {
                collect_free_stmt(stmt, seen, out);
            }
        }
        HirStmt::While(cond, body, _) => {
            collect_free_expr(cond, seen, out);
            for stmt in body {
                collect_free_stmt(stmt, seen, out);
            }
        }
        HirStmt::Repeat(count, index, body, _) => {
            collect_free_expr(count, seen, out);
            if let Some(index) = index {
                seen.insert(index.clone());
            }
            for stmt in body {
                collect_free_stmt(stmt, seen, out);
            }
        }
        HirStmt::Each(names, iterable, body, _) => {
            collect_free_expr(iterable, seen, out);
            for name in names {
                seen.insert(name.clone());
            }
            for stmt in body {
                collect_free_stmt(stmt, seen, out);
            }
        }
        HirStmt::Return(expr, _) => {
            if let Some(expr) = expr {
                collect_free_expr(expr, seen, out);
            }
        }
        HirStmt::Fail(expr, _) | HirStmt::Expr(expr) => collect_free_expr(expr, seen, out),
        HirStmt::AwaitDo(body, _) => {
            for stmt in body {
                collect_free_stmt(stmt, seen, out);
            }
        }
        HirStmt::FnDecl(f) => {
            seen.insert(f.name.clone());
            let mut inner_seen = HashSet::new();
            inner_seen.insert(f.name.clone());
            for p in &f.params {
                inner_seen.insert(p.name.clone());
            }
            let mut inner_free = Vec::new();
            for stmt in &f.body {
                collect_free_stmt(stmt, &mut inner_seen, &mut inner_free);
            }
            for name in inner_free {
                if !seen.contains(&name) {
                    seen.insert(name.clone());
                    out.push(name);
                }
            }
        }
        HirStmt::Attempt(s) => {
            for stmt in &s.body {
                collect_free_stmt(stmt, seen, out);
            }
            if let Some(binding) = &s.error_binding {
                seen.insert(binding.clone());
            }
            for stmt in &s.handler {
                collect_free_stmt(stmt, seen, out);
            }
        }
        HirStmt::Break(_) | HirStmt::Continue(_) => {}
    }
}

fn collect_free_expr(expr: &HirExpr, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    match expr {
        HirExpr::Identifier(name, _) => {
            if !seen.contains(name) {
                seen.insert(name.clone());
                out.push(name.clone());
            }
        }
        HirExpr::Unary(_, inner, _) => collect_free_expr(inner, seen, out),
        HirExpr::Binary(_, l, r, _) | HirExpr::OrElse(l, r, _) => {
            collect_free_expr(l, seen, out);
            collect_free_expr(r, seen, out);
        }
        HirExpr::Await(inner, _) | HirExpr::Try(inner, _) => collect_free_expr(inner, seen, out),
        HirExpr::With(base, updates, _) => {
            collect_free_expr(base, seen, out);
            for (_, value) in updates {
                collect_free_expr(value, seen, out);
            }
        }
        HirExpr::Call(callee, args, _) => {
            collect_free_expr(callee, seen, out);
            for arg in args {
                collect_free_expr(&arg.value, seen, out);
            }
        }
        HirExpr::Dot(base, _, _) | HirExpr::QuestionDot(base, _, _) => {
            collect_free_expr(base, seen, out);
        }
        HirExpr::Index(base, idx, _) => {
            collect_free_expr(base, seen, out);
            collect_free_expr(idx, seen, out);
        }
        HirExpr::List(items, _) => {
            for item in items {
                collect_free_expr(item, seen, out);
            }
        }
        HirExpr::Dict(pairs, _) => {
            for (k, v) in pairs {
                collect_free_expr(k, seen, out);
                collect_free_expr(v, seen, out);
            }
        }
        HirExpr::Construct(_, fields, _) => {
            for (_, value) in fields {
                collect_free_expr(value, seen, out);
            }
        }
        HirExpr::If(cond, then_b, else_b, _) => {
            collect_free_expr(cond, seen, out);
            collect_free_expr(then_b, seen, out);
            collect_free_expr(else_b, seen, out);
        }
        HirExpr::Fn(f) => {
            let mut inner_seen = HashSet::new();
            for p in &f.params {
                inner_seen.insert(p.name.clone());
            }
            let mut inner_free = Vec::new();
            for stmt in &f.body {
                collect_free_stmt(stmt, &mut inner_seen, &mut inner_free);
            }
            for name in inner_free {
                if !seen.contains(&name) {
                    seen.insert(name.clone());
                    out.push(name);
                }
            }
        }
        HirExpr::Literal(_, _) => {}
    }
}
