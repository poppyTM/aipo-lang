//! Semantic analysis and verification pass.

use crate::prelude::{HostFunction, PreludeSurface};
use crate::symbol::{MethodSignature, Mutability, ScopeTree, Symbol, SymbolKind};
use aipo_ast::{BinaryOp, Literal, TypeAnnotation, UnaryOp};
use aipo_diagnostics::{Diagnostic, DiagnosticCode};
use aipo_hir::*;
use aipo_source::{Source, SourceSpan};
use std::collections::{HashMap, HashSet};

/// Core categories a written contract can name.
///
/// The list mirrors what the runtime can decide about a value (`aipo-vm::TypeTag`); a contract
/// naming anything else is left to the runtime, because a provable category test is the only
/// thing a pre-execution report may act on.
const CORE_CONTRACT_CATEGORIES: [&str; 13] = [
    "Bool", "Int", "Float", "Byte", "String", "List", "Dict", "Bytes", "Range", "Set", "Duration",
    "Sequence", "Task",
];

const LIST_MEMBERS: &[&str] = &[
    "add",
    "insert",
    "remove",
    "remove_at",
    "remove_last",
    "clear",
    "contains",
    "find",
    "find_index",
    "count",
    "first",
    "first_or",
    "last",
    "last_or",
    "is_empty",
    "reverse",
    "sort",
    "len",
    "take",
    "skip",
    "distinct",
    "zip",
    "chain",
    "chunk",
    "window",
    "enumerate",
    "lazy",
    "map",
    "transform",
    "filter",
    "sort_by",
    "any",
    "all",
    "flat_map",
    "reduce",
    "each",
    "clone",
];

const STRING_MEMBERS: &[&str] = &[
    "len",
    "byte_len",
    "contains",
    "starts_with",
    "ends_with",
    "find",
    "lower",
    "upper",
    "capitalize",
    "reverse",
    "trim",
    "split",
    "join",
    "replace",
    "slice",
    "format",
    "graphemes",
    "words",
    "lines",
    "casefold",
    "encode",
    "encode_utf8",
    "chars",
    "to_lowercase",
    "to_uppercase",
    "is_empty",
];

fn extract_inferred_type(expr: &HirExpr) -> Option<String> {
    match expr {
        HirExpr::Construct(target, _, _) => Some(target.clone()),
        HirExpr::List(..) => Some("List".to_string()),
        HirExpr::Literal(Literal::String(..), _) => Some("String".to_string()),
        _ => None,
    }
}

/// Canonical category of a literal, when the category is provable from the literal itself.
fn literal_category(literal: &Literal) -> Option<&'static str> {
    match literal {
        Literal::Int(_) => Some("Int"),
        Literal::Float(_) => Some("Float"),
        Literal::Bool(_) => Some("Bool"),
        Literal::String(_, _) => Some("String"),
        // `none` belongs to no category: it only satisfies a nullable contract.
        Literal::None => None,
    }
}

/// Returns whether a literal expression provably cannot satisfy `annotation`.
///
/// Only a literal has a provable category, so a core-type contract naming a different category is
/// a mismatch before execution; a `struct`/interface contract, or any non-literal expression,
/// stays a runtime contract fault.
fn literal_violates_contract(expression: &HirExpr, annotation: &TypeAnnotation) -> bool {
    let HirExpr::Literal(literal, _) = expression else {
        return false;
    };
    match literal_category(literal) {
        // `none` satisfies only a contract that accepts it.
        None => !annotation.is_nullable && annotation.name != "none",
        // Only a core-type contract is provable from the literal itself: a `struct` or interface
        // name needs the runtime, which decides it at the boundary a fault.
        Some(category) => {
            CORE_CONTRACT_CATEGORIES.contains(&annotation.name.as_str())
                && annotation.name != category
        }
    }
}

/// Renders a contract the way it is written (`Int`, `Int?`).
fn contract_label(annotation: &TypeAnnotation) -> String {
    if annotation.is_nullable {
        format!("{}?", annotation.name)
    } else {
        annotation.name.clone()
    }
}

/// Renders a literal as its source text for a diagnostic message.
fn describe_literal(literal: &Literal) -> String {
    match literal {
        Literal::Int(raw) => raw.clone(),
        Literal::Float(raw) => raw.clone(),
        Literal::Bool(value) => value.to_string(),
        Literal::String(text, _) => format!("\"{text}\""),
        Literal::None => "none".to_string(),
    }
}

/// One declared parameter contract, aligned with its declaration position.
#[derive(Debug, Clone)]
struct ParameterContract {
    /// Parameter name, used to match a named argument.
    name: String,
    /// Optional written contract.
    annotation: Option<TypeAnnotation>,
}

/// Written signature contracts of a declared function.
#[derive(Debug, Clone, Default)]
struct DeclaredContract {
    /// Parameter contracts in declaration order.
    params: Vec<ParameterContract>,
}

impl DeclaredContract {
    /// Collects the parameter contracts of a declaration.
    ///
    /// A return contract is checked where the `return` is written (it belongs to the enclosing
    /// function body), so only the parameter layout is needed at the call site.
    fn of(function: &HirFunctionDecl) -> Self {
        Self {
            params: function
                .params
                .iter()
                .filter(|param| !param.is_self)
                .map(|param| ParameterContract {
                    name: param.name.clone(),
                    annotation: param.type_annotation.clone(),
                })
                .collect(),
        }
    }
}

/// Resulting semantic facts produced by the analyzer.
#[derive(Debug, Default)]
pub struct SemanticFacts {
    /// Scope hierarchy.
    pub scopes: ScopeTree,
}

/// Extracts the root identifier and span of an assignment target path.
fn path_root(expr: &HirExpr) -> Option<(&str, aipo_source::SourceSpan)> {
    match expr {
        HirExpr::Identifier(name, span) => Some((name.as_str(), *span)),
        HirExpr::Dot(base, _, _) | HirExpr::Index(base, _, _) => path_root(base),
        _ => None,
    }
}

/// Semantic analyzer walking the HIR and collecting diagnostics.
pub struct SemanticAnalyzer<'a> {
    source: &'a Source,
    facts: SemanticFacts,
    /// Free-function name -> whether its first parameter is `self`.
    /// Used to validate batch bindings: batch requires `self` as the first parameter.
    free_fn_has_self: HashMap<String, bool>,
    current_scope: usize,
    diagnostics: Vec<Diagnostic>,
    impl_methods: HashMap<String, HashMap<String, MethodSignature>>, // struct -> (method -> signature)
    current_fn_is_mut: bool,
    /// Written signature contracts of every declared function, by name.
    fn_contracts: HashMap<String, DeclaredContract>,
    host_modules: HashMap<String, HashMap<String, HostFunction>>,
    /// Return contract of the function currently being analyzed, checked at every `return`.
    current_return_contract: Option<TypeAnnotation>,
    /// Root scope of the module, which owns the module-level bindings.
    root_scope: usize,
    /// Names declared by a top-level `let`/`var`, collected before any body is analyzed.
    ///
    /// Canon gives every function of a module access to the module's bindings, so the names
    /// exist in the module scope from the start; the executable flow is what stays textual,
    /// which is why reaching a declaration is tracked separately.
    module_bindings: HashSet<String>,
    /// Module bindings the executable flow has already reached, in textual order.
    reached_bindings: HashSet<String>,
    /// `true` while the module's executable statements are analyzed.
    ///
    /// Function, method and closure bodies are deferred code: they run after the module has
    /// initialized, so they see every module binding and never depend on textual position.
    in_module_flow: bool,
    /// Names of declared `async fn` functions: calling one produces a `Task`.
    async_fns: HashSet<String>,
    /// Struct currently being implemented in `impl` block.
    current_struct: Option<String>,
    /// `true` while analyzing an `init` hook inside an `impl`.
    in_init: bool,
    /// Names of declared `async` methods.
    async_methods: HashSet<String>,
    /// Declared struct fixed fields: struct_name -> set of fixed field names.
    struct_fixed_fields: HashMap<String, HashSet<String>>,
    /// Declared field type contracts: struct (or `Enum.Variant`) name -> field -> contract.
    struct_field_contracts: HashMap<String, HashMap<String, TypeAnnotation>>,
    /// Declared enum variants: enum_name -> list of variant names.
    enum_variants: HashMap<String, Vec<String>>,
    /// Tuple-variant constructor arity: `Enum.Variant` -> payload field count.
    enum_tuple_arity: HashMap<String, usize>,
    /// Items marked with `#!deprecated`: name -> optional message.
    deprecated_items: HashMap<String, Option<String>>,
    /// Maps variable name to known struct type name (e.g. `var p = Point{...}` -> "Point").
    var_struct_types: HashMap<String, String>,
    /// Maps variable name to known underlying type when declared nullable (e.g. `p: Point?` -> "Point").
    var_nullable_types: HashMap<String, String>,
    /// `true` inside an `await do` body (reset on function boundaries: the
    /// block form never enters lambdas defined inside it).
    in_await_do: bool,
}

impl<'a> SemanticAnalyzer<'a> {
    /// Constructs a new semantic analyzer with only the language's own surface.
    #[must_use]
    pub fn new(source: &'a Source) -> Self {
        Self::with_prelude(source, &PreludeSurface::fundamental())
    }

    /// Constructs a semantic analyzer with an explicit global surface.
    ///
    /// Callers that embed the standard library pass the surface derived from the same
    /// registration the VM will execute with, so `check` and `run` agree on what exists.
    #[must_use]
    pub fn with_prelude(source: &'a Source, surface: &PreludeSurface) -> Self {
        let mut facts = SemanticFacts::default();
        let root = facts.scopes.new_scope(None);

        let mut analyzer = Self {
            source,
            facts,
            free_fn_has_self: HashMap::new(),
            current_scope: root,
            diagnostics: Vec::new(),
            impl_methods: HashMap::new(),
            current_fn_is_mut: false,
            fn_contracts: HashMap::new(),
            host_modules: HashMap::new(),
            current_return_contract: None,
            root_scope: root,
            module_bindings: HashSet::new(),
            reached_bindings: HashSet::new(),
            in_module_flow: false,
            async_fns: HashSet::new(),
            in_await_do: false,
            current_struct: None,
            in_init: false,
            async_methods: HashSet::new(),
            struct_fixed_fields: HashMap::new(),
            struct_field_contracts: HashMap::new(),
            enum_variants: HashMap::new(),
            enum_tuple_arity: HashMap::new(),
            deprecated_items: HashMap::new(),
            var_struct_types: HashMap::new(),
            var_nullable_types: HashMap::new(),
        };

        analyzer.register_surface(surface);
        analyzer
    }

    /// Installs every name of the configured surface into the root scope.
    fn register_surface(&mut self, surface: &PreludeSurface) {
        let mut entries: Vec<(&String, &SymbolKind)> = surface.iter().collect();
        entries.sort_by(|left, right| left.0.cmp(right.0));

        for (name, kind) in entries {
            self.facts.scopes.insert(
                self.current_scope,
                Symbol {
                    name: name.clone(),
                    kind: kind.clone(),
                    mutability: Mutability::Immutable,
                    span: aipo_source::SourceSpan::empty(0),
                },
            );
        }

        for (module, functions) in surface.host_modules() {
            self.host_modules.insert(module.clone(), functions.clone());
        }
    }

    /// Performs semantic analysis on an `HirProgram`.
    pub fn analyze(mut self, program: &HirProgram) -> (SemanticFacts, Vec<Diagnostic>) {
        // Pass 1: Declare top-level items
        for item in &program.items {
            self.declare_item(item);
        }

        // Pass 2: Declare the module's own bindings. Functions, methods and closures resolve
        // them by name, and canon makes the whole module scope visible to them, so this runs
        // before any body is walked.
        self.declare_module_bindings(program);

        // Pass 3: Collect impl methods and verify satisfy declarations
        let mut seen_inits: HashSet<String> = HashSet::new();
        let mut seen_invariants: HashSet<String> = HashSet::new();
        for item in &program.items {
            if let HirItem::Impl(impl_block) = item {
                if let Some(inv) = &impl_block.invariant {
                    if !seen_invariants.insert(impl_block.target.clone()) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_HOOK_DUPLICADO,
                                format!("'{}' já declara 'invariant'", impl_block.target),
                            )
                            .with_primary_span(self.source, inv.span)
                            .with_note("reúna as condições em um único `invariant`"),
                        );
                    }
                    let is_empty = matches!(
                        inv.body.first(),
                        Some(HirStmt::Return(
                            Some(HirExpr::Literal(Literal::Bool(true), _)),
                            _
                        ))
                    );
                    if is_empty {
                        self.diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AIPO_SEM_HOOK_VAZIO,
                                format!(
                                    "'invariant' no tipo '{}' sem condição não verifica nada",
                                    impl_block.target
                                ),
                            )
                            .with_primary_span(self.source, inv.span)
                            .with_note("escreva ao menos uma condição, ou remova o hook"),
                        );
                    }
                }
                if let Some(init) = &impl_block.init {
                    if !seen_inits.insert(impl_block.target.clone()) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_HOOK_DUPLICADO,
                                format!("'{}' já declara 'init'", impl_block.target),
                            )
                            .with_primary_span(self.source, init.span)
                            .with_note("reúna as inicializações em um único `init`"),
                        );
                    }
                }
                let methods = self
                    .impl_methods
                    .entry(impl_block.target.clone())
                    .or_default();
                for m in &impl_block.methods {
                    if matches!(
                        m.name.as_str(),
                        "constructor" | "initialize" | "inicializar"
                    ) {
                        self.diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AIPO_SEM_NOME_DE_HOOK,
                                format!("'{}' parece um hook de construção, não um método", m.name),
                            )
                            .with_primary_span(self.source, m.span)
                            .with_note(format!(
                                "use `{}:init(...) {{ ... }}` para o hook de construção",
                                impl_block.target
                            )),
                        );
                    } else if matches!(m.name.as_str(), "validar" | "validate") {
                        self.diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AIPO_SEM_NOME_DE_HOOK,
                                format!("'{}' parece um hook, não um método", m.name),
                            )
                            .with_primary_span(self.source, m.span)
                            .with_note(format!(
                                "renomeie para `{}:invariant {{ ... }}` se a intenção é validar",
                                impl_block.target
                            )),
                        );
                    }
                    let min_args = m
                        .params
                        .iter()
                        .filter(|p| !p.is_self && p.default.is_none())
                        .count();
                    let max_args = m.params.iter().filter(|p| !p.is_self).count();
                    let is_mut_self = m.params.iter().any(|p| p.is_self && p.is_mut);
                    let param_types = m
                        .params
                        .iter()
                        .filter(|p| !p.is_self)
                        .map(|p| (p.name.clone(), p.type_annotation.clone()))
                        .collect();
                    methods.insert(
                        m.name.clone(),
                        MethodSignature {
                            min_args,
                            max_args,
                            is_mut_self,
                            is_async: m.is_async,
                            param_types,
                            return_type: m.return_type.clone(),
                        },
                    );
                    if m.is_async {
                        self.async_methods.insert(m.name.clone());
                    }
                    self.fn_contracts.insert(
                        format!("{}.{}", impl_block.target, m.name),
                        DeclaredContract::of(m),
                    );
                }
            } else if let HirItem::Batch(binding) = item {
                for function_name in &binding.functions {
                    if let Some(source) = program.items.iter().find_map(|it| match it {
                        HirItem::Fn(f) if &f.name == function_name => Some(f),
                        _ => None,
                    }) {
                        if source.params.first().is_some_and(|p| p.is_self) {
                            let min_args = source
                                .params
                                .iter()
                                .filter(|p| !p.is_self && p.default.is_none())
                                .count();
                            let max_args = source.params.iter().filter(|p| !p.is_self).count();
                            let is_mut_self = source.params.iter().any(|p| p.is_self && p.is_mut);
                            let param_types = source
                                .params
                                .iter()
                                .filter(|p| !p.is_self)
                                .map(|p| (p.name.clone(), p.type_annotation.clone()))
                                .collect();
                            let methods =
                                self.impl_methods.entry(binding.target.clone()).or_default();
                            methods.insert(
                                function_name.clone(),
                                MethodSignature {
                                    min_args,
                                    max_args,
                                    is_mut_self,
                                    is_async: source.is_async,
                                    param_types,
                                    return_type: source.return_type.clone(),
                                },
                            );
                            if source.is_async {
                                self.async_methods.insert(function_name.clone());
                            }
                            self.fn_contracts.insert(
                                format!("{}.{}", binding.target, function_name),
                                DeclaredContract::of(source),
                            );
                        }
                    }
                }
            }
        }

        for item in &program.items {
            if let HirItem::Struct(s) = item {
                for dir in &s.directives {
                    if dir.name.name == "satisfies" {
                        if let Some(arg) = &dir.argument {
                            let interfaces: Vec<String> = arg
                                .split(',')
                                .map(|part| part.trim().to_string())
                                .filter(|part| !part.is_empty())
                                .collect();
                            let sat = aipo_hir::HirSatisfyDecl {
                                target: s.name.clone(),
                                interfaces,
                                span: dir.span,
                            };
                            self.verify_satisfy(&sat);
                        }
                    }
                }
            }
        }

        // Pass 4: Analyze item bodies (deferred code, so no textual-order tracking)
        for item in &program.items {
            self.analyze_item_body(item);
        }

        // Pass 5: Analyze top-level statements in textual order
        self.in_module_flow = true;
        for stmt in &program.statements {
            self.analyze_stmt(stmt);
        }
        self.in_module_flow = false;

        (self.facts, self.diagnostics)
    }

    fn declare_item(&mut self, item: &HirItem) {
        match item {
            HirItem::Fn(f) => {
                let min_args = f.params.iter().filter(|p| p.default.is_none()).count();
                let max_args = f.params.len();
                let sym = Symbol {
                    name: f.name.clone(),
                    kind: SymbolKind::Function { min_args, max_args },
                    mutability: Mutability::Immutable,
                    span: f.span,
                };
                if self.facts.scopes.insert(self.current_scope, sym).is_some() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE,
                            format!("function '{}' is already declared in this scope", f.name),
                        )
                        .with_primary_span(self.source, f.span),
                    );
                }
                // Canon reports a provable contract incompatibility before execution, so the
                // written annotations are collected here for the call and return checks.
                self.fn_contracts
                    .insert(f.name.clone(), DeclaredContract::of(f));
                // Batch bindings require `self` as the first parameter; recording whether the
                // function declares it lets the batch check run without re-walking signatures.
                self.free_fn_has_self
                    .insert(f.name.clone(), f.params.first().is_some_and(|p| p.is_self));
                // Calling an `async fn` produces a `Task`: the name table backs
                // the known-Task analysis (await-do desugar, forgotten tasks).
                if f.is_async {
                    self.async_fns.insert(f.name.clone());
                }
                for dir in &f.directives {
                    if dir.name.name == "deprecated" {
                        let msg = dir.argument.as_deref().map(|s| {
                            let trimmed = s.trim();
                            trimmed
                                .trim_matches(|c| c == '(' || c == ')' || c == '"')
                                .to_string()
                        });
                        self.deprecated_items.insert(f.name.clone(), msg);
                    } else if dir.name.name == "todo" {
                        self.diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AIPO_SEM_TODO,
                                format!("item '{}' is marked with #!todo", f.name),
                            )
                            .with_primary_span(self.source, dir.span),
                        );
                    }
                }
            }
            HirItem::Struct(s) => {
                for dir in &s.directives {
                    if dir.name.name == "deprecated" {
                        let msg = dir.argument.as_deref().map(|s| {
                            let trimmed = s.trim();
                            trimmed
                                .trim_matches(|c| c == '(' || c == ')' || c == '"')
                                .to_string()
                        });
                        self.deprecated_items.insert(s.name.clone(), msg);
                    } else if dir.name.name == "todo" {
                        self.diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AIPO_SEM_TODO,
                                format!("struct '{}' is marked with #!todo", s.name),
                            )
                            .with_primary_span(self.source, dir.span),
                        );
                    }
                }
                let mut fields = HashMap::new();
                let mut fixed_fields = HashSet::new();
                let mut contracts = HashMap::new();
                for f in &s.fields {
                    fields.insert(f.name.clone(), f.is_fixed);
                    if f.is_fixed {
                        fixed_fields.insert(f.name.clone());
                    }
                    if let Some(contract) = &f.type_annotation {
                        contracts.insert(f.name.clone(), contract.clone());
                    }
                }
                self.struct_fixed_fields
                    .insert(s.name.clone(), fixed_fields);
                if !contracts.is_empty() {
                    self.struct_field_contracts
                        .insert(s.name.clone(), contracts);
                }
                let sym = Symbol {
                    name: s.name.clone(),
                    kind: SymbolKind::Struct { fields },
                    mutability: Mutability::Immutable,
                    span: s.span,
                };
                if self.facts.scopes.insert(self.current_scope, sym).is_some() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE,
                            format!("struct '{}' is already declared in this scope", s.name),
                        )
                        .with_primary_span(self.source, s.span),
                    );
                }
            }
            HirItem::Enum(e) => {
                let mut variant_names = Vec::new();
                let mut seen_variants = HashSet::new();
                for v in &e.variants {
                    if !seen_variants.insert(v.name.clone()) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE,
                                format!(
                                    "enum '{}' declares variant '{}' more than once",
                                    e.name, v.name
                                ),
                            )
                            .with_primary_span(self.source, v.span),
                        );
                        continue;
                    }
                    variant_names.push(v.name.clone());
                    let variant_full_name = format!("{}.{}", e.name, v.name);
                    let mut fields = HashMap::new();
                    match &v.payload {
                        aipo_hir::HirEnumVariantPayload::Unit => {}
                        aipo_hir::HirEnumVariantPayload::Tuple(t_fields) => {
                            for (idx, field) in t_fields.iter().enumerate() {
                                let f_name = field.name.clone().unwrap_or_else(|| idx.to_string());
                                fields.insert(f_name, true);
                            }
                            self.enum_tuple_arity
                                .insert(variant_full_name.clone(), t_fields.len());
                            let sym = Symbol {
                                name: variant_full_name.clone(),
                                kind: SymbolKind::Function {
                                    min_args: t_fields.len(),
                                    max_args: t_fields.len(),
                                },
                                mutability: Mutability::Immutable,
                                span: v.span,
                            };
                            self.facts.scopes.insert(self.current_scope, sym);
                        }
                        aipo_hir::HirEnumVariantPayload::Struct(s_fields) => {
                            let mut contracts = HashMap::new();
                            for field in s_fields {
                                fields.insert(field.name.clone(), true);
                                if let Some(contract) = &field.type_annotation {
                                    contracts.insert(field.name.clone(), contract.clone());
                                }
                            }
                            if !contracts.is_empty() {
                                self.struct_field_contracts
                                    .insert(variant_full_name.clone(), contracts);
                            }
                        }
                    }
                    let sym = Symbol {
                        name: variant_full_name.clone(),
                        kind: SymbolKind::Struct { fields },
                        mutability: Mutability::Immutable,
                        span: v.span,
                    };
                    self.facts.scopes.insert(self.current_scope, sym);
                }
                self.enum_variants.insert(e.name.clone(), variant_names);
                let sym = Symbol {
                    name: e.name.clone(),
                    kind: SymbolKind::Variable,
                    mutability: Mutability::Immutable,
                    span: e.span,
                };
                if self.facts.scopes.insert(self.current_scope, sym).is_some() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE,
                            format!("enum '{}' is already declared in this scope", e.name),
                        )
                        .with_primary_span(self.source, e.span),
                    );
                }
            }
            HirItem::Interface(i) => {
                let mut methods = HashMap::new();
                for m in &i.methods {
                    let min_args = m
                        .params
                        .iter()
                        .filter(|p| !p.is_self && p.default.is_none())
                        .count();
                    let max_args = m.params.iter().filter(|p| !p.is_self).count();
                    let is_mut_self = m.params.iter().any(|p| p.is_self && p.is_mut);
                    let param_types = m
                        .params
                        .iter()
                        .filter(|p| !p.is_self)
                        .map(|p| (p.name.clone(), p.type_annotation.clone()))
                        .collect();
                    methods.insert(
                        m.name.clone(),
                        MethodSignature {
                            min_args,
                            max_args,
                            is_mut_self,
                            is_async: m.is_async,
                            param_types,
                            return_type: m.return_type.clone(),
                        },
                    );
                }
                let sym = Symbol {
                    name: i.name.clone(),
                    kind: SymbolKind::Interface { methods },
                    mutability: Mutability::Immutable,
                    span: i.span,
                };
                if self.facts.scopes.insert(self.current_scope, sym).is_some() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE,
                            format!("interface '{}' is already declared in this scope", i.name),
                        )
                        .with_primary_span(self.source, i.span),
                    );
                }
            }
            HirItem::Import(imp) => {
                for name in &imp.names {
                    let sym = Symbol {
                        name: name.clone(),
                        kind: SymbolKind::Variable,
                        mutability: Mutability::Immutable,
                        span: imp.span,
                    };
                    self.facts.scopes.insert(self.current_scope, sym);
                }
                if let Some(alias) = &imp.alias {
                    let sym = Symbol {
                        name: alias.clone(),
                        kind: SymbolKind::Variable,
                        mutability: Mutability::Immutable,
                        span: imp.span,
                    };
                    self.facts.scopes.insert(self.current_scope, sym);
                }
            }
            HirItem::Batch(binding) => {
                // A batch association promotes free functions to methods, so every listed
                // name must resolve to a declared function in this scope.
                for name in &binding.functions {
                    if self.facts.scopes.lookup(self.current_scope, name).is_none() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                format!("unknown function '{name}' in batch association"),
                            )
                            .with_primary_span(self.source, binding.span),
                        );
                        continue;
                    }
                    // Canon (SYNTAX.md §5.3): batch promotes *type behavior*, so the source
                    // function must declare `self` as its first parameter. Without it the
                    // promoted method would carry a receiver the body never uses — namespacing
                    // dressed as a method, which is worse than rejecting it.
                    if self.free_fn_has_self.get(name.as_str()) == Some(&false) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                format!(
                                    "function '{name}' has no 'self' as first parameter; \
                                     batch associations require it"
                                ),
                            )
                            .with_primary_span(self.source, binding.span),
                        );
                    }
                }
            }
            HirItem::Impl(_) | HirItem::Export(_) => {}
        }
    }

    fn verify_satisfy(&mut self, sat: &HirSatisfyDecl) {
        let target_sym = self.facts.scopes.lookup(self.current_scope, &sat.target);
        if target_sym.is_none() {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                    format!("unknown satisfy target type '{}'", sat.target),
                )
                .with_primary_span(self.source, sat.span),
            );
            return;
        }

        let struct_methods = self
            .impl_methods
            .get(&sat.target)
            .cloned()
            .unwrap_or_default();

        for iface_name in &sat.interfaces {
            let iface_sym = self.facts.scopes.lookup(self.current_scope, iface_name);
            match iface_sym {
                Some(Symbol {
                    kind: SymbolKind::Interface { methods },
                    ..
                }) => {
                    for (method_name, req) in methods {
                        if let Some(prov) = struct_methods.get(method_name) {
                            if prov.min_args > req.min_args || prov.max_args < req.max_args {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_ARITY_MISMATCH,
                                        format!(
                                            "method '{}' on '{}' does not match interface '{}' arity contract",
                                            method_name, sat.target, iface_name
                                        ),
                                    )
                                    .with_primary_span(self.source, sat.span),
                                );
                            }

                            if req.is_mut_self && !prov.is_mut_self {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                                        format!(
                                            "method '{}' on '{}' requires mutable receiver 'var self' to satisfy interface '{}'",
                                            method_name, sat.target, iface_name
                                        ),
                                    )
                                    .with_primary_span(self.source, sat.span),
                                );
                            }

                            if req.is_async != prov.is_async {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                                        format!(
                                            "method '{}' on '{}' async modifier does not match interface '{}'",
                                            method_name, sat.target, iface_name
                                        ),
                                    )
                                    .with_primary_span(self.source, sat.span),
                                );
                            }

                            if let Some(req_ret) = &req.return_type {
                                if let Some(prov_ret) = &prov.return_type {
                                    if req_ret.name != prov_ret.name
                                        || req_ret.is_nullable != prov_ret.is_nullable
                                    {
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                                                format!(
                                                    "method '{}' on '{}' return type '{}' does not match interface '{}' expected '{}'",
                                                    method_name,
                                                    sat.target,
                                                    prov_ret.name,
                                                    iface_name,
                                                    req_ret.name
                                                ),
                                            )
                                            .with_primary_span(self.source, sat.span),
                                        );
                                    }
                                } else {
                                    self.diagnostics.push(
                                        Diagnostic::error(
                                            DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                                            format!(
                                                "method '{}' on '{}' is missing required return type '{}' from interface '{}'",
                                                method_name, sat.target, req_ret.name, iface_name
                                            ),
                                        )
                                        .with_primary_span(self.source, sat.span),
                                    );
                                }
                            }

                            for (req_p, prov_p) in
                                req.param_types.iter().zip(prov.param_types.iter())
                            {
                                if let Some(req_ty) = &req_p.1 {
                                    if let Some(prov_ty) = &prov_p.1 {
                                        if req_ty.name != prov_ty.name
                                            || req_ty.is_nullable != prov_ty.is_nullable
                                        {
                                            self.diagnostics.push(
                                                Diagnostic::error(
                                                    DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                                                    format!(
                                                        "parameter '{}' of method '{}' on '{}' type '{}' does not match interface '{}' required '{}'",
                                                        prov_p.0,
                                                        method_name,
                                                        sat.target,
                                                        prov_ty.name,
                                                        iface_name,
                                                        req_ty.name
                                                    ),
                                                )
                                                .with_primary_span(self.source, sat.span),
                                            );
                                        }
                                    } else {
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                                                format!(
                                                    "parameter '{}' of method '{}' on '{}' is missing required type annotation '{}' from interface '{}'",
                                                    prov_p.0,
                                                    method_name,
                                                    sat.target,
                                                    req_ty.name,
                                                    iface_name
                                                ),
                                            )
                                            .with_primary_span(self.source, sat.span),
                                        );
                                    }
                                }
                            }
                        } else {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                    format!(
                                        "struct '{}' satisfies '{}' but is missing method '{}'",
                                        sat.target, iface_name, method_name
                                    ),
                                )
                                .with_primary_span(self.source, sat.span),
                            );
                        }
                    }
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                            format!("unknown interface '{}' in satisfy declaration", iface_name),
                        )
                        .with_primary_span(self.source, sat.span),
                    );
                }
            }
        }
    }

    fn analyze_item_body(&mut self, item: &HirItem) {
        match item {
            HirItem::Fn(f) => self.analyze_function(f),
            HirItem::Impl(i) => {
                let prev_struct = self.current_struct.take();
                self.current_struct = Some(i.target.clone());
                if let Some(init) = &i.init {
                    let prev_init = self.in_init;
                    self.in_init = true;
                    self.analyze_function(init);
                    self.in_init = prev_init;
                }
                for method in &i.methods {
                    self.analyze_function(method);
                }
                self.current_struct = prev_struct;
            }
            _ => {}
        }
    }

    fn analyze_function(&mut self, f: &HirFunctionDecl) {
        let parent = self.current_scope;
        let fn_scope = self.facts.scopes.new_scope(Some(parent));
        self.current_scope = fn_scope;

        let prev_var_types = self.var_struct_types.clone();
        let prev_nullable_types = self.var_nullable_types.clone();
        let prev_fn_is_mut = self.current_fn_is_mut;
        self.current_fn_is_mut = f.params.iter().any(|p| p.is_self && p.is_mut);
        let prev_return_contract = self.current_return_contract.clone();
        self.current_return_contract = f.return_type.clone();

        for p in &f.params {
            if let Some(ann) = &p.type_annotation {
                if ann.is_nullable {
                    self.var_nullable_types
                        .insert(p.name.clone(), ann.name.clone());
                } else {
                    self.var_struct_types
                        .insert(p.name.clone(), ann.name.clone());
                }
            }
            let mutability = if p.is_mut {
                Mutability::Mutable
            } else {
                Mutability::Immutable
            };
            let sym = Symbol {
                name: p.name.clone(),
                kind: SymbolKind::Parameter,
                mutability,
                span: p.span,
            };
            if self.facts.scopes.insert(self.current_scope, sym).is_some() {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE,
                        format!("parameter '{}' is declared more than once", p.name),
                    )
                    .with_primary_span(self.source, p.span),
                );
            }
        }

        for stmt in &f.body {
            self.analyze_stmt(stmt);
        }

        self.check_return_consistency(f);

        self.current_fn_is_mut = prev_fn_is_mut;
        self.current_return_contract = prev_return_contract;
        self.var_struct_types = prev_var_types;
        self.var_nullable_types = prev_nullable_types;
        self.current_scope = parent;
    }

    /// Reports a function whose `return` statements disagree about producing a value.
    ///
    /// Mixing `return x` with a bare `return` leaves the caller unable to know
    /// which shape it received, so it is reported once per function. A body that
    /// returns values on every path but can also fall off the end is reported
    /// separately, since the caller would observe `none` instead of a value.
    fn check_return_consistency(&mut self, f: &HirFunctionDecl) {
        let report = crate::flow::analyze_function(f);

        if report.returns == crate::flow::ReturnShape::Mixed {
            let message = if f.return_type.is_some() {
                "function declares a value contract but mixes `return <value>` with a bare \
                 `return`"
            } else {
                "function mixes `return <value>` with a bare `return`"
            };
            self.diagnostics.push(
                Diagnostic::error(DiagnosticCode::AIPO_SEM_RETURN_VALUE_MISMATCH, message)
                    .with_primary_span(self.source, f.span),
            );
        }

        // A missing value only matters when the caller was promised one, either
        // by a written contract or by the value-returning paths themselves.
        let promises_value =
            f.return_type.is_some() || report.returns == crate::flow::ReturnShape::AllValues;
        if promises_value && report.missing_value_path {
            let message = if let Some(return_type) = &f.return_type {
                format!(
                    "function declares `-> {}` but a path reaches the end without returning a value",
                    contract_label(return_type)
                )
            } else {
                "a path reaches the end of the function without returning a value".to_string()
            };
            self.diagnostics.push(
                Diagnostic::error(DiagnosticCode::AIPO_SEM_PATH_MISSING_RETURN_VALUE, message)
                    .with_primary_span(self.source, f.span),
            );
        }
    }

    fn analyze_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Let(name, expr, span) => {
                // Canon keeps a binding out of its own initializer, so the expression is walked
                // before the name becomes visible to the executable flow.
                self.analyze_expr_top(expr);
                self.declare_binding(name, Mutability::Immutable, *span);
                if let Some(target) = extract_inferred_type(expr) {
                    self.var_struct_types.insert(name.clone(), target);
                }
            }
            HirStmt::Var(name, expr, span) => {
                self.analyze_expr_top(expr);
                self.declare_binding(name, Mutability::Mutable, *span);
                if let Some(target) = extract_inferred_type(expr) {
                    self.var_struct_types.insert(name.clone(), target);
                }
            }
            HirStmt::Assign(target, value, span) => {
                self.analyze_expr_top(value);
                self.check_assignment_target(target, *span);
                if let HirExpr::Identifier(name, _) = target {
                    if let Some(st) = extract_inferred_type(value) {
                        self.var_struct_types.insert(name.clone(), st);
                    }
                }
            }
            HirStmt::CompoundAssign(_, target, value, span) => {
                self.analyze_expr_top(value);
                self.check_assignment_target(target, *span);
            }
            HirStmt::If(s) => {
                self.check_condition_bool(&s.condition);
                self.analyze_expr(&s.condition);
                let mut then_narrowings = Vec::new();
                self.collect_type_narrowings(&s.condition, true, &mut then_narrowings);
                self.with_block_scope(|this| {
                    for (var_name, narrowed_type) in then_narrowings {
                        this.var_struct_types.insert(var_name, narrowed_type);
                    }
                    for st in &s.then_branch {
                        this.analyze_stmt(st);
                    }
                });
                for (cond, body) in &s.elif_branches {
                    self.check_condition_bool(cond);
                    self.analyze_expr(cond);
                    let mut elif_narrowings = Vec::new();
                    self.collect_type_narrowings(cond, true, &mut elif_narrowings);
                    self.with_block_scope(|this| {
                        for (var_name, narrowed_type) in elif_narrowings {
                            this.var_struct_types.insert(var_name, narrowed_type);
                        }
                        for st in body {
                            this.analyze_stmt(st);
                        }
                    });
                }
                if let Some(else_branch) = &s.else_branch {
                    let mut else_narrowings = Vec::new();
                    if s.elif_branches.is_empty() {
                        self.collect_type_narrowings(&s.condition, false, &mut else_narrowings);
                    }
                    self.with_block_scope(|this| {
                        for (var_name, narrowed_type) in else_narrowings {
                            this.var_struct_types.insert(var_name, narrowed_type);
                        }
                        for st in else_branch {
                            this.analyze_stmt(st);
                        }
                    });
                }
            }
            HirStmt::Match(s) => {
                self.analyze_expr(&s.target);
                let mut matched_enum_variants: HashMap<String, HashSet<String>> = HashMap::new();
                for arm in &s.when_arms {
                    for p in &arm.patterns {
                        match p {
                            aipo_hir::HirMatchPattern::Value(expr) => {
                                self.analyze_expr(expr);
                            }
                            aipo_hir::HirMatchPattern::Variant {
                                enum_name: Some(e_name),
                                variant_name,
                                ..
                            } => {
                                match self.enum_variants.get(e_name) {
                                    None => {
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                                format!("unknown enum '{e_name}' in match pattern"),
                                            )
                                            .with_primary_span(self.source, s.span),
                                        );
                                    }
                                    Some(variants)
                                        if !variants.iter().any(|v| v == variant_name) =>
                                    {
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                                format!(
                                                    "unknown variant '{variant_name}' of enum '{e_name}' in match pattern"
                                                ),
                                            )
                                            .with_primary_span(self.source, s.span),
                                        );
                                    }
                                    _ => {}
                                }
                                matched_enum_variants
                                    .entry(e_name.clone())
                                    .or_default()
                                    .insert(variant_name.clone());
                            }
                            _ => {}
                        }
                    }
                    self.with_block_scope(|this| {
                        let target_var = match &s.target {
                            HirExpr::Identifier(name, _) => Some(name.clone()),
                            _ => None,
                        };
                        // The arm's destructured fields exist inside its scope and guard.
                        for p in &arm.patterns {
                            match p {
                                aipo_hir::HirMatchPattern::Destructure(fields) => {
                                    for field in fields {
                                        this.declare_binding(
                                            field,
                                            Mutability::Immutable,
                                            aipo_source::SourceSpan::empty(0),
                                        );
                                        if let Some(tv) = &target_var {
                                            if let Some(st_name) = this.var_struct_types.get(tv) {
                                                if let Some(contracts) =
                                                    this.struct_field_contracts.get(st_name)
                                                {
                                                    if let Some(ann) = contracts.get(field) {
                                                        if ann.is_nullable {
                                                            this.var_nullable_types.insert(
                                                                field.clone(),
                                                                ann.name.clone(),
                                                            );
                                                        } else {
                                                            this.var_struct_types.insert(
                                                                field.clone(),
                                                                ann.name.clone(),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                aipo_hir::HirMatchPattern::Variant {
                                    enum_name,
                                    variant_name,
                                    payload,
                                    ..
                                } => {
                                    let full_name = match enum_name {
                                        Some(e) => format!("{e}.{variant_name}"),
                                        None => variant_name.clone(),
                                    };
                                    if let Some(tv) = &target_var {
                                        this.var_struct_types.insert(tv.clone(), full_name.clone());
                                    }
                                    match payload {
                                        aipo_hir::HirVariantPatternPayload::Unit => {}
                                        aipo_hir::HirVariantPatternPayload::Tuple(ids)
                                        | aipo_hir::HirVariantPatternPayload::Struct(ids) => {
                                            for field in ids {
                                                this.declare_binding(
                                                    field,
                                                    Mutability::Immutable,
                                                    aipo_source::SourceSpan::empty(0),
                                                );
                                                if let Some(contracts) =
                                                    this.struct_field_contracts.get(&full_name)
                                                {
                                                    if let Some(ann) = contracts.get(field) {
                                                        if ann.is_nullable {
                                                            this.var_nullable_types.insert(
                                                                field.clone(),
                                                                ann.name.clone(),
                                                            );
                                                        } else {
                                                            this.var_struct_types.insert(
                                                                field.clone(),
                                                                ann.name.clone(),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                aipo_hir::HirMatchPattern::Value(expr) => {
                                    if let HirExpr::Identifier(type_name, _) = expr {
                                        if let Some(tv) = &target_var {
                                            this.var_struct_types
                                                .insert(tv.clone(), type_name.clone());
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(guard) = &arm.guard {
                            let mut guard_narrowings = Vec::new();
                            this.collect_type_narrowings(guard, true, &mut guard_narrowings);
                            for (var_name, narrowed_type) in guard_narrowings {
                                this.var_struct_types.insert(var_name, narrowed_type);
                            }
                            this.check_condition_bool(guard);
                            this.analyze_expr(guard);
                        }
                        for st in &arm.body {
                            this.analyze_stmt(st);
                        }
                    });
                }
                // Canon (SYNTAX.md §17.4): match over enum without else must cover all variants.
                if s.else_arm.is_none() {
                    for (enum_name, covered) in &matched_enum_variants {
                        if let Some(all_variants) = self.enum_variants.get(enum_name) {
                            let missing: Vec<&String> = all_variants
                                .iter()
                                .filter(|v| !covered.contains(*v))
                                .collect();
                            if !missing.is_empty() {
                                let missing_names = missing
                                    .iter()
                                    .map(|s| s.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_NON_EXHAUSTIVE_MATCH,
                                        format!(
                                            "non-exhaustive match on enum '{enum_name}': missing variants {missing_names}"
                                        ),
                                    )
                                    .with_primary_span(self.source, s.span),
                                );
                            }
                        }
                    }
                }
                if let Some(else_arm) = &s.else_arm {
                    self.with_block_scope(|this| {
                        for st in else_arm {
                            this.analyze_stmt(st);
                        }
                    });
                }
            }
            HirStmt::Loop(body, _) => {
                self.with_block_scope(|this| {
                    for st in body {
                        this.analyze_stmt(st);
                    }
                });
            }
            HirStmt::While(cond, body, _) => {
                self.check_condition_bool(cond);
                self.analyze_expr(cond);
                self.with_block_scope(|this| {
                    for st in body {
                        this.analyze_stmt(st);
                    }
                });
            }
            HirStmt::Repeat(count, alias, body, span) => {
                self.analyze_expr(count);
                self.with_block_scope(|this| {
                    if let Some(name) = alias {
                        this.facts.scopes.insert(
                            this.current_scope,
                            Symbol {
                                name: name.clone(),
                                kind: SymbolKind::Variable,
                                mutability: Mutability::Immutable,
                                span: *span,
                            },
                        );
                    }
                    for st in body {
                        this.analyze_stmt(st);
                    }
                });
            }
            HirStmt::Each(vars, iter, body, span) => {
                self.analyze_expr(iter);
                self.with_block_scope(|this| {
                    for v in vars {
                        this.facts.scopes.insert(
                            this.current_scope,
                            Symbol {
                                name: v.clone(),
                                kind: SymbolKind::Variable,
                                mutability: Mutability::Immutable,
                                span: *span,
                            },
                        );
                    }
                    for st in body {
                        this.analyze_stmt(st);
                    }
                });
            }
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
            HirStmt::Return(expr, span) => {
                if let Some(e) = expr {
                    self.analyze_expr_top(e);
                    self.check_return_contract(e, *span);
                }
            }
            HirStmt::Fail(expr, _) => self.analyze_expr(expr),
            HirStmt::AwaitDo(body, span) => {
                if self.in_await_do {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_NESTED_AWAIT_DO,
                            "redundant nested `await do`: the inner block awaits nothing new",
                        )
                        .with_primary_span(self.source, *span),
                    );
                }
                let was_inside = self.in_await_do;
                self.in_await_do = true;
                for stmt in body {
                    self.analyze_stmt(stmt);
                }
                self.in_await_do = was_inside;
            }
            HirStmt::Attempt(s) => {
                self.with_block_scope(|this| {
                    for st in &s.body {
                        this.analyze_stmt(st);
                    }
                });
                self.with_block_scope(|this| {
                    if let Some(err) = &s.error_binding {
                        this.facts.scopes.insert(
                            this.current_scope,
                            Symbol {
                                name: err.clone(),
                                kind: SymbolKind::Variable,
                                mutability: Mutability::Immutable,
                                span: s.span,
                            },
                        );
                    }
                    for st in &s.handler {
                        this.analyze_stmt(st);
                    }
                });
            }
            HirStmt::FnDecl(f) => {
                // Canon: a local function's binding exists when execution reaches the
                // declaration and stays visible inside its own body for recursion, so the
                // name is declared before the body is walked. Enclosing bindings stay
                // visible (lexical capture), but the body is deferred code: `let` bindings
                // of the enclosing flow that were not reached yet must not leak in, which
                // the block scope + module-flow reset already handle for closures.
                self.declare_binding(&f.name, Mutability::Immutable, f.span);
                self.fn_contracts
                    .insert(f.name.clone(), DeclaredContract::of(f));
                if f.is_async {
                    self.async_fns.insert(f.name.clone());
                }
                let prev_return_contract = self.current_return_contract.clone();
                let prev_module_flow = self.in_module_flow;
                self.current_return_contract = f.return_type.clone();
                self.in_module_flow = false;
                // `await do` never enters a nested function body.
                let prev_await_do = self.in_await_do;
                self.in_await_do = false;
                self.with_block_scope(|this| {
                    for p in &f.params {
                        let mutability = if p.is_mut {
                            Mutability::Mutable
                        } else {
                            Mutability::Immutable
                        };
                        this.facts.scopes.insert(
                            this.current_scope,
                            Symbol {
                                name: p.name.clone(),
                                kind: SymbolKind::Parameter,
                                mutability,
                                span: p.span,
                            },
                        );
                    }
                    for st in &f.body {
                        this.analyze_stmt(st);
                    }
                });
                self.current_return_contract = prev_return_contract;
                self.in_module_flow = prev_module_flow;
                self.in_await_do = prev_await_do;
            }
            HirStmt::Expr(expr) => {
                self.analyze_expr_top(expr);
                self.check_forgotten_task(expr);
            }
        }
    }

    /// Reports a condition operand whose category is provably not `Bool`.
    ///
    /// Only a literal is decidable before execution, which is exactly the case
    /// worth catching: the author wrote `if 42 then`, not a value whose type is
    /// merely unknown. Every other expression is accepted, leaving a
    /// dynamically-typed program to decide at runtime.
    fn check_condition_bool(&mut self, expr: &HirExpr) {
        let Some(problem) = crate::flow::non_bool_condition(expr) else {
            return;
        };
        let crate::flow::ConditionProblem::Literal(literal) = problem;
        let found = describe_literal(&literal);
        let span = expr.span();
        let replacement = format!("{found} != 0");
        let suggestion = aipo_diagnostics::Suggestion {
            message: "compare the value against zero".to_string(),
            replacement,
            start: span.start,
            end: span.end,
        };
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::AIPO_SEM_NON_BOOL_CONDITION,
                format!("condition must be `Bool`, found `{found}`"),
            )
            .with_primary_span(self.source, span)
            .with_note(format!(
                "a condition is `true` or `false`; `{found}` is not one"
            ))
            .with_suggestion(suggestion),
        );
    }

    /// Walks an expression in statement, initializer or return position, where
    /// one top-level `await` is legal. Anything nested stays strict.
    fn analyze_expr_top(&mut self, expr: &HirExpr) {
        if let HirExpr::Await(inner, span) = expr {
            self.check_await_contract(inner, *span);
            self.analyze_expr(inner);
        } else {
            self.analyze_expr(expr);
        }
    }

    /// Reports an `await` operand that provably cannot be a `Task`.
    ///
    /// ADP-006 §G: a `Task` is only ever produced by an `async fn` call or a
    /// combinator, so a literal operand is a provable contract violation and a
    /// pre-execution report — not something to leave to the runtime fault.
    fn check_await_contract(&mut self, inner: &HirExpr, span: aipo_source::SourceSpan) {
        if !matches!(inner, HirExpr::Literal(_, _)) {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                "`await` operand is a literal, which is never a `Task`; await an `async fn` call or a task handle",
            )
            .with_primary_span(self.source, inner.span().merge(span)),
        );
    }

    /// Reports a known-`Task` value discarded by an expression statement.
    ///
    /// Canon requires a diagnostic for a task created and discarded without
    /// `await`, group combinator or explicit spawn handling. The check is
    /// deliberately syntactic (a full dataflow is out of scope): a root call
    /// to a declared `async fn`, or to `task.spawn`/`all`/`race`, whose value
    /// no statement consumes. `let x = spawn(f)` for later awaiting is fine.
    fn check_forgotten_task(&mut self, expr: &HirExpr) {
        let HirExpr::Call(callee, _, span) = expr else {
            return;
        };
        let task_producing = match &**callee {
            HirExpr::Identifier(name, _) => self.async_fns.contains(name),
            HirExpr::Dot(target, member, _) => {
                (matches!(&**target, HirExpr::Identifier(name, _) if name == "task")
                    && matches!(member.as_str(), "spawn" | "all" | "race"))
                    || self.async_methods.contains(member)
            }
            _ => false,
        };
        if task_producing {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_FORGOTTEN_TASK,
                    "created `Task` is discarded without `await`, group or combinator; bind it or await it",
                )
                .with_primary_span(self.source, *span),
            );
        }
    }

    fn check_assignment_target(&mut self, target: &HirExpr, span: aipo_source::SourceSpan) {
        match target {
            HirExpr::Identifier(name, id_span) => {
                let symbol = self
                    .name_is_visible(name)
                    .then(|| self.facts.scopes.lookup(self.current_scope, name))
                    .flatten();
                match symbol {
                    Some(sym) if sym.mutability == Mutability::Immutable => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_READONLY_MUTATION,
                                format!("cannot reassign to immutable binding '{}'", name),
                            )
                            .with_primary_span(self.source, *id_span),
                        );
                    }
                    Some(_) => {}
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                format!("unknown identifier '{}' in assignment", name),
                            )
                            .with_primary_span(self.source, *id_span),
                        );
                    }
                }
            }
            HirExpr::Dot(base, member, dot_span) => {
                self.analyze_expr(base);
                let root = path_root(base);
                if let Some((root_name, root_span)) = root {
                    if root_name == "self" {
                        if !self.current_fn_is_mut {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_READONLY_MUTATION,
                                    format!(
                                        "cannot mutate field '{}' through immutable receiver 'self'; method requires 'var self'",
                                        member
                                    ),
                                )
                                .with_primary_span(self.source, *dot_span),
                            );
                        } else if !self.in_init {
                            if let Some(struct_name) = &self.current_struct {
                                if let Some(fixed_set) = self.struct_fixed_fields.get(struct_name) {
                                    if fixed_set.contains(member) {
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticCode::AIPO_SEM_FIXED_REASSIGN,
                                                format!(
                                                    "cannot mutate fixed field '{}' of struct '{}' after construction",
                                                    member, struct_name
                                                ),
                                            )
                                            .with_primary_span(self.source, *dot_span),
                                        );
                                    }
                                }
                            }
                        }
                    } else {
                        let symbol = self
                            .name_is_visible(root_name)
                            .then(|| self.facts.scopes.lookup(self.current_scope, root_name))
                            .flatten();
                        match symbol {
                            Some(_) => {
                                if let Some(struct_name) = self.var_struct_types.get(root_name) {
                                    if let Some(fixed_set) =
                                        self.struct_fixed_fields.get(struct_name)
                                    {
                                        if fixed_set.contains(member) {
                                            self.diagnostics.push(
                                                Diagnostic::error(
                                                    DiagnosticCode::AIPO_SEM_FIXED_REASSIGN,
                                                    format!(
                                                        "cannot mutate fixed field '{}' of struct '{}' after construction",
                                                        member, struct_name
                                                    ),
                                                )
                                                .with_primary_span(self.source, *dot_span),
                                            );
                                        }
                                    }
                                }
                            }
                            None => {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                        format!(
                                            "unknown identifier '{}' in assignment target",
                                            root_name
                                        ),
                                    )
                                    .with_primary_span(self.source, root_span),
                                );
                            }
                        }
                    }
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_PARSE_INVALID_TARGET,
                            "invalid target in assignment",
                        )
                        .with_primary_span(self.source, *dot_span),
                    );
                }
            }
            HirExpr::Index(base, idx, index_span) => {
                self.analyze_expr(base);
                self.analyze_expr(idx);
                let root = path_root(base);
                if let Some((root_name, root_span)) = root {
                    if root_name == "self" {
                        if !self.current_fn_is_mut {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_READONLY_MUTATION,
                                    "cannot mutate through immutable receiver 'self'; method requires 'var self'",
                                )
                                .with_primary_span(self.source, *index_span),
                            );
                        }
                    } else {
                        let symbol = self
                            .name_is_visible(root_name)
                            .then(|| self.facts.scopes.lookup(self.current_scope, root_name))
                            .flatten();
                        match symbol {
                            Some(_) => {}
                            None => {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                        format!(
                                            "unknown identifier '{}' in assignment target",
                                            root_name
                                        ),
                                    )
                                    .with_primary_span(self.source, root_span),
                                );
                            }
                        }
                    }
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_PARSE_INVALID_TARGET,
                            "invalid target in assignment",
                        )
                        .with_primary_span(self.source, *index_span),
                    );
                }
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_INVALID_TARGET,
                        "invalid target in assignment",
                    )
                    .with_primary_span(self.source, span),
                );
            }
        }
    }

    /// Reports contract mismatches the analyzer can prove from literal arguments.
    ///
    /// Canon reports a provable incompatibility before execution and leaves an unprovable one as
    /// a runtime contract fault, so only literal categories are judged here. Arguments follow the
    /// declaration: positional arguments first, then named ones matched by parameter name.
    fn check_argument_contracts(
        &mut self,
        callee: &str,
        args: &[HirCallArg],
        contract: &DeclaredContract,
    ) {
        let positional = args.iter().take_while(|arg| arg.name.is_none()).count();
        for (index, param) in contract.params.iter().enumerate() {
            let Some(annotation) = &param.annotation else {
                continue;
            };
            let argument = if index < positional {
                args.get(index)
            } else {
                args.iter()
                    .skip(positional)
                    .find(|arg| arg.name.as_deref() == Some(param.name.as_str()))
            };
            let Some(argument) = argument else {
                continue;
            };
            if literal_violates_contract(&argument.value, annotation) {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                        format!(
                            "argument for parameter '{}' of '{}' cannot satisfy contract '{}'",
                            param.name,
                            callee,
                            contract_label(annotation)
                        ),
                    )
                    .with_primary_span(self.source, argument.value.span()),
                );
            }
        }
    }

    /// Host metadata applies only to the original prelude binding, not a local shadow.
    fn is_host_module_reference(&self, name: &str) -> bool {
        if !self.host_modules.contains_key(name) || self.module_bindings.contains(name) {
            return false;
        }
        let root = self.facts.scopes.scopes[self.root_scope].symbols.get(name);
        let resolved = self.facts.scopes.lookup(self.current_scope, name);
        match (root, resolved) {
            (Some(root), Some(resolved)) => {
                root.span == SourceSpan::empty(0) && std::ptr::eq(root, resolved)
            }
            _ => false,
        }
    }

    /// Checks a call against a host-module signature without assuming non-literal value types.
    fn check_host_call(
        &mut self,
        module: &str,
        member: &str,
        args: &[HirCallArg],
        span: SourceSpan,
    ) {
        let Some(functions) = self.host_modules.get(module) else {
            return;
        };
        let Some(function) = functions.get(member).cloned() else {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                    format!("unknown host member '{module}.{member}'"),
                )
                .with_primary_span(self.source, span),
            );
            return;
        };

        let positional = args.iter().filter(|arg| arg.name.is_none()).count();
        let mut supplied = HashSet::new();
        for (index, arg) in args.iter().enumerate() {
            if let Some(name) = &arg.name {
                if !function.params.iter().any(|param| &param.name == name) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_NAMED_ARG_UNKNOWN,
                            format!("unknown named argument '{name}' for '{module}.{member}'"),
                        )
                        .with_primary_span(self.source, arg.value.span()),
                    );
                } else if !supplied.insert(name.clone())
                    || function
                        .params
                        .iter()
                        .position(|param| &param.name == name)
                        .is_some_and(|position| position < positional)
                {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_DUPLICATE_NAMED_ARG,
                            format!("duplicate argument '{name}' for '{module}.{member}'"),
                        )
                        .with_primary_span(self.source, arg.value.span()),
                    );
                }
            } else if index < function.params.len() {
                supplied.insert(function.params[index].name.clone());
            }
        }
        let min_args = function
            .params
            .iter()
            .filter(|param| !param.optional)
            .count();
        let max_args = function.params.len();
        // An open-ended function states only the checked prefix of its parameters,
        // so extra positional arguments are expected rather than an error.
        let too_many = positional > max_args && !function.variadic;
        if too_many
            || function
                .params
                .iter()
                .any(|param| !param.optional && !supplied.contains(&param.name))
        {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_ARITY_MISMATCH,
                    format!(
                        "function '{module}.{member}' expected between {min_args} and {max_args} arguments, found {}",
                        args.len()
                    ),
                )
                .with_primary_span(self.source, span),
            );
        }

        let contract = DeclaredContract {
            params: function
                .params
                .iter()
                .map(|param| ParameterContract {
                    name: param.name.clone(),
                    annotation: Some(TypeAnnotation {
                        name: param.ty.clone(),
                        is_nullable: param.nullable,
                        span,
                    }),
                })
                .collect(),
        };
        self.check_argument_contracts(&format!("{module}.{member}"), args, &contract);
    }

    /// Reports a return value that provably cannot satisfy the declared return contract.
    fn check_return_contract(&mut self, expression: &HirExpr, span: SourceSpan) {
        let Some(annotation) = self.current_return_contract.clone() else {
            return;
        };
        if literal_violates_contract(expression, &annotation) {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                    format!(
                        "return value cannot satisfy contract '{}'",
                        contract_label(&annotation)
                    ),
                )
                .with_primary_span(self.source, span),
            );
        }
    }

    /// Declares the bindings a top-level `let`/`var` introduces, in module scope.
    ///
    /// The executable flow still sees them in textual order, so this only records the names;
    /// [`SemanticAnalyzer::declare_binding`] marks each one reached when the flow arrives at its
    /// declaration, and an early reference is reported as an unknown name.
    fn declare_module_bindings(&mut self, program: &HirProgram) {
        for stmt in &program.statements {
            let (name, mutability, span) = match stmt {
                HirStmt::Let(name, _, span) => (name, Mutability::Immutable, *span),
                HirStmt::Var(name, _, span) => (name, Mutability::Mutable, *span),
                _ => continue,
            };
            self.module_bindings.insert(name.clone());
            self.facts.scopes.insert(
                self.root_scope,
                Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Variable,
                    mutability,
                    span,
                },
            );
        }
    }

    /// Declares a `let`/`var` binding in the scope the statement is written in.
    fn declare_binding(&mut self, name: &str, mutability: Mutability, span: SourceSpan) {
        let is_module_flow = self.in_module_flow && self.current_scope == self.root_scope;
        if is_module_flow && self.module_bindings.contains(name) {
            // The name already exists in the module scope, so this statement is where the
            // executable flow reaches it — not a redeclaration, unless it was reached twice.
            if !self.reached_bindings.insert(name.to_string()) {
                self.report_redeclared_binding(name, span);
            }
            return;
        }

        if self.facts.scopes.scopes[self.current_scope]
            .symbols
            .contains_key(name)
        {
            self.report_redeclared_binding(name, span);
        } else {
            self.facts.scopes.insert(
                self.current_scope,
                Symbol {
                    name: name.to_string(),
                    kind: SymbolKind::Variable,
                    mutability,
                    span,
                },
            );
        }
    }

    fn report_redeclared_binding(&mut self, name: &str, span: SourceSpan) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::AIPO_SEM_REDECLARED_IN_SCOPE,
                format!("binding '{}' is already declared in this scope", name),
            )
            .with_primary_span(self.source, span),
        );
    }

    /// Reports whether `name` resolves at the current point of the program.
    ///
    /// A module binding is visible to every function of the module, but the executable flow
    /// only reaches it at its declaration, which keeps canon's textual-order rule for `let`/`var`.
    fn name_is_visible(&self, name: &str) -> bool {
        if self.in_module_flow
            && self.module_bindings.contains(name)
            && !self.reached_bindings.contains(name)
        {
            return false;
        }
        self.facts.scopes.lookup(self.current_scope, name).is_some()
    }

    fn analyze_expr(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Literal(_, _) => {}
            HirExpr::Identifier(name, span) => {
                if !self.name_is_visible(name) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                            format!("unknown identifier '{}'", name),
                        )
                        .with_primary_span(self.source, *span),
                    );
                }
            }
            HirExpr::Unary(op, inner, _) => {
                // `not` requires a Bool operand, exactly like `and`/`or`.
                if matches!(op, aipo_ast::UnaryOp::Not) {
                    self.check_condition_bool(inner);
                }
                self.analyze_expr(inner);
            }
            HirExpr::Await(inner, span) => {
                // Explicit `await` lives in statements, initializers and
                // returns (checked via `analyze_expr_top`); anywhere else it
                // cannot suspend, so it is a dedicated diagnostic, not a guess.
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_SEM_AWAIT_IN_SUBEXPRESSION,
                        "`await` is only allowed as a statement, initializer or return value, never inside a subexpression",
                    )
                    .with_primary_span(self.source, *span),
                );
                self.analyze_expr(inner);
            }
            HirExpr::Try(inner, _) => {
                // `expr?` is legal in any expression position; it evaluates the
                // operand and propagates a `Failure` immediately.
                self.analyze_expr(inner);
            }
            HirExpr::With(base, updates, span) => {
                // Functional struct update. When the base is a struct literal, its type
                // is known statically, so the block is checked against the declaration
                // up front: every name must exist and none may be `fixed`, turning a
                // typo into a diagnostic rather than a runtime fault. Any other base
                // (a variable, a call result) is checked by the VM instead.
                self.analyze_expr(base);
                let declared = match &**base {
                    HirExpr::Construct(target, _, _) => self
                        .facts
                        .scopes
                        .lookup(self.current_scope, target)
                        .and_then(|symbol| match &symbol.kind {
                            SymbolKind::Struct { fields } => Some((target.clone(), fields.clone())),
                            _ => None,
                        }),
                    _ => None,
                };
                for (name, value) in updates {
                    if let Some((target, fields)) = &declared {
                        match fields.get(name) {
                            None => self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                    format!("struct '{target}' has no field '{name}'"),
                                )
                                .with_primary_span(self.source, *span),
                            ),
                            Some(true) => self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_READONLY_MUTATION,
                                    format!(
                                        "cannot replace fixed field '{name}' through 'with'; 'fixed' is part of the type's identity"
                                    ),
                                )
                                .with_primary_span(self.source, *span),
                            ),
                            Some(false) => {}
                        }
                    }
                    self.analyze_expr(value);
                }
            }
            HirExpr::Binary(op, left, right, _) => {
                // `and`/`or` short-circuit on a Bool: a non-Bool literal is
                // provable before execution and reported, like a wrong `if`.
                if matches!(op, aipo_ast::BinaryOp::And | aipo_ast::BinaryOp::Or) {
                    self.check_condition_bool(left);
                    self.check_condition_bool(right);
                }
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            HirExpr::Call(callee, args, span) => {
                self.analyze_expr(callee);
                for arg in args {
                    self.analyze_expr(&arg.value);
                }

                if let HirExpr::Identifier(name, callee_span) = &**callee {
                    if let Some(msg_opt) = self.deprecated_items.get(name) {
                        let detail = match msg_opt {
                            Some(msg) => format!("call to deprecated function '{name}': {msg}"),
                            None => format!("call to deprecated function '{name}'"),
                        };
                        self.diagnostics.push(
                            Diagnostic::warning(DiagnosticCode::AIPO_SEM_DEPRECATED, detail)
                                .with_primary_span(self.source, *callee_span),
                        );
                    }
                    if let Some(Symbol {
                        kind: SymbolKind::Function { min_args, max_args },
                        ..
                    }) = self.facts.scopes.lookup(self.current_scope, name)
                    {
                        if args.len() < *min_args || args.len() > *max_args {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_ARITY_MISMATCH,
                                    format!(
                                        "function '{}' expected between {} and {} arguments, found {}",
                                        name, min_args, max_args, args.len()
                                    ),
                                )
                                .with_primary_span(self.source, *span),
                            );
                        }
                    }
                    if let Some(contract) = self.fn_contracts.get(name).cloned() {
                        self.check_argument_contracts(name, args, &contract);
                    }
                } else if let HirExpr::Dot(target, member, dot_span) = &**callee {
                    if let HirExpr::Identifier(module_name, _) = &**target {
                        if self.is_host_module_reference(module_name) {
                            self.check_host_call(module_name, member, args, *dot_span);
                        }
                        if let Some(known) = self.enum_variants.get(module_name).cloned() {
                            let full = format!("{module_name}.{member}");
                            if !known.iter().any(|v| v == member) {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                        format!(
                                            "unknown variant '{member}' of enum '{module_name}'"
                                        ),
                                    )
                                    .with_primary_span(self.source, *span),
                                );
                            } else if let Some(expected) = self.enum_tuple_arity.get(&full).copied()
                            {
                                if args.len() != expected {
                                    self.diagnostics.push(
                                        Diagnostic::error(
                                            DiagnosticCode::AIPO_SEM_ARITY_MISMATCH,
                                            format!(
                                                "variant '{member}' of enum '{module_name}' expected {expected} arguments, found {}",
                                                args.len()
                                            ),
                                        )
                                        .with_primary_span(self.source, *span),
                                    );
                                }
                            } else {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_ARITY_MISMATCH,
                                        format!(
                                            "variant '{member}' of enum '{module_name}' is not a tuple variant and cannot be called; use `{full}` or `{full}{{ ... }}`"
                                        ),
                                    )
                                    .with_primary_span(self.source, *span),
                                );
                            }
                        }
                    }
                    let target_struct = match &**target {
                        HirExpr::Identifier(name, _) if name == "self" => {
                            self.current_struct.clone()
                        }
                        HirExpr::Identifier(name, _) => self.var_struct_types.get(name).cloned(),
                        _ => None,
                    };
                    if let Some(st) = target_struct {
                        let key = format!("{}.{}", st, member);
                        if let Some(methods) = self.impl_methods.get(&st) {
                            if let Some(sig) = methods.get(member) {
                                if args.len() < sig.min_args || args.len() > sig.max_args {
                                    self.diagnostics.push(
                                        Diagnostic::error(
                                            DiagnosticCode::AIPO_SEM_ARITY_MISMATCH,
                                            format!(
                                                "method '{}' on '{}' expected between {} and {} arguments, found {}",
                                                member, st, sig.min_args, sig.max_args, args.len()
                                            ),
                                        )
                                        .with_primary_span(self.source, *span),
                                    );
                                }
                            }
                        }
                        if let Some(contract) = self.fn_contracts.get(&key).cloned() {
                            self.check_argument_contracts(&key, args, &contract);
                        }
                    }
                }
            }
            HirExpr::Dot(target, member, span) | HirExpr::QuestionDot(target, member, span) => {
                self.check_member_access(target, member, *span);
                self.analyze_expr(target);
            }
            HirExpr::Index(target, idx, _) => {
                self.analyze_expr(target);
                self.analyze_expr(idx);
            }
            HirExpr::List(items, _) => {
                for i in items {
                    self.analyze_expr(i);
                }
            }
            HirExpr::Dict(pairs, _) => {
                for (k, v) in pairs {
                    self.analyze_expr(k);
                    self.analyze_expr(v);
                }
            }
            HirExpr::Construct(target, fields, span) => {
                if let Some(msg_opt) = self.deprecated_items.get(target) {
                    let detail = match msg_opt {
                        Some(msg) => format!("construction of deprecated struct '{target}': {msg}"),
                        None => format!("construction of deprecated struct '{target}'"),
                    };
                    self.diagnostics.push(
                        Diagnostic::warning(DiagnosticCode::AIPO_SEM_DEPRECATED, detail)
                            .with_primary_span(self.source, *span),
                    );
                }
                let declared_fields = self
                    .facts
                    .scopes
                    .lookup(self.current_scope, target)
                    .and_then(|symbol| match &symbol.kind {
                        SymbolKind::Struct { fields } => Some(fields.clone()),
                        _ => None,
                    });

                if declared_fields.is_none() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                            format!("unknown struct type '{target}' in construction"),
                        )
                        .with_primary_span(self.source, *span),
                    );
                }

                if let Some(known) = declared_fields {
                    let contracts = self
                        .struct_field_contracts
                        .get(target)
                        .cloned()
                        .unwrap_or_default();
                    for (maybe_name, f_expr) in fields {
                        // Positional fields cannot be validated against a name.
                        let Some(name) = maybe_name else {
                            self.analyze_expr(f_expr);
                            continue;
                        };
                        if !known.contains_key(name) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                    format!("struct '{target}' has no field '{name}'"),
                                )
                                .with_primary_span(self.source, *span),
                            );
                        }
                        if let Some(contract) = contracts.get(name) {
                            if literal_violates_contract(f_expr, contract) {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC,
                                        format!(
                                            "field '{name}' of '{target}' cannot satisfy contract '{}'",
                                            contract_label(contract)
                                        ),
                                    )
                                    .with_primary_span(self.source, f_expr.span()),
                                );
                            }
                        }
                        self.analyze_expr(f_expr);
                    }
                } else {
                    for (_, f_expr) in fields {
                        self.analyze_expr(f_expr);
                    }
                }
            }
            HirExpr::Fn(f) => {
                // A closure owns its own return contract: an inner `return` must not be judged
                // against the contract of the function that declares the closure. Its body is
                // deferred code too, so it resolves module bindings without textual order.
                let prev_return_contract = self.current_return_contract.clone();
                let prev_module_flow = self.in_module_flow;
                self.current_return_contract = f.return_type.clone();
                self.in_module_flow = false;
                let prev_await_do = self.in_await_do;
                self.in_await_do = false;
                self.with_block_scope(|this| {
                    for p in &f.params {
                        let mutability = if p.is_mut {
                            Mutability::Mutable
                        } else {
                            Mutability::Immutable
                        };
                        this.facts.scopes.insert(
                            this.current_scope,
                            Symbol {
                                name: p.name.clone(),
                                kind: SymbolKind::Parameter,
                                mutability,
                                span: p.span,
                            },
                        );
                    }
                    for st in &f.body {
                        this.analyze_stmt(st);
                    }
                });
                self.current_return_contract = prev_return_contract;
                self.in_module_flow = prev_module_flow;
                self.in_await_do = prev_await_do;
            }
            HirExpr::If(c, t, e, _) => {
                self.analyze_expr(c);
                self.analyze_expr(t);
                self.analyze_expr(e);
            }
            HirExpr::OrElse(l, r, _) => {
                self.analyze_expr(l);
                self.analyze_expr(r);
            }
        }
    }

    fn check_member_access(&mut self, target: &HirExpr, member: &str, span: SourceSpan) {
        if let HirExpr::Identifier(name, _) = target {
            if self.is_host_module_reference(name) || self.enum_variants.contains_key(name) {
                return;
            }
        }
        let target_type = match target {
            HirExpr::Identifier(name, _) if name == "self" => self.current_struct.clone(),
            HirExpr::Identifier(name, _) => self.var_struct_types.get(name).cloned(),
            HirExpr::List(..) => Some("List".to_string()),
            HirExpr::Literal(Literal::String(..), _) => Some("String".to_string()),
            _ => None,
        };

        if let Some(ty) = target_type {
            match ty.as_str() {
                "List" => {
                    if !LIST_MEMBERS.contains(&member) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                format!("type 'List' has no member '{member}'"),
                            )
                            .with_primary_span(self.source, span),
                        );
                    }
                }
                "String" => {
                    if !STRING_MEMBERS.contains(&member) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                format!("type 'String' has no member '{member}'"),
                            )
                            .with_primary_span(self.source, span),
                        );
                    }
                }
                st => {
                    let has_struct_def = self.struct_fixed_fields.contains_key(st)
                        || self
                            .facts
                            .scopes
                            .lookup(self.current_scope, st)
                            .is_some_and(|sym| matches!(sym.kind, SymbolKind::Struct { .. }));
                    if has_struct_def {
                        let has_field = self
                            .struct_fixed_fields
                            .get(st)
                            .is_some_and(|f| f.contains(member))
                            || self
                                .facts
                                .scopes
                                .lookup(self.current_scope, st)
                                .is_some_and(|sym| match &sym.kind {
                                    SymbolKind::Struct { fields } => fields.contains_key(member),
                                    _ => false,
                                });
                        // Methods declared as `Tipo:nome` live under the declared type
                        // name. An enum payload variant is a struct named `Enum.Variant`,
                        // but its methods are declared on the enum (`Enum:nome`), so fall
                        // back to the parent type before reporting the member as unknown.
                        let has_method = self
                            .impl_methods
                            .get(st)
                            .is_some_and(|methods| methods.contains_key(member))
                            || st.split_once('.').is_some_and(|(parent, _)| {
                                self.impl_methods
                                    .get(parent)
                                    .is_some_and(|methods| methods.contains_key(member))
                            });
                        if !has_field && !has_method {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_SEM_UNKNOWN_NAME,
                                    format!("struct '{st}' has no member '{member}'"),
                                )
                                .with_primary_span(self.source, span),
                            );
                        }
                    }
                }
            }
        }
    }

    fn collect_type_narrowings(
        &self,
        cond: &HirExpr,
        is_positive: bool,
        out: &mut Vec<(String, String)>,
    ) {
        match cond {
            HirExpr::Binary(BinaryOp::Is, left, right, _) if is_positive => {
                if let HirExpr::Identifier(var_name, _) = &**left {
                    match &**right {
                        HirExpr::Identifier(type_name, _) => {
                            out.push((var_name.clone(), type_name.clone()));
                        }
                        HirExpr::Dot(base, member, _) => {
                            if let HirExpr::Identifier(enum_name, _) = &**base {
                                out.push((var_name.clone(), format!("{enum_name}.{member}")));
                            }
                        }
                        _ => {}
                    }
                }
            }
            HirExpr::Unary(UnaryOp::Not, inner, _) => {
                self.collect_type_narrowings(inner, !is_positive, out);
            }
            HirExpr::Binary(BinaryOp::And, left, right, _) if is_positive => {
                self.collect_type_narrowings(left, true, out);
                self.collect_type_narrowings(right, true, out);
            }
            HirExpr::Binary(BinaryOp::Or, left, right, _) if !is_positive => {
                self.collect_type_narrowings(left, false, out);
                self.collect_type_narrowings(right, false, out);
            }
            HirExpr::Binary(BinaryOp::NotEqual, left, right, _) if is_positive => {
                if let (HirExpr::Identifier(var_name, _), HirExpr::Literal(Literal::None, _)) =
                    (&**left, &**right)
                {
                    if let Some(underlying) = self.var_nullable_types.get(var_name) {
                        out.push((var_name.clone(), underlying.clone()));
                    }
                } else if let (
                    HirExpr::Literal(Literal::None, _),
                    HirExpr::Identifier(var_name, _),
                ) = (&**left, &**right)
                {
                    if let Some(underlying) = self.var_nullable_types.get(var_name) {
                        out.push((var_name.clone(), underlying.clone()));
                    }
                }
            }
            HirExpr::Binary(BinaryOp::Equal, left, right, _) if !is_positive => {
                if let (HirExpr::Identifier(var_name, _), HirExpr::Literal(Literal::None, _)) =
                    (&**left, &**right)
                {
                    if let Some(underlying) = self.var_nullable_types.get(var_name) {
                        out.push((var_name.clone(), underlying.clone()));
                    }
                } else if let (
                    HirExpr::Literal(Literal::None, _),
                    HirExpr::Identifier(var_name, _),
                ) = (&**left, &**right)
                {
                    if let Some(underlying) = self.var_nullable_types.get(var_name) {
                        out.push((var_name.clone(), underlying.clone()));
                    }
                }
            }
            HirExpr::Call(callee, args, _) if is_positive => {
                if let HirExpr::Identifier(name, _) = &**callee {
                    if name == "some" && args.len() == 1 {
                        if let HirExpr::Identifier(var_name, _) = &args[0].value {
                            if let Some(underlying) = self.var_nullable_types.get(var_name) {
                                out.push((var_name.clone(), underlying.clone()));
                            }
                        }
                    }
                } else if let HirExpr::Dot(target, member, _) = &**callee {
                    if member == "some" && args.is_empty() {
                        if let HirExpr::Identifier(var_name, _) = &**target {
                            if let Some(underlying) = self.var_nullable_types.get(var_name) {
                                out.push((var_name.clone(), underlying.clone()));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn with_block_scope<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let parent = self.current_scope;
        let block_scope = self.facts.scopes.new_scope(Some(parent));
        self.current_scope = block_scope;
        let prev_var_types = self.var_struct_types.clone();
        let prev_nullable_types = self.var_nullable_types.clone();
        f(self);
        self.var_struct_types = prev_var_types;
        self.var_nullable_types = prev_nullable_types;
        self.current_scope = parent;
    }
}
