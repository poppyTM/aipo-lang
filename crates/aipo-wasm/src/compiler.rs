//! WebAssembly compiler lowering Aipo HIR to standard Wasm binary modules (ADP-013).

use crate::emitter::WasmEmitter;
use crate::error::WasmCompileError;
use crate::types::{WasmFnType, WasmType};
use aipo_ast::{BinaryOp, Literal, UnaryOp};
use aipo_hir::{
    HirAttemptStmt, HirCallArg, HirExpr, HirFunctionDecl, HirIfStmt, HirItem, HirMatchStmt,
    HirParam, HirProgram, HirStmt, HirStructDecl,
};
use aipo_lexer::{parse_float_literal, parse_int_literal};
use aipo_source::SourceSpan;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use wasm_encoder::{BlockType, ConstExpr, Function, GlobalType, Instruction, MemArg, ValType};

/// Represents a frame in the WebAssembly structured control stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlFrame {
    /// Outer block of a loop (target for forward `break`).
    LoopBreak,
    /// Loop header (target for backward `continue`).
    LoopContinue,
    /// Step/increment block of a `repeat` loop (target for `continue`).
    RepeatStep,
    /// Standard structured block (e.g., `if` condition or inline block).
    Block,
    /// Handler block of an `attempt` statement (target for failure propagation).
    AttemptHandler,
}

/// High-level type tag for local variables to disambiguate field accesses, string properties, and function pointers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalKind {
    Unknown,
    None,
    Int,
    Float,
    Bool,
    String,
    Struct(String),
    Fn(u32),
    Task,
    /// A `Range` value produced by the `..` operator.
    Range,
    /// A `List` value, carrying the element kind.
    ///
    /// The element kind is what lets `each x in names` print a string instead of
    /// the pointer it occupies: a list element is staged in an `i64` slot, so the
    /// storage type alone cannot tell a `String` from an `Int`.
    List(Box<LocalKind>),
    /// A `Dict` value, carrying the value kind.
    Dict(Box<LocalKind>),
    /// A `Bytes` value produced by `Bytes(len)`.
    Bytes,
    /// A value whose declared contract is `T?`: it is either a `T` or `none`.
    ///
    /// A type test cannot answer statically for this one, so `is T` narrows the
    /// static part and the runtime `none` sentinel decides the rest.
    Nullable(Box<LocalKind>),
}

/// Function symbols and module bindings relevant to static semantic proofs.
struct FunctionTable {
    symbols: HashMap<String, (u32, u32, WasmFnType)>,
    global_bindings: HashSet<String>,
    declared_types: HashSet<String>,
    declared_vars: HashSet<String>,
    /// Default expressions per parameter, by function name.
    ///
    /// Wasm cannot tell an omitted argument from a passed zero, so an omitted
    /// parameter is filled at the call site with its declared default rather than
    /// with a zero of the representation.
    defaults: HashMap<String, Vec<Option<HirExpr>>>,
    /// Declared parameter names of each function, by the key used in `defaults`.
    ///
    /// A default may name an earlier parameter (`mirror = value`), and the call site
    /// compiles that default, so it needs the callee's names to bind them.
    param_names: HashMap<String, Vec<String>>,
    /// Semantic kind of each function's result, by name.
    ///
    /// `Bool`, `String`, a struct and a `Bytes` handle all occupy `i32`, so the Wasm
    /// representation alone cannot say how to render a returned value. Without this,
    /// `io.println(p.alive())` printed an address instead of `true`.
    return_kinds: HashMap<String, LocalKind>,
    /// Module-scope `let`/`var` bindings, as Wasm globals.
    ///
    /// A module binding is shared by the entry script and every function that
    /// names it, which a per-frame local cannot express. A Wasm global gives the
    /// one storage cell both sides read and write, so `total += n` inside a
    /// function is visible to the script that declared it.
    module_globals: HashMap<String, (u32, WasmType, LocalKind)>,
    /// Struct types that declare an `invariant()` hook.
    invariant_types: HashSet<String>,
    /// Semantic kind of each declared parameter, by function name.
    ///
    /// An unannotated parameter that receives an `i32` may hold a `String`, a
    /// `List`, a `Dict` or a struct, and the representation alone cannot tell them
    /// apart. Without this, `fn size(xs) { return len(xs) }` lowered `len` through
    /// the String path and printed a byte count.
    param_kinds: HashMap<String, Vec<Option<LocalKind>>>,
}

impl FunctionTable {
    fn new(program: &HirProgram) -> Self {
        let mut declared_vars = HashSet::new();
        collect_declared_vars(&program.statements, &mut declared_vars);
        for item in &program.items {
            match item {
                HirItem::Fn(decl) => {
                    declared_vars.insert(decl.name.clone());
                    for param in &decl.params {
                        declared_vars.insert(param.name.clone());
                    }
                    collect_declared_vars(&decl.body, &mut declared_vars);
                }
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        for param in &m.params {
                            declared_vars.insert(param.name.clone());
                        }
                        collect_declared_vars(&m.body, &mut declared_vars);
                    }
                }
                _ => {}
            }
        }
        Self {
            symbols: HashMap::new(),
            global_bindings: program
                .statements
                .iter()
                .filter_map(|statement| match statement {
                    HirStmt::Let(name, ..) | HirStmt::Var(name, ..) => Some(name.clone()),
                    _ => None,
                })
                .collect(),
            declared_types: program
                .items
                .iter()
                .filter_map(|item| match item {
                    HirItem::Struct(declaration) => Some(declaration.name.clone()),
                    HirItem::Enum(declaration) => Some(declaration.name.clone()),
                    _ => None,
                })
                .collect(),
            declared_vars,
            param_names: program
                .items
                .iter()
                .flat_map(|item| match item {
                    HirItem::Fn(decl) => vec![(
                        decl.name.clone(),
                        decl.params.iter().map(|p| p.name.clone()).collect(),
                    )],
                    HirItem::Impl(imp) => imp
                        .init
                        .iter()
                        .map(|init| {
                            (
                                format!("{}.init", imp.target),
                                init.params.iter().map(|p| p.name.clone()).collect(),
                            )
                        })
                        .collect(),
                    _ => Vec::new(),
                })
                .collect(),
            return_kinds: HashMap::new(),
            module_globals: HashMap::new(),
            invariant_types: program
                .items
                .iter()
                .filter_map(|item| match item {
                    HirItem::Impl(imp) if imp.invariant.is_some() => Some(imp.target.clone()),
                    _ => None,
                })
                .collect(),
            param_kinds: HashMap::new(),
            defaults: program
                .items
                .iter()
                .flat_map(|item| match item {
                    HirItem::Fn(decl) => vec![(
                        decl.name.clone(),
                        decl.params
                            .iter()
                            .map(|param| param.default.clone())
                            .collect(),
                    )],
                    // `Type:init` is compiled as `Type.init`, so its defaults belong
                    // under that key; a construction site looks them up the same way a
                    // free-function call does.
                    HirItem::Impl(imp) => imp
                        .init
                        .iter()
                        .map(|init| {
                            (
                                format!("{}.init", imp.target),
                                init.params
                                    .iter()
                                    .map(|param| param.default.clone())
                                    .collect(),
                            )
                        })
                        .collect(),
                    _ => Vec::new(),
                })
                .collect(),
        }
    }

    fn builtin_is_unshadowed(&self, name: &str) -> bool {
        !self.symbols.contains_key(name)
            && !self.global_bindings.contains(name)
            && !self.declared_types.contains(name)
    }
}

impl Deref for FunctionTable {
    type Target = HashMap<String, (u32, u32, WasmFnType)>;
    fn deref(&self) -> &Self::Target {
        &self.symbols
    }
}

impl DerefMut for FunctionTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.symbols
    }
}

fn collect_declared_vars(stmts: &[HirStmt], out: &mut HashSet<String>) {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(name, _, _) | HirStmt::Var(name, _, _) => {
                out.insert(name.clone());
            }
            HirStmt::FnDecl(decl) => {
                out.insert(decl.name.clone());
                for param in &decl.params {
                    out.insert(param.name.clone());
                }
                collect_declared_vars(&decl.body, out);
            }
            HirStmt::If(if_stmt) => {
                collect_declared_vars(&if_stmt.then_branch, out);
                if let Some(e) = &if_stmt.else_branch {
                    collect_declared_vars(e, out);
                }
            }
            HirStmt::Repeat(_, _, body, _) => collect_declared_vars(body, out),
            HirStmt::Each(vars, _, body, _) => {
                for v in vars {
                    out.insert(v.clone());
                }
                collect_declared_vars(body, out);
            }
            HirStmt::Attempt(a) => {
                collect_declared_vars(&a.body, out);
                collect_declared_vars(&a.handler, out);
            }
            HirStmt::Match(m) => {
                for arm in &m.when_arms {
                    collect_declared_vars(&arm.body, out);
                }
                if let Some(e) = &m.else_arm {
                    collect_declared_vars(e, out);
                }
            }
            HirStmt::AwaitDo(s, _) => collect_declared_vars(s, out),
            _ => {}
        }
    }
}

fn annotated_kind(
    annotation: Option<&aipo_ast::TypeAnnotation>,
    structs: &HashMap<String, StructLayout>,
) -> LocalKind {
    let Some(annotation) = annotation else {
        return LocalKind::Unknown;
    };
    if annotation.is_nullable {
        let inner = annotated_kind(
            Some(&aipo_ast::TypeAnnotation {
                is_nullable: false,
                ..annotation.clone()
            }),
            structs,
        );
        return match inner {
            LocalKind::Unknown => LocalKind::Unknown,
            other => LocalKind::Nullable(Box::new(other)),
        };
    }
    match annotation.name.as_str() {
        "Int" => LocalKind::Int,
        "Float" => LocalKind::Float,
        "Bool" => LocalKind::Bool,
        "String" => LocalKind::String,
        "List" => LocalKind::List(Box::new(LocalKind::Unknown)),
        "Dict" => LocalKind::Dict(Box::new(LocalKind::Unknown)),
        "Range" => LocalKind::Range,
        "Task" => LocalKind::Task,
        "Bytes" => LocalKind::Bytes,
        name if structs.contains_key(name) && !name.contains('.') => {
            // An enum parent annotation does not identify the concrete variant.
            if structs
                .keys()
                .any(|variant| variant.starts_with(&format!("{name}.")))
            {
                LocalKind::Unknown
            } else {
                LocalKind::Struct(name.to_string())
            }
        }
        name if structs.contains_key(name) => LocalKind::Struct(name.to_string()),
        _ => LocalKind::Unknown,
    }
}

/// Describes the byte layout of a struct in linear memory.
#[derive(Debug, Clone)]
struct StructLayout {
    size: u32,
    fields: HashMap<String, (u32, WasmType)>,
}

/// Helper containing function indices of built-in host I/O functions.
#[derive(Debug, Clone, Copy)]
pub struct HostIoHelpers {
    /// Function index for `aipo_host.print_int`.
    pub print_int_idx: u32,
    /// Function index for `aipo_host.print_float`.
    pub print_float_idx: u32,
    /// Function index for `aipo_host.print_str`.
    pub print_str_idx: u32,
    /// Function index for `aipo_host.print_bool`.
    pub print_bool_idx: u32,
    /// Function index for `aipo_host.println`.
    pub println_idx: u32,
}

/// Helper containing global indices used by the hybrid `Failure` model.
///
/// A `Failure` never changes the static type of an expression: functions keep
/// returning their declared scalar. Instead a pending failure is recorded in a
/// module-level status flag together with the message pointer, and the statement
/// boundary propagates it exactly like the stack VM's `CheckFailure`.
#[derive(Debug, Clone, Copy)]
struct FailureGlobals {
    /// `i32`: 0 when no failure is pending, 1 otherwise.
    status_idx: u32,
    /// `i32`: pointer into the static string pool holding the failure message.
    message_idx: u32,
}

/// Helper containing function indices of built-in async runtime functions and host I/O helpers.
#[derive(Debug, Clone, Copy)]
struct AsyncHelpers {
    task_create_idx: u32,
    await_idx: u32,
    host_io: Option<HostIoHelpers>,
    /// Failure status/message globals implementing the hybrid failure model.
    fail_globals: FailureGlobals,
    /// Result type of the function currently being compiled.
    ///
    /// A failure that propagates out of the body must leave a value of this type
    /// on the stack, so an expression-level `?` cannot emit a bare `return`.
    function_return: Option<WasmType>,
    /// Globals holding the provisional-mutation journal: the instance, the value
    /// it had before the write, the field offset, and a flag saying an entry is
    /// live.
    journal: JournalGlobals,
    /// Second journal slot, for an operation that mutates two instances.
    journal2: JournalGlobals,
    /// Set while compiling the `invariant()` predicate, which must not re-enter
    /// itself through a boundary check.
    suppress_boundary: bool,
    /// Set while compiling `Type.init`, whose writes are part of construction.
    constructing_type: bool,
}

/// Globals of the provisional-mutation journal.
#[derive(Debug, Clone, Copy)]
struct JournalGlobals {
    instance: u32,
    value: u32,
    offset: u32,
    live: u32,
}

/// Compiles an `HirProgram` into a standard WebAssembly binary module (`.wasm`).
///
/// Functions declared in the program are compiled into Wasm functions and exported
/// under their declared names. Any top-level statements are compiled and exported
/// under the entrypoint symbol `__top_level__`.
///
/// # Errors
///
/// Returns a [`WasmCompileError`] if an unsupported language construct, unknown variable,
/// or type mismatch is encountered.
pub fn compile_hir(program: &HirProgram) -> Result<Vec<u8>, WasmCompileError> {
    let mut emitter = WasmEmitter::new();
    let mut functions = FunctionTable::new(program);
    let mut structs = HashMap::new();

    // Pass 0: Collect Struct and Enum definitions and compute layouts
    for item in &program.items {
        match item {
            HirItem::Struct(decl) => {
                register_struct_layout(decl, &mut structs);
            }
            HirItem::Enum(decl) => {
                for v in &decl.variants {
                    let full_name = format!("{}.{}", decl.name, v.name);
                    let mut fields = HashMap::new();
                    match &v.payload {
                        aipo_hir::HirEnumVariantPayload::Unit => {
                            structs.insert(full_name, StructLayout { size: 8, fields });
                        }
                        aipo_hir::HirEnumVariantPayload::Tuple(t_fields) => {
                            for (idx, f) in t_fields.iter().enumerate() {
                                let f_name = f.name.clone().unwrap_or_else(|| idx.to_string());
                                let offset = 8 + (idx as u32) * 8;
                                fields.insert(f_name, (offset, WasmType::I64));
                            }
                            let size = 8 + (t_fields.len() as u32) * 8;
                            structs.insert(full_name, StructLayout { size, fields });
                        }
                        aipo_hir::HirEnumVariantPayload::Struct(s_fields) => {
                            for (idx, f) in s_fields.iter().enumerate() {
                                let offset = 8 + (idx as u32) * 8;
                                fields.insert(f.name.clone(), (offset, WasmType::I64));
                            }
                            let size = 8 + (s_fields.len() as u32) * 8;
                            structs.insert(full_name, StructLayout { size, fields });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Field representations may also be witnessed by a constructor argument that
    // is a function parameter (`Entry{label = suffix}`). The parameter hints are
    // available before the layouts are final, so refine with them, then recompute
    // the hints against the refined layouts.
    let provisional_hints = collect_param_hints(program, &structs);
    refine_struct_layouts(program, &mut structs, &provisional_hints);

    // Pass 0.2: Collect anonymous functions and assign synthetic names
    let mut anon_map = HashMap::new();
    let mut anon_decls = Vec::new();
    collect_anon_functions(program, &mut anon_map, &mut anon_decls);

    let mut func_decls: Vec<HirFunctionDecl> = Vec::new();
    let mut async_wrappers: Vec<(String, String, Vec<HirParam>)> = Vec::new();
    for item in &program.items {
        match item {
            HirItem::Fn(func) => {
                if func.is_async {
                    let inner_body_name = format!("__async_body_{}", func.name);
                    func_decls.push(HirFunctionDecl {
                        directives: func.directives.clone(),
                        name: inner_body_name.clone(),
                        is_async: false,
                        params: func.params.clone(),
                        return_type: func.return_type.clone(),
                        body: func.body.clone(),
                        span: func.span,
                    });
                    async_wrappers.push((func.name.clone(), inner_body_name, func.params.clone()));
                } else {
                    func_decls.push(func.clone());
                }
            }
            HirItem::Impl(imp) => {
                // `invariant()` is a read-only predicate over `self`, compiled as a
                // function returning Bool so construction can call it and trap on a
                // violation.
                if let Some(inv) = &imp.invariant {
                    func_decls.push(HirFunctionDecl {
                        directives: inv.directives.clone(),
                        name: format!("{}.invariant", imp.target),
                        is_async: false,
                        params: vec![HirParam {
                            name: "self".to_string(),
                            is_mut: false,
                            is_self: true,
                            type_annotation: None,
                            default: None,
                            span: inv.span,
                        }],
                        return_type: Some(aipo_ast::TypeAnnotation {
                            name: "Bool".to_string(),
                            is_nullable: false,
                            span: inv.span,
                        }),
                        body: inv.body.clone(),
                        span: inv.span,
                    });
                }
                if let Some(init) = &imp.init {
                    let full_name = format!("{}.init", imp.target);
                    let mut params = init.params.clone();
                    if !params
                        .first()
                        .is_some_and(|p| p.is_self || p.name == "self")
                    {
                        params.insert(
                            0,
                            HirParam {
                                name: "self".to_string(),
                                is_mut: true,
                                is_self: true,
                                type_annotation: None,
                                default: None,
                                span: init.span,
                            },
                        );
                    }
                    func_decls.push(HirFunctionDecl {
                        directives: init.directives.clone(),
                        name: full_name,
                        is_async: false,
                        params,
                        return_type: init.return_type.clone(),
                        body: init.body.clone(),
                        span: init.span,
                    });
                }
                for method in &imp.methods {
                    let full_name = format!("{}.{}", imp.target, method.name);
                    let mut params = method.params.clone();
                    if !params
                        .first()
                        .is_some_and(|p| p.is_self || p.name == "self")
                    {
                        params.insert(
                            0,
                            HirParam {
                                name: "self".to_string(),
                                is_mut: false,
                                is_self: true,
                                type_annotation: None,
                                default: None,
                                span: method.span,
                            },
                        );
                    }
                    if method.is_async {
                        let inner_body_name = format!("__async_body_{}", full_name);
                        func_decls.push(HirFunctionDecl {
                            directives: method.directives.clone(),
                            name: inner_body_name.clone(),
                            is_async: false,
                            params: params.clone(),
                            return_type: method.return_type.clone(),
                            body: method.body.clone(),
                            span: method.span,
                        });
                        async_wrappers.push((full_name, inner_body_name, params));
                    } else {
                        func_decls.push(HirFunctionDecl {
                            directives: method.directives.clone(),
                            name: full_name,
                            is_async: false,
                            params,
                            return_type: method.return_type.clone(),
                            body: method.body.clone(),
                            span: method.span,
                        });
                    }
                }
            }
            HirItem::Batch(binding) => {
                for function_name in &binding.functions {
                    if let Some(source) = program.items.iter().find_map(|it| match it {
                        HirItem::Fn(f) if &f.name == function_name => Some(f),
                        _ => None,
                    }) && source.params.first().is_some_and(|p| p.is_self)
                    {
                        let full_name = format!("{}.{}", binding.target, function_name);
                        func_decls.push(HirFunctionDecl {
                            directives: source.directives.clone(),
                            name: full_name,
                            is_async: source.is_async,
                            params: source.params.clone(),
                            return_type: source.return_type.clone(),
                            body: source.body.clone(),
                            span: source.span,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    func_decls.extend(anon_decls);

    // Pass 0.5: Linear Memory and Static String Pool
    emitter.enable_memory(1, None);
    emitter.export_memory("memory");

    let mut static_strings = HashMap::new();
    let mut data_segments = Vec::new();
    let mut next_static_offset = 1024i32;

    collect_program_strings(
        program,
        &func_decls,
        &mut static_strings,
        &mut data_segments,
        &mut next_static_offset,
    );

    // A type with an `invariant()` names itself in the violation message, so its
    // name needs a pool entry even though no literal in the program spells it.
    for item in &program.items {
        if let HirItem::Impl(imp) = item
            && imp.invariant.is_some()
        {
            add_static_string(
                &imp.target,
                &mut static_strings,
                &mut data_segments,
                &mut next_static_offset,
            );
        }
    }

    // `Int(text)` reports the same wording the VM uses, which quotes the offending
    // text. The two fragments are interned here because they belong to the helper
    // rather than to any expression in the program.
    for text in [
        INT_PARSE_PREFIX,
        "\"",
        "math.clamp bounds are inverted",
        INVARIANT_PREFIX,
        INVARIANT_SUFFIX,
    ] {
        add_static_string(
            text,
            &mut static_strings,
            &mut data_segments,
            &mut next_static_offset,
        );
    }

    for (offset, bytes) in data_segments {
        emitter.add_data_segment(0, offset, bytes);
    }

    // Global 0: Dynamic Heap Pointer (`__aipo_heap_ptr`)
    let heap_start = next_static_offset;
    emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(heap_start),
    );

    // Global 1: Virtual Time in milliseconds (`__aipo_virtual_time`)
    let virtual_time_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I64,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i64_const(0),
    );
    emitter.export_global("__aipo_virtual_time", virtual_time_idx);

    // Global 2: Failure status flag (`__aipo_failure_status`)
    let failure_status_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );
    emitter.export_global("__aipo_failure_status", failure_status_idx);

    // Global 3: Failure message pointer (`__aipo_failure_msg`)
    let failure_msg_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );
    emitter.export_global("__aipo_failure_msg", failure_msg_idx);

    let failure_globals = FailureGlobals {
        status_idx: failure_status_idx,
        message_idx: failure_msg_idx,
    };

    // Optional Host I/O Imports (Milestone 6 / ADP-013)
    let has_io = scan_program_for_io(program);
    let host_io = if has_io {
        let print_int_ty = emitter.add_type(WasmFnType::new(vec![WasmType::I64], vec![]));
        let print_int_idx = emitter.add_import_func("aipo_host", "print_int", print_int_ty);

        let print_float_ty = emitter.add_type(WasmFnType::new(vec![WasmType::F64], vec![]));
        let print_float_idx = emitter.add_import_func("aipo_host", "print_float", print_float_ty);

        let print_str_ty = emitter.add_type(WasmFnType::new(vec![WasmType::I32], vec![]));
        let print_str_idx = emitter.add_import_func("aipo_host", "print_str", print_str_ty);

        let print_bool_ty = emitter.add_type(WasmFnType::new(vec![WasmType::I32], vec![]));
        let print_bool_idx = emitter.add_import_func("aipo_host", "print_bool", print_bool_ty);

        let println_ty = emitter.add_type(WasmFnType::new(vec![], vec![]));
        let println_idx = emitter.add_import_func("aipo_host", "println", println_ty);

        Some(HostIoHelpers {
            print_int_idx,
            print_float_idx,
            print_str_idx,
            print_bool_idx,
            println_idx,
        })
    } else {
        None
    };

    // Function 0: Built-in Allocator `__aipo_alloc(size: i32) -> i32`
    let alloc_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let alloc_type_idx = emitter.add_type(alloc_fn_type.clone());
    let alloc_fn = build_allocator_function();
    let alloc_func_idx = emitter.add_function(alloc_type_idx, alloc_fn);
    emitter.export_function("__aipo_alloc", alloc_func_idx);
    functions.insert(
        "__aipo_alloc".to_string(),
        (alloc_func_idx, alloc_type_idx, alloc_fn_type),
    );

    // Pre-register standard indirect call signatures for arities 0..=8
    let mut indirect_sigs: HashMap<usize, u32> = HashMap::new();
    for arity in 0..=8 {
        let sig = WasmFnType::new(vec![WasmType::I64; arity], vec![WasmType::I64]);
        let t_idx = emitter.add_type(sig);
        indirect_sigs.insert(arity, t_idx);
    }

    // Callback signature for predicates that return Bool.
    let callback_bool_type_idx =
        emitter.add_type(WasmFnType::new(vec![WasmType::I64], vec![WasmType::I32]));

    // Built-in Async Runtime Functions (Milestone 5 / ADP-013)
    // Function 1: `__aipo_task_create(fn_table_idx: i32, arg_count: i32, arg0: i64, arg1: i64) -> i32`
    let task_create_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I64, WasmType::I64],
        vec![WasmType::I32],
    );
    let task_create_type_idx = emitter.add_type(task_create_fn_type.clone());
    let task_create_fn = build_task_create_function(alloc_func_idx);
    let task_create_func_idx = emitter.add_function(task_create_type_idx, task_create_fn);
    emitter.export_function("__aipo_task_create", task_create_func_idx);
    functions.insert(
        "__aipo_task_create".to_string(),
        (
            task_create_func_idx,
            task_create_type_idx,
            task_create_fn_type,
        ),
    );

    // Function 2: `__aipo_task_drive(task_ptr: i32) -> i64`
    let task_drive_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let task_drive_type_idx = emitter.add_type(task_drive_fn_type.clone());
    let task_drive_fn =
        build_task_drive_function(indirect_sigs[&0], indirect_sigs[&1], indirect_sigs[&2]);
    let task_drive_func_idx = emitter.add_function(task_drive_type_idx, task_drive_fn);
    emitter.export_function("__aipo_task_drive", task_drive_func_idx);
    functions.insert(
        "__aipo_task_drive".to_string(),
        (task_drive_func_idx, task_drive_type_idx, task_drive_fn_type),
    );

    // Function 3: `__aipo_await(task_ptr: i32) -> i64`
    let await_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let await_type_idx = emitter.add_type(await_fn_type.clone());
    let await_fn = build_await_function(task_drive_func_idx);
    let await_func_idx = emitter.add_function(await_type_idx, await_fn);
    emitter.export_function("__aipo_await", await_func_idx);
    functions.insert(
        "__aipo_await".to_string(),
        (await_func_idx, await_type_idx, await_fn_type),
    );

    // Function 4: `__aipo_task_sleep(duration_ms: i64) -> i64`
    let task_sleep_fn_type = WasmFnType::new(vec![WasmType::I64], vec![WasmType::I64]);
    let task_sleep_type_idx = emitter.add_type(task_sleep_fn_type.clone());
    let task_sleep_fn = build_task_sleep_function();
    let task_sleep_func_idx = emitter.add_function(task_sleep_type_idx, task_sleep_fn);
    emitter.export_function("__aipo_task_sleep", task_sleep_func_idx);
    functions.insert(
        "__aipo_task_sleep".to_string(),
        (task_sleep_func_idx, task_sleep_type_idx, task_sleep_fn_type),
    );

    // Function 5: `__aipo_task_cancel(task_ptr: i32) -> i64`
    let task_cancel_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let task_cancel_type_idx = emitter.add_type(task_cancel_fn_type.clone());
    let task_cancel_fn = build_task_cancel_function();
    let task_cancel_func_idx = emitter.add_function(task_cancel_type_idx, task_cancel_fn);
    emitter.export_function("__aipo_task_cancel", task_cancel_func_idx);
    functions.insert(
        "__aipo_task_cancel".to_string(),
        (
            task_cancel_func_idx,
            task_cancel_type_idx,
            task_cancel_fn_type,
        ),
    );

    // Function 6: `__aipo_range_new(start: i64, end: i64) -> i32`
    let range_new_fn_type =
        WasmFnType::new(vec![WasmType::I64, WasmType::I64], vec![WasmType::I32]);
    let range_new_type_idx = emitter.add_type(range_new_fn_type.clone());
    let range_new_fn = build_range_new_function(alloc_func_idx);
    let range_new_func_idx = emitter.add_function(range_new_type_idx, range_new_fn);
    emitter.export_function("__aipo_range_new", range_new_func_idx);
    functions.insert(
        "__aipo_range_new".to_string(),
        (range_new_func_idx, range_new_type_idx, range_new_fn_type),
    );

    // Function 7: `__aipo_range_len(range_ptr: i32) -> i64`
    let range_len_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let range_len_type_idx = emitter.add_type(range_len_fn_type.clone());
    let range_len_fn = build_range_len_function();
    let range_len_func_idx = emitter.add_function(range_len_type_idx, range_len_fn);
    emitter.export_function("__aipo_range_len", range_len_func_idx);
    functions.insert(
        "__aipo_range_len".to_string(),
        (range_len_func_idx, range_len_type_idx, range_len_fn_type),
    );

    // Function 8: `__aipo_list_new(len: i32) -> i32`
    let list_new_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let list_new_type_idx = emitter.add_type(list_new_fn_type.clone());
    let list_new_fn = build_list_new_function(alloc_func_idx);
    let list_new_func_idx = emitter.add_function(list_new_type_idx, list_new_fn);
    emitter.export_function("__aipo_list_new", list_new_func_idx);
    functions.insert(
        "__aipo_list_new".to_string(),
        (list_new_func_idx, list_new_type_idx, list_new_fn_type),
    );

    // Function 9: `__aipo_list_get(list_ptr: i32, index: i32) -> i64`
    let list_get_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I64]);
    let list_get_type_idx = emitter.add_type(list_get_fn_type.clone());
    let list_get_fn = build_list_get_function();
    let list_get_func_idx = emitter.add_function(list_get_type_idx, list_get_fn);
    emitter.export_function("__aipo_list_get", list_get_func_idx);
    functions.insert(
        "__aipo_list_get".to_string(),
        (list_get_func_idx, list_get_type_idx, list_get_fn_type),
    );

    // Function 10: `__aipo_list_set(list_ptr: i32, index: i32, value: i64) -> i64`
    let list_set_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I64],
        vec![WasmType::I64],
    );
    let list_set_type_idx = emitter.add_type(list_set_fn_type.clone());
    let list_set_fn = build_list_set_function();
    let list_set_func_idx = emitter.add_function(list_set_type_idx, list_set_fn);
    emitter.export_function("__aipo_list_set", list_set_func_idx);
    functions.insert(
        "__aipo_list_set".to_string(),
        (list_set_func_idx, list_set_type_idx, list_set_fn_type),
    );

    // Function 11: `__aipo_list_len(list_ptr: i32) -> i64`
    let list_len_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let list_len_type_idx = emitter.add_type(list_len_fn_type.clone());
    let list_len_fn = build_list_len_function();
    let list_len_func_idx = emitter.add_function(list_len_type_idx, list_len_fn);
    emitter.export_function("__aipo_list_len", list_len_func_idx);
    functions.insert(
        "__aipo_list_len".to_string(),
        (list_len_func_idx, list_len_type_idx, list_len_fn_type),
    );

    // Function 11.5: `__aipo_list_add(list_ptr: i32, value: i64) -> i64`
    let list_add_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I64]);
    let list_add_type_idx = emitter.add_type(list_add_fn_type.clone());
    let list_add_fn = build_list_add_function(alloc_func_idx);
    let list_add_func_idx = emitter.add_function(list_add_type_idx, list_add_fn);
    emitter.export_function("__aipo_list_add", list_add_func_idx);
    functions.insert(
        "__aipo_list_add".to_string(),
        (list_add_func_idx, list_add_type_idx, list_add_fn_type),
    );

    // Function 11.6: `__aipo_list_contains(list_ptr: i32, value: i64) -> i32`
    let list_contains_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I32]);
    let list_contains_type_idx = emitter.add_type(list_contains_fn_type.clone());
    let list_contains_fn = build_list_contains_function();
    let list_contains_func_idx = emitter.add_function(list_contains_type_idx, list_contains_fn);
    emitter.export_function("__aipo_list_contains", list_contains_func_idx);
    functions.insert(
        "__aipo_list_contains".to_string(),
        (
            list_contains_func_idx,
            list_contains_type_idx,
            list_contains_fn_type,
        ),
    );

    // Function 11.7: `__aipo_list_insert(list_ptr: i32, index: i32, value: i64) -> i64`
    let list_insert_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I64],
        vec![WasmType::I64],
    );
    let list_insert_type_idx = emitter.add_type(list_insert_fn_type.clone());
    let list_insert_fn = build_list_insert_function(
        list_add_func_idx,
        list_len_func_idx,
        list_get_func_idx,
        list_set_func_idx,
    );
    let list_insert_func_idx = emitter.add_function(list_insert_type_idx, list_insert_fn);
    emitter.export_function("__aipo_list_insert", list_insert_func_idx);
    functions.insert(
        "__aipo_list_insert".to_string(),
        (
            list_insert_func_idx,
            list_insert_type_idx,
            list_insert_fn_type,
        ),
    );

    // Function 11.8: `__aipo_list_remove_last(list_ptr: i32) -> i64`
    let list_remove_last_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let list_remove_last_type_idx = emitter.add_type(list_remove_last_fn_type.clone());
    let list_remove_last_fn = build_list_remove_last_function();
    let list_remove_last_func_idx =
        emitter.add_function(list_remove_last_type_idx, list_remove_last_fn);
    emitter.export_function("__aipo_list_remove_last", list_remove_last_func_idx);
    functions.insert(
        "__aipo_list_remove_last".to_string(),
        (
            list_remove_last_func_idx,
            list_remove_last_type_idx,
            list_remove_last_fn_type,
        ),
    );

    // Function 11.9: `__aipo_list_remove_at(list_ptr: i32, index: i32) -> i64`
    let list_remove_at_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I64]);
    let list_remove_at_type_idx = emitter.add_type(list_remove_at_fn_type.clone());
    let list_remove_at_fn = build_list_remove_at_function();
    let list_remove_at_func_idx = emitter.add_function(list_remove_at_type_idx, list_remove_at_fn);
    emitter.export_function("__aipo_list_remove_at", list_remove_at_func_idx);
    functions.insert(
        "__aipo_list_remove_at".to_string(),
        (
            list_remove_at_func_idx,
            list_remove_at_type_idx,
            list_remove_at_fn_type,
        ),
    );

    // Function 11.10: `__aipo_list_remove(list_ptr: i32, value: i64) -> i64`
    let list_remove_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I64]);
    let list_remove_type_idx = emitter.add_type(list_remove_fn_type.clone());
    let list_remove_fn = build_list_remove_function(list_remove_at_func_idx);
    let list_remove_func_idx = emitter.add_function(list_remove_type_idx, list_remove_fn);
    emitter.export_function("__aipo_list_remove", list_remove_func_idx);
    functions.insert(
        "__aipo_list_remove".to_string(),
        (
            list_remove_func_idx,
            list_remove_type_idx,
            list_remove_fn_type,
        ),
    );

    // Function 11.11: `__aipo_list_clear(list_ptr: i32) -> i64`
    let list_clear_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let list_clear_type_idx = emitter.add_type(list_clear_fn_type.clone());
    let list_clear_fn = build_list_clear_function();
    let list_clear_func_idx = emitter.add_function(list_clear_type_idx, list_clear_fn);
    emitter.export_function("__aipo_list_clear", list_clear_func_idx);
    functions.insert(
        "__aipo_list_clear".to_string(),
        (list_clear_func_idx, list_clear_type_idx, list_clear_fn_type),
    );

    // Function 11.12: `__aipo_list_count(list_ptr: i32, value: i64) -> i64`
    let list_count_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I64]);
    let list_count_type_idx = emitter.add_type(list_count_fn_type.clone());
    let list_count_fn = build_list_count_function();
    let list_count_func_idx = emitter.add_function(list_count_type_idx, list_count_fn);
    emitter.export_function("__aipo_list_count", list_count_func_idx);
    functions.insert(
        "__aipo_list_count".to_string(),
        (list_count_func_idx, list_count_type_idx, list_count_fn_type),
    );

    // Function 11.13: `__aipo_list_find(list_ptr: i32, value: i64) -> i64`
    let list_find_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I64]);
    let list_find_type_idx = emitter.add_type(list_find_fn_type.clone());
    let list_find_fn = build_list_find_function();
    let list_find_func_idx = emitter.add_function(list_find_type_idx, list_find_fn);
    emitter.export_function("__aipo_list_find", list_find_func_idx);
    functions.insert(
        "__aipo_list_find".to_string(),
        (list_find_func_idx, list_find_type_idx, list_find_fn_type),
    );

    // Function 11.14: `__aipo_list_reverse(list_ptr: i32) -> i32`
    let list_reverse_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let list_reverse_type_idx = emitter.add_type(list_reverse_fn_type.clone());
    let list_reverse_fn = build_list_reverse_function();
    let list_reverse_func_idx = emitter.add_function(list_reverse_type_idx, list_reverse_fn);
    emitter.export_function("__aipo_list_reverse", list_reverse_func_idx);
    functions.insert(
        "__aipo_list_reverse".to_string(),
        (
            list_reverse_func_idx,
            list_reverse_type_idx,
            list_reverse_fn_type,
        ),
    );

    // Function 11.14: `__aipo_list_slice(list_ptr: i32, start: i32, end: i32) -> i32`
    let list_slice_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I32],
        vec![WasmType::I32],
    );
    let list_slice_type_idx = emitter.add_type(list_slice_fn_type.clone());
    let list_slice_fn =
        build_list_slice_function(list_new_func_idx, list_add_func_idx, list_get_func_idx);
    let list_slice_func_idx = emitter.add_function(list_slice_type_idx, list_slice_fn);
    emitter.export_function("__aipo_list_slice", list_slice_func_idx);
    functions.insert(
        "__aipo_list_slice".to_string(),
        (list_slice_func_idx, list_slice_type_idx, list_slice_fn_type),
    );

    // Function 11.15: `__aipo_str_char(ptr: i32, index: i32) -> i32`
    let str_char_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let str_char_type_idx = emitter.add_type(str_char_fn_type.clone());
    let str_char_fn = build_str_char_function(alloc_func_idx);
    let str_char_func_idx = emitter.add_function(str_char_type_idx, str_char_fn);
    emitter.export_function("__aipo_str_char", str_char_func_idx);
    functions.insert(
        "__aipo_str_char".to_string(),
        (str_char_func_idx, str_char_type_idx, str_char_fn_type),
    );

    // Function 11.16: `__aipo_str_slice(ptr: i32, start: i32, end: i32) -> i32`
    let str_slice_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I32],
        vec![WasmType::I32],
    );
    let str_slice_type_idx = emitter.add_type(str_slice_fn_type.clone());
    let str_slice_fn = build_str_slice_function(alloc_func_idx);
    let str_slice_func_idx = emitter.add_function(str_slice_type_idx, str_slice_fn);
    emitter.export_function("__aipo_str_slice", str_slice_func_idx);
    functions.insert(
        "__aipo_str_slice".to_string(),
        (str_slice_func_idx, str_slice_type_idx, str_slice_fn_type),
    );

    // Function 11.17: `__aipo_str_len(ptr: i32) -> i64`
    let str_len_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let str_len_type_idx = emitter.add_type(str_len_fn_type.clone());
    let str_len_fn = build_str_len_function();
    let str_len_func_idx = emitter.add_function(str_len_type_idx, str_len_fn);
    emitter.export_function("__aipo_str_len", str_len_func_idx);
    functions.insert(
        "__aipo_str_len".to_string(),
        (str_len_func_idx, str_len_type_idx, str_len_fn_type),
    );

    // Function 11.15: `__aipo_list_transform(list_ptr: i32, callback: i32) -> i32`
    let list_transform_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let list_transform_type_idx = emitter.add_type(list_transform_fn_type.clone());
    let list_transform_fn = build_list_transform_function(
        list_new_func_idx,
        list_len_func_idx,
        list_get_func_idx,
        list_set_func_idx,
        indirect_sigs[&1],
    );
    let list_transform_func_idx = emitter.add_function(list_transform_type_idx, list_transform_fn);
    emitter.export_function("__aipo_list_transform", list_transform_func_idx);
    functions.insert(
        "__aipo_list_transform".to_string(),
        (
            list_transform_func_idx,
            list_transform_type_idx,
            list_transform_fn_type,
        ),
    );

    // Function 11.16: `__aipo_list_filter(list_ptr: i32, callback: i32) -> i32`
    let list_filter_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let list_filter_type_idx = emitter.add_type(list_filter_fn_type.clone());
    let list_filter_fn = build_list_filter_function(
        list_new_func_idx,
        list_len_func_idx,
        list_get_func_idx,
        list_add_func_idx,
        callback_bool_type_idx,
    );
    let list_filter_func_idx = emitter.add_function(list_filter_type_idx, list_filter_fn);
    emitter.export_function("__aipo_list_filter", list_filter_func_idx);
    functions.insert(
        "__aipo_list_filter".to_string(),
        (
            list_filter_func_idx,
            list_filter_type_idx,
            list_filter_fn_type,
        ),
    );

    // Function 11.17: `__aipo_list_sort(list_ptr: i32) -> i32`
    let list_sort_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let list_sort_type_idx = emitter.add_type(list_sort_fn_type.clone());
    let list_sort_fn = build_list_sort_function();
    let list_sort_func_idx = emitter.add_function(list_sort_type_idx, list_sort_fn);
    emitter.export_function("__aipo_list_sort", list_sort_func_idx);
    functions.insert(
        "__aipo_list_sort".to_string(),
        (list_sort_func_idx, list_sort_type_idx, list_sort_fn_type),
    );

    // Function 11.18: `__aipo_list_sort_by(list_ptr: i32, callback: i32) -> i32`
    let list_sort_by_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let list_sort_by_type_idx = emitter.add_type(list_sort_by_fn_type.clone());
    let list_sort_by_fn = build_list_sort_by_function(indirect_sigs[&1]);
    let list_sort_by_func_idx = emitter.add_function(list_sort_by_type_idx, list_sort_by_fn);
    emitter.export_function("__aipo_list_sort_by", list_sort_by_func_idx);
    functions.insert(
        "__aipo_list_sort_by".to_string(),
        (
            list_sort_by_func_idx,
            list_sort_by_type_idx,
            list_sort_by_fn_type,
        ),
    );

    // Function 12: `__aipo_range_get(range_ptr: i32, index: i32) -> i64`
    let range_get_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I64]);
    let range_get_type_idx = emitter.add_type(range_get_fn_type.clone());
    let range_get_fn = build_range_get_function();
    let range_get_func_idx = emitter.add_function(range_get_type_idx, range_get_fn);
    emitter.export_function("__aipo_range_get", range_get_func_idx);
    functions.insert(
        "__aipo_range_get".to_string(),
        (range_get_func_idx, range_get_type_idx, range_get_fn_type),
    );

    // Function 13: `__aipo_dict_new(capacity: i32) -> i32`
    let dict_new_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let dict_new_type_idx = emitter.add_type(dict_new_fn_type.clone());
    let dict_new_fn = build_dict_new_function(alloc_func_idx);
    let dict_new_func_idx = emitter.add_function(dict_new_type_idx, dict_new_fn);
    emitter.export_function("__aipo_dict_new", dict_new_func_idx);
    functions.insert(
        "__aipo_dict_new".to_string(),
        (dict_new_func_idx, dict_new_type_idx, dict_new_fn_type),
    );

    // Function 14: `__aipo_dict_set(dict: i32, hash: i64, raw_key: i64, value: i64) -> i64`
    //
    // Both keys travel: the hash answers lookup, the raw key is what `keys()` and
    // `each key, value in dict` hand back, because a hash cannot be reversed.
    let dict_set_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I64, WasmType::I64, WasmType::I64],
        vec![WasmType::I64],
    );
    let dict_set_type_idx = emitter.add_type(dict_set_fn_type.clone());
    let dict_set_fn = build_dict_set_function();
    let dict_set_func_idx = emitter.add_function(dict_set_type_idx, dict_set_fn);
    emitter.export_function("__aipo_dict_set", dict_set_func_idx);
    functions.insert(
        "__aipo_dict_set".to_string(),
        (dict_set_func_idx, dict_set_type_idx, dict_set_fn_type),
    );

    // Function 15: `__aipo_dict_get(dict: i32, key: i64) -> i64`
    let dict_get_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I64]);
    let dict_get_type_idx = emitter.add_type(dict_get_fn_type.clone());
    let dict_get_fn = build_dict_get_function();
    let dict_get_func_idx = emitter.add_function(dict_get_type_idx, dict_get_fn);
    emitter.export_function("__aipo_dict_get", dict_get_func_idx);
    functions.insert(
        "__aipo_dict_get".to_string(),
        (dict_get_func_idx, dict_get_type_idx, dict_get_fn_type),
    );

    // Function 16: `__aipo_dict_len(dict: i32) -> i64`
    let dict_len_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let dict_len_type_idx = emitter.add_type(dict_len_fn_type.clone());
    let dict_len_fn = build_dict_len_function();
    let dict_len_func_idx = emitter.add_function(dict_len_type_idx, dict_len_fn);
    emitter.export_function("__aipo_dict_len", dict_len_func_idx);
    functions.insert(
        "__aipo_dict_len".to_string(),
        (dict_len_func_idx, dict_len_type_idx, dict_len_fn_type),
    );

    // Function 17: `__aipo_dict_has(dict: i32, key: i64) -> i32`
    let dict_has_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I32]);
    let dict_has_type_idx = emitter.add_type(dict_has_fn_type.clone());
    let dict_has_fn = build_dict_has_function();
    let dict_has_func_idx = emitter.add_function(dict_has_type_idx, dict_has_fn);
    emitter.export_function("__aipo_dict_has", dict_has_func_idx);
    functions.insert(
        "__aipo_dict_has".to_string(),
        (dict_has_func_idx, dict_has_type_idx, dict_has_fn_type),
    );

    // Function 17.1: `__aipo_dict_is_empty(dict: i32) -> i32`
    let dict_is_empty_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let dict_is_empty_type_idx = emitter.add_type(dict_is_empty_fn_type.clone());
    let dict_is_empty_fn = build_dict_is_empty_function();
    let dict_is_empty_func_idx = emitter.add_function(dict_is_empty_type_idx, dict_is_empty_fn);
    emitter.export_function("__aipo_dict_is_empty", dict_is_empty_func_idx);
    functions.insert(
        "__aipo_dict_is_empty".to_string(),
        (
            dict_is_empty_func_idx,
            dict_is_empty_type_idx,
            dict_is_empty_fn_type,
        ),
    );

    // Function 17.2: `__aipo_dict_key_at(dict: i32, index: i32) -> i64`
    let dict_key_at_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I64]);
    let dict_key_at_type_idx = emitter.add_type(dict_key_at_fn_type.clone());
    let dict_key_at_fn = build_dict_key_at_function();
    let dict_key_at_func_idx = emitter.add_function(dict_key_at_type_idx, dict_key_at_fn);
    emitter.export_function("__aipo_dict_key_at", dict_key_at_func_idx);
    functions.insert(
        "__aipo_dict_key_at".to_string(),
        (
            dict_key_at_func_idx,
            dict_key_at_type_idx,
            dict_key_at_fn_type,
        ),
    );

    // Function 17.3: `__aipo_dict_value_at(dict: i32, index: i32) -> i64`
    let dict_value_at_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I64]);
    let dict_value_at_type_idx = emitter.add_type(dict_value_at_fn_type.clone());
    let dict_value_at_fn = build_dict_value_at_function();
    let dict_value_at_func_idx = emitter.add_function(dict_value_at_type_idx, dict_value_at_fn);
    emitter.export_function("__aipo_dict_value_at", dict_value_at_func_idx);
    functions.insert(
        "__aipo_dict_value_at".to_string(),
        (
            dict_value_at_func_idx,
            dict_value_at_type_idx,
            dict_value_at_fn_type,
        ),
    );

    // Function 17.2: `__aipo_dict_remove(dict: i32, key: i64) -> i64`
    let dict_remove_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I64]);
    let dict_remove_type_idx = emitter.add_type(dict_remove_fn_type.clone());
    let dict_remove_fn = build_dict_remove_function();
    let dict_remove_func_idx = emitter.add_function(dict_remove_type_idx, dict_remove_fn);
    emitter.export_function("__aipo_dict_remove", dict_remove_func_idx);
    functions.insert(
        "__aipo_dict_remove".to_string(),
        (
            dict_remove_func_idx,
            dict_remove_type_idx,
            dict_remove_fn_type,
        ),
    );

    // Function 17.3: `__aipo_dict_keys(dict: i32) -> i32`
    let dict_keys_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let dict_keys_type_idx = emitter.add_type(dict_keys_fn_type.clone());
    let dict_keys_fn = build_dict_keys_function(list_new_func_idx, list_set_func_idx);
    let dict_keys_func_idx = emitter.add_function(dict_keys_type_idx, dict_keys_fn);
    emitter.export_function("__aipo_dict_keys", dict_keys_func_idx);
    functions.insert(
        "__aipo_dict_keys".to_string(),
        (dict_keys_func_idx, dict_keys_type_idx, dict_keys_fn_type),
    );

    // Function 17.4: `__aipo_dict_values(dict: i32) -> i32`
    let dict_values_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let dict_values_type_idx = emitter.add_type(dict_values_fn_type.clone());
    let dict_values_fn = build_dict_values_function(list_new_func_idx, list_set_func_idx);
    let dict_values_func_idx = emitter.add_function(dict_values_type_idx, dict_values_fn);
    emitter.export_function("__aipo_dict_values", dict_values_func_idx);
    functions.insert(
        "__aipo_dict_values".to_string(),
        (
            dict_values_func_idx,
            dict_values_type_idx,
            dict_values_fn_type,
        ),
    );

    // Function 18: `__aipo_string_hash(ptr: i32) -> i64`
    let string_hash_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let string_hash_type_idx = emitter.add_type(string_hash_fn_type.clone());
    let string_hash_fn = build_string_hash_function();
    let string_hash_func_idx = emitter.add_function(string_hash_type_idx, string_hash_fn);
    emitter.export_function("__aipo_string_hash", string_hash_func_idx);
    functions.insert(
        "__aipo_string_hash".to_string(),
        (
            string_hash_func_idx,
            string_hash_type_idx,
            string_hash_fn_type,
        ),
    );

    // Function 19: `__aipo_str_concat(ptr_a: i32, ptr_b: i32) -> i32`
    let str_concat_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let str_concat_type_idx = emitter.add_type(str_concat_fn_type.clone());
    let str_concat_fn = build_string_concat_function(alloc_func_idx);
    let str_concat_func_idx = emitter.add_function(str_concat_type_idx, str_concat_fn);
    emitter.export_function("__aipo_str_concat", str_concat_func_idx);
    functions.insert(
        "__aipo_str_concat".to_string(),
        (str_concat_func_idx, str_concat_type_idx, str_concat_fn_type),
    );

    // Globals for the provisional-mutation journal.
    //
    // A guarded type validates at a stable boundary, so a write is provisional
    // until the boundary commits. The journal holds the most recent write; canon
    // rolls every participating instance back, and a function body touches at
    // most one field per instance per operation in the fixtures this backend
    // supports.
    let journal_instance_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );
    emitter.export_global("__aipo_journal_instance", journal_instance_idx);
    let journal_value_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I64,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i64_const(0),
    );
    emitter.export_global("__aipo_journal_value", journal_value_idx);
    let journal_offset_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(-1),
    );
    emitter.export_global("__aipo_journal_offset", journal_offset_idx);
    let journal_type_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );
    emitter.export_global("__aipo_journal_type", journal_type_idx);

    // A second slot, so an operation over two guarded instances can restore both.
    let journal_instance_2_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );
    let journal_value_2_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I64,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i64_const(0),
    );
    let journal_offset_2_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(-1),
    );
    let journal_live_2_idx = emitter.add_global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );

    // Function 19.5: `__aipo_str_to_int(ptr: i32) -> i64`
    let str_to_int_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let str_to_int_type_idx = emitter.add_type(str_to_int_fn_type.clone());
    let str_to_int_fn = build_str_to_int_function(
        failure_globals.status_idx,
        failure_globals.message_idx,
        str_concat_func_idx,
        static_strings
            .get(INT_PARSE_PREFIX)
            .copied()
            .unwrap_or_default(),
        static_strings.get("\"").copied().unwrap_or_default(),
    );
    let str_to_int_func_idx = emitter.add_function(str_to_int_type_idx, str_to_int_fn);
    emitter.export_function("__aipo_str_to_int", str_to_int_func_idx);
    functions.insert(
        "__aipo_str_to_int".to_string(),
        (str_to_int_func_idx, str_to_int_type_idx, str_to_int_fn_type),
    );

    // Function 19.9f: `__aipo_str_join(sep: i32, list: i32) -> i32`
    let str_join_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let str_join_type_idx = emitter.add_type(str_join_fn_type.clone());
    let str_join_fn =
        build_str_join_function(list_len_func_idx, list_get_func_idx, str_concat_func_idx);
    let str_join_func_idx = emitter.add_function(str_join_type_idx, str_join_fn);
    emitter.export_function("__aipo_str_join", str_join_func_idx);
    functions.insert(
        "__aipo_str_join".to_string(),
        (str_join_func_idx, str_join_type_idx, str_join_fn_type),
    );

    // Function 19.9d: `__aipo_str_copy(src: i32, src_off: i32, len: i32) -> i32`
    let str_copy_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I32],
        vec![WasmType::I32],
    );
    let str_copy_type_idx = emitter.add_type(str_copy_fn_type.clone());
    let str_copy_fn = build_str_copy_function(alloc_func_idx);
    let str_copy_func_idx = emitter.add_function(str_copy_type_idx, str_copy_fn);
    emitter.export_function("__aipo_str_copy", str_copy_func_idx);
    functions.insert(
        "__aipo_str_copy".to_string(),
        (str_copy_func_idx, str_copy_type_idx, str_copy_fn_type),
    );

    // Function 19.9g: `__aipo_str_replace(text: i32, from: i32, to: i32) -> i32`
    let str_replace_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I32],
        vec![WasmType::I32],
    );
    let str_replace_type_idx = emitter.add_type(str_replace_fn_type.clone());
    let str_replace_fn = build_str_replace_function(str_copy_func_idx, str_concat_func_idx);
    let str_replace_func_idx = emitter.add_function(str_replace_type_idx, str_replace_fn);
    emitter.export_function("__aipo_str_replace", str_replace_func_idx);
    functions.insert(
        "__aipo_str_replace".to_string(),
        (
            str_replace_func_idx,
            str_replace_type_idx,
            str_replace_fn_type,
        ),
    );

    // Function 19.9e: `__aipo_str_split(text: i32, sep: i32) -> i32`
    let str_split_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let str_split_type_idx = emitter.add_type(str_split_fn_type.clone());
    let str_split_fn =
        build_str_split_function(list_new_func_idx, list_add_func_idx, str_copy_func_idx);
    let str_split_func_idx = emitter.add_function(str_split_type_idx, str_split_fn);
    emitter.export_function("__aipo_str_split", str_split_func_idx);
    functions.insert(
        "__aipo_str_split".to_string(),
        (str_split_func_idx, str_split_type_idx, str_split_fn_type),
    );

    // Function 19.9c: `__aipo_str_reverse(ptr: i32) -> i32`
    let str_reverse_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let str_reverse_type_idx = emitter.add_type(str_reverse_fn_type.clone());
    let str_reverse_fn = build_str_reverse_function(str_concat_func_idx, alloc_func_idx);
    let str_reverse_func_idx = emitter.add_function(str_reverse_type_idx, str_reverse_fn);
    emitter.export_function("__aipo_str_reverse", str_reverse_func_idx);
    functions.insert(
        "__aipo_str_reverse".to_string(),
        (
            str_reverse_func_idx,
            str_reverse_type_idx,
            str_reverse_fn_type,
        ),
    );

    // Functions 19.9a/b: `__aipo_str_starts` and `__aipo_str_ends`.
    let str_affix_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    for (name, at_end) in [("__aipo_str_starts", false), ("__aipo_str_ends", true)] {
        let type_idx = emitter.add_type(str_affix_fn_type.clone());
        let func_idx = emitter.add_function(type_idx, build_str_affix_function(at_end));
        emitter.export_function(name, func_idx);
        functions.insert(
            name.to_string(),
            (func_idx, type_idx, str_affix_fn_type.clone()),
        );
    }

    // Function 19.6: `__aipo_str_trim(ptr: i32) -> i32`
    let str_trim_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let str_trim_type_idx = emitter.add_type(str_trim_fn_type.clone());
    let str_trim_fn = build_str_trim_function(alloc_func_idx);
    let str_trim_func_idx = emitter.add_function(str_trim_type_idx, str_trim_fn);
    emitter.export_function("__aipo_str_trim", str_trim_func_idx);
    functions.insert(
        "__aipo_str_trim".to_string(),
        (str_trim_func_idx, str_trim_type_idx, str_trim_fn_type),
    );

    // Function 19.7: `__aipo_str_case(ptr: i32, upper: i32) -> i32`
    let str_case_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let str_case_type_idx = emitter.add_type(str_case_fn_type.clone());
    let str_case_fn = build_str_case_function(alloc_func_idx);
    let str_case_func_idx = emitter.add_function(str_case_type_idx, str_case_fn);
    emitter.export_function("__aipo_str_case", str_case_func_idx);
    functions.insert(
        "__aipo_str_case".to_string(),
        (str_case_func_idx, str_case_type_idx, str_case_fn_type),
    );

    // Function 19.8: `__aipo_str_find(haystack: i32, needle: i32) -> i64`
    let str_find_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I64]);
    let str_find_type_idx = emitter.add_type(str_find_fn_type.clone());
    let str_find_fn = build_str_find_function();
    let str_find_func_idx = emitter.add_function(str_find_type_idx, str_find_fn);
    emitter.export_function("__aipo_str_find", str_find_func_idx);
    functions.insert(
        "__aipo_str_find".to_string(),
        (str_find_func_idx, str_find_type_idx, str_find_fn_type),
    );

    // Function 20: `__aipo_int_to_str(val: i64) -> i32`
    let int_to_str_fn_type = WasmFnType::new(vec![WasmType::I64], vec![WasmType::I32]);
    let int_to_str_type_idx = emitter.add_type(int_to_str_fn_type.clone());
    let int_to_str_fn = build_int_to_str_function(alloc_func_idx);
    let int_to_str_func_idx = emitter.add_function(int_to_str_type_idx, int_to_str_fn);
    emitter.export_function("__aipo_int_to_str", int_to_str_func_idx);
    functions.insert(
        "__aipo_int_to_str".to_string(),
        (int_to_str_func_idx, int_to_str_type_idx, int_to_str_fn_type),
    );

    // Function 21: `__aipo_bool_to_str(val: i32) -> i32`
    let true_ptr = static_strings.get("true").copied().unwrap_or(0);
    let false_ptr = static_strings.get("false").copied().unwrap_or(0);
    let bool_to_str_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let bool_to_str_type_idx = emitter.add_type(bool_to_str_fn_type.clone());
    let bool_to_str_fn = build_bool_to_str_function(true_ptr, false_ptr);
    let bool_to_str_func_idx = emitter.add_function(bool_to_str_type_idx, bool_to_str_fn);
    emitter.export_function("__aipo_bool_to_str", bool_to_str_func_idx);
    functions.insert(
        "__aipo_bool_to_str".to_string(),
        (
            bool_to_str_func_idx,
            bool_to_str_type_idx,
            bool_to_str_fn_type,
        ),
    );

    // Function 22: `__aipo_str_eq(ptr_a: i32, ptr_b: i32) -> i32`
    let str_eq_fn_type = WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I32]);
    let str_eq_type_idx = emitter.add_type(str_eq_fn_type.clone());
    let str_eq_fn = build_string_eq_function();
    let str_eq_func_idx = emitter.add_function(str_eq_type_idx, str_eq_fn);
    emitter.export_function("__aipo_str_eq", str_eq_func_idx);
    functions.insert(
        "__aipo_str_eq".to_string(),
        (str_eq_func_idx, str_eq_type_idx, str_eq_fn_type),
    );

    // Function 23: `__aipo_bytes_new(len: i32) -> i32`
    let bytes_new_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I32]);
    let bytes_new_type_idx = emitter.add_type(bytes_new_fn_type.clone());
    let bytes_new_fn = build_bytes_new_function(alloc_func_idx);
    let bytes_new_func_idx = emitter.add_function(bytes_new_type_idx, bytes_new_fn);
    emitter.export_function("__aipo_bytes_new", bytes_new_func_idx);
    functions.insert(
        "__aipo_bytes_new".to_string(),
        (bytes_new_func_idx, bytes_new_type_idx, bytes_new_fn_type),
    );

    // Function 24: `__aipo_bytes_get(buf_ptr: i32, index: i32) -> i64`
    let bytes_get_fn_type =
        WasmFnType::new(vec![WasmType::I32, WasmType::I32], vec![WasmType::I64]);
    let bytes_get_type_idx = emitter.add_type(bytes_get_fn_type.clone());
    let bytes_get_fn = build_bytes_get_function();
    let bytes_get_func_idx = emitter.add_function(bytes_get_type_idx, bytes_get_fn);
    emitter.export_function("__aipo_bytes_get", bytes_get_func_idx);
    functions.insert(
        "__aipo_bytes_get".to_string(),
        (bytes_get_func_idx, bytes_get_type_idx, bytes_get_fn_type),
    );

    // Function 25: `__aipo_bytes_set(buf_ptr: i32, index: i32, val: i64) -> i64`
    let bytes_set_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I64],
        vec![WasmType::I64],
    );
    let bytes_set_type_idx = emitter.add_type(bytes_set_fn_type.clone());
    let bytes_set_fn = build_bytes_set_function();
    let bytes_set_func_idx = emitter.add_function(bytes_set_type_idx, bytes_set_fn);
    emitter.export_function("__aipo_bytes_set", bytes_set_func_idx);
    functions.insert(
        "__aipo_bytes_set".to_string(),
        (bytes_set_func_idx, bytes_set_type_idx, bytes_set_fn_type),
    );

    // Function 26: `__aipo_bytes_len(buf_ptr: i32) -> i64`
    let bytes_len_fn_type = WasmFnType::new(vec![WasmType::I32], vec![WasmType::I64]);
    let bytes_len_type_idx = emitter.add_type(bytes_len_fn_type.clone());
    let bytes_len_fn = build_bytes_len_function();
    let bytes_len_func_idx = emitter.add_function(bytes_len_type_idx, bytes_len_fn);
    emitter.export_function("__aipo_bytes_len", bytes_len_func_idx);
    functions.insert(
        "__aipo_bytes_len".to_string(),
        (bytes_len_func_idx, bytes_len_type_idx, bytes_len_fn_type),
    );

    // Function 26.1: `__aipo_bytes_slice(buf: i32, start: i32, end: i32) -> i32`
    let bytes_slice_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I32, WasmType::I32],
        vec![WasmType::I32],
    );
    let bytes_slice_type_idx = emitter.add_type(bytes_slice_fn_type.clone());
    let bytes_slice_fn = build_bytes_slice_function(alloc_func_idx);
    let bytes_slice_func_idx = emitter.add_function(bytes_slice_type_idx, bytes_slice_fn);
    emitter.export_function("__aipo_bytes_slice", bytes_slice_func_idx);
    functions.insert(
        "__aipo_bytes_slice".to_string(),
        (
            bytes_slice_func_idx,
            bytes_slice_type_idx,
            bytes_slice_fn_type,
        ),
    );

    let async_helpers = AsyncHelpers {
        task_create_idx: task_create_func_idx,
        await_idx: await_func_idx,
        host_io,
        fail_globals: failure_globals,
        function_return: None,
        journal: JournalGlobals {
            instance: journal_instance_idx,
            value: journal_value_idx,
            offset: journal_offset_idx,
            live: journal_type_idx,
        },
        journal2: JournalGlobals {
            instance: journal_instance_2_idx,
            value: journal_value_2_idx,
            offset: journal_offset_2_idx,
            live: journal_live_2_idx,
        },
        suppress_boundary: false,
        constructing_type: false,
    };

    // Defaults are applied here, at the call site: the callee's own runtime
    // cannot distinguish "omitted" from "passed the zero value" in Wasm, and
    // padding with zeros silently replaced `count: Int = 1` with `0`.

    // Pass 1: Collect signatures of all declared and anonymous functions.
    //
    // An unannotated parameter has no declared representation, so its Wasm type
    // comes from the arguments the program actually passes to it. Without this a
    // `String` argument crosses the call boundary as a truncated pointer and the
    // callee prints an address instead of the text.
    let param_hints = collect_param_hints(program, &structs);
    let mut next_func_idx = emitter.next_func_idx();
    for func in &func_decls {
        let params = func
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                param_hints
                    .get(&func.name)
                    .and_then(|row| row.get(index).copied())
                    .unwrap_or_else(|| param_wasm_type(param))
            })
            .collect::<Vec<_>>();
        let ret = if func.name.starts_with("__async_body_") {
            Some(WasmType::I64)
        } else {
            resolve_return_type(
                &func.name,
                &func.return_type,
                &func.params,
                &func.body,
                &functions,
                &structs,
            )
        };
        let kind = resolve_return_kind(
            &func.return_type,
            &func.params,
            &func.body,
            &func.name,
            &functions,
            &structs,
        );
        let fn_type = WasmFnType::new(params, ret.into_iter().collect());
        let type_idx = emitter.add_type(fn_type.clone());
        let func_idx = next_func_idx;
        next_func_idx += 1;
        functions.insert(func.name.clone(), (func_idx, type_idx, fn_type));
        functions.return_kinds.insert(func.name.clone(), kind);
    }

    // Module bindings become Wasm globals before any body compiles, so a function
    // that names one resolves it the same way the entry script does.
    //
    // The initializer runs in the entry script, not here: a global starts at zero
    // and `__top_level__` assigns it, which keeps the evaluation order and side
    // effects of `let x = expensive()` intact.
    // A binding may be initialized from an earlier one (`let a = ...; let b =
    // a.values()`), so the earlier bindings are already in the kind environment
    // when the later initializer is classified.
    let mut module_env: HashMap<String, LocalKind> = HashMap::new();
    for statement in &program.statements {
        let (name, expr) = match statement {
            HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => (name, expr),
            _ => continue,
        };
        let env_ty = |kind: &LocalKind| match kind {
            LocalKind::Float => WasmType::F64,
            LocalKind::Int => WasmType::I64,
            _ => WasmType::I32,
        };
        let locals_for_expr: HashMap<String, (u32, WasmType, LocalKind)> = module_env
            .iter()
            .map(|(binding, kind)| (binding.clone(), (0, env_ty(kind), kind.clone())))
            .collect();
        let ty = infer_expr_type(
            expr,
            &locals_for_expr,
            &functions,
            &structs,
            &HashMap::new(),
        )
        .unwrap_or(WasmType::I64);
        let kind = infer_expr_kind(
            expr,
            &locals_for_expr,
            &functions,
            &HashMap::new(),
            &anon_map,
        );
        module_env.insert(name.clone(), kind.clone());
        let global_idx = emitter.add_global(
            GlobalType {
                val_type: match ty {
                    WasmType::I32 => ValType::I32,
                    WasmType::I64 => ValType::I64,
                    WasmType::F64 => ValType::F64,
                },
                mutable: true,
                shared: false,
            },
            &match ty {
                WasmType::I32 => ConstExpr::i32_const(0),
                WasmType::I64 => ConstExpr::i64_const(0),
                WasmType::F64 => ConstExpr::f64_const(0.0f64.into()),
            },
        );
        functions
            .module_globals
            .insert(name.clone(), (global_idx, ty, kind));
    }

    // A parameter's semantic kind is declared, or else learned from the calls that
    // reach it. Declared kinds seed the table so the learning pass knows how many
    // parameters each function has.
    for func in &func_decls {
        let declared_kinds: Vec<Option<LocalKind>> = func
            .params
            .iter()
            .map(|param| {
                let declared = annotated_kind(param.type_annotation.as_ref(), &structs);
                if !matches!(declared, LocalKind::Unknown) {
                    return Some(declared);
                }
                // A default is a witness for a parameter every call leaves out, so
                // `fn remember(value, into = [])` is known to take a List.
                let default = param.default.as_ref()?;
                let kind = infer_expr_kind(
                    default,
                    &HashMap::new(),
                    &functions,
                    &HashMap::new(),
                    &anon_map,
                );
                (!matches!(kind, LocalKind::Unknown)).then_some(kind)
            })
            .collect();
        functions
            .param_kinds
            .insert(func.name.clone(), declared_kinds);
    }
    let learned_kinds = collect_param_kinds(program, &func_decls, &functions, &structs);
    for (name, learned) in learned_kinds {
        let row = functions.param_kinds.entry(name).or_default();
        for (index, hint) in learned.into_iter().enumerate() {
            if index < row.len() && row[index].is_none() {
                row[index] = hint;
            }
        }
    }

    // Register async wrapper function signatures (returning task_ptr: i32)
    let mut async_wrapper_infos = Vec::new();
    for (name, inner_body_name, params) in &async_wrappers {
        let params_wasm = params.iter().map(param_wasm_type).collect::<Vec<_>>();
        let fn_type = WasmFnType::new(params_wasm.clone(), vec![WasmType::I32]);
        let type_idx = emitter.add_type(fn_type.clone());
        let func_idx = next_func_idx;
        next_func_idx += 1;
        functions.insert(name.clone(), (func_idx, type_idx, fn_type));
        async_wrapper_infos.push((name.clone(), inner_body_name.clone(), params_wasm, type_idx));
    }

    // Pass 1.5: Setup Table 0 for indirect function calls (call_indirect)
    let mut table_indices: HashMap<String, u32> = HashMap::new();
    let table_size = (func_decls.len() as u64).max(1);
    emitter.enable_table(table_size, Some(table_size));
    if !func_decls.is_empty() {
        let mut func_indices = Vec::new();
        for (table_idx, func) in func_decls.iter().enumerate() {
            let (func_idx, _, _) = functions[&func.name];
            func_indices.push(func_idx);
            table_indices.insert(func.name.clone(), table_idx as u32);
        }
        emitter.add_element_segment(0, 0, func_indices);
    }

    // Top-level statements become an entrypoint function `__top_level__`
    let top_level_info = if !program.statements.is_empty() {
        let mut top_locals = HashMap::new();
        let ret = infer_body_return_type(
            &program.statements,
            &mut top_locals,
            &functions,
            &structs,
            &table_indices,
            &anon_map,
        );
        let fn_type = WasmFnType::new(vec![], ret.into_iter().collect());
        let type_idx = emitter.add_type(fn_type.clone());
        let func_idx = next_func_idx;
        functions.insert("__top_level__".to_string(), (func_idx, type_idx, fn_type));
        Some((func_idx, type_idx, ret))
    } else {
        None
    };

    // Pass 2: Compile the body of each function
    for func in &func_decls {
        let (_, type_idx, fn_type) = &functions[&func.name];
        let compiled_fn = compile_function_body(
            &func.name,
            &func.params,
            &fn_type.params,
            fn_type.results.first().copied(),
            &func.body,
            &functions,
            &structs,
            &static_strings,
            &table_indices,
            &anon_map,
            &indirect_sigs,
            alloc_func_idx,
            async_helpers,
        )?;
        let assigned_idx = emitter.add_function(*type_idx, compiled_fn);
        emitter.export_function(&func.name, assigned_idx);
    }

    // Compile async wrapper functions
    for (name, inner_body_name, param_types, type_idx) in async_wrapper_infos {
        let inner_table_idx = table_indices[&inner_body_name];
        let wrapper_fn = build_async_wrapper_function(
            inner_table_idx,
            &param_types,
            async_helpers.task_create_idx,
        );
        let assigned_idx = emitter.add_function(type_idx, wrapper_fn);
        emitter.export_function(&name, assigned_idx);
    }

    // Compile top-level entrypoint if present
    if let Some((_, type_idx, ret)) = top_level_info {
        let compiled_fn = compile_function_body(
            "__top_level__",
            &[],
            &[],
            ret,
            &program.statements,
            &functions,
            &structs,
            &static_strings,
            &table_indices,
            &anon_map,
            &indirect_sigs,
            alloc_func_idx,
            async_helpers,
        )?;
        let assigned_idx = emitter.add_function(type_idx, compiled_fn);
        emitter.export_function("__top_level__", assigned_idx);
    }

    Ok(emitter.finish())
}

/// Learns the semantic kind of each declared parameter from the calls that reach it.
///
/// An unannotated parameter takes its representation from the arguments (`i32` for
/// a `String`, `List`, `Dict` or struct), but those four share one representation.
/// The body still has to know which one it received: `len(value)` counts bytes on a
/// `String` and elements on a `List`. This pass reads the argument kinds at every
/// call site until the answer stops moving.
fn collect_param_kinds(
    program: &HirProgram,
    func_decls: &[HirFunctionDecl],
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
) -> HashMap<String, Vec<Option<LocalKind>>> {
    let mut hints: HashMap<String, Vec<Option<LocalKind>>> = HashMap::new();
    for _ in 0..3 {
        let mut changed = false;
        for func in func_decls {
            let mut env: HashMap<String, LocalKind> = HashMap::new();
            for (index, param) in func.params.iter().enumerate() {
                let declared = annotated_kind(param.type_annotation.as_ref(), structs);
                let kind = if matches!(declared, LocalKind::Unknown) {
                    hints
                        .get(&func.name)
                        .and_then(|row| row.get(index))
                        .cloned()
                        .flatten()
                        .unwrap_or(LocalKind::Unknown)
                } else {
                    declared
                };
                env.insert(param.name.clone(), kind);
            }
            walk_param_kinds(
                &func.body,
                &mut env,
                &mut hints,
                functions,
                structs,
                &mut changed,
            );
        }
        let mut env: HashMap<String, LocalKind> = HashMap::new();
        walk_param_kinds(
            &program.statements,
            &mut env,
            &mut hints,
            functions,
            structs,
            &mut changed,
        );
        if !changed {
            break;
        }
    }
    hints
}

/// Records the argument kinds of every call and keeps the local environment current.
fn walk_param_kinds(
    stmts: &[HirStmt],
    env: &mut HashMap<String, LocalKind>,
    hints: &mut HashMap<String, Vec<Option<LocalKind>>>,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
    changed: &mut bool,
) {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => {
                let kind = kind_with_env(expr, env, functions, structs);
                env.insert(name.clone(), kind);
                walk_param_kinds_expr(expr, env, hints, functions, structs, changed);
            }
            HirStmt::Assign(_, expr, _) | HirStmt::CompoundAssign(_, _, expr, _) => {
                walk_param_kinds_expr(expr, env, hints, functions, structs, changed);
            }
            HirStmt::Expr(expr) | HirStmt::Return(Some(expr), _) | HirStmt::Fail(expr, _) => {
                walk_param_kinds_expr(expr, env, hints, functions, structs, changed);
            }
            HirStmt::If(s) => {
                walk_param_kinds_expr(&s.condition, env, hints, functions, structs, changed);
                walk_param_kinds(&s.then_branch, env, hints, functions, structs, changed);
                for (cond, body) in &s.elif_branches {
                    walk_param_kinds_expr(cond, env, hints, functions, structs, changed);
                    walk_param_kinds(body, env, hints, functions, structs, changed);
                }
                if let Some(body) = &s.else_branch {
                    walk_param_kinds(body, env, hints, functions, structs, changed);
                }
            }
            HirStmt::While(cond, body, _) => {
                walk_param_kinds_expr(cond, env, hints, functions, structs, changed);
                walk_param_kinds(body, env, hints, functions, structs, changed);
            }
            HirStmt::Loop(body, _) => {
                walk_param_kinds(body, env, hints, functions, structs, changed);
            }
            HirStmt::Repeat(count, _, body, _) => {
                walk_param_kinds_expr(count, env, hints, functions, structs, changed);
                walk_param_kinds(body, env, hints, functions, structs, changed);
            }
            HirStmt::Each(vars, iterable, body, _) => {
                walk_param_kinds_expr(iterable, env, hints, functions, structs, changed);
                let element = match kind_with_env(iterable, env, functions, structs) {
                    LocalKind::List(element) => (*element).clone(),
                    _ => LocalKind::Unknown,
                };
                for var in vars {
                    env.insert(var.clone(), element.clone());
                }
                walk_param_kinds(body, env, hints, functions, structs, changed);
            }
            HirStmt::Attempt(attempt) => {
                walk_param_kinds(&attempt.body, env, hints, functions, structs, changed);
                walk_param_kinds(&attempt.handler, env, hints, functions, structs, changed);
            }
            HirStmt::Match(m) => {
                for arm in &m.when_arms {
                    walk_param_kinds(&arm.body, env, hints, functions, structs, changed);
                }
                if let Some(body) = &m.else_arm {
                    walk_param_kinds(body, env, hints, functions, structs, changed);
                }
            }
            HirStmt::AwaitDo(body, _) => {
                walk_param_kinds(body, env, hints, functions, structs, changed);
            }
            HirStmt::FnDecl(decl) => {
                // A nested `fn` sees its own parameters, but it can still call a
                // declared function, so its body is walked with the enclosing env.
                walk_param_kinds(&decl.body, env, hints, functions, structs, changed);
            }
            _ => {}
        }
    }
}

/// Walks one expression, recording argument kinds for declared-function calls.
fn walk_param_kinds_expr(
    expr: &HirExpr,
    env: &HashMap<String, LocalKind>,
    hints: &mut HashMap<String, Vec<Option<LocalKind>>>,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
    changed: &mut bool,
) {
    match expr {
        HirExpr::Call(callee, args, _) => {
            if let HirExpr::Identifier(name, _) = &**callee
                && !env.contains_key(name)
                && let Some(row_spec) = functions.param_kinds.get(name)
            {
                let mut row = hints.remove(name).unwrap_or_default();
                if row.len() < row_spec.len() {
                    row.resize(row_spec.len(), None);
                }
                for (index, arg) in args.iter().enumerate() {
                    if index >= row.len() || row[index].is_some() {
                        continue;
                    }
                    let observed = kind_with_env(&arg.value, env, functions, structs);
                    if !matches!(observed, LocalKind::Unknown) {
                        row[index] = Some(observed);
                        *changed = true;
                    }
                }
                hints.insert(name.clone(), row);
            }
            for arg in args {
                walk_param_kinds_expr(&arg.value, env, hints, functions, structs, changed);
            }
        }
        HirExpr::Binary(_, left, right, _) => {
            walk_param_kinds_expr(left, env, hints, functions, structs, changed);
            walk_param_kinds_expr(right, env, hints, functions, structs, changed);
        }
        HirExpr::Unary(_, inner, _)
        | HirExpr::Dot(inner, _, _)
        | HirExpr::QuestionDot(inner, _, _)
        | HirExpr::Await(inner, _)
        | HirExpr::Try(inner, _)
        | HirExpr::With(inner, _, _) => {
            walk_param_kinds_expr(inner, env, hints, functions, structs, changed);
        }
        HirExpr::Index(target, index, _) => {
            walk_param_kinds_expr(target, env, hints, functions, structs, changed);
            walk_param_kinds_expr(index, env, hints, functions, structs, changed);
        }
        HirExpr::List(items, _) => {
            for item in items {
                walk_param_kinds_expr(item, env, hints, functions, structs, changed);
            }
        }
        HirExpr::Dict(entries, _) => {
            for (key, value) in entries {
                walk_param_kinds_expr(key, env, hints, functions, structs, changed);
                walk_param_kinds_expr(value, env, hints, functions, structs, changed);
            }
        }
        HirExpr::Construct(_, fields, _) => {
            for (_, value) in fields {
                walk_param_kinds_expr(value, env, hints, functions, structs, changed);
            }
        }
        HirExpr::If(cond, then_expr, else_expr, _) => {
            walk_param_kinds_expr(cond, env, hints, functions, structs, changed);
            walk_param_kinds_expr(then_expr, env, hints, functions, structs, changed);
            walk_param_kinds_expr(else_expr, env, hints, functions, structs, changed);
        }
        HirExpr::OrElse(left, right, _) => {
            walk_param_kinds_expr(left, env, hints, functions, structs, changed);
            walk_param_kinds_expr(right, env, hints, functions, structs, changed);
        }
        HirExpr::Fn(_) | HirExpr::Literal(..) | HirExpr::Identifier(..) => {}
    }
}

/// Infers an expression kind against a kind-only environment.
fn kind_with_env(
    expr: &HirExpr,
    env: &HashMap<String, LocalKind>,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
) -> LocalKind {
    let mapped: HashMap<String, (u32, WasmType, LocalKind)> = env
        .iter()
        .map(|(name, kind)| {
            let ty = match kind {
                LocalKind::Float => WasmType::F64,
                LocalKind::Int => WasmType::I64,
                LocalKind::String
                | LocalKind::List(_)
                | LocalKind::Dict(_)
                | LocalKind::Bytes
                | LocalKind::Struct(_)
                | LocalKind::Fn(_)
                | LocalKind::Task
                | LocalKind::Range => WasmType::I32,
                _ => WasmType::I64,
            };
            (name.clone(), (0, ty, kind.clone()))
        })
        .collect();
    let _ = structs;
    infer_expr_kind(expr, &mapped, functions, &HashMap::new(), &HashMap::new())
}

/// Collects all anonymous functions and closures from the program.
fn collect_anon_functions(
    program: &HirProgram,
    anon_map: &mut HashMap<SourceSpan, String>,
    anon_decls: &mut Vec<HirFunctionDecl>,
) {
    for item in &program.items {
        match item {
            HirItem::Fn(func) => {
                collect_stmts_anon_functions(&func.body, anon_map, anon_decls);
            }
            HirItem::Impl(imp) => {
                if let Some(init) = &imp.init {
                    collect_stmts_anon_functions(&init.body, anon_map, anon_decls);
                }
                if let Some(inv) = &imp.invariant {
                    collect_stmts_anon_functions(&inv.body, anon_map, anon_decls);
                }
                for m in &imp.methods {
                    collect_stmts_anon_functions(&m.body, anon_map, anon_decls);
                }
            }
            _ => {}
        }
    }
    collect_stmts_anon_functions(&program.statements, anon_map, anon_decls);
}

fn collect_stmts_anon_functions(
    stmts: &[HirStmt],
    anon_map: &mut HashMap<SourceSpan, String>,
    anon_decls: &mut Vec<HirFunctionDecl>,
) {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(_, expr, _) | HirStmt::Var(_, expr, _) | HirStmt::Expr(expr) => {
                collect_expr_anon_functions(expr, anon_map, anon_decls);
            }
            HirStmt::Assign(t, e, _) | HirStmt::CompoundAssign(_, t, e, _) => {
                collect_expr_anon_functions(t, anon_map, anon_decls);
                collect_expr_anon_functions(e, anon_map, anon_decls);
            }
            HirStmt::Return(Some(expr), _) => {
                collect_expr_anon_functions(expr, anon_map, anon_decls);
            }
            HirStmt::If(s) => {
                collect_expr_anon_functions(&s.condition, anon_map, anon_decls);
                collect_stmts_anon_functions(&s.then_branch, anon_map, anon_decls);
                for (cond, body) in &s.elif_branches {
                    collect_expr_anon_functions(cond, anon_map, anon_decls);
                    collect_stmts_anon_functions(body, anon_map, anon_decls);
                }
                if let Some(else_branch) = &s.else_branch {
                    collect_stmts_anon_functions(else_branch, anon_map, anon_decls);
                }
            }
            HirStmt::While(cond, body, _) => {
                collect_expr_anon_functions(cond, anon_map, anon_decls);
                collect_stmts_anon_functions(body, anon_map, anon_decls);
            }
            HirStmt::Loop(body, _) => {
                collect_stmts_anon_functions(body, anon_map, anon_decls);
            }
            HirStmt::Repeat(count, _, body, _) => {
                collect_expr_anon_functions(count, anon_map, anon_decls);
                collect_stmts_anon_functions(body, anon_map, anon_decls);
            }
            HirStmt::Each(_, iterable, body, _) => {
                collect_expr_anon_functions(iterable, anon_map, anon_decls);
                collect_stmts_anon_functions(body, anon_map, anon_decls);
            }
            HirStmt::Fail(expr, _) => {
                collect_expr_anon_functions(expr, anon_map, anon_decls);
            }
            HirStmt::Attempt(a) => {
                collect_stmts_anon_functions(&a.body, anon_map, anon_decls);
                collect_stmts_anon_functions(&a.handler, anon_map, anon_decls);
            }
            HirStmt::Match(m) => {
                collect_expr_anon_functions(&m.target, anon_map, anon_decls);
                for arm in &m.when_arms {
                    for p in &arm.patterns {
                        if let aipo_hir::HirMatchPattern::Value(expr) = p {
                            collect_expr_anon_functions(expr, anon_map, anon_decls);
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        collect_expr_anon_functions(guard, anon_map, anon_decls);
                    }
                    collect_stmts_anon_functions(&arm.body, anon_map, anon_decls);
                }
                if let Some(else_body) = &m.else_arm {
                    collect_stmts_anon_functions(else_body, anon_map, anon_decls);
                }
            }
            HirStmt::AwaitDo(stmts, _) => {
                collect_stmts_anon_functions(stmts, anon_map, anon_decls);
            }
            HirStmt::FnDecl(decl) => {
                collect_stmts_anon_functions(&decl.body, anon_map, anon_decls);
                let anon_name = format!("__anon_fn_{}", anon_decls.len());
                anon_map.insert(decl.span, anon_name.clone());
                let mut renamed = decl.clone();
                renamed.name = anon_name;
                anon_decls.push(renamed);
            }
            _ => {}
        }
    }
}

fn collect_expr_anon_functions(
    expr: &HirExpr,
    anon_map: &mut HashMap<SourceSpan, String>,
    anon_decls: &mut Vec<HirFunctionDecl>,
) {
    match expr {
        HirExpr::Fn(func_expr) => {
            collect_stmts_anon_functions(&func_expr.body, anon_map, anon_decls);
            let anon_name = format!("__anon_fn_{}", anon_decls.len());
            anon_map.insert(func_expr.span, anon_name.clone());
            anon_decls.push(HirFunctionDecl {
                directives: Vec::new(),
                name: anon_name,
                is_async: func_expr.is_async,
                params: func_expr.params.clone(),
                return_type: func_expr.return_type.clone(),
                body: func_expr.body.clone(),
                span: func_expr.span,
            });
        }
        HirExpr::Await(inner, _) => {
            collect_expr_anon_functions(inner, anon_map, anon_decls);
        }
        HirExpr::Call(callee, args, _) => {
            collect_expr_anon_functions(callee, anon_map, anon_decls);
            for arg in args {
                collect_expr_anon_functions(&arg.value, anon_map, anon_decls);
            }
        }
        HirExpr::Binary(_, left, right, _) => {
            collect_expr_anon_functions(left, anon_map, anon_decls);
            collect_expr_anon_functions(right, anon_map, anon_decls);
        }
        HirExpr::Unary(_, inner, _) => {
            collect_expr_anon_functions(inner, anon_map, anon_decls);
        }
        HirExpr::If(cond, then_e, else_e, _) => {
            collect_expr_anon_functions(cond, anon_map, anon_decls);
            collect_expr_anon_functions(then_e, anon_map, anon_decls);
            collect_expr_anon_functions(else_e, anon_map, anon_decls);
        }
        HirExpr::Construct(_, fields, _) => {
            for (_, field_expr) in fields {
                collect_expr_anon_functions(field_expr, anon_map, anon_decls);
            }
        }
        HirExpr::Dot(receiver, _, _) => {
            collect_expr_anon_functions(receiver, anon_map, anon_decls);
        }
        _ => {}
    }
}

/// Registers struct layout in linear memory (fields aligned to 8 bytes).
fn register_struct_layout(decl: &HirStructDecl, structs: &mut HashMap<String, StructLayout>) {
    let mut fields = HashMap::new();
    let mut offset = 0u32;
    for f in &decl.fields {
        // A declared field contract wins over literal sniffing: `var x: Float`
        // stores an `f64` for every value, not only when the constructor happens
        // to receive a float literal.
        let ty = if let Some(annotation) = &f.type_annotation {
            match annotation.name.as_str() {
                "Float" => WasmType::F64,
                "Bool" => WasmType::I32,
                "String" | "Bytes" | "List" | "Dict" | "Range" | "Task" => WasmType::I32,
                name if structs.contains_key(name) => WasmType::I32,
                _ => WasmType::I64,
            }
        } else if let Some(def) = &f.default {
            match def {
                HirExpr::Literal(Literal::Float(..), _) => WasmType::F64,
                HirExpr::Literal(Literal::Bool(..), _) => WasmType::I32,
                HirExpr::Literal(Literal::String(..), _) => WasmType::I32,
                _ => WasmType::I64,
            }
        } else {
            WasmType::I64
        };
        fields.insert(f.name.clone(), (offset, ty));
        offset += 8;
    }
    let size = if offset == 0 { 8 } else { offset };
    structs.insert(decl.name.clone(), StructLayout { size, fields });
}

/// Scans program expressions to refine field types in `structs` based on constructor literals.
fn refine_struct_layouts(
    program: &HirProgram,
    structs: &mut HashMap<String, StructLayout>,
    param_hints: &HashMap<String, Vec<WasmType>>,
) {
    for item in &program.items {
        match item {
            HirItem::Fn(func) => {
                let mut locals: HashMap<String, WasmType> = func
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        (
                            param.name.clone(),
                            param_hints
                                .get(&func.name)
                                .and_then(|row| row.get(index).copied())
                                .unwrap_or_else(|| param_wasm_type(param)),
                        )
                    })
                    .collect();
                scan_stmts_for_constructs(&func.body, &mut locals, structs);
            }
            HirItem::Impl(imp) => {
                for body in [&imp.init, &imp.invariant].into_iter().flatten() {
                    let mut locals: HashMap<String, WasmType> = body
                        .params
                        .iter()
                        .map(|param| (param.name.clone(), param_wasm_type(param)))
                        .collect();
                    scan_stmts_for_constructs(&body.body, &mut locals, structs);
                }
                for m in &imp.methods {
                    let mut locals: HashMap<String, WasmType> = m
                        .params
                        .iter()
                        .map(|param| (param.name.clone(), param_wasm_type(param)))
                        .collect();
                    scan_stmts_for_constructs(&m.body, &mut locals, structs);
                }
            }
            _ => {}
        }
    }
    let mut locals: HashMap<String, WasmType> = HashMap::new();
    scan_stmts_for_constructs(&program.statements, &mut locals, structs);
}

fn scan_stmts_for_constructs(
    stmts: &[HirStmt],
    locals: &mut HashMap<String, WasmType>,
    structs: &mut HashMap<String, StructLayout>,
) {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => {
                scan_expr_for_constructs(expr, locals, structs);
                if let Some(ty) = static_expr_type(expr, locals, &HashMap::new(), structs) {
                    locals.insert(name.clone(), ty);
                }
            }
            HirStmt::Expr(expr) => {
                scan_expr_for_constructs(expr, locals, structs);
            }
            HirStmt::Assign(target, expr, _) | HirStmt::CompoundAssign(_, target, expr, _) => {
                scan_expr_for_constructs(target, locals, structs);
                scan_expr_for_constructs(expr, locals, structs);
            }
            HirStmt::Return(Some(expr), _) => {
                scan_expr_for_constructs(expr, locals, structs);
            }
            HirStmt::If(s) => {
                scan_expr_for_constructs(&s.condition, locals, structs);
                scan_stmts_for_constructs(&s.then_branch, locals, structs);
                for (c, b) in &s.elif_branches {
                    scan_expr_for_constructs(c, locals, structs);
                    scan_stmts_for_constructs(b, locals, structs);
                }
                if let Some(eb) = &s.else_branch {
                    scan_stmts_for_constructs(eb, locals, structs);
                }
            }
            HirStmt::While(cond, body, _) => {
                scan_expr_for_constructs(cond, locals, structs);
                scan_stmts_for_constructs(body, locals, structs);
            }
            HirStmt::Loop(body, _) => {
                scan_stmts_for_constructs(body, locals, structs);
            }
            HirStmt::Repeat(count, _, body, _) => {
                scan_expr_for_constructs(count, locals, structs);
                scan_stmts_for_constructs(body, locals, structs);
            }
            HirStmt::Each(_, iterable, body, _) => {
                scan_expr_for_constructs(iterable, locals, structs);
                scan_stmts_for_constructs(body, locals, structs);
            }
            HirStmt::Fail(expr, _) => {
                scan_expr_for_constructs(expr, locals, structs);
            }
            HirStmt::Attempt(a) => {
                scan_stmts_for_constructs(&a.body, locals, structs);
                scan_stmts_for_constructs(&a.handler, locals, structs);
            }
            HirStmt::Match(m) => {
                scan_expr_for_constructs(&m.target, locals, structs);
                for arm in &m.when_arms {
                    for p in &arm.patterns {
                        if let aipo_hir::HirMatchPattern::Value(expr) = p {
                            scan_expr_for_constructs(expr, locals, structs);
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        scan_expr_for_constructs(guard, locals, structs);
                    }
                    scan_stmts_for_constructs(&arm.body, locals, structs);
                }
                if let Some(else_body) = &m.else_arm {
                    scan_stmts_for_constructs(else_body, locals, structs);
                }
            }
            HirStmt::AwaitDo(stmts, _) => {
                scan_stmts_for_constructs(stmts, locals, structs);
            }
            _ => {}
        }
    }
}

fn scan_expr_for_constructs(
    expr: &HirExpr,
    locals: &mut HashMap<String, WasmType>,
    structs: &mut HashMap<String, StructLayout>,
) {
    match expr {
        HirExpr::Await(inner, _) => {
            scan_expr_for_constructs(inner, locals, structs);
        }
        HirExpr::Construct(type_name, fields, _) => {
            // A constructor is the only place a field's representation is stated
            // when the declaration has no contract. Recording it keeps
            // `User{name = "ana"}` from storing a pointer in a slot the printer
            // then reads as an integer.
            let mut observed: Vec<(String, WasmType)> = Vec::new();
            for (maybe_name, field_expr) in fields {
                scan_expr_for_constructs(field_expr, locals, structs);
                let Some(name) = maybe_name else { continue };
                let ty = match field_expr {
                    HirExpr::Literal(Literal::Float(..), _) => Some(WasmType::F64),
                    HirExpr::Literal(Literal::Bool(..), _) => Some(WasmType::I32),
                    HirExpr::Literal(Literal::String(..), _) => Some(WasmType::I32),
                    HirExpr::List(..) | HirExpr::Dict(..) | HirExpr::Fn(..) => Some(WasmType::I32),
                    // A nested constructor stores the instance pointer.
                    HirExpr::Construct(..) => Some(WasmType::I32),
                    // A parameter (or any local) carries the representation the
                    // call sites gave it; `Entry{label = suffix}` is a String field.
                    HirExpr::Identifier(name, _) => locals.get(name).copied(),
                    _ => None,
                };
                if let Some(ty) = ty {
                    observed.push((name.clone(), ty));
                }
            }
            if let Some(layout) = structs.get_mut(type_name) {
                for (name, ty) in observed {
                    if let Some((_, field_ty)) = layout.fields.get_mut(&name) {
                        // Keep an `i64` slot when the observation is an integer, so a
                        // later `i64` use stays valid; otherwise adopt the witness.
                        if *field_ty == WasmType::I64 && ty == WasmType::I64 {
                            continue;
                        }
                        *field_ty = ty;
                    }
                }
            }
        }
        HirExpr::Binary(_, left, right, _) => {
            scan_expr_for_constructs(left, locals, structs);
            scan_expr_for_constructs(right, locals, structs);
        }
        HirExpr::Unary(_, inner, _) => {
            scan_expr_for_constructs(inner, locals, structs);
        }
        HirExpr::If(cond, then_e, else_e, _) => {
            scan_expr_for_constructs(cond, locals, structs);
            scan_expr_for_constructs(then_e, locals, structs);
            scan_expr_for_constructs(else_e, locals, structs);
        }
        HirExpr::Call(callee, args, _) => {
            scan_expr_for_constructs(callee, locals, structs);
            for arg in args {
                scan_expr_for_constructs(&arg.value, locals, structs);
            }
        }
        HirExpr::Dot(receiver, _, _) => {
            scan_expr_for_constructs(receiver, locals, structs);
        }
        HirExpr::List(elements, _) => {
            for elem in elements {
                scan_expr_for_constructs(elem, locals, structs);
            }
        }
        HirExpr::Dict(entries, _) => {
            for (key, value) in entries {
                scan_expr_for_constructs(key, locals, structs);
                scan_expr_for_constructs(value, locals, structs);
            }
        }
        HirExpr::Index(target, index, _) => {
            scan_expr_for_constructs(target, locals, structs);
            scan_expr_for_constructs(index, locals, structs);
        }
        _ => {}
    }
}

/// Scans the program to determine if any I/O printing calls (`print`, `println`, `io.print`, `io.println`) are present.
fn scan_program_for_io(program: &HirProgram) -> bool {
    for item in &program.items {
        let has_io = match item {
            HirItem::Fn(func) => scan_stmts_for_io(&func.body),
            HirItem::Impl(impl_block) => {
                impl_block
                    .init
                    .as_ref()
                    .is_some_and(|f| scan_stmts_for_io(&f.body))
                    || impl_block
                        .invariant
                        .as_ref()
                        .is_some_and(|f| scan_stmts_for_io(&f.body))
                    || impl_block
                        .methods
                        .iter()
                        .any(|m| scan_stmts_for_io(&m.body))
            }
            _ => false,
        };
        if has_io {
            return true;
        }
    }
    scan_stmts_for_io(&program.statements)
}

fn scan_stmts_for_io(stmts: &[HirStmt]) -> bool {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(_, expr, _) | HirStmt::Var(_, expr, _) | HirStmt::Expr(expr) => {
                if scan_expr_for_io(expr) {
                    return true;
                }
            }
            HirStmt::Assign(target, expr, _) | HirStmt::CompoundAssign(_, target, expr, _) => {
                if scan_expr_for_io(target) || scan_expr_for_io(expr) {
                    return true;
                }
            }
            HirStmt::Return(Some(expr), _) if scan_expr_for_io(expr) => return true,
            HirStmt::Fail(expr, _) if scan_expr_for_io(expr) => return true,
            HirStmt::Attempt(a) if scan_stmts_for_io(&a.body) || scan_stmts_for_io(&a.handler) => {
                return true;
            }
            HirStmt::If(s) => {
                if scan_expr_for_io(&s.condition)
                    || scan_stmts_for_io(&s.then_branch)
                    || s.elif_branches
                        .iter()
                        .any(|(c, b)| scan_expr_for_io(c) || scan_stmts_for_io(b))
                    || s.else_branch.as_ref().is_some_and(|b| scan_stmts_for_io(b))
                {
                    return true;
                }
            }
            HirStmt::Match(m) => {
                if scan_expr_for_io(&m.target)
                    || m.when_arms.iter().any(|arm| {
                        arm.guard.as_ref().is_some_and(scan_expr_for_io)
                            || scan_stmts_for_io(&arm.body)
                    })
                    || m.else_arm.as_ref().is_some_and(|b| scan_stmts_for_io(b))
                {
                    return true;
                }
            }
            HirStmt::While(cond, body, _) if scan_expr_for_io(cond) || scan_stmts_for_io(body) => {
                return true;
            }
            HirStmt::Loop(body, _) | HirStmt::AwaitDo(body, _) if scan_stmts_for_io(body) => {
                return true;
            }
            HirStmt::Repeat(count, _, body, _)
                if scan_expr_for_io(count) || scan_stmts_for_io(body) =>
            {
                return true;
            }
            HirStmt::Each(_, iterable, body, _)
                if scan_expr_for_io(iterable) || scan_stmts_for_io(body) =>
            {
                return true;
            }
            HirStmt::FnDecl(f) if scan_stmts_for_io(&f.body) => return true,
            _ => {}
        }
    }
    false
}

fn scan_expr_for_io(expr: &HirExpr) -> bool {
    match expr {
        HirExpr::Call(callee, args, _) => {
            match &**callee {
                HirExpr::Identifier(name, _) if name == "print" || name == "println" => {
                    return true;
                }
                HirExpr::Dot(base, member, _) if member == "print" || member == "println" => {
                    if let HirExpr::Identifier(base_name, _) = &**base
                        && base_name == "io"
                    {
                        return true;
                    }
                }
                _ => {}
            }
            if scan_expr_for_io(callee) {
                return true;
            }
            for arg in args {
                if scan_expr_for_io(&arg.value) {
                    return true;
                }
            }
            false
        }
        HirExpr::Unary(_, inner, _) | HirExpr::Await(inner, _) => scan_expr_for_io(inner),
        HirExpr::Binary(_, left, right, _) => scan_expr_for_io(left) || scan_expr_for_io(right),
        HirExpr::If(cond, then_e, else_e, _) => {
            scan_expr_for_io(cond) || scan_expr_for_io(then_e) || scan_expr_for_io(else_e)
        }
        HirExpr::Construct(_, fields, _) => {
            fields.iter().any(|(_, f_expr)| scan_expr_for_io(f_expr))
        }
        HirExpr::Dot(receiver, _, _) | HirExpr::QuestionDot(receiver, _, _) => {
            scan_expr_for_io(receiver)
        }
        HirExpr::List(items, _) => items.iter().any(scan_expr_for_io),
        HirExpr::Index(target, index, _) => scan_expr_for_io(target) || scan_expr_for_io(index),
        _ => false,
    }
}

/// Builds the linear memory bump allocator function `__aipo_alloc(size: i32) -> i32`.
fn build_allocator_function() -> Function {
    let mut alloc_fn = Function::new([(2, ValType::I32)]);
    // local 0: size (param)
    // local 1: old_ptr
    // local 2: new_ptr

    // Align size to multiple of 8: size = (size + 7) & -8
    alloc_fn.instruction(&Instruction::LocalGet(0));
    alloc_fn.instruction(&Instruction::I32Const(7));
    alloc_fn.instruction(&Instruction::I32Add);
    alloc_fn.instruction(&Instruction::I32Const(-8));
    alloc_fn.instruction(&Instruction::I32And);
    alloc_fn.instruction(&Instruction::LocalSet(0));

    // old_ptr = global.get 0
    alloc_fn.instruction(&Instruction::GlobalGet(0));
    alloc_fn.instruction(&Instruction::LocalSet(1));

    // new_ptr = old_ptr + size
    alloc_fn.instruction(&Instruction::LocalGet(1));
    alloc_fn.instruction(&Instruction::LocalGet(0));
    alloc_fn.instruction(&Instruction::I32Add);
    alloc_fn.instruction(&Instruction::LocalSet(2));

    // Check if new_ptr > (memory.size << 16)
    // If so, grow memory dynamically
    alloc_fn.instruction(&Instruction::LocalGet(2));
    alloc_fn.instruction(&Instruction::MemorySize(0));
    alloc_fn.instruction(&Instruction::I32Const(16));
    alloc_fn.instruction(&Instruction::I32Shl);
    alloc_fn.instruction(&Instruction::I32GtU);
    alloc_fn.instruction(&Instruction::If(BlockType::Empty));

    // delta_pages = ((new_ptr - (memory.size << 16)) + 65535) >> 16
    alloc_fn.instruction(&Instruction::LocalGet(2));
    alloc_fn.instruction(&Instruction::MemorySize(0));
    alloc_fn.instruction(&Instruction::I32Const(16));
    alloc_fn.instruction(&Instruction::I32Shl);
    alloc_fn.instruction(&Instruction::I32Sub);
    alloc_fn.instruction(&Instruction::I32Const(65535));
    alloc_fn.instruction(&Instruction::I32Add);
    alloc_fn.instruction(&Instruction::I32Const(16));
    alloc_fn.instruction(&Instruction::I32ShrU);
    alloc_fn.instruction(&Instruction::MemoryGrow(0));
    alloc_fn.instruction(&Instruction::I32Const(-1));
    alloc_fn.instruction(&Instruction::I32Eq);
    alloc_fn.instruction(&Instruction::If(BlockType::Empty));
    alloc_fn.instruction(&Instruction::Unreachable); // Trap on out-of-memory
    alloc_fn.instruction(&Instruction::End);
    alloc_fn.instruction(&Instruction::End);

    // global.set 0 new_ptr
    alloc_fn.instruction(&Instruction::LocalGet(2));
    alloc_fn.instruction(&Instruction::GlobalSet(0));

    // return old_ptr
    alloc_fn.instruction(&Instruction::LocalGet(1));
    alloc_fn.instruction(&Instruction::End);

    alloc_fn
}

/// Builds the `__aipo_task_create(fn_table_idx: i32, arg_count: i32, arg0: i64, arg1: i64) -> i32` function.
fn build_task_create_function(alloc_func_idx: u32) -> Function {
    // 1 local: local 4 (task_ptr: i32)
    let mut func = Function::new([(1, ValType::I32)]);

    // task_ptr = alloc(48)
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(4));

    // [task_ptr + 0] = status = 0 (Pending)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 4] = fn_table_idx (param 0)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 8] = result = 0 (i64)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // [task_ptr + 16] = state = 0 (i32)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 16,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 20] = arg_count (param 1)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 20,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 24] = arg0 (param 2)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 24,
        align: 3,
        memory_index: 0,
    }));

    // [task_ptr + 32] = arg1 (param 3)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 32,
        align: 3,
        memory_index: 0,
    }));

    // [task_ptr + 40] = next_ptr = 0 (i32)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 40,
        align: 2,
        memory_index: 0,
    }));

    // return task_ptr
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);

    func
}

/// Builds the `__aipo_task_drive(task_ptr: i32) -> i64` function.
fn build_task_drive_function(sig_0: u32, sig_1: u32, sig_2: u32) -> Function {
    // 4 locals:
    // local 1: status (i32)
    // local 2: fn_idx (i32)
    // local 3: arg_count (i32)
    // local 4: res (i64)
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);

    // status = [task_ptr + 0]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    // if status == 2 (Ready) -> return [task_ptr + 8]
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // if status == 4 (Cancelled) -> trap (unreachable)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // mark status = 1 (Running)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // fn_idx = [task_ptr + 4]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // arg_count = [task_ptr + 20]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 20,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // Dispatch via call_indirect based on arg_count
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
    // Arity 0
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::CallIndirect {
        type_index: sig_0,
        table_index: 0,
    });
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
    // Arity 1: load arg0 from [task_ptr + 24]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 24,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::CallIndirect {
        type_index: sig_1,
        table_index: 0,
    });
    func.instruction(&Instruction::Else);
    // Arity 2: load arg0 from [task_ptr + 24], arg1 from [task_ptr + 32]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 24,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 32,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::CallIndirect {
        type_index: sig_2,
        table_index: 0,
    });
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Save result to local 4
    func.instruction(&Instruction::LocalSet(4));

    // [task_ptr + 8] = res
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // mark status = 2 (Ready)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // return res
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);

    func
}

/// Builds the `__aipo_await(task_ptr: i32) -> i64` function.
fn build_await_function(task_drive_func_idx: u32) -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(task_drive_func_idx));
    func.instruction(&Instruction::End);
    func
}

/// Builds the `__aipo_task_sleep(duration_ms: i64) -> i64` function.
/// Advances `__aipo_virtual_time` (Global 1) by `duration_ms` and returns 0.
fn build_task_sleep_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::GlobalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::GlobalSet(1));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds the `__aipo_task_cancel(task_ptr: i32) -> i64` function.
/// Marks the task's status as Cancelled (4) and returns 0.
fn build_task_cancel_function() -> Function {
    let mut func = Function::new([]);
    // [task_ptr + 0] = 4 (Cancelled)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_range_new(start: i64, end: i64) -> i32`.
///
/// A range is a 16-byte heap block: `[start_lo, start_hi, end_lo, end_hi]`,
/// holding the inclusive-exclusive integer bounds produced by the `..` operator.
fn build_range_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // local 2 is the allocated ptr
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(2));

    // store start (local 0) at offset 0
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    // store end (local 1) at offset 8
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_range_len(range_ptr: i32) -> i64`.
///
/// Returns `max(0, end - start)`, the number of steps the range yields.
fn build_range_len_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Sub);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_range_get(range_ptr: i32, index: i32) -> i64`.
///
/// Index `0` reads `start`, any other index reads `end`.
fn build_range_get_function() -> Function {
    let mut func = Function::new([]);
    // if index == 0 { load start } else { load end }
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_new(capacity: i32) -> i32`.
///
/// A dictionary is a heap block laid out as an 8-byte entry count header
/// followed by `capacity` fixed-width 24-byte entries (`hash: i64`,
/// `key: i64`, `value: i64`). Entries are filled in order and searched
/// linearly.
///
/// The hashed key is what lookup compares; the raw key is kept beside it so
/// `keys()` and `each key, value in dict` can hand back the value the program
/// wrote. A hash alone cannot be reversed, and a String key's raw pointer is what
/// the caller expects to receive.
///
/// `ponytail:` linear scan is `O(n)` per lookup. Replace with an open-addressing
/// table once dictionaries above a few dozen entries show up in benchmarks.
fn build_dict_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // bytes = 8 (8-byte aligned entry count header) + capacity * 16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(1));

    // entry count starts at 0
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_set(dict: i32, key: i64, value: i64) -> i64`.
///
/// Replaces the value when the key is already present, otherwise appends a new
/// entry. Returns `value` so the call can stay in expression position.
fn build_dict_set_function() -> Function {
    // params: 0=dict, 1=hash, 2=raw key, 3=value
    // locals: 4=index, 5=entry_count
    let mut func = Function::new([(2, ValType::I32)]);

    // entry_count = dict[0]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));

    // Scan for an existing key, remembering the first free slot.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    // block $done
    func.instruction(&Instruction::Block(BlockType::Empty));
    // loop
    func.instruction(&Instruction::Loop(BlockType::Empty));

    // if index == entry_count then leave the loop
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // entry key = dict[4 + index*16]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    // key matched: overwrite the value in place.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));
    // Return `value` immediately without appending or altering entry_count.
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // index += 1
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Append at index: hash first, then the raw key beside it.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));

    // entry_count = index + 1
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_get(dict: i32, key: i64) -> i64`.
///
/// Returns the stored value, or `none` (0) when the key is absent.
fn build_dict_get_function() -> Function {
    // locals: 0=dict, 1=key, 2=index, 3=entry_count
    let mut func = Function::new([(2, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_len(dict: i32) -> i64`.
fn build_dict_len_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_has(dict: i32, key: i64) -> i32`.
///
/// Returns 1 when the key is present, 0 otherwise.
fn build_dict_has_function() -> Function {
    // locals: 0=dict, 1=key, 2=index, 3=entry_count
    let mut func = Function::new([(2, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_is_empty(dict: i32) -> i32`.
fn build_dict_is_empty_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_remove(dict: i32, key: i64) -> i64`.
///
/// Removes first entry matching `key` by shifting subsequent entries down.
/// Returns 1 if removed, 0 if not found.
fn build_dict_remove_function() -> Function {
    // locals: 0=dict, 1=key, 2=index, 3=entry_count, 4=j
    let mut func = Function::new([(3, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // entry_addr = dict + 8 + index * 16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));

    // Shift entries down: j = index; while j < entry_count-1
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // copy entry j+1 key to entry j key
    // dest = dict + 8 + j*16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    // src key = dict + 8 + (j+1)*16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    // copy entry j+1 value to entry j value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End); // loop
    func.instruction(&Instruction::End); // block

    // Decrement entry_count
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::Return);

    func.instruction(&Instruction::End); // if

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End); // loop
    func.instruction(&Instruction::End); // block

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_keys(dict: i32) -> i32` (returns a List).
fn build_dict_keys_function(list_new_func_idx: u32, list_set_func_idx: u32) -> Function {
    // locals: 0=dict, 1=count, 2=list, 3=i
    let mut func = Function::new([(3, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(list_new_func_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // key = dict[8 + i*16]
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Call(list_set_func_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_key_at(dict: i32, index: i32) -> i64` (returns the raw key).
/// Compiles a dictionary key onto the stack as `(hash: i64, raw: i64)`.
///
/// The hash answers lookup, and the raw value is what iteration hands back. A
/// String key's raw value is its pointer; a non-String key hashes to itself.
#[allow(clippy::too_many_arguments)]
fn compile_dict_key_pair(
    key: &HirExpr,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    let key_kind = infer_expr_kind(key, locals, functions, table_indices, anon_map);
    let key_ty = compile_expr(
        key,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        struct_depth,
        call_depth,
    )?;
    let raw_slot = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
    if matches!(key_kind, LocalKind::String) {
        // Keep the pointer for the raw key, and hash a copy for lookup.
        let ptr_slot = locals[&format!("__call_temp_{}", call_depth.min(7))].0;
        coerce_type(func, key_ty, WasmType::I32);
        func.instruction(&Instruction::LocalSet(ptr_slot));
        func.instruction(&Instruction::LocalGet(ptr_slot));
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::LocalSet(raw_slot));
        // Push in the callee's order: hash first, then the raw key.
        func.instruction(&Instruction::LocalGet(ptr_slot));
        let (string_hash_idx, _, _) = functions["__aipo_string_hash"];
        func.instruction(&Instruction::Call(string_hash_idx));
        func.instruction(&Instruction::LocalGet(raw_slot));
    } else {
        // A scalar key hashes to itself, so the two pushes carry the same value.
        coerce_type(func, key_ty, WasmType::I64);
        func.instruction(&Instruction::LocalSet(raw_slot));
        func.instruction(&Instruction::LocalGet(raw_slot));
        func.instruction(&Instruction::LocalGet(raw_slot));
    }
    Ok(())
}

fn build_dict_key_at_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_value_at(dict: i32, index: i32) -> i64`.
fn build_dict_value_at_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_values(dict: i32) -> i32` (returns a List).
fn build_dict_values_function(list_new_func_idx: u32, list_set_func_idx: u32) -> Function {
    // locals: 0=dict, 1=count, 2=list, 3=i
    let mut func = Function::new([(3, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(list_new_func_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // value = dict header + i*24 + offset 16
    // value = dict + 8 (header) + i*24 + 16
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Call(list_set_func_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_string_hash(ptr: i32) -> i64`.
///
/// FNV-1a 64-bit hash over the string bytes so identical strings with different
/// pointers can collide onto the same key bucket when hashing is introduced.
fn build_string_hash_function() -> Function {
    // locals: 0=ptr, 1=hash(i64), 2=len(i32), 3=i(i32)
    let mut func = Function::new([(1, ValType::I64), (2, ValType::I32)]);

    // hash = 0xcbf29ce484222325 (FNV offset basis)
    func.instruction(&Instruction::I64Const(-3750763034362895579i64));
    func.instruction(&Instruction::LocalSet(1));

    // len = [ptr+0]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // i = 0
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // hash = (hash ^ byte) * 0x100000001b3
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Xor);
    func.instruction(&Instruction::I64Const(1099511628211i64));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_new(len: i32) -> i32`.
///
/// A list value is an 8-byte handle block `[data_ptr: i32, pad: i32]` in linear memory.
/// The `data_ptr` points to the dynamically resizable buffer `[len: i32, cap: i32, elements: [i64; cap]]`.
/// Handlers never change address across growth, ensuring reference identity is preserved.
fn build_list_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(4, ValType::I32)]);
    // local 0: len (param)
    // local 1: cap
    // local 2: buf_size
    // local 3: buf_ptr
    // local 4: handle_ptr

    // cap = if len < 8 { 8 } else { len }
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(1));

    // buf_size = 8 + cap * 8
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));

    // buf_ptr = alloc(buf_size)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(3));

    // store len at buf_ptr + 0
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // store cap at buf_ptr + 4
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    // handle_ptr = alloc(8)
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(4));

    // store buf_ptr at handle_ptr + 0
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // return handle_ptr
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_get(list_ptr: i32, index: i32) -> i64`.
fn build_list_get_function() -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: buf_ptr

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // bounds check: if index < 0 trap
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // bounds check: if index >= len trap
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // element address = buf_ptr + 8 + index * 8
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_len(list_ptr: i32) -> i64`.
fn build_list_len_function() -> Function {
    let mut func = Function::new([]);
    // local 0: handle_ptr
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_set(list_ptr: i32, index: i32, value: i64) -> i64`.
fn build_list_set_function() -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: value
    // local 3: buf_ptr

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // bounds check: if index < 0 or index >= len trap
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // element address = buf_ptr + 8 + index * 8
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_add(list_ptr: i32, value: i64) -> i64`.
///
/// Appends an item to the list, expanding the backing buffer exponentially (2x) when full.
fn build_list_add_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(6, ValType::I32)]);
    // local 0: handle_ptr (param)
    // local 1: val (param i64)
    // local 2: buf_ptr
    // local 3: len
    // local 4: cap
    // local 5: new_cap
    // local 6: new_buf
    // local 7: i

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len = i32.load(buf_ptr + 0)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // cap = i32.load(buf_ptr + 4)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    // if len >= cap { grow }
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));

    // new_cap = if cap <= 0 { 8 } else { cap * 2 }
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LeS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    // new_buf = alloc(8 + new_cap * 8)
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(6));

    // new_buf[0] = len
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // new_buf[4] = new_cap
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    // copy loop: i = 0; while i < len { new_buf[8 + i*8] = buf_ptr[8 + i*8]; i++ }
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // dst = new_buf + (i << 3)
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);

    // src = buf_ptr + (i << 3)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // i += 1
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End); // loop
    func.instruction(&Instruction::End); // block

    // handle_ptr[0] = new_buf
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // buf_ptr = new_buf
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::End); // if len >= cap

    // store val at buf_ptr + 8 + (len << 3)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // update len at buf_ptr + 0 = len + 1
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_contains(list_ptr: i32, value: i64) -> i32`.
fn build_list_contains_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: value
    // local 2: buf_ptr
    // local 3: len
    // local 4: i

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len = i32.load(buf_ptr + 0)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_insert(list_ptr: i32, index: i32, value: i64) -> i64`.
fn build_list_insert_function(
    list_add_idx: u32,
    list_len_idx: u32,
    list_get_idx: u32,
    list_set_idx: u32,
) -> Function {
    let mut func = Function::new([(2, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: value
    // local 3: old_len
    // local 4: i

    // Append a dummy to grow and update len, then shift right.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::BrIf(1));

    // list[i + 1] = list[i]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::Call(list_set_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // list[index] = value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(list_set_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_remove_last(list_ptr: i32) -> i64`.
fn build_list_remove_last_function() -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    // local 0: handle_ptr
    // local 1: buf_ptr
    // local 2: len
    // local 3: index
    // local 4: value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_clear(list_ptr: i32) -> i64`.
fn build_list_clear_function() -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_remove_at(list_ptr: i32, index: i32) -> i64`.
fn build_list_remove_at_function() -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: buf_ptr
    // local 3: len
    // local 4: i
    // local 5: value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_remove(list_ptr: i32, value: i64) -> i64`.
fn build_list_remove_function(list_remove_at_idx: u32) -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: value
    // local 2: buf_ptr
    // local 3: len
    // local 4: i
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_remove_at_idx));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_count(list_ptr: i32, value: i64) -> i64`.
fn build_list_count_function() -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_find(list_ptr: i32, value: i64) -> i64`.
fn build_list_find_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: handle, local 1: value, local 2: buf, local 3: len, local 4: i
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I64Const(-1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64ExtendI32S);
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::I64Const(-1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_reverse(list_ptr: i32) -> i64`.
fn build_list_reverse_function() -> Function {
    let mut func = Function::new([(4, ValType::I32), (1, ValType::I64)]);
    // local 0: handle, local 1: buf, local 2: len, local 3: i, local 4: j, local 5: tmp
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    // Reversing mutates in place and yields the same list handle, so the result
    // keeps its `i32` representation and can be chained.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_transform(list_ptr: i32, callback: i32) -> i32`.
fn build_list_transform_function(
    list_new_idx: u32,
    list_len_idx: u32,
    list_get_idx: u32,
    list_set_idx: u32,
    callback_type_idx: u32,
) -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: list
    // local 1: callback table index
    // local 2: new list
    // local 3: len
    // local 4: i

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::Call(list_set_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_filter(list_ptr: i32, callback: i32) -> i32`.
fn build_list_filter_function(
    list_new_idx: u32,
    list_len_idx: u32,
    list_get_idx: u32,
    list_add_idx: u32,
    callback_type_idx: u32,
) -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    // local 0: list
    // local 1: callback table index
    // local 2: new list
    // local 3: len
    // local 4: i
    // local 5: item

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_sort(list_ptr: i32) -> i32` (signed i64 insertion sort).
fn build_list_sort_function() -> Function {
    let mut func = Function::new([(4, ValType::I32), (1, ValType::I64)]);
    // local 0: list
    // local 1: buf
    // local 2: len
    // local 3: i
    // local 4: j
    // local 5: key
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_sort_by(list_ptr: i32, callback: i32) -> i32`.
fn build_list_sort_by_function(callback_type_idx: u32) -> Function {
    let mut func = Function::new([(4, ValType::I32), (3, ValType::I64)]);
    // local 0: list
    // local 1: callback
    // local 2: buf
    // local 3: len
    // local 4: i
    // local 5: j
    // local 6: key_i
    // local 7: key_i_sort
    // local 8: key_j_sort
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_concat(ptr_a: i32, ptr_b: i32) -> i32`.
fn build_string_concat_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(5, ValType::I32)]);
    // local 0: ptr_a (param)
    // local 1: ptr_b (param)
    // local 2: len_a
    // local 3: len_b
    // local 4: total_len
    // local 5: new_ptr
    // local 6: i

    // len_a = i32.load(ptr_a)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len_b = i32.load(ptr_b)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // total_len = len_a + len_b
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));

    // new_ptr = alloc(total_len + 4)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(5));

    // store total_len at new_ptr + 0
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // copy bytes from a: loop i from 0 to len_a - 1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // copy bytes from b: loop i from 0 to len_b - 1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_int_to_str(val: i64) -> i32`.
/// Builds `__aipo_str_to_int(ptr: i32) -> i64`.
/// Prefix of the message `Int(text)` reports for text that is not a number.
const INT_PARSE_PREFIX: &str = "invalid integer text: \"";

/// Prefix and suffix of the message a violated `invariant()` reports, so the
/// handler sees the same wording the VM produces.
const INVARIANT_PREFIX: &str = "invariant() of ";
const INVARIANT_SUFFIX: &str = " did not hold after mutation";

fn build_str_to_int_function(
    failure_status_global: u32,
    failure_message_global: u32,
    concat_func_idx: u32,
    prefix_ptr: i32,
    quote_ptr: i32,
) -> Function {
    // locals: 0=ptr (param), 1=len, 2=i, 3=result(i64), 4=sign, 5=digit, 6=started
    let mut func = Function::new([(2, ValType::I32), (2, ValType::I64), (2, ValType::I32)]);
    // local 0: ptr
    // local 1: len (i32)
    // local 2: i (i32)
    // local 3: result (i64)
    // local 4: sign (i64, 1 or -1)
    // local 5: digit (i32)
    // local 6: started (i32)

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    // skip leading spaces
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // sign
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(45)); // '-'
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I64Const(-1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // parse digits
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(9));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(3));
    // Canon makes `Int(text)` a `Failure` when the text is not a number, so a
    // `or_else` fallback and an `attempt` handler both have to see it. The message
    // quotes the offending text, matching the wording the VM produces, so the
    // handler sees the same message on every backend.
    func.instruction(&Instruction::I32Const(prefix_ptr));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(concat_func_idx));
    func.instruction(&Instruction::I32Const(quote_ptr));
    func.instruction(&Instruction::Call(concat_func_idx));
    func.instruction(&Instruction::GlobalSet(failure_message_global));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::GlobalSet(failure_status_global));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_case(ptr: i32, upper: i32) -> i32`.
///
/// ASCII case folding over a fresh string. A non-ASCII byte is copied unchanged,
/// which keeps a multi-byte sequence intact.
fn build_str_case_function(alloc_func_idx: u32) -> Function {
    // params: 0=ptr, 1=upper
    // locals: 2=len, 3=out, 4=i, 5=byte
    let mut func = Function::new([(4, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));

    // `a..=z` folds to upper; `A..=Z` folds to lower. Both directions are the
    // same XOR 0x20, so the choice is only which range triggers it.
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(97));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(122));
    func.instruction(&Instruction::I32LeU);
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(65));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(90));
    func.instruction(&Instruction::I32LeU);
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(0x20));
    func.instruction(&Instruction::I32Xor);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_find(haystack: i32, needle: i32) -> i64`.
///
/// Returns the code-point index of the first occurrence, or `-1` for none — the
/// canonical `string.find` answer, which a caller reads as a plain Int.
fn build_str_find_function() -> Function {
    // params: 0=haystack, 1=needle
    // locals: 2=h_len, 3=n_len, 4=i, 5=j, 6=byte, 7=hit
    let mut func = Function::new([(6, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::If(BlockType::Empty));
    // Report a code-point index, matching `string.find`'s canonical answer.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64ExtendI32S);
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Copies `len` bytes from `src + src_off` into a fresh pool string.
///
/// `strings` are `[len: i32][bytes]`, so a builder needs the byte count and the
/// source offset rather than the `(start, end)` code-point window a slice takes.
fn build_str_copy_function(alloc_func_idx: u32) -> Function {
    // params: 0=src, 1=src_off, 2=len
    // locals: 3=out, 4=i, 5=byte
    let mut func = Function::new([(3, ValType::I32)]);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_reverse(ptr: i32, copy_idx: u32) -> i32`.
///
/// Reverses by code point, so a multi-byte character keeps its byte order.
fn build_str_reverse_function(copy_idx: u32, alloc_func_idx: u32) -> Function {
    // params: 0=ptr
    // locals: 1=len, 2=out, 3=cursor(dst), 4=pos, 5=char_start, 6=byte, 7=step, 8=i
    let mut func = Function::new([(8, ValType::I32)]);
    let _ = copy_idx;

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // Walk the source forward once, recording each code-point start so the second
    // pass can replay the boundaries from the end. A reverse over raw bytes would
    // cut a multi-byte sequence in half.
    let mut starts: Vec<u32> = Vec::new();
    let _ = &mut starts;
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    // char_start = pos; advance pos past one whole character.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Second pass: walk the same boundaries backwards, copying each character to
    // the output cursor in reverse order.
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    // Retreat to the start of the character that ends at `pos`.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0xC0));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Copy bytes [5, 4) to the output cursor.
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_split(text: i32, sep: i32, list_new: u32, list_set: u32) -> i32`.
///
/// Splits on every occurrence of a non-empty separator and returns a List of the
/// pieces, in order. The separator itself is never part of a piece.
/// Builds `__aipo_str_join(sep: i32, list: i32) -> i32`.
///
/// Concatenates every element (a String pointer) with `sep` between them.
fn build_str_join_function(list_len_idx: u32, list_get_idx: u32, concat_idx: u32) -> Function {
    // params: 0=sep, 1=list
    // locals: 2=len, 3=i, 4=acc, 5=elem
    let mut func = Function::new([(4, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(2));

    // Start from the empty string so the first concatenation has a left operand.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_replace(text: i32, from: i32, to: i32, ...) -> i32`.
///
/// Replaces every non-overlapping occurrence of `from`. The piece before each
/// match and the replacement are concatenated onto the accumulator.
fn build_str_replace_function(copy_idx: u32, concat_idx: u32) -> Function {
    // params: 0=text, 1=from, 2=to
    // locals: 3=len, 4=from_len, 5=pos, 6=acc, 7=i, 8=match, 9=piece_start
    let mut func = Function::new([(7, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(6));

    // An empty pattern has no meaningful replacement, so the text is unchanged.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::If(BlockType::Empty));
    // text[piece_start..5] + to
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(1));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // The tail after the last match.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::End);
    func
}

fn build_str_split_function(list_new_idx: u32, list_add_idx: u32, copy_idx: u32) -> Function {
    // params: 0=text, 1=sep
    // locals: 2=len, 3=sep_len, 4=piece_start, 5=pos, 6=list, 7=count, 8=i, 9=match
    let mut func = Function::new([(8, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // A fresh list starts empty: the length is the element count, so the capacity
    // argument here must be zero and not the initial capacity's minimum.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(6));

    // An empty separator has no meaningful split, so the whole text is one piece.
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::BrIf(1));

    // Compare the separator against `text[5..5+sep_len]`.
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::If(BlockType::Empty));
    // A match closes the current piece at `5` and resumes past the separator.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(1));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // The trailing piece runs from `4` to the end.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_starts(haystack: i32, needle: i32) -> i32`.
///
/// Returns 1 when `haystack` begins with `needle`. `__aipo_str_ends` is the same
/// comparison anchored at the tail.
fn build_str_affix_function(at_end: bool) -> Function {
    // params: 0=haystack, 1=needle
    // locals: 2=h_len, 3=n_len, 4=offset, 5=i, 6=byte
    let mut func = Function::new([(5, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // A needle longer than the haystack cannot be an affix.
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    if at_end {
        func.instruction(&Instruction::LocalGet(2));
        func.instruction(&Instruction::LocalGet(3));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(4));
    } else {
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(4));
    }

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_trim(ptr: i32) -> i32`.
///
/// Drops leading and trailing whitespace, returning a fresh string. Only the ends
/// are trimmed, so an interior run is preserved.
fn build_str_trim_function(alloc_func_idx: u32) -> Function {
    // params: 0=ptr
    // locals: 1=len, 2=start, 3=end, 4=out, 5=i, 6=byte
    let mut func = Function::new([(6, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    // start: advance while the byte is whitespace (space, or < 0x20).
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32GtU);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // end: retreat from the tail while the byte is whitespace.
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32LeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32GtU);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // out = alloc(4 + (end - start))
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

fn build_int_to_str_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(5, ValType::I32), (2, ValType::I64)]);
    // local 0: val (param, i64)
    // local 1: is_neg (i32)
    // local 2: digits_len (i32)
    // local 3: total_len (i32)
    // local 4: ptr (i32)
    // local 5: idx (i32)
    // local 6: uval (i64)
    // local 7: temp (i64)

    // Check if val < 0
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Sub);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::End);

    // If uval == 0, digits_len = 1
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(7));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64DivU);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // total_len = digits_len + is_neg
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));

    // ptr = __aipo_alloc(total_len + 4)
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(4));

    // store total_len at ptr + 0
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // If is_neg == 1, store '-' (45) at ptr + 4
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(45));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);

    // If uval == 0, store '0' (48) at ptr + 4 + is_neg
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(7));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64RemU);
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64DivU);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bool_to_str(val: i32) -> i32`.
fn build_bool_to_str_function(true_ptr: i32, false_ptr: i32) -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(true_ptr));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(false_ptr));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_eq(ptr_a: i32, ptr_b: i32) -> i32`.
fn build_string_eq_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: ptr_a (param)
    // local 1: ptr_b (param)
    // local 2: len_a
    // local 3: len_b
    // local 4: i

    // if ptr_a == ptr_b return 1
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // len_a = i32.load(ptr_a)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len_b = i32.load(ptr_b)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // if len_a != len_b return 0
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // loop i from 0 to len_a - 1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_new(len: i32) -> i32`.
fn build_bytes_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(2, ValType::I32)]);
    // local 0: len (param)
    // local 1: ptr
    // local 2: i

    // alloc size = len + 4
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(1));

    // store len at ptr + 0
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // Zero out bytes: loop i = 0..len-1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_get(buf_ptr: i32, index: i32) -> i64`.
fn build_bytes_get_function() -> Function {
    let mut func = Function::new([]);
    // local 0: ptr (i32)
    // local 1: index (i32)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_set(buf_ptr: i32, index: i32, val: i64) -> i64`.
fn build_bytes_set_function() -> Function {
    let mut func = Function::new([]);
    // local 0: ptr (i32)
    // local 1: index (i32)
    // local 2: val (i64)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_len(buf_ptr: i32) -> i64`.
fn build_bytes_len_function() -> Function {
    let mut func = Function::new([]);
    // local 0: ptr (i32)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_slice(buf: i32, start: i32, end: i32) -> i32`.
///
/// Returns a fresh managed block holding `buf[start..end]`, clamped to the source
/// length. `Bytes` is mutable and by value in canon, so a slice must not alias the
/// original: writing to the window cannot disturb the base block.
fn build_bytes_slice_function(alloc_func_idx: u32) -> Function {
    // locals: 0=buf, 1=start, 2=end, 3=len, 4=clamped_start, 5=clamped_end,
    //         6=out, 7=i
    let mut func = Function::new([(5, ValType::I32)]);

    // len = load(buf)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // clamped_start = clamp(start, 0, len)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(4));

    // clamped_end = clamp(end, clamped_start, len)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    // out = alloc(4 + (clamped_end - clamped_start))
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(6));

    // store len at out + 0
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // copy loop
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_slice(list_ptr: i32, start: i32, end: i32) -> i32`.
///
/// Returns a fresh list holding `list[start..end]`, with both bounds clamped into
/// `0..=len`. Canon slicing is tolerant, so an out-of-range bound narrows to the
/// nearest valid edge instead of trapping.
fn build_list_slice_function(list_new_idx: u32, list_add_idx: u32, list_get_idx: u32) -> Function {
    // params: 0=list, 1=start, 2=end
    // locals: 3=len, 4=lo, 5=hi, 6=out, 7=i
    let mut func = Function::new([(5, ValType::I32)]);
    const LEN: u32 = 3;
    const LO: u32 = 4;
    const HI: u32 = 5;
    const OUT: u32 = 6;
    const I: u32 = 7;

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(LEN));

    // lo = clamp(start, 0, len)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(LO));

    // hi = clamp(end, lo, len)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(LO));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(LO));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(HI));

    // out = list_new(0), then grow through `list_add`. `list_new(n)` already
    // reports `n` as the length, so passing the window size here and then
    // appending would double it.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(OUT));

    func.instruction(&Instruction::LocalGet(LO));
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::LocalGet(HI));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // out = list.add(out, list_get(list, i))
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_char(ptr: i32, index: i32) -> i32`.
///
/// Returns a fresh one-character string holding the code point at `index`. The
/// index is a *code point* position in canon, but the pool stores UTF-8 bytes, so
/// the walk advances by whole encoded characters and an out-of-range index yields
/// the empty string instead of trapping.
fn build_str_char_function(alloc_func_idx: u32) -> Function {
    // params: 0=ptr, 1=index
    // locals: 2=len, 3=i, 4=cursor, 5=step, 6=byte, 7=out
    let mut func = Function::new([(6, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // Walk forward `index` characters to find the byte offset of the target.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));

    // UTF-8 leading byte decides the encoded length.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // A cursor past the end means the index named no character.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // The walk above only sets `step` while it advances, so index `0` reaches
    // here with no length recorded. Measure the character at the cursor directly.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    // out = alloc(4 + step)
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(7));

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // Copy the encoded character byte by byte.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_slice(ptr: i32, start: i32, end: i32) -> i32`.
///
/// Slices by code point, clamping both bounds into `0..=len`, and returns a fresh
/// string. Slicing by byte offset would cut multi-byte characters in half.
fn build_str_slice_function(alloc_func_idx: u32) -> Function {
    // params: 0=ptr, 1=start, 2=end
    // locals: 3=len, 4=i, 5=cursor, 6=step, 7=byte, 8=lo, 9=hi, 10=out, 11=n,
    //         12=code points
    let mut func = Function::new([(10, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // Count code points, then normalize both bounds against that count.
    //
    // A negative bound counts from the end, and canonical slicing clamps rather
    // than faulting, so `word[-2..99]` is the last two characters instead of an
    // empty window. The walk below compares code-point indices, so the count has
    // to be in code points and not in bytes.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(12));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    for bound in [1u32, 2u32] {
        func.instruction(&Instruction::LocalGet(bound));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(12));
        func.instruction(&Instruction::LocalGet(bound));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(bound));
        func.instruction(&Instruction::LocalGet(bound));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(bound));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    // Walk to the start bound, recording the byte offset.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(8));

    // Walk on to the end bound.
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(9));

    // n = hi - lo; an inverted window is empty rather than a trap.
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(11));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(11));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(10));
    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_len(ptr: i32) -> i64`.
///
/// Counts code points rather than UTF-8 bytes: only bytes that are not
/// continuation bytes (`0b10xxxxxx`) start a character.
fn build_str_len_function() -> Function {
    // params: 0=ptr
    // locals: 1=bytes, 2=i, 3=count, 4=byte
    let mut func = Function::new([(4, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    // 0b1100_0000 == 0xC0 marks a continuation byte; only leading bytes count.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0xC0));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}

/// Builds a wrapper function for an `async fn` that creates a `Task` handle and returns it (`task_ptr: i32`).
fn build_async_wrapper_function(
    inner_table_idx: u32,
    param_types: &[WasmType],
    task_create_func_idx: u32,
) -> Function {
    let param_count = param_types.len();
    let mut func = Function::new([]);
    // arg 0: fn_table_idx
    func.instruction(&Instruction::I32Const(inner_table_idx as i32));

    // arg 1: arg_count
    func.instruction(&Instruction::I32Const(param_count as i32));

    // arg 2: arg0 — `task_create` carries both arguments as `i64`, so a pointer
    // parameter is widened rather than left as `i32`.
    for (index, ty) in [(0usize, param_types.first()), (1usize, param_types.get(1))] {
        match ty {
            Some(WasmType::I64) => {
                func.instruction(&Instruction::LocalGet(index as u32));
            }
            Some(WasmType::F64) => {
                func.instruction(&Instruction::LocalGet(index as u32));
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Some(WasmType::I32) => {
                func.instruction(&Instruction::LocalGet(index as u32));
                func.instruction(&Instruction::I64ExtendI32U);
            }
            None => {
                func.instruction(&Instruction::I64Const(0));
            }
        }
    }

    // call __aipo_task_create
    func.instruction(&Instruction::Call(task_create_func_idx));
    func.instruction(&Instruction::End);
    func
}

/// Collects static string literals into the linear memory data segment pool.
fn collect_program_strings(
    program: &HirProgram,
    func_decls: &[HirFunctionDecl],
    static_strings: &mut HashMap<String, i32>,
    data_segments: &mut Vec<(i32, Vec<u8>)>,
    next_offset: &mut i32,
) {
    for item in &program.items {
        if let HirItem::Enum(e) = item {
            for v in &e.variants {
                let full = format!("{}.{}", e.name, v.name);
                add_static_string(&full, static_strings, data_segments, next_offset);
            }
        }
    }
    for func in func_decls {
        // A default is compiled at the call site, so its literals belong to the
        // pool exactly like a body literal does.
        for param in &func.params {
            if let Some(default) = &param.default {
                collect_expr_strings(default, static_strings, data_segments, next_offset);
            }
        }
        collect_stmts_strings(&func.body, static_strings, data_segments, next_offset);
    }
    collect_stmts_strings(
        &program.statements,
        static_strings,
        data_segments,
        next_offset,
    );
    // Ensure boolean string representations are always present in the static pool
    add_static_string("true", static_strings, data_segments, next_offset);
    add_static_string("false", static_strings, data_segments, next_offset);
    // `none` prints as its own word wherever a value of unknown type is rendered.
    add_static_string("none", static_strings, data_segments, next_offset);
    // Collection rendering punctuation, shared by every shape that prints them.
    for punctuation in ["[", "]", ", ", "{", "}", ": "] {
        add_static_string(punctuation, static_strings, data_segments, next_offset);
    }
}

fn collect_stmts_strings(
    stmts: &[HirStmt],
    static_strings: &mut HashMap<String, i32>,
    data_segments: &mut Vec<(i32, Vec<u8>)>,
    next_offset: &mut i32,
) {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(_, expr, _) | HirStmt::Var(_, expr, _) | HirStmt::Expr(expr) => {
                collect_expr_strings(expr, static_strings, data_segments, next_offset);
            }
            HirStmt::Assign(target, expr, _) | HirStmt::CompoundAssign(_, target, expr, _) => {
                collect_expr_strings(target, static_strings, data_segments, next_offset);
                collect_expr_strings(expr, static_strings, data_segments, next_offset);
            }
            HirStmt::Return(Some(expr), _) => {
                collect_expr_strings(expr, static_strings, data_segments, next_offset);
            }
            HirStmt::If(s) => {
                collect_expr_strings(&s.condition, static_strings, data_segments, next_offset);
                collect_stmts_strings(&s.then_branch, static_strings, data_segments, next_offset);
                for (cond, body) in &s.elif_branches {
                    collect_expr_strings(cond, static_strings, data_segments, next_offset);
                    collect_stmts_strings(body, static_strings, data_segments, next_offset);
                }
                if let Some(else_branch) = &s.else_branch {
                    collect_stmts_strings(else_branch, static_strings, data_segments, next_offset);
                }
            }
            HirStmt::While(cond, body, _) => {
                collect_expr_strings(cond, static_strings, data_segments, next_offset);
                collect_stmts_strings(body, static_strings, data_segments, next_offset);
            }
            HirStmt::Loop(body, _) => {
                collect_stmts_strings(body, static_strings, data_segments, next_offset);
            }
            HirStmt::Repeat(count, _, body, _) => {
                collect_expr_strings(count, static_strings, data_segments, next_offset);
                collect_stmts_strings(body, static_strings, data_segments, next_offset);
            }
            HirStmt::Each(_, iterable, body, _) => {
                collect_expr_strings(iterable, static_strings, data_segments, next_offset);
                collect_stmts_strings(body, static_strings, data_segments, next_offset);
            }
            HirStmt::Fail(expr, _) => {
                collect_expr_strings(expr, static_strings, data_segments, next_offset);
            }
            HirStmt::Attempt(a) => {
                collect_stmts_strings(&a.body, static_strings, data_segments, next_offset);
                collect_stmts_strings(&a.handler, static_strings, data_segments, next_offset);
            }
            HirStmt::Match(match_stmt) => {
                collect_expr_strings(
                    &match_stmt.target,
                    static_strings,
                    data_segments,
                    next_offset,
                );
                for arm in &match_stmt.when_arms {
                    for p in &arm.patterns {
                        match p {
                            aipo_hir::HirMatchPattern::Value(expr) => {
                                collect_expr_strings(
                                    expr,
                                    static_strings,
                                    data_segments,
                                    next_offset,
                                );
                            }
                            aipo_hir::HirMatchPattern::Variant {
                                enum_name,
                                variant_name,
                                ..
                            } => {
                                let full = match enum_name {
                                    Some(e) => format!("{e}.{variant_name}"),
                                    None => variant_name.clone(),
                                };
                                add_static_string(
                                    &full,
                                    static_strings,
                                    data_segments,
                                    next_offset,
                                );
                            }
                            _ => {}
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        collect_expr_strings(guard, static_strings, data_segments, next_offset);
                    }
                    collect_stmts_strings(&arm.body, static_strings, data_segments, next_offset);
                }
                if let Some(else_body) = &match_stmt.else_arm {
                    collect_stmts_strings(else_body, static_strings, data_segments, next_offset);
                }
            }
            HirStmt::AwaitDo(stmts, _) => {
                collect_stmts_strings(stmts, static_strings, data_segments, next_offset);
            }
            _ => {}
        }
    }
}

fn add_static_string(
    text: &str,
    static_strings: &mut HashMap<String, i32>,
    data_segments: &mut Vec<(i32, Vec<u8>)>,
    next_offset: &mut i32,
) {
    if !static_strings.contains_key(text) {
        let offset = *next_offset;
        let len = text.len() as i32;
        let mut bytes = Vec::with_capacity(4 + text.len());
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());

        data_segments.push((offset, bytes));
        static_strings.insert(text.to_string(), offset);

        *next_offset += (4 + text.len() as i32 + 7) & !7;
    }
}

fn collect_expr_strings(
    expr: &HirExpr,
    static_strings: &mut HashMap<String, i32>,
    data_segments: &mut Vec<(i32, Vec<u8>)>,
    next_offset: &mut i32,
) {
    match expr {
        HirExpr::Await(inner, _) => {
            collect_expr_strings(inner, static_strings, data_segments, next_offset);
        }
        HirExpr::Literal(Literal::String(text, _), _) => {
            add_static_string(text, static_strings, data_segments, next_offset);
        }
        HirExpr::Binary(_, left, right, _) => {
            collect_expr_strings(left, static_strings, data_segments, next_offset);
            collect_expr_strings(right, static_strings, data_segments, next_offset);
        }
        HirExpr::Dict(entries, _) => {
            for (key, value) in entries {
                collect_expr_strings(key, static_strings, data_segments, next_offset);
                collect_expr_strings(value, static_strings, data_segments, next_offset);
            }
        }
        HirExpr::List(elements, _) => {
            for elem in elements {
                collect_expr_strings(elem, static_strings, data_segments, next_offset);
            }
        }
        HirExpr::Index(target, index, _) => {
            collect_expr_strings(target, static_strings, data_segments, next_offset);
            collect_expr_strings(index, static_strings, data_segments, next_offset);
        }
        HirExpr::Unary(_, inner, _) => {
            collect_expr_strings(inner, static_strings, data_segments, next_offset);
        }
        HirExpr::If(cond, then_e, else_e, _) => {
            collect_expr_strings(cond, static_strings, data_segments, next_offset);
            collect_expr_strings(then_e, static_strings, data_segments, next_offset);
            collect_expr_strings(else_e, static_strings, data_segments, next_offset);
        }
        HirExpr::Call(callee, args, _) => {
            collect_expr_strings(callee, static_strings, data_segments, next_offset);
            for arg in args {
                collect_expr_strings(&arg.value, static_strings, data_segments, next_offset);
            }
        }
        HirExpr::Construct(_, fields, _) => {
            for (_, field_expr) in fields {
                collect_expr_strings(field_expr, static_strings, data_segments, next_offset);
            }
        }
        HirExpr::Dot(receiver, _, _) => {
            collect_expr_strings(receiver, static_strings, data_segments, next_offset);
        }
        HirExpr::OrElse(left, right, _) => {
            collect_expr_strings(left, static_strings, data_segments, next_offset);
            collect_expr_strings(right, static_strings, data_segments, next_offset);
        }
        HirExpr::Try(inner, _) => {
            collect_expr_strings(inner, static_strings, data_segments, next_offset);
        }
        _ => {}
    }
}

/// Resolves the semantic kind of a field reached through a known receiver kind.
///
/// Falls back to a program-wide lookup because a destructuring pattern does not
/// name its struct: `when { name }` on a `User` still has to know that `name`
/// holds a `String`.
fn struct_field_kind(
    receiver: &LocalKind,
    field: &str,
    structs: &HashMap<String, StructLayout>,
) -> LocalKind {
    let layout = match receiver {
        LocalKind::Struct(name) => structs.get(name),
        _ => None,
    };
    let field_ty = layout
        .and_then(|layout| layout.fields.get(field).map(|(_, ty)| *ty))
        .or_else(|| {
            structs
                .values()
                .find_map(|layout| layout.fields.get(field).map(|(_, ty)| *ty))
        });
    match field_ty {
        Some(WasmType::F64) => LocalKind::Float,
        Some(WasmType::I32) => LocalKind::String,
        _ => LocalKind::Unknown,
    }
}

/// Resolves the Wasm type for a function parameter.
fn param_wasm_type(param: &HirParam) -> WasmType {
    if param.name == "self" || param.is_self {
        return WasmType::I32;
    }
    if let Some(ty) = &param.type_annotation {
        match ty.name.as_str() {
            "Float" => WasmType::F64,
            "Bool" => WasmType::I32,
            "String" | "Bytes" | "List" | "Dict" | "Range" | "Task" => WasmType::I32,
            _ => WasmType::I64,
        }
    } else {
        WasmType::I64
    }
}

/// Merges two observations of the same parameter into one Wasm type.
///
/// Aipo promotes an `Int` argument to a `Float` parameter, so `i64` and `f64`
/// merge upward. Anything else that disagrees (notably a pointer `i32` against
/// a scalar) is a real contradiction the frontend should have rejected; the
/// widest scalar representation keeps the module valid instead of guessing.
fn merge_hint(current: Option<WasmType>, observed: WasmType) -> Option<WasmType> {
    match current {
        None => Some(observed),
        Some(existing) if existing == observed => Some(existing),
        Some(WasmType::I64) if observed == WasmType::F64 => Some(WasmType::F64),
        Some(WasmType::F64) if observed == WasmType::I64 => Some(WasmType::F64),
        Some(_) => Some(WasmType::I64),
    }
}

/// Static Wasm representation of an expression, when the program alone proves it.
///
/// This is deliberately conservative: `None` means "cannot be proven here", not
/// "untyped". It exists to give unannotated parameters a concrete representation
/// so a `String` argument is not passed as a truncated pointer.
fn static_expr_type(
    expr: &HirExpr,
    locals: &HashMap<String, WasmType>,
    functions: &HashMap<String, (Vec<WasmType>, Option<WasmType>)>,
    structs: &HashMap<String, StructLayout>,
) -> Option<WasmType> {
    match expr {
        HirExpr::Literal(lit, _) => match lit {
            Literal::Int(_) | Literal::None => Some(WasmType::I64),
            Literal::Float(_) => Some(WasmType::F64),
            Literal::Bool(_) => Some(WasmType::I32),
            Literal::String(..) => Some(WasmType::I32),
        },
        HirExpr::Identifier(name, _) => locals.get(name).copied(),
        HirExpr::List(..) | HirExpr::Dict(..) | HirExpr::Construct(..) | HirExpr::Fn(..) => {
            Some(WasmType::I32)
        }
        HirExpr::Unary(UnaryOp::Not, ..) => Some(WasmType::I32),
        HirExpr::Unary(_, inner, _) => static_expr_type(inner, locals, functions, structs),
        HirExpr::Binary(op, left, right, _) => match op {
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual
            | BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Is
            | BinaryOp::IsNullable => Some(WasmType::I32),
            BinaryOp::Div => Some(WasmType::F64),
            BinaryOp::IntDiv | BinaryOp::Mod | BinaryOp::Range => Some(WasmType::I64),
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                let l = static_expr_type(left, locals, functions, structs);
                let r = static_expr_type(right, locals, functions, structs);
                match (l, r) {
                    (Some(WasmType::F64), _) | (_, Some(WasmType::F64)) => Some(WasmType::F64),
                    // A pointer operand makes `+` a concatenation, whose result is a pointer.
                    (Some(WasmType::I32), _) | (_, Some(WasmType::I32)) if *op == BinaryOp::Add => {
                        Some(WasmType::I32)
                    }
                    (Some(a), Some(b)) if a == b => Some(a),
                    _ => None,
                }
            }
            _ => None,
        },
        HirExpr::If(_, then_expr, else_expr, _) => {
            let t = static_expr_type(then_expr, locals, functions, structs);
            let e = static_expr_type(else_expr, locals, functions, structs);
            match (t, e) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(WasmType::F64), _) | (_, Some(WasmType::F64)) => Some(WasmType::F64),
                _ => None,
            }
        }
        HirExpr::OrElse(left, right, _) => {
            let l = static_expr_type(left, locals, functions, structs);
            let r = static_expr_type(right, locals, functions, structs);
            match (l, r) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(WasmType::F64), _) | (_, Some(WasmType::F64)) => Some(WasmType::F64),
                _ => None,
            }
        }
        HirExpr::Try(inner, _) | HirExpr::With(inner, ..) => {
            static_expr_type(inner, locals, functions, structs)
        }
        HirExpr::Call(callee, _, _) => match &**callee {
            HirExpr::Identifier(name, _) => match name.as_str() {
                "String" | "Bytes" => Some(WasmType::I32),
                "Int" => Some(WasmType::I64),
                "Float" => Some(WasmType::F64),
                "Bool" => Some(WasmType::I32),
                _ => functions.get(name).and_then(|(_, ret)| *ret),
            },
            HirExpr::Dot(receiver, field, _) => {
                let recv_kind = static_expr_type(receiver, locals, functions, structs);
                if field == "len" && recv_kind == Some(WasmType::I64) {
                    return Some(WasmType::I64);
                }
                structs
                    .values()
                    .find_map(|layout| layout.fields.get(field).map(|(_, ty)| *ty))
            }
            _ => None,
        },
        HirExpr::Dot(receiver, field, _) => {
            if field == "len" {
                return Some(WasmType::I64);
            }
            let _ = static_expr_type(receiver, locals, functions, structs);
            structs
                .values()
                .find_map(|layout| layout.fields.get(field).map(|(_, ty)| *ty))
        }
        HirExpr::Index(..) | HirExpr::Await(..) | HirExpr::QuestionDot(..) => None,
    }
}

/// Collects per-parameter Wasm types from the calls the program actually makes.
///
/// Runs a few rounds so a hint can travel through a helper that forwards its own
/// parameter. Parameters that receive nothing stay unconstrained.
fn collect_param_hints(
    program: &HirProgram,
    structs: &HashMap<String, StructLayout>,
) -> HashMap<String, Vec<WasmType>> {
    let mut declared: Vec<&str> = program
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Fn(decl) => Some(decl.name.as_str()),
            _ => None,
        })
        .collect();
    declared.sort_unstable();

    let mut hints: HashMap<String, Vec<Option<WasmType>>> = HashMap::new();
    // A default is a witness too: `fn remember(value, into = [])` is called with
    // one argument, so the call site says nothing about `into`, but the default it
    // is filled with is a list handle and the body has to know that.
    for item in &program.items {
        if let HirItem::Fn(decl) = item {
            for (index, param) in decl.params.iter().enumerate() {
                // Later defaults may name earlier parameters (`tag = name`), so the
                // already-resolved ones stay in scope while this one is read.
                let known: HashMap<String, WasmType> = decl
                    .params
                    .iter()
                    .enumerate()
                    .take(index)
                    .filter_map(|(earlier_index, earlier)| {
                        // The earlier parameter's own row entry is the witness, which is
                        // what makes `tag = name` inherit `name`'s representation.
                        hints
                            .get(&decl.name)
                            .and_then(|row| row.get(earlier_index).copied())
                            .flatten()
                            .or_else(|| {
                                earlier
                                    .type_annotation
                                    .as_ref()
                                    .map(|_| param_wasm_type(earlier))
                            })
                            .map(|ty| (earlier.name.clone(), ty))
                    })
                    .collect();
                let Some(default) = &param.default else {
                    continue;
                };
                let Some(observed) = static_expr_type(default, &known, &HashMap::new(), structs)
                else {
                    continue;
                };
                let row = hints.entry(decl.name.clone()).or_default();
                if row.len() <= index {
                    row.resize(index + 1, None);
                }
                row[index] = Some(observed);
            }
        }
    }
    let mut signatures: HashMap<String, (Vec<WasmType>, Option<WasmType>)> = HashMap::new();

    for _ in 0..3 {
        // Rebuild the signature view from the hints gathered so far.
        signatures.clear();
        for item in &program.items {
            let HirItem::Fn(decl) = item else { continue };
            let params = decl
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    hints
                        .get(&decl.name)
                        .and_then(|row| row.get(index).copied().flatten())
                        .unwrap_or_else(|| param_wasm_type(param))
                })
                .collect::<Vec<_>>();
            let ret = static_return_type(decl, &params, &signatures, structs);
            signatures.insert(decl.name.clone(), (params, ret));
        }

        let mut changed = false;
        for item in &program.items {
            match item {
                HirItem::Fn(decl) => {
                    observe_stmts(
                        &decl.body,
                        &decl.params,
                        &decl.name,
                        &mut hints,
                        &signatures,
                        structs,
                        &mut changed,
                    );
                }
                HirItem::Impl(imp) => {
                    for method in &imp.methods {
                        observe_stmts(
                            &method.body,
                            &method.params,
                            &format!("{}.{}", imp.target, method.name),
                            &mut hints,
                            &signatures,
                            structs,
                            &mut changed,
                        );
                    }
                    if let Some(init) = &imp.init {
                        observe_stmts(
                            &init.body,
                            &init.params,
                            &format!("{}.init", imp.target),
                            &mut hints,
                            &signatures,
                            structs,
                            &mut changed,
                        );
                    }
                }
                _ => {}
            }
        }
        observe_stmts(
            &program.statements,
            &[],
            "",
            &mut hints,
            &signatures,
            structs,
            &mut changed,
        );
        if !changed {
            break;
        }
    }

    hints
        .into_iter()
        .map(|(name, row)| {
            let resolved = row
                .into_iter()
                .map(|hint| hint.unwrap_or(WasmType::I64))
                .collect();
            (name, resolved)
        })
        .collect()
}

/// Return type of a function from its annotation or its body, without a full pass.
fn static_return_type(
    decl: &HirFunctionDecl,
    params: &[WasmType],
    signatures: &HashMap<String, (Vec<WasmType>, Option<WasmType>)>,
    structs: &HashMap<String, StructLayout>,
) -> Option<WasmType> {
    if decl.is_async {
        return Some(WasmType::I32);
    }
    if let Some(annot) = &decl.return_type {
        return match annot.name.as_str() {
            "None" => None,
            "Float" => Some(WasmType::F64),
            "Bool" => Some(WasmType::I32),
            "String" | "Bytes" | "List" | "Dict" | "Range" | "Task" => Some(WasmType::I32),
            name if structs.contains_key(name) => Some(WasmType::I32),
            _ => Some(WasmType::I64),
        };
    }
    let mut locals: HashMap<String, WasmType> = decl
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            (
                param.name.clone(),
                params.get(index).copied().unwrap_or(WasmType::I64),
            )
        })
        .collect();
    static_return_type_in_stmts(&decl.body, &mut locals, signatures, structs)
}

fn static_return_type_in_stmts(
    stmts: &[HirStmt],
    locals: &mut HashMap<String, WasmType>,
    signatures: &HashMap<String, (Vec<WasmType>, Option<WasmType>)>,
    structs: &HashMap<String, StructLayout>,
) -> Option<WasmType> {
    let mut found: Option<WasmType> = None;
    for stmt in stmts {
        match stmt {
            HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => {
                if let Some(ty) = static_expr_type(expr, locals, signatures, structs) {
                    locals.insert(name.clone(), ty);
                }
            }
            HirStmt::Assign(HirExpr::Identifier(name, _), expr, _) => {
                if let Some(ty) = static_expr_type(expr, locals, signatures, structs) {
                    locals.insert(name.clone(), ty);
                }
            }
            HirStmt::Return(Some(expr), _) => {
                let ty = static_expr_type(expr, locals, signatures, structs);
                found = merge_return(found, ty);
                if ty.is_some() {
                    return ty;
                }
            }
            HirStmt::Return(None, _) => return None,
            HirStmt::If(inner) => {
                if let Some(ty) =
                    static_return_type_in_stmts(&inner.then_branch, locals, signatures, structs)
                {
                    return Some(ty);
                }
                for (_, body) in &inner.elif_branches {
                    if let Some(ty) = static_return_type_in_stmts(body, locals, signatures, structs)
                    {
                        return Some(ty);
                    }
                }
                if let Some(body) = &inner.else_branch
                    && let Some(ty) = static_return_type_in_stmts(body, locals, signatures, structs)
                {
                    return Some(ty);
                }
            }
            HirStmt::While(_, body, _)
            | HirStmt::Loop(body, _)
            | HirStmt::Repeat(_, _, body, _)
            | HirStmt::Each(_, _, body, _)
            | HirStmt::AwaitDo(body, _) => {
                if let Some(ty) = static_return_type_in_stmts(body, locals, signatures, structs) {
                    return Some(ty);
                }
            }
            HirStmt::Match(m) => {
                for arm in &m.when_arms {
                    if let Some(ty) =
                        static_return_type_in_stmts(&arm.body, locals, signatures, structs)
                    {
                        return Some(ty);
                    }
                }
                if let Some(body) = &m.else_arm
                    && let Some(ty) = static_return_type_in_stmts(body, locals, signatures, structs)
                {
                    return Some(ty);
                }
            }
            HirStmt::Attempt(a) => {
                if let Some(ty) = static_return_type_in_stmts(&a.body, locals, signatures, structs)
                {
                    return Some(ty);
                }
                if let Some(ty) =
                    static_return_type_in_stmts(&a.handler, locals, signatures, structs)
                {
                    return Some(ty);
                }
            }
            _ => {}
        }
    }
    if let Some(HirStmt::Expr(expr)) = stmts.last()
        && let Some(ty) = static_expr_type(expr, locals, signatures, structs)
    {
        return Some(ty);
    }
    found
}

fn merge_return(current: Option<WasmType>, observed: Option<WasmType>) -> Option<WasmType> {
    match (current, observed) {
        (None, other) | (other, None) => other,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => Some(WasmType::I64),
    }
}

/// Records the argument types observed at every direct call inside `stmts`.
fn observe_stmts(
    stmts: &[HirStmt],
    params: &[HirParam],
    fn_name: &str,
    hints: &mut HashMap<String, Vec<Option<WasmType>>>,
    signatures: &HashMap<String, (Vec<WasmType>, Option<WasmType>)>,
    structs: &HashMap<String, StructLayout>,
    changed: &mut bool,
) {
    // Seed the body's locals with everything known so far, including this
    // function's own parameters: a helper that forwards its parameter to another
    // function is the only witness for that inner parameter's representation.
    //
    // An unresolved parameter stays absent from the map instead of falling back to
    // `i64`. Observing through the fallback would feed `i64` back into the next
    // round and freeze every parameter at the scalar default, which is exactly how
    // a pointer argument ended up printed as a number.
    let mut locals: HashMap<String, WasmType> = params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let hint = hints
                .get(fn_name)
                .and_then(|row| row.get(index).copied())
                .flatten();
            let annotated = param
                .type_annotation
                .as_ref()
                .map(|_| param_wasm_type(param));
            hint.or(annotated).map(|ty| (param.name.clone(), ty))
        })
        .collect();
    observe_stmts_inner(stmts, &mut locals, hints, signatures, structs, changed);
}

fn observe_stmts_inner(
    stmts: &[HirStmt],
    locals: &mut HashMap<String, WasmType>,
    hints: &mut HashMap<String, Vec<Option<WasmType>>>,
    signatures: &HashMap<String, (Vec<WasmType>, Option<WasmType>)>,
    structs: &HashMap<String, StructLayout>,
    changed: &mut bool,
) {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => {
                observe_expr(expr, locals, hints, signatures, structs, changed);
                if let Some(ty) = static_expr_type(expr, locals, signatures, structs) {
                    locals.insert(name.clone(), ty);
                }
            }
            HirStmt::Assign(target, expr, _) | HirStmt::CompoundAssign(_, target, expr, _) => {
                observe_expr(target, locals, hints, signatures, structs, changed);
                observe_expr(expr, locals, hints, signatures, structs, changed);
            }
            HirStmt::Expr(expr) | HirStmt::Return(Some(expr), _) | HirStmt::Fail(expr, _) => {
                observe_expr(expr, locals, hints, signatures, structs, changed);
            }
            HirStmt::If(inner) => {
                observe_expr(
                    &inner.condition,
                    locals,
                    hints,
                    signatures,
                    structs,
                    changed,
                );
                observe_stmts_inner(
                    &inner.then_branch,
                    locals,
                    hints,
                    signatures,
                    structs,
                    changed,
                );
                for (cond, body) in &inner.elif_branches {
                    observe_expr(cond, locals, hints, signatures, structs, changed);
                    observe_stmts_inner(body, locals, hints, signatures, structs, changed);
                }
                if let Some(body) = &inner.else_branch {
                    observe_stmts_inner(body, locals, hints, signatures, structs, changed);
                }
            }
            HirStmt::While(cond, body, _) => {
                observe_expr(cond, locals, hints, signatures, structs, changed);
                observe_stmts_inner(body, locals, hints, signatures, structs, changed);
            }
            HirStmt::Loop(body, _) | HirStmt::AwaitDo(body, _) => {
                observe_stmts_inner(body, locals, hints, signatures, structs, changed);
            }
            HirStmt::Repeat(count, _, body, _) => {
                observe_expr(count, locals, hints, signatures, structs, changed);
                observe_stmts_inner(body, locals, hints, signatures, structs, changed);
            }
            HirStmt::Each(bindings, iterable, body, _) => {
                // A `Dict` binds a String key (and a value), which is more precise
                // than the scalar default; without it the key printed as a pointer.
                let iterable_type = static_expr_type(iterable, locals, signatures, structs);
                let binds_dict = matches!(iterable, HirExpr::Dict(..))
                    || iterable_type == Some(WasmType::I32)
                        && matches!(iterable, HirExpr::Identifier(name, _)
                            if locals.get(name).is_some_and(|ty| *ty == WasmType::I32));
                observe_expr(iterable, locals, hints, signatures, structs, changed);
                for (index, name) in bindings.iter().enumerate() {
                    let ty = if binds_dict && index == 0 {
                        WasmType::I32
                    } else {
                        WasmType::I64
                    };
                    locals.insert(name.clone(), ty);
                }
                observe_stmts_inner(body, locals, hints, signatures, structs, changed);
            }
            HirStmt::Attempt(a) => {
                observe_stmts_inner(&a.body, locals, hints, signatures, structs, changed);
                observe_stmts_inner(&a.handler, locals, hints, signatures, structs, changed);
            }
            HirStmt::Match(m) => {
                observe_expr(&m.target, locals, hints, signatures, structs, changed);
                for arm in &m.when_arms {
                    for pattern in &arm.patterns {
                        if let aipo_hir::HirMatchPattern::Value(expr) = pattern {
                            observe_expr(expr, locals, hints, signatures, structs, changed);
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        observe_expr(guard, locals, hints, signatures, structs, changed);
                    }
                    observe_stmts_inner(&arm.body, locals, hints, signatures, structs, changed);
                }
                if let Some(body) = &m.else_arm {
                    observe_stmts_inner(body, locals, hints, signatures, structs, changed);
                }
            }
            HirStmt::FnDecl(decl) => {
                observe_stmts_inner(&decl.body, locals, hints, signatures, structs, changed);
            }
            HirStmt::Break(_) | HirStmt::Continue(_) | HirStmt::Return(None, _) => {}
        }
    }
}

fn observe_expr(
    expr: &HirExpr,
    locals: &mut HashMap<String, WasmType>,
    hints: &mut HashMap<String, Vec<Option<WasmType>>>,
    signatures: &HashMap<String, (Vec<WasmType>, Option<WasmType>)>,
    structs: &HashMap<String, StructLayout>,
    changed: &mut bool,
) {
    match expr {
        HirExpr::Call(callee, args, _) => {
            if let HirExpr::Identifier(name, _) = &**callee
                && signatures.contains_key(name)
            {
                let mut row = hints.remove(name).unwrap_or_default();
                for (index, arg) in args.iter().enumerate() {
                    let Some(observed) = static_expr_type(&arg.value, locals, signatures, structs)
                    else {
                        continue;
                    };
                    if row.len() <= index {
                        row.resize(index + 1, None);
                    }
                    let merged = merge_hint(row[index], observed);
                    if merged != row[index] {
                        row[index] = merged;
                        *changed = true;
                    }
                }
                hints.insert(name.clone(), row);
            }
            observe_expr(callee, locals, hints, signatures, structs, changed);
            for arg in args {
                observe_expr(&arg.value, locals, hints, signatures, structs, changed);
            }
        }
        HirExpr::Binary(_, left, right, _) => {
            observe_expr(left, locals, hints, signatures, structs, changed);
            observe_expr(right, locals, hints, signatures, structs, changed);
        }
        HirExpr::Unary(_, inner, _)
        | HirExpr::Dot(inner, _, _)
        | HirExpr::QuestionDot(inner, _, _)
        | HirExpr::Await(inner, _)
        | HirExpr::Try(inner, _)
        | HirExpr::With(inner, _, _) => {
            observe_expr(inner, locals, hints, signatures, structs, changed);
        }
        HirExpr::Index(target, index, _) => {
            observe_expr(target, locals, hints, signatures, structs, changed);
            observe_expr(index, locals, hints, signatures, structs, changed);
        }
        HirExpr::List(items, _) => {
            for item in items {
                observe_expr(item, locals, hints, signatures, structs, changed);
            }
        }
        HirExpr::Dict(entries, _) => {
            for (key, value) in entries {
                observe_expr(key, locals, hints, signatures, structs, changed);
                observe_expr(value, locals, hints, signatures, structs, changed);
            }
        }
        HirExpr::Construct(_, fields, _) => {
            for (_, value) in fields {
                observe_expr(value, locals, hints, signatures, structs, changed);
            }
        }
        HirExpr::If(cond, then_expr, else_expr, _) => {
            observe_expr(cond, locals, hints, signatures, structs, changed);
            observe_expr(then_expr, locals, hints, signatures, structs, changed);
            observe_expr(else_expr, locals, hints, signatures, structs, changed);
        }
        HirExpr::OrElse(left, right, _) => {
            observe_expr(left, locals, hints, signatures, structs, changed);
            observe_expr(right, locals, hints, signatures, structs, changed);
        }
        HirExpr::Fn(func_expr) => {
            observe_stmts_inner(
                &func_expr.body,
                &mut func_expr
                    .params
                    .iter()
                    .map(|param| (param.name.clone(), param_wasm_type(param)))
                    .collect(),
                hints,
                signatures,
                structs,
                changed,
            );
        }
        HirExpr::Literal(..) | HirExpr::Identifier(..) => {}
    }
}

/// Resolves the return type from explicit annotation or infers it from the function body.
fn resolve_return_type(
    func_name: &str,
    annot: &Option<aipo_ast::TypeAnnotation>,
    params: &[HirParam],
    body: &[HirStmt],
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
) -> Option<WasmType> {
    if let Some(ty) = annot {
        match ty.name.as_str() {
            "Float" => Some(WasmType::F64),
            "Bool" => Some(WasmType::I32),
            "None" => None,
            "String" | "Bytes" => Some(WasmType::I32),
            s if structs.contains_key(s) => Some(WasmType::I32),
            _ => Some(WasmType::I64),
        }
    } else {
        let struct_target = if func_name.contains('.') {
            func_name.split('.').next()
        } else {
            None
        };
        let mut locals = HashMap::new();
        for (i, param) in params.iter().enumerate() {
            let ty = param_wasm_type(param);
            let kind = match annotated_kind(param.type_annotation.as_ref(), structs) {
                LocalKind::Unknown
                    if (param.name == "self" || param.is_self)
                        && struct_target.is_some_and(|s| structs.contains_key(s)) =>
                {
                    LocalKind::Struct(struct_target.unwrap().to_string())
                }
                other => other,
            };
            locals.insert(param.name.clone(), (i as u32, ty, kind));
        }
        infer_body_return_type(
            body,
            &mut locals,
            functions,
            structs,
            &HashMap::new(),
            &HashMap::new(),
        )
    }
}

/// Resolves the semantic kind of a function's result.
///
/// A declared contract is authoritative. Otherwise the first `return` with a value
/// decides, which is what makes `P:alive() { return self.h > 0 }` render as a bool.
fn resolve_return_kind(
    annot: &Option<aipo_ast::TypeAnnotation>,
    params: &[HirParam],
    body: &[HirStmt],
    func_name: &str,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
) -> LocalKind {
    if let Some(ty) = annot {
        return annotated_kind(Some(ty), structs);
    }
    let struct_target = if func_name.contains('.') {
        func_name.split('.').next()
    } else {
        None
    };
    let mut locals = HashMap::new();
    for (i, param) in params.iter().enumerate() {
        let ty = param_wasm_type(param);
        let kind = match annotated_kind(param.type_annotation.as_ref(), structs) {
            LocalKind::Unknown
                if (param.name == "self" || param.is_self)
                    && struct_target.is_some_and(|s| structs.contains_key(s)) =>
            {
                LocalKind::Struct(struct_target.unwrap().to_string())
            }
            other => other,
        };
        locals.insert(param.name.clone(), (i as u32, ty, kind));
    }
    infer_body_return_kind(
        body,
        &mut locals,
        functions,
        structs,
        &HashMap::new(),
        &HashMap::new(),
    )
    .unwrap_or(LocalKind::Unknown)
}

/// Scans statements for the semantic kind of the value they return.
///
/// Mirrors [`infer_body_return_type`], including the `match`/`attempt` bodies that
/// can carry the only return, but answers with the rendering kind instead of the
/// Wasm representation.
fn infer_body_return_kind(
    body: &[HirStmt],
    locals: &mut HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
) -> Option<LocalKind> {
    for stmt in body {
        match stmt {
            HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => {
                let ty = infer_expr_type(expr, locals, functions, structs, table_indices)
                    .unwrap_or(WasmType::I64);
                let kind = infer_expr_kind(expr, locals, functions, table_indices, anon_map);
                let idx = locals.len() as u32;
                locals.insert(name.clone(), (idx, ty, kind));
            }
            HirStmt::Return(Some(expr), _) => {
                let kind = infer_expr_kind(expr, locals, functions, table_indices, anon_map);
                if !matches!(kind, LocalKind::Unknown | LocalKind::None) {
                    return Some(kind);
                }
            }
            HirStmt::If(s) => {
                if let Some(kind) = infer_body_return_kind(
                    &s.then_branch,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(kind);
                }
                for (_, elif_body) in &s.elif_branches {
                    if let Some(kind) = infer_body_return_kind(
                        elif_body,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    ) {
                        return Some(kind);
                    }
                }
                if let Some(else_branch) = &s.else_branch
                    && let Some(kind) = infer_body_return_kind(
                        else_branch,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    )
                {
                    return Some(kind);
                }
            }
            HirStmt::While(_, loop_body, _)
            | HirStmt::Loop(loop_body, _)
            | HirStmt::Repeat(_, _, loop_body, _)
            | HirStmt::Each(_, _, loop_body, _) => {
                if let Some(kind) = infer_body_return_kind(
                    loop_body,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(kind);
                }
            }
            HirStmt::AwaitDo(stmts, _) => {
                if let Some(kind) = infer_body_return_kind(
                    stmts,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(kind);
                }
            }
            HirStmt::Match(match_stmt) => {
                for arm in &match_stmt.when_arms {
                    if let Some(kind) = infer_body_return_kind(
                        &arm.body,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    ) {
                        return Some(kind);
                    }
                }
                if let Some(else_body) = &match_stmt.else_arm
                    && let Some(kind) = infer_body_return_kind(
                        else_body,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    )
                {
                    return Some(kind);
                }
            }
            HirStmt::Attempt(attempt) => {
                if let Some(kind) = infer_body_return_kind(
                    &attempt.body,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(kind);
                }
                if let Some(kind) = infer_body_return_kind(
                    &attempt.handler,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(kind);
                }
            }
            _ => {}
        }
    }
    // An expression in final position is the implicit return.
    if let Some(HirStmt::Expr(expr)) = body.last() {
        let kind = infer_expr_kind(expr, locals, functions, table_indices, anon_map);
        if !matches!(kind, LocalKind::Unknown | LocalKind::None) {
            return Some(kind);
        }
    }
    None
}

/// Scans statements to deduce the return type of a block or function body.
fn infer_body_return_type(
    body: &[HirStmt],
    locals: &mut HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
) -> Option<WasmType> {
    let mut has_return = false;
    for stmt in body {
        match stmt {
            HirStmt::Let(name, expr, _) | HirStmt::Var(name, expr, _) => {
                let ty = infer_expr_type(expr, locals, functions, structs, table_indices)
                    .unwrap_or(WasmType::I64);
                let kind = infer_expr_kind(expr, locals, functions, table_indices, anon_map);
                let idx = locals.len() as u32;
                locals.insert(name.clone(), (idx, ty, kind));
            }
            HirStmt::Return(Some(expr), _) => {
                has_return = true;
                if let Ok(ty) = infer_expr_type(expr, locals, functions, structs, table_indices) {
                    return Some(ty);
                }
            }
            HirStmt::Return(None, _) => {
                return None;
            }
            HirStmt::If(s) => {
                if let Some(ty) = infer_body_return_type(
                    &s.then_branch,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(ty);
                }
                for (_, elif_body) in &s.elif_branches {
                    if let Some(ty) = infer_body_return_type(
                        elif_body,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    ) {
                        return Some(ty);
                    }
                }
                if let Some(else_branch) = &s.else_branch
                    && let Some(ty) = infer_body_return_type(
                        else_branch,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    )
                {
                    return Some(ty);
                }
            }
            HirStmt::While(_, loop_body, _)
            | HirStmt::Loop(loop_body, _)
            | HirStmt::Repeat(_, _, loop_body, _)
            | HirStmt::Each(_, _, loop_body, _) => {
                if let Some(ty) = infer_body_return_type(
                    loop_body,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(ty);
                }
            }
            HirStmt::AwaitDo(stmts, _) => {
                if let Some(ty) = infer_body_return_type(
                    stmts,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(ty);
                }
            }
            // `match` and `attempt` bodies can return; a function whose only
            // returns live inside them still has a result type. Missing this made
            // such functions compile as void while emitting `return value`.
            HirStmt::Match(match_stmt) => {
                for arm in &match_stmt.when_arms {
                    if let Some(ty) = infer_body_return_type(
                        &arm.body,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    ) {
                        return Some(ty);
                    }
                }
                if let Some(else_body) = &match_stmt.else_arm
                    && let Some(ty) = infer_body_return_type(
                        else_body,
                        locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                    )
                {
                    return Some(ty);
                }
            }
            HirStmt::Attempt(attempt) => {
                if let Some(ty) = infer_body_return_type(
                    &attempt.body,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(ty);
                }
                if let Some(ty) = infer_body_return_type(
                    &attempt.handler,
                    locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                ) {
                    return Some(ty);
                }
            }
            _ => {}
        }
    }
    // If the last statement is an expression statement, its type is the implicit return
    if let Some(HirStmt::Expr(expr)) = body.last()
        && let Ok(ty) = infer_expr_type(expr, locals, functions, structs, table_indices)
    {
        return Some(ty);
    }
    if has_return {
        Some(WasmType::I64)
    } else {
        None
    }
}

/// Recursively scans statements to register all declared locals and helpers.
#[allow(clippy::too_many_arguments)]
fn pre_scan_stmts(
    stmts: &[HirStmt],
    params_count: usize,
    locals: &mut HashMap<String, (u32, WasmType, LocalKind)>,
    declared_locals: &mut Vec<WasmType>,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    repeat_id: &mut u32,
) -> Result<(), WasmCompileError> {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(name, init_expr, _) | HirStmt::Var(name, init_expr, _) => {
                let ty = infer_expr_type(init_expr, locals, functions, structs, table_indices)
                    .unwrap_or(WasmType::I64);
                let kind = infer_expr_kind(init_expr, locals, functions, table_indices, anon_map);
                let idx = (params_count + declared_locals.len()) as u32;
                declared_locals.push(ty);
                locals.insert(name.clone(), (idx, ty, kind));
            }
            HirStmt::CompoundAssign(op, HirExpr::Identifier(name, _), _, _)
                if *op == BinaryOp::Add
                    && locals
                        .get(name)
                        .is_some_and(|(_, _, k)| *k == LocalKind::String) =>
            {
                // String compound addition preserves String kind!
            }
            HirStmt::Assign(HirExpr::Identifier(name, _), value, _)
            | HirStmt::CompoundAssign(_, HirExpr::Identifier(name, _), value, _) => {
                let assigned = infer_expr_kind(value, locals, functions, table_indices, anon_map);
                if let Some((_, _, kind)) = locals.get_mut(name)
                    && *kind != assigned
                {
                    *kind = LocalKind::Unknown;
                }
            }
            HirStmt::If(s) => {
                pre_scan_stmts(
                    &s.then_branch,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
                for (_, elif_body) in &s.elif_branches {
                    pre_scan_stmts(
                        elif_body,
                        params_count,
                        locals,
                        declared_locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                        repeat_id,
                    )?;
                }
                if let Some(else_branch) = &s.else_branch {
                    pre_scan_stmts(
                        else_branch,
                        params_count,
                        locals,
                        declared_locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                        repeat_id,
                    )?;
                }
            }
            HirStmt::While(_, body, _) | HirStmt::Loop(body, _) => {
                pre_scan_stmts(
                    body,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
            }
            HirStmt::Repeat(_, maybe_index, body, _) => {
                *repeat_id += 1;
                let id = *repeat_id;
                // repeat_limit slot
                let limit_idx = (params_count + declared_locals.len()) as u32;
                declared_locals.push(WasmType::I64);
                locals.insert(
                    format!("__repeat_limit_{id}"),
                    (limit_idx, WasmType::I64, LocalKind::Int),
                );
                // repeat_idx slot
                let idx_idx = (params_count + declared_locals.len()) as u32;
                declared_locals.push(WasmType::I64);
                locals.insert(
                    format!("__repeat_idx_{id}"),
                    (idx_idx, WasmType::I64, LocalKind::Int),
                );
                // user index variable if specified
                if let Some(name) = maybe_index {
                    let user_idx = (params_count + declared_locals.len()) as u32;
                    declared_locals.push(WasmType::I64);
                    locals.insert(name.clone(), (user_idx, WasmType::I64, LocalKind::Int));
                }
                pre_scan_stmts(
                    body,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
            }
            HirStmt::Match(match_stmt) => {
                *repeat_id += 1;
                let id = *repeat_id;
                let slot = (params_count + declared_locals.len()) as u32;
                declared_locals.push(WasmType::I64);
                locals.insert(
                    format!("__match_target_{id}"),
                    (slot, WasmType::I64, LocalKind::Unknown),
                );
                // Destructured fields keep the representation the struct layout
                // assigns them, so `when { name, age }` prints the string field
                // instead of its pointer.
                let target_kind = infer_expr_kind(
                    &match_stmt.target,
                    locals,
                    functions,
                    table_indices,
                    anon_map,
                );
                for arm in &match_stmt.when_arms {
                    for pat in &arm.patterns {
                        match pat {
                            aipo_hir::HirMatchPattern::Destructure(fields) => {
                                for f in fields {
                                    if !locals.contains_key(f) {
                                        let slot = (params_count + declared_locals.len()) as u32;
                                        declared_locals.push(WasmType::I64);
                                        let kind = struct_field_kind(&target_kind, f, structs);
                                        locals.insert(f.clone(), (slot, WasmType::I64, kind));
                                    }
                                }
                            }
                            aipo_hir::HirMatchPattern::Variant { payload, .. } => {
                                let fields: &[String] = match payload {
                                    aipo_hir::HirVariantPatternPayload::Unit => &[][..],
                                    aipo_hir::HirVariantPatternPayload::Tuple(ids) => {
                                        ids.as_slice()
                                    }
                                    aipo_hir::HirVariantPatternPayload::Struct(ids) => {
                                        ids.as_slice()
                                    }
                                };
                                for f in fields {
                                    if !locals.contains_key(f) {
                                        let slot = (params_count + declared_locals.len()) as u32;
                                        declared_locals.push(WasmType::I64);
                                        let kind = match payload {
                                            aipo_hir::HirVariantPatternPayload::Struct(_) => {
                                                struct_field_kind(&target_kind, f, structs)
                                            }
                                            _ => LocalKind::Unknown,
                                        };
                                        locals.insert(f.clone(), (slot, WasmType::I64, kind));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    let arm_body = &arm.body;
                    pre_scan_stmts(
                        arm_body,
                        params_count,
                        locals,
                        declared_locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                        repeat_id,
                    )?;
                }
                if let Some(else_body) = &match_stmt.else_arm {
                    pre_scan_stmts(
                        else_body,
                        params_count,
                        locals,
                        declared_locals,
                        functions,
                        structs,
                        table_indices,
                        anon_map,
                        repeat_id,
                    )?;
                }
            }
            HirStmt::Attempt(attempt_stmt) => {
                // The error binding points at the static string pool, like any
                // other string value.
                if let Some(err_name) = &attempt_stmt.error_binding {
                    let slot = (params_count + declared_locals.len()) as u32;
                    declared_locals.push(WasmType::I32);
                    locals.insert(err_name.clone(), (slot, WasmType::I32, LocalKind::String));
                }
                pre_scan_stmts(
                    &attempt_stmt.body,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
                pre_scan_stmts(
                    &attempt_stmt.handler,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
            }
            HirStmt::Each(bindings, iterable, body, _) => {
                *repeat_id += 1;
                let id = *repeat_id;
                let seq_idx = (params_count + declared_locals.len()) as u32;
                declared_locals.push(WasmType::I32);
                locals.insert(
                    format!("__each_seq_{id}"),
                    (
                        seq_idx,
                        WasmType::I32,
                        LocalKind::List(Box::new(LocalKind::Unknown)),
                    ),
                );
                let cur_idx = (params_count + declared_locals.len()) as u32;
                declared_locals.push(WasmType::I64);
                locals.insert(
                    format!("__each_cursor_{id}"),
                    (cur_idx, WasmType::I64, LocalKind::Int),
                );
                let len_idx = (params_count + declared_locals.len()) as u32;
                declared_locals.push(WasmType::I64);
                locals.insert(
                    format!("__each_len_{id}"),
                    (len_idx, WasmType::I64, LocalKind::Int),
                );
                // A single binding takes the element kind of the collection; with two
                // bindings the first is the key/index (`Int`) and the second is the
                // value (`String` for `keys`, the element otherwise).
                let iterable_kind =
                    infer_expr_kind(iterable, locals, functions, table_indices, anon_map);
                let element_kind = match &iterable_kind {
                    LocalKind::List(element) => (**element).clone(),
                    LocalKind::Dict(value) => (**value).clone(),
                    _ => LocalKind::Unknown,
                };
                // A `Dict` iterates String keys, and its second binding holds the
                // value kind. A `List`/`Range` keeps the positional `Int` index.
                let is_dict = matches!(iterable_kind, LocalKind::Dict(_));
                for (position, name) in bindings.iter().enumerate() {
                    let slot = (params_count + declared_locals.len()) as u32;
                    let ty = if bindings.len() > 1 && position == 0 && is_dict {
                        declared_locals.push(WasmType::I32);
                        WasmType::I32
                    } else {
                        declared_locals.push(WasmType::I64);
                        WasmType::I64
                    };
                    let kind = if bindings.len() > 1 {
                        match (is_dict, position) {
                            (true, 0) => LocalKind::String,
                            (_, 0) => LocalKind::Int,
                            (_, _) => element_kind.clone(),
                        }
                    } else if is_dict {
                        LocalKind::String
                    } else {
                        element_kind.clone()
                    };
                    locals.insert(name.clone(), (slot, ty, kind));
                }
                pre_scan_stmts(
                    body,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
            }
            HirStmt::AwaitDo(stmts, _) => {
                pre_scan_stmts(
                    stmts,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
            }
            HirStmt::FnDecl(f) => {
                let idx = (params_count + declared_locals.len()) as u32;
                declared_locals.push(WasmType::I32);
                let fn_kind = if let Some(anon_name) = anon_map.get(&f.span) {
                    if let Some((_, type_idx, _)) = functions.get(anon_name) {
                        LocalKind::Fn(*type_idx)
                    } else {
                        LocalKind::Unknown
                    }
                } else {
                    LocalKind::Unknown
                };
                locals.insert(f.name.clone(), (idx, WasmType::I32, fn_kind));
                pre_scan_stmts(
                    &f.body,
                    params_count,
                    locals,
                    declared_locals,
                    functions,
                    structs,
                    table_indices,
                    anon_map,
                    repeat_id,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Infers the high-level kind of an expression for local tracking.
fn infer_expr_kind(
    expr: &HirExpr,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
) -> LocalKind {
    match expr {
        HirExpr::Literal(Literal::Int(..), _) => LocalKind::Int,
        HirExpr::Literal(Literal::None, _) => LocalKind::None,
        HirExpr::Literal(Literal::Float(..), _) => LocalKind::Float,
        HirExpr::Literal(Literal::Bool(..), _) => LocalKind::Bool,
        HirExpr::Literal(Literal::String(..), _) => LocalKind::String,
        HirExpr::Construct(name, ..) => LocalKind::Struct(name.clone()),
        HirExpr::Identifier(name, _) => {
            if let Some((_, _, k)) = locals.get(name) {
                k.clone()
            } else if let Some((_, _, k)) = functions.module_globals.get(name) {
                // A module binding lives in a global, not in the current frame, so its
                // kind has to come from the table rather than from `locals`.
                k.clone()
            } else if functions.global_bindings.contains(name) {
                LocalKind::Unknown
            } else if table_indices.contains_key(name) {
                if let Some((_, type_idx, _)) = functions.get(name) {
                    LocalKind::Fn(*type_idx)
                } else {
                    LocalKind::Unknown
                }
            } else {
                LocalKind::Unknown
            }
        }
        HirExpr::Fn(func_expr) => {
            if let Some(anon_name) = anon_map.get(&func_expr.span) {
                if let Some((_, type_idx, _)) = functions.get(anon_name) {
                    LocalKind::Fn(*type_idx)
                } else {
                    LocalKind::Unknown
                }
            } else {
                LocalKind::Unknown
            }
        }
        HirExpr::Binary(BinaryOp::Range, ..) => LocalKind::Range,
        HirExpr::List(items, _) => {
            let element = items.first().map_or(LocalKind::Unknown, |first| {
                infer_expr_kind(first, locals, functions, table_indices, anon_map)
            });
            LocalKind::List(Box::new(element))
        }
        HirExpr::Dict(entries, _) => {
            let value = entries.first().map_or(LocalKind::Unknown, |(_, value)| {
                infer_expr_kind(value, locals, functions, table_indices, anon_map)
            });
            LocalKind::Dict(Box::new(value))
        }
        HirExpr::With(base, ..) => {
            infer_expr_kind(base, locals, functions, table_indices, anon_map)
        }
        HirExpr::Try(inner, ..) => {
            infer_expr_kind(inner, locals, functions, table_indices, anon_map)
        }
        HirExpr::Await(inner, _) => {
            // `await task` yields what the body returned, so the kind of the awaited
            // call is the kind of the result.
            let inner_kind = infer_expr_kind(inner, locals, functions, table_indices, anon_map);
            if let HirExpr::Call(callee, _, _) = &**inner
                && let HirExpr::Identifier(name, _) = &**callee
                && let Some(kind) = functions.return_kinds.get(&format!("__async_body_{name}"))
            {
                return kind.clone();
            }
            let _ = inner_kind;
            LocalKind::Unknown
        }
        HirExpr::Index(target, index, _) => {
            // Slicing returns the container's own kind: a window over a List is a
            // List, so `len(xs[a..b])` must resolve through the list helper and not
            // through an element read.
            let target_kind = infer_expr_kind(target, locals, functions, table_indices, anon_map);
            if matches!(&**index, HirExpr::Binary(BinaryOp::Range, ..)) {
                match &target_kind {
                    LocalKind::List(element) => return LocalKind::List(element.clone()),
                    LocalKind::Dict(value) => return LocalKind::Dict(value.clone()),
                    LocalKind::Bytes => return LocalKind::Bytes,
                    LocalKind::String => return LocalKind::String,
                    _ => {}
                }
            }
            // An element read keeps the container's element kind, which is what
            // makes `letters[0]` print text instead of the element's pointer.
            match target_kind {
                LocalKind::List(element) => (*element).clone(),
                LocalKind::Dict(value) => (*value).clone(),
                LocalKind::Bytes => LocalKind::Int,
                _ => LocalKind::Unknown,
            }
        }
        HirExpr::Binary(BinaryOp::Add, left, right, _) => {
            let lk = infer_expr_kind(left, locals, functions, table_indices, anon_map);
            let rk = infer_expr_kind(right, locals, functions, table_indices, anon_map);
            match (lk, rk) {
                (LocalKind::String, _) | (_, LocalKind::String) => LocalKind::String,
                (LocalKind::Int, LocalKind::Int) => LocalKind::Int,
                (LocalKind::Int | LocalKind::Float, LocalKind::Int | LocalKind::Float) => {
                    LocalKind::Float
                }
                // The numeric lowering does not prove string concatenation or dynamic operands.
                _ => LocalKind::Unknown,
            }
        }
        HirExpr::Binary(
            BinaryOp::Is
            | BinaryOp::IsNullable
            | BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual
            | BinaryOp::And
            | BinaryOp::Or,
            ..,
        ) => LocalKind::Bool,
        HirExpr::Call(callee, _, _) => {
            if let HirExpr::Identifier(name, _) = &**callee {
                if name == "String"
                    && !locals.contains_key(name)
                    && functions.builtin_is_unshadowed(name)
                {
                    return LocalKind::String;
                }
                if name == "Bytes"
                    && !locals.contains_key(name)
                    && functions.builtin_is_unshadowed(name)
                {
                    return LocalKind::Bytes;
                }
            }
            if let HirExpr::Dot(receiver, method_name, _) = &**callee {
                // A method on a struct has a declared or inferred result kind, which is
                // what makes `p.alive()` render as `true` instead of an address. The
                // receiver resolves either to the instance or to the type name itself
                // for a static call (`P.build()`).
                let receiver_type = match &**receiver {
                    // A module binding is an instance, so its name never names a type.
                    HirExpr::Identifier(name, _)
                        if !locals.contains_key(name)
                            && !functions.module_globals.contains_key(name) =>
                    {
                        functions
                            .declared_types
                            .contains(name)
                            .then(|| name.clone())
                    }
                    _ => {
                        match infer_expr_kind(receiver, locals, functions, table_indices, anon_map)
                        {
                            LocalKind::Struct(name) => Some(name),
                            _ => None,
                        }
                    }
                };
                if let Some(type_name) = receiver_type
                    && let Some(kind) = functions
                        .return_kinds
                        .get(&format!("{type_name}.{method_name}"))
                    && !matches!(kind, LocalKind::Unknown)
                {
                    return kind.clone();
                }
                let rec_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(rec_kind, LocalKind::Bytes)
                    && (method_name == "decode" || method_name == "decode_utf8")
                {
                    return LocalKind::String;
                }
                if let LocalKind::List(element) = &rec_kind
                    && matches!(
                        method_name.as_str(),
                        "transform" | "map" | "filter" | "sort" | "sort_by" | "reverse"
                    )
                {
                    return LocalKind::List(element.clone());
                }
                // A predicate answers `Bool`, whatever the receiver kind; without
                // this the result printed as a string pointer. `task.all` is not one:
                // it answers the list of completed values.
                let is_task_module = matches!(&**receiver, HirExpr::Identifier(name, _)
                    if name == "task" && !locals.contains_key(name));
                if !is_task_module
                    && matches!(
                        method_name.as_str(),
                        "is_empty"
                            | "contains"
                            | "any"
                            | "all"
                            | "has"
                            | "starts_with"
                            | "ends_with"
                            | "is_match"
                    )
                {
                    return LocalKind::Bool;
                }
                // String-valued helpers answer a pool pointer, which prints as text.
                if matches!(
                    method_name.as_str(),
                    "trim"
                        | "upper"
                        | "lower"
                        | "reverse"
                        | "capitalize"
                        | "slice"
                        | "replace"
                        | "join"
                        | "format"
                        | "to_string"
                ) {
                    return LocalKind::String;
                }
                if matches!(method_name.as_str(), "len" | "byte_len" | "find") {
                    return LocalKind::Int;
                }
                if method_name == "split" {
                    return LocalKind::List(Box::new(LocalKind::String));
                }
                if matches!(rec_kind, LocalKind::Dict(_))
                    && matches!(method_name.as_str(), "keys" | "values")
                {
                    // `keys` yields String keys; `values` yields the stored values.
                    let element = match (&rec_kind, method_name.as_str()) {
                        (_, "keys") => LocalKind::String,
                        (LocalKind::Dict(value), _) => (**value).clone(),
                        _ => LocalKind::Unknown,
                    };
                    return LocalKind::List(Box::new(element));
                }
                if let HirExpr::Identifier(rec_name, _) = &**receiver
                    && rec_name == "task"
                    && !locals.contains_key(rec_name)
                    && functions.builtin_is_unshadowed(rec_name)
                {
                    return match method_name.as_str() {
                        "spawn" => LocalKind::Task,
                        // `task.all`/`task.race` answer the list of completed values.
                        "all" | "race" => LocalKind::List(Box::new(LocalKind::Unknown)),
                        _ => LocalKind::Unknown,
                    };
                }
                // `math` helpers answer numbers; the integer-valued ones print as
                // integers, which the representation alone cannot distinguish.
                if let HirExpr::Identifier(rec_name, _) = &**receiver
                    && rec_name == "math"
                    && !locals.contains_key(rec_name)
                    && functions.builtin_is_unshadowed(rec_name)
                {
                    return match method_name.as_str() {
                        "floor" | "ceil" | "round" | "truncate" | "clamp" => LocalKind::Int,
                        // `abs`/`min`/`max`/`pow` keep the operand's own kind, which
                        // the call site knows but this signature does not receive.
                        _ => LocalKind::Unknown,
                    };
                }
            }
            if let HirExpr::Identifier(name, _) = &**callee
                && !locals.contains_key(name)
                && !functions.global_bindings.contains(name)
            {
                if let Some(kind) = functions.return_kinds.get(name)
                    && !matches!(kind, LocalKind::Unknown)
                {
                    return kind.clone();
                }
                if let Some((_, _, fn_type)) = functions.get(name)
                    && fn_type.results.first() == Some(&WasmType::F64)
                {
                    return LocalKind::Float;
                }
            }
            // I64 also represents `none`; storage type alone proves no semantic type.
            LocalKind::Unknown
        }
        HirExpr::Binary(BinaryOp::Div, left, right, _) => {
            let left = infer_expr_kind(left, locals, functions, table_indices, anon_map);
            let right = infer_expr_kind(right, locals, functions, table_indices, anon_map);
            if matches!(left, LocalKind::Int | LocalKind::Float)
                && matches!(right, LocalKind::Int | LocalKind::Float)
            {
                LocalKind::Float
            } else {
                LocalKind::Unknown
            }
        }
        HirExpr::Binary(
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::IntDiv | BinaryOp::Mod,
            left,
            right,
            _,
        ) => {
            let left = infer_expr_kind(left, locals, functions, table_indices, anon_map);
            let right = infer_expr_kind(right, locals, functions, table_indices, anon_map);
            match (left, right) {
                (LocalKind::Int, LocalKind::Int) => LocalKind::Int,
                (LocalKind::Int | LocalKind::Float, LocalKind::Int | LocalKind::Float) => {
                    LocalKind::Float
                }
                _ => LocalKind::Unknown,
            }
        }
        HirExpr::Unary(UnaryOp::Not, ..) => LocalKind::Bool,
        HirExpr::Unary(UnaryOp::Neg | UnaryOp::Pos, value, _) => {
            match infer_expr_kind(value, locals, functions, table_indices, anon_map) {
                kind @ (LocalKind::Int | LocalKind::Float) => kind,
                _ => LocalKind::Unknown,
            }
        }
        HirExpr::If(_, left, right, _) | HirExpr::Binary(BinaryOp::OrElse, left, right, _) => {
            let left = infer_expr_kind(left, locals, functions, table_indices, anon_map);
            let right = infer_expr_kind(right, locals, functions, table_indices, anon_map);
            if left == right {
                left
            } else {
                LocalKind::Unknown
            }
        }
        _ => LocalKind::Unknown,
    }
}

/// Compiles a single function's body into a WebAssembly `Function`.
#[allow(clippy::too_many_arguments)]
fn compile_function_body(
    func_name: &str,
    params: &[HirParam],
    param_types: &[WasmType],
    return_type: Option<WasmType>,
    body: &[HirStmt],
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
) -> Result<Function, WasmCompileError> {
    let mut locals: HashMap<String, (u32, WasmType, LocalKind)> = HashMap::new();
    let mut declared_locals: Vec<WasmType> = Vec::new();

    // The body's own failure propagation needs the result type: an expression-level
    // `?` that unwinds out of the function must leave a value of that type.
    let async_helpers = AsyncHelpers {
        function_return: return_type,
        ..async_helpers
    };

    let struct_target = if func_name.contains('.') {
        func_name.split('.').next()
    } else {
        None
    };

    // Map parameters to locals 0..params.len()
    for (i, param) in params.iter().enumerate() {
        let ty = param_types
            .get(i)
            .copied()
            .unwrap_or_else(|| param_wasm_type(param));
        // The representation decides the semantic kind when there is no
        // annotation: a parameter the call sites pass as a pointer holds a
        // `String` (or another heap value), not a number.
        let kind = match annotated_kind(param.type_annotation.as_ref(), structs) {
            LocalKind::Unknown
                if (param.name == "self" || param.is_self)
                    && struct_target.is_some_and(|s| structs.contains_key(s)) =>
            {
                LocalKind::Struct(struct_target.unwrap().to_string())
            }
            // An unannotated parameter keeps the kind the call sites revealed. The
            // representation only says `i32`, which a String, a List, a Dict and a
            // struct all share.
            LocalKind::Unknown => functions
                .param_kinds
                .get(func_name)
                .and_then(|row| row.get(i))
                .cloned()
                .flatten()
                .unwrap_or_else(|| {
                    if ty == WasmType::I32 {
                        LocalKind::String
                    } else {
                        LocalKind::Unknown
                    }
                }),
            other => other,
        };
        locals.insert(param.name.clone(), (i as u32, ty, kind));
    }

    // Allocate 8 helper scratch locals for struct construction nesting
    for d in 0..8 {
        let idx = (params.len() + declared_locals.len()) as u32;
        declared_locals.push(WasmType::I32);
        locals.insert(
            format!("__struct_temp_{d}"),
            (idx, WasmType::I32, LocalKind::Int),
        );
    }

    // Allocate 8 helper scratch locals for call nesting
    for d in 0..8 {
        let idx = (params.len() + declared_locals.len()) as u32;
        declared_locals.push(WasmType::I32);
        locals.insert(
            format!("__call_temp_{d}"),
            (idx, WasmType::I32, LocalKind::Int),
        );
    }

    // Scratch locals for the `math` helpers. Those bodies stage `f64` operands
    // (`round`, `pow`) and `i64` intermediates, so they need their own slots:
    // reusing an `i32` scratch local makes the module invalid, not merely wrong.
    for (prefix, ty) in [
        ("__f64_temp_", WasmType::F64),
        ("__i64_temp_", WasmType::I64),
    ] {
        for d in 0..8 {
            let idx = (params.len() + declared_locals.len()) as u32;
            declared_locals.push(ty);
            locals.insert(format!("{prefix}{d}"), (idx, ty, LocalKind::Unknown));
        }
    }

    // Allocate 8 helper scratch locals for list literal construction nesting
    for d in 0..8 {
        let idx = (params.len() + declared_locals.len()) as u32;
        declared_locals.push(WasmType::I32);
        locals.insert(
            format!("__list_temp_{d}"),
            (idx, WasmType::I32, LocalKind::Int),
        );
    }

    // Allocate 8 helper scratch i64 locals for `or_else` primary value staging
    for d in 0..8 {
        let idx = (params.len() + declared_locals.len()) as u32;
        declared_locals.push(WasmType::I64);
        locals.insert(
            format!("__or_else_temp_{d}"),
            (idx, WasmType::I64, LocalKind::Int),
        );
    }

    // Allocate 8 helper scratch locals for dict literal construction nesting
    for d in 0..8 {
        let idx = (params.len() + declared_locals.len()) as u32;
        declared_locals.push(WasmType::I32);
        locals.insert(
            format!("__dict_temp_{d}"),
            (idx, WasmType::I32, LocalKind::Int),
        );
    }

    let dot_temp_idx = (params.len() + declared_locals.len()) as u32;
    declared_locals.push(WasmType::I32);
    locals.insert(
        "__dot_temp".to_string(),
        (dot_temp_idx, WasmType::I32, LocalKind::Int),
    );

    // Dedicated staging for a guarded write's previous value. The generic scratch
    // pool is reused by every nested expression, and the value has to survive until
    // the boundary check.
    let journal_prev_idx = (params.len() + declared_locals.len()) as u32;
    declared_locals.push(WasmType::I64);
    locals.insert(
        "__journal_prev".to_string(),
        (journal_prev_idx, WasmType::I64, LocalKind::Unknown),
    );

    // Pre-scan pass to register all local variable declarations (let/var/repeat)
    let mut pre_scan_repeat_id = 0;
    pre_scan_stmts(
        body,
        params.len(),
        &mut locals,
        &mut declared_locals,
        functions,
        structs,
        table_indices,
        anon_map,
        &mut pre_scan_repeat_id,
    )?;

    // A module binding declared by the entry script lives in its global, not in a
    // frame local. The pre-scan cannot know which scope it is scanning, so the
    // promotion happens here, once, for the entry script only: a function that
    // declares its own `let` of the same name still shadows into a local.
    if func_name == "__top_level__" {
        for name in functions.module_globals.keys() {
            locals.remove(name);
        }
    }

    // Build the Wasm function with its locals declaration
    let locals_spec = compress_locals(&declared_locals);
    let mut func = Function::new(locals_spec);
    // The predicate reads the state; verifying it would recurse forever.
    let async_helpers = if func_name.ends_with(".invariant") {
        AsyncHelpers {
            suppress_boundary: true,
            ..async_helpers
        }
    } else if func_name.ends_with(".init") {
        AsyncHelpers {
            constructing_type: true,
            ..async_helpers
        }
    } else {
        async_helpers
    };
    let mut control_stack: Vec<ControlFrame> = Vec::new();
    let mut emit_repeat_id = 0;

    // Every mutating function owns its mutation journal: an entry left by the
    // caller must not be restored by this boundary. The read-only `invariant`
    // predicate is exempt, or checking it would wipe the journal it is guarding.
    // `init` assigns the fields that make the invariant true, so it cannot be
    // verified until the construction site has run it; `invariant` is the check
    // itself and must not recurse into it.
    let owns_journal = func_name != "__top_level__"
        && !func_name.ends_with(".invariant")
        && !func_name.ends_with(".init");
    if owns_journal {
        for slot in [async_helpers.journal, async_helpers.journal2] {
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::GlobalSet(slot.live));
        }
    }

    let terminated = compile_stmts(
        body,
        &mut func,
        &locals,
        functions,
        &mut control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        return_type,
        func_name == "__top_level__",
        &mut emit_repeat_id,
        0,
        0,
    )?;

    // A function body is a stable mutable boundary: canon validates the invariant
    // of every instance the operation touched once the body reaches its end, and a
    // violation restores each journaled field before the failure propagates.
    if owns_journal {
        let journal = async_helpers.journal;
        let journal2 = async_helpers.journal2;
        for param in params {
            if !param.is_mut {
                continue;
            }
            let Some((slot, _, kind)) = locals.get(&param.name) else {
                continue;
            };
            let LocalKind::Struct(type_name) = kind else {
                continue;
            };
            if !functions.invariant_types.contains(type_name) {
                continue;
            }
            let invariant_name = format!("{type_name}.invariant");
            let Some(&(inv_idx, _, _)) = functions.get(&invariant_name) else {
                continue;
            };
            func.instruction(&Instruction::LocalGet(*slot));
            func.instruction(&Instruction::Call(inv_idx));
            func.instruction(&Instruction::I32Eqz);
            func.instruction(&Instruction::If(BlockType::Empty));
            validator_restore(&mut func, journal, journal2);
            validator_message(
                &mut func,
                functions,
                static_strings,
                type_name,
                async_helpers.fail_globals,
            );
            func.instruction(&Instruction::End);
        }
    }

    // Ensure valid Wasm block termination
    if !terminated && let Some(ret_ty) = return_type {
        match ret_ty {
            WasmType::I64 => {
                func.instruction(&Instruction::I64Const(0));
            }
            WasmType::F64 => {
                func.instruction(&Instruction::F64Const(0.0.into()));
            }
            WasmType::I32 => {
                func.instruction(&Instruction::I32Const(0));
            }
        }
    }

    func.instruction(&Instruction::End);
    Ok(func)
}

/// Emits the invariant check for every guarded instance the frame holds.
///
/// A boundary that is not the body's end — a `return` inside a loop, say — has to
/// validate before it leaves, or a caller would observe a state canon forbids.
fn emit_boundary_checks(
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    static_strings: &HashMap<String, i32>,
    async_helpers: AsyncHelpers,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    let journal = async_helpers.journal;
    let journal2 = async_helpers.journal2;
    let mut seen: Vec<&String> = Vec::new();
    for (name, (slot, _, kind)) in locals {
        let LocalKind::Struct(type_name) = kind else {
            continue;
        };
        if !functions.invariant_types.contains(type_name) || seen.contains(&type_name) {
            continue;
        }
        seen.push(type_name);
        // A boundary only validates an instance this frame actually mutated; the
        // journal is the record of that.
        let invariant_name = format!("{type_name}.invariant");
        if let Some(&(inv_idx, _, _)) = functions.get(&invariant_name) {
            let _ = name;
            func.instruction(&Instruction::LocalGet(*slot));
            func.instruction(&Instruction::Call(inv_idx));
            func.instruction(&Instruction::I32Eqz);
            func.instruction(&Instruction::If(BlockType::Empty));
            validator_restore(func, journal, journal2);
            validator_message(
                func,
                functions,
                static_strings,
                type_name,
                async_helpers.fail_globals,
            );
            func.instruction(&Instruction::End);
        }
    }
}

/// Emits the restore of both journal slots.
fn validator_restore(func: &mut Function, journal: JournalGlobals, journal2: JournalGlobals) {
    for slot in [journal, journal2] {
        func.instruction(&Instruction::GlobalGet(slot.live));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::GlobalGet(slot.instance));
        func.instruction(&Instruction::GlobalGet(slot.offset));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::GlobalGet(slot.value));
        func.instruction(&Instruction::I64Store(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::GlobalSet(slot.live));
        func.instruction(&Instruction::End);
    }
}

/// Emits the failure message a violated `invariant()` reports.
fn validator_message(
    func: &mut Function,
    functions: &FunctionTable,
    static_strings: &HashMap<String, i32>,
    type_name: &str,
    fail_globals: FailureGlobals,
) {
    let (concat_idx, _, _) = functions["__aipo_str_concat"];
    func.instruction(&Instruction::I32Const(
        static_strings
            .get(INVARIANT_PREFIX)
            .copied()
            .unwrap_or_default(),
    ));
    func.instruction(&Instruction::I32Const(
        static_strings.get(type_name).copied().unwrap_or_default(),
    ));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::I32Const(
        static_strings
            .get(INVARIANT_SUFFIX)
            .copied()
            .unwrap_or_default(),
    ));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::GlobalSet(fail_globals.message_idx));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::GlobalSet(fail_globals.status_idx));
}

/// Compiles a slice of statements into a WebAssembly `Function`.
#[allow(clippy::too_many_arguments)]
fn compile_stmts(
    stmts: &[HirStmt],
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    is_top_level: bool,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<bool, WasmCompileError> {
    let mut terminated = false;
    let stmt_count = stmts.len();

    for (idx, stmt) in stmts.iter().enumerate() {
        let is_last = idx + 1 == stmt_count;
        match stmt {
            HirStmt::Let(name, init_expr, _) | HirStmt::Var(name, init_expr, _) => {
                // A module binding lives in its global; assigning here keeps the
                // declaration as the single place the initial value is produced, so a
                // function can never observe a half-initialized module.
                if let Some(&(global_idx, global_ty, _)) = functions.module_globals.get(name)
                    && !locals.contains_key(name)
                {
                    let expr_ty = compile_expr(
                        init_expr,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, expr_ty, global_ty);
                    func.instruction(&Instruction::GlobalSet(global_idx));
                    continue;
                }
                let (local_idx, local_ty, _) = locals[name];
                let expr_ty = compile_expr(
                    init_expr,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, expr_ty, local_ty);
                func.instruction(&Instruction::LocalSet(local_idx));
            }
            HirStmt::Assign(target, expr, span) => match target {
                HirExpr::Identifier(name, _) => {
                    if let Some(&(local_idx, target_ty, _)) = locals.get(name) {
                        let expr_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, expr_ty, target_ty);
                        func.instruction(&Instruction::LocalSet(local_idx));
                    } else if let Some(&(global_idx, global_ty, _)) =
                        functions.module_globals.get(name)
                    {
                        // A module binding is shared with every function that names
                        // it, so the write goes to the one storage cell both sides use.
                        let expr_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, expr_ty, global_ty);
                        func.instruction(&Instruction::GlobalSet(global_idx));
                    } else {
                        // Approximation: unknown/captured assignment target is a no-op.
                        let _ = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        );
                        func.instruction(&Instruction::Drop);
                    }
                }
                HirExpr::Dot(receiver, field_name, _) => {
                    // A module binding lives in a global, so its kind comes from the
                    // module table rather than from `locals`.
                    let receiver_struct_name = match &**receiver {
                        HirExpr::Identifier(name, _) => locals
                            .get(name)
                            .and_then(|(_, _, kind)| match kind {
                                LocalKind::Struct(s_name) => Some(s_name.clone()),
                                _ => None,
                            })
                            .or_else(|| {
                                functions.module_globals.get(name).and_then(|(_, _, kind)| {
                                    match kind {
                                        LocalKind::Struct(s_name) => Some(s_name.clone()),
                                        _ => None,
                                    }
                                })
                            }),
                        _ => None,
                    };
                    let receiver_struct_name = receiver_struct_name.as_deref();
                    let field_info = if let Some(s_name) = receiver_struct_name {
                        structs
                            .get(s_name)
                            .and_then(|s| s.fields.get(field_name).copied())
                    } else {
                        structs
                            .values()
                            .find_map(|s| s.fields.get(field_name).copied())
                    };

                    if let Some((field_offset, field_ty)) = field_info {
                        let instance_slot = locals["__dot_temp"].0;
                        // Inside a function the write is provisional: the boundary is
                        // the call, so the entry value is journaled here and restored
                        // only if the boundary check fails.
                        let r_ty = compile_expr(
                            receiver,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, r_ty, WasmType::I32);
                        // Keep the instance for the invariant check after the store,
                        // and stage the previous value so a violation can restore it.
                        func.instruction(&Instruction::LocalTee(instance_slot));
                        // A dedicated staging slot: the guard against clobbering is
                        // that the value is read back through the journal, not through
                        // the scratch pool.
                        func.instruction(&Instruction::LocalGet(instance_slot));
                        match field_ty {
                            WasmType::F64 => {
                                func.instruction(&Instruction::F64Load(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            WasmType::I32 => {
                                func.instruction(&Instruction::I32Load(MemArg {
                                    offset: field_offset as u64,
                                    align: 2,
                                    memory_index: 0,
                                }));
                                func.instruction(&Instruction::I64ExtendI32U);
                            }
                            WasmType::I64 => {
                                func.instruction(&Instruction::I64Load(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                        }
                        func.instruction(&Instruction::LocalSet(locals["__journal_prev"].0));
                        let journal = async_helpers.journal;
                        let journal2 = async_helpers.journal2;
                        // Journal the entry value before the store replaces it. A
                        // guarded write inside a function is provisional until the call
                        // boundary verifies it, and a violation restores this value.
                        if !is_top_level
                            && !async_helpers.constructing_type
                            && receiver_struct_name
                                .is_some_and(|name| functions.invariant_types.contains(name))
                        {
                            // Slot 1 keeps the first instance; a different instance
                            // takes slot 2, so a two-instance operation can restore
                            // both.
                            func.instruction(&Instruction::GlobalGet(journal.live));
                            func.instruction(&Instruction::If(BlockType::Empty));
                            func.instruction(&Instruction::GlobalGet(journal.instance));
                            func.instruction(&Instruction::LocalGet(instance_slot));
                            func.instruction(&Instruction::I32Ne);
                            func.instruction(&Instruction::If(BlockType::Empty));
                            func.instruction(&Instruction::LocalGet(instance_slot));
                            func.instruction(&Instruction::GlobalSet(journal2.instance));
                            func.instruction(&Instruction::I32Const(field_offset as i32));
                            func.instruction(&Instruction::GlobalSet(journal2.offset));
                            func.instruction(&Instruction::LocalGet(locals["__journal_prev"].0));
                            func.instruction(&Instruction::GlobalSet(journal2.value));
                            func.instruction(&Instruction::I32Const(1));
                            func.instruction(&Instruction::GlobalSet(journal2.live));
                            func.instruction(&Instruction::End);
                            // The first instance keeps slot 1; only a fresh journal
                            // stores there.
                            func.instruction(&Instruction::Else);
                            func.instruction(&Instruction::LocalGet(instance_slot));
                            func.instruction(&Instruction::GlobalSet(journal.instance));
                            func.instruction(&Instruction::I32Const(field_offset as i32));
                            func.instruction(&Instruction::GlobalSet(journal.offset));
                            func.instruction(&Instruction::LocalGet(locals["__journal_prev"].0));
                            func.instruction(&Instruction::GlobalSet(journal.value));
                            func.instruction(&Instruction::I32Const(1));
                            func.instruction(&Instruction::GlobalSet(journal.live));
                            func.instruction(&Instruction::End);
                        }
                        let val_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, val_ty, field_ty);
                        match field_ty {
                            WasmType::F64 => {
                                func.instruction(&Instruction::F64Store(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                            WasmType::I32 => {
                                func.instruction(&Instruction::I32Store(MemArg {
                                    offset: field_offset as u64,
                                    align: 2,
                                    memory_index: 0,
                                }));
                            }
                            WasmType::I64 => {
                                func.instruction(&Instruction::I64Store(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                        }

                        // A write to a guarded type is a stable boundary of its own in
                        // the entry script, so the invariant is verified right away and
                        // a violation raises a recoverable failure. Inside a function
                        // the boundary is the whole operation, which `compile_stmts`
                        // verifies when the body finishes, so a transient state is
                        // allowed here.
                        if is_top_level && let Some(s_name) = receiver_struct_name {
                            // Journal the entry before checking, so the restore path
                            // has the value the field held. The instance is re-read
                            // from its slot because compiling the value may have
                            // reused the scratch it was staged in.
                            let journal = async_helpers.journal;
                            func.instruction(&Instruction::LocalGet(instance_slot));
                            func.instruction(&Instruction::GlobalSet(journal.instance));
                            func.instruction(&Instruction::I32Const(field_offset as i32));
                            func.instruction(&Instruction::GlobalSet(journal.offset));
                            // The pre-write value was staged before the store; reuse it
                            // instead of re-reading the field, which now holds the new
                            // value.
                            func.instruction(&Instruction::LocalGet(locals["__journal_prev"].0));
                            func.instruction(&Instruction::GlobalSet(journal.value));
                            func.instruction(&Instruction::I32Const(1));
                            func.instruction(&Instruction::GlobalSet(journal.live));
                            let invariant_name = format!("{s_name}.invariant");
                            if let Some(&(inv_idx, _, _)) = functions.get(&invariant_name) {
                                func.instruction(&Instruction::LocalGet(instance_slot));
                                func.instruction(&Instruction::Call(inv_idx));
                                func.instruction(&Instruction::I32Eqz);
                                func.instruction(&Instruction::If(BlockType::Empty));
                                // Report the same message the VM produces, then unwind
                                // to the nearest handler.
                                let (concat_idx, _, _) = functions["__aipo_str_concat"];
                                func.instruction(&Instruction::I32Const(
                                    static_strings
                                        .get(INVARIANT_PREFIX)
                                        .copied()
                                        .unwrap_or_default(),
                                ));
                                func.instruction(&Instruction::I32Const(
                                    static_strings.get(s_name).copied().unwrap_or_default(),
                                ));
                                func.instruction(&Instruction::Call(concat_idx));
                                func.instruction(&Instruction::I32Const(
                                    static_strings
                                        .get(INVARIANT_SUFFIX)
                                        .copied()
                                        .unwrap_or_default(),
                                ));
                                func.instruction(&Instruction::Call(concat_idx));
                                func.instruction(&Instruction::GlobalSet(
                                    async_helpers.fail_globals.message_idx,
                                ));
                                func.instruction(&Instruction::I32Const(1));
                                func.instruction(&Instruction::GlobalSet(
                                    async_helpers.fail_globals.status_idx,
                                ));
                                // Canon restores the entry value of every field the
                                // operation touched before the failure propagates.
                                func.instruction(&Instruction::LocalGet(instance_slot));
                                match field_ty {
                                    WasmType::F64 => {
                                        func.instruction(&Instruction::GlobalGet(journal.value));
                                        func.instruction(&Instruction::F64ReinterpretI64);
                                        func.instruction(&Instruction::F64Store(MemArg {
                                            offset: field_offset as u64,
                                            align: 3,
                                            memory_index: 0,
                                        }));
                                    }
                                    WasmType::I32 => {
                                        func.instruction(&Instruction::GlobalGet(journal.value));
                                        func.instruction(&Instruction::I32WrapI64);
                                        func.instruction(&Instruction::I32Store(MemArg {
                                            offset: field_offset as u64,
                                            align: 2,
                                            memory_index: 0,
                                        }));
                                    }
                                    WasmType::I64 => {
                                        func.instruction(&Instruction::GlobalGet(journal.value));
                                        func.instruction(&Instruction::I64Store(MemArg {
                                            offset: field_offset as u64,
                                            align: 3,
                                            memory_index: 0,
                                        }));
                                    }
                                }
                                func.instruction(&Instruction::I32Const(0));
                                func.instruction(&Instruction::GlobalSet(journal.live));
                                func.instruction(&Instruction::End);
                            }
                        }
                    } else {
                        return Err(WasmCompileError::UnsupportedExpr {
                            message: format!("unknown field `{field_name}` on struct"),
                            span: *span,
                        });
                    }
                }
                HirExpr::Index(target_coll, index, _) => {
                    let coll_kind =
                        infer_expr_kind(target_coll, locals, functions, table_indices, anon_map);
                    if matches!(coll_kind, LocalKind::Dict(_)) {
                        let (dict_set_idx, _, _) = functions["__aipo_dict_set"];
                        let (string_hash_idx, _, _) = functions["__aipo_string_hash"];

                        let c_ty = compile_expr(
                            target_coll,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, c_ty, WasmType::I32);

                        let _ = (string_hash_idx,);
                        compile_dict_key_pair(
                            index,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;

                        let val_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, val_ty, WasmType::I64);
                        func.instruction(&Instruction::Call(dict_set_idx));
                        func.instruction(&Instruction::Drop);
                    } else if matches!(coll_kind, LocalKind::Bytes) {
                        let (bytes_set_idx, _, _) = functions["__aipo_bytes_set"];
                        let c_ty = compile_expr(
                            target_coll,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, c_ty, WasmType::I32);

                        let i_ty = compile_expr(
                            index,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, i_ty, WasmType::I32);

                        let val_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, val_ty, WasmType::I64);
                        func.instruction(&Instruction::Call(bytes_set_idx));
                        func.instruction(&Instruction::Drop);
                    } else {
                        let (list_set_idx, _, _) = functions["__aipo_list_set"];
                        let c_ty = compile_expr(
                            target_coll,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, c_ty, WasmType::I32);

                        let i_ty = compile_expr(
                            index,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, i_ty, WasmType::I32);

                        let val_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, val_ty, WasmType::I64);
                        func.instruction(&Instruction::Call(list_set_idx));
                        func.instruction(&Instruction::Drop);
                    }
                }
                _ => {
                    return Err(WasmCompileError::UnsupportedStmt {
                        message: "unsupported assignment target in Wasm backend".into(),
                        span: *span,
                    });
                }
            },
            HirStmt::CompoundAssign(op, target, expr, span) => match target {
                HirExpr::Identifier(name, _) => {
                    if let Some(&(local_idx, target_ty, ref target_kind)) = locals.get(name) {
                        if *op == BinaryOp::Add && matches!(target_kind, LocalKind::String) {
                            func.instruction(&Instruction::LocalGet(local_idx));
                            let expr_kind =
                                infer_expr_kind(expr, locals, functions, table_indices, anon_map);
                            let expr_ty = compile_expr(
                                expr,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            match expr_kind {
                                LocalKind::String => {
                                    coerce_type(func, expr_ty, WasmType::I32);
                                }
                                LocalKind::Int => {
                                    coerce_type(func, expr_ty, WasmType::I64);
                                    let (int_to_str_idx, _, _) = functions["__aipo_int_to_str"];
                                    func.instruction(&Instruction::Call(int_to_str_idx));
                                }
                                LocalKind::Bool => {
                                    coerce_type(func, expr_ty, WasmType::I32);
                                    let (bool_to_str_idx, _, _) = functions["__aipo_bool_to_str"];
                                    func.instruction(&Instruction::Call(bool_to_str_idx));
                                }
                                _ => {
                                    if expr_ty == WasmType::I64 {
                                        let (int_to_str_idx, _, _) = functions["__aipo_int_to_str"];
                                        func.instruction(&Instruction::Call(int_to_str_idx));
                                    } else {
                                        coerce_type(func, expr_ty, WasmType::I32);
                                    }
                                }
                            }
                            let (str_concat_idx, _, _) = functions["__aipo_str_concat"];
                            func.instruction(&Instruction::Call(str_concat_idx));
                            func.instruction(&Instruction::LocalSet(local_idx));
                        } else {
                            func.instruction(&Instruction::LocalGet(local_idx));
                            let expr_ty = compile_expr(
                                expr,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            emit_binary_op(func, *op, target_ty, expr_ty, *span)?;
                            func.instruction(&Instruction::LocalSet(local_idx));
                        }
                    } else if let Some(&(global_idx, global_ty, ref global_kind)) =
                        functions.module_globals.get(name)
                    {
                        // A module binding compounds into its global, which is the one
                        // cell every function that names it shares.
                        if *op == BinaryOp::Add && matches!(global_kind, LocalKind::String) {
                            func.instruction(&Instruction::GlobalGet(global_idx));
                            let expr_kind =
                                infer_expr_kind(expr, locals, functions, table_indices, anon_map);
                            let expr_ty = compile_expr(
                                expr,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            match expr_kind {
                                LocalKind::String => {
                                    coerce_type(func, expr_ty, WasmType::I32);
                                }
                                LocalKind::Int => {
                                    coerce_type(func, expr_ty, WasmType::I64);
                                    let (int_to_str_idx, _, _) = functions["__aipo_int_to_str"];
                                    func.instruction(&Instruction::Call(int_to_str_idx));
                                }
                                LocalKind::Bool => {
                                    coerce_type(func, expr_ty, WasmType::I32);
                                    let (bool_to_str_idx, _, _) = functions["__aipo_bool_to_str"];
                                    func.instruction(&Instruction::Call(bool_to_str_idx));
                                }
                                _ => {
                                    if expr_ty == WasmType::I64 {
                                        let (int_to_str_idx, _, _) = functions["__aipo_int_to_str"];
                                        func.instruction(&Instruction::Call(int_to_str_idx));
                                    } else {
                                        coerce_type(func, expr_ty, WasmType::I32);
                                    }
                                }
                            }
                            let (str_concat_idx, _, _) = functions["__aipo_str_concat"];
                            func.instruction(&Instruction::Call(str_concat_idx));
                            func.instruction(&Instruction::GlobalSet(global_idx));
                        } else {
                            func.instruction(&Instruction::GlobalGet(global_idx));
                            let expr_ty = compile_expr(
                                expr,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            emit_binary_op(func, *op, global_ty, expr_ty, *span)?;
                            func.instruction(&Instruction::GlobalSet(global_idx));
                        }
                    } else {
                        // Approximation: unknown/captured assignment target is a no-op.
                        let _ = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        );
                        func.instruction(&Instruction::Drop);
                    }
                }
                HirExpr::Dot(receiver, field_name, _) => {
                    let receiver_struct_name = match &**receiver {
                        HirExpr::Identifier(name, _) => {
                            locals.get(name).and_then(|(_, _, kind)| match kind {
                                LocalKind::Struct(s_name) => Some(s_name.as_str()),
                                _ => None,
                            })
                        }
                        _ => None,
                    };
                    let field_info = if let Some(s_name) = receiver_struct_name {
                        structs
                            .get(s_name)
                            .and_then(|s| s.fields.get(field_name).copied())
                    } else {
                        structs
                            .values()
                            .find_map(|s| s.fields.get(field_name).copied())
                    };

                    if let Some((field_offset, field_ty)) = field_info {
                        let dot_temp = locals["__dot_temp"].0;
                        let r_ty = compile_expr(
                            receiver,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, r_ty, WasmType::I32);
                        func.instruction(&Instruction::LocalSet(dot_temp));

                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        match field_ty {
                            WasmType::F64 => {
                                func.instruction(&Instruction::F64Load(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                            WasmType::I32 => {
                                func.instruction(&Instruction::I32Load(MemArg {
                                    offset: field_offset as u64,
                                    align: 2,
                                    memory_index: 0,
                                }));
                            }
                            WasmType::I64 => {
                                func.instruction(&Instruction::I64Load(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                        }

                        let expr_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        emit_binary_op(func, *op, field_ty, expr_ty, *span)?;
                        match field_ty {
                            WasmType::F64 => {
                                func.instruction(&Instruction::F64Store(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                            WasmType::I32 => {
                                func.instruction(&Instruction::I32Store(MemArg {
                                    offset: field_offset as u64,
                                    align: 2,
                                    memory_index: 0,
                                }));
                            }
                            WasmType::I64 => {
                                func.instruction(&Instruction::I64Store(MemArg {
                                    offset: field_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                        }
                    } else {
                        return Err(WasmCompileError::UnsupportedExpr {
                            message: format!("unknown field `{field_name}` on struct"),
                            span: *span,
                        });
                    }
                }
                HirExpr::Index(coll, index, index_span) => {
                    let coll_kind =
                        infer_expr_kind(coll, locals, functions, table_indices, anon_map);
                    let dot_temp = locals["__dot_temp"].0;
                    let c_ty = compile_expr(
                        coll,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, c_ty, WasmType::I32);
                    func.instruction(&Instruction::LocalSet(dot_temp));

                    if matches!(coll_kind, LocalKind::Dict(_)) {
                        let (dict_get_idx, _, _) = functions["__aipo_dict_get"];
                        let (dict_set_idx, _, _) = functions["__aipo_dict_set"];
                        let (string_hash_idx, _, _) = functions["__aipo_string_hash"];
                        let key_temp = locals["__or_else_temp_0"].0;
                        let val_temp = locals["__or_else_temp_1"].0;

                        let _ = (string_hash_idx,);
                        // The key pair is `(hash, raw)`: the hash drives lookup, the raw
                        // value is stored beside it so `keys()` can hand it back.
                        let raw_temp = locals["__or_else_temp_2"].0;
                        compile_dict_key_pair(
                            index,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        func.instruction(&Instruction::LocalSet(raw_temp));
                        func.instruction(&Instruction::LocalSet(key_temp));

                        // Load current value
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(key_temp));
                        func.instruction(&Instruction::Call(dict_get_idx));

                        // Compile expr and perform op
                        let expr_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        emit_binary_op(func, *op, WasmType::I64, expr_ty, *index_span)?;
                        func.instruction(&Instruction::LocalSet(val_temp));

                        // Store new value: the hash and the raw key both go back in.
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(key_temp));
                        func.instruction(&Instruction::LocalGet(raw_temp));
                        func.instruction(&Instruction::LocalGet(val_temp));
                        func.instruction(&Instruction::Call(dict_set_idx));
                        func.instruction(&Instruction::Drop);
                    } else if matches!(coll_kind, LocalKind::Bytes) {
                        let (bytes_get_idx, _, _) = functions["__aipo_bytes_get"];
                        let (bytes_set_idx, _, _) = functions["__aipo_bytes_set"];
                        let idx_temp = locals["__call_temp_0"].0;
                        let val_temp = locals["__or_else_temp_0"].0;

                        let i_ty = compile_expr(
                            index,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, i_ty, WasmType::I32);
                        func.instruction(&Instruction::LocalSet(idx_temp));

                        // Load current byte
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(idx_temp));
                        func.instruction(&Instruction::Call(bytes_get_idx));

                        // Compile expr and perform op
                        let expr_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        emit_binary_op(func, *op, WasmType::I64, expr_ty, *index_span)?;
                        func.instruction(&Instruction::LocalSet(val_temp));

                        // Store new byte
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(idx_temp));
                        func.instruction(&Instruction::LocalGet(val_temp));
                        func.instruction(&Instruction::Call(bytes_set_idx));
                        func.instruction(&Instruction::Drop);
                    } else {
                        // List
                        let (list_get_idx, _, _) = functions["__aipo_list_get"];
                        let (list_set_idx, _, _) = functions["__aipo_list_set"];
                        let idx_temp = locals["__call_temp_0"].0;
                        let val_temp = locals["__or_else_temp_0"].0;

                        let i_ty = compile_expr(
                            index,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, i_ty, WasmType::I32);
                        func.instruction(&Instruction::LocalSet(idx_temp));

                        // Load current element
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(idx_temp));
                        func.instruction(&Instruction::Call(list_get_idx));

                        // Compile expr and perform op
                        let expr_ty = compile_expr(
                            expr,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        emit_binary_op(func, *op, WasmType::I64, expr_ty, *index_span)?;
                        func.instruction(&Instruction::LocalSet(val_temp));

                        // Store new element
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(idx_temp));
                        func.instruction(&Instruction::LocalGet(val_temp));
                        func.instruction(&Instruction::Call(list_set_idx));
                        func.instruction(&Instruction::Drop);
                    }
                }
                _ => {
                    return Err(WasmCompileError::UnsupportedStmt {
                        message: "unsupported compound assignment target in Wasm backend".into(),
                        span: *span,
                    });
                }
            },
            HirStmt::Return(maybe_expr, span) => {
                // A `return` is a stable boundary: the operation ends here, so a
                // guarded instance it mutated is verified before the value leaves.
                emit_boundary_checks(
                    func,
                    locals,
                    functions,
                    static_strings,
                    async_helpers,
                    !is_top_level && !async_helpers.suppress_boundary,
                );
                if let Some(expr) = maybe_expr {
                    let expr_ty = compile_expr(
                        expr,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    if let Some(expected_ret) = return_type {
                        coerce_type(func, expr_ty, expected_ret);
                    }
                } else if return_type.is_some() {
                    return Err(WasmCompileError::TypeMismatch {
                        expected: format!("{return_type:?}"),
                        found: "void".into(),
                        span: *span,
                    });
                }
                func.instruction(&Instruction::Return);
                terminated = true;
            }
            HirStmt::If(if_stmt) => {
                compile_if_stmt(
                    if_stmt,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
            }
            HirStmt::While(cond, loop_body, _) => {
                compile_while_stmt(
                    cond,
                    loop_body,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
            }
            HirStmt::Loop(loop_body, _) => {
                compile_loop_stmt(
                    loop_body,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
            }
            HirStmt::Repeat(count, maybe_index, loop_body, _) => {
                compile_repeat_stmt(
                    count,
                    maybe_index.as_deref(),
                    loop_body,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
            }
            HirStmt::Each(bindings, iterable, loop_body, _) => {
                compile_each_stmt(
                    bindings,
                    iterable,
                    loop_body,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
            }
            HirStmt::Match(match_stmt) => {
                compile_match_stmt(
                    match_stmt,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
            }
            HirStmt::Break(span) => {
                if let Some(pos) = control_stack
                    .iter()
                    .rev()
                    .position(|f| *f == ControlFrame::LoopBreak)
                {
                    func.instruction(&Instruction::Br(pos as u32));
                } else {
                    return Err(WasmCompileError::UnsupportedStmt {
                        message: "break outside of loop".into(),
                        span: *span,
                    });
                }
            }
            HirStmt::Continue(span) => {
                if let Some(pos) = control_stack.iter().rev().position(|f| {
                    *f == ControlFrame::RepeatStep || *f == ControlFrame::LoopContinue
                }) {
                    func.instruction(&Instruction::Br(pos as u32));
                } else {
                    return Err(WasmCompileError::UnsupportedStmt {
                        message: "continue outside of loop".into(),
                        span: *span,
                    });
                }
            }
            HirStmt::Expr(expr) => {
                let expr_ty = compile_expr(
                    expr,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                if is_top_level && is_last && return_type.is_some() {
                    if let Some(expected_ret) = return_type {
                        coerce_type(func, expr_ty, expected_ret);
                    }
                    terminated = true;
                } else {
                    func.instruction(&Instruction::Drop);
                }
            }
            HirStmt::AwaitDo(inner_stmts, _) => {
                let inner_term = compile_stmts(
                    inner_stmts,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    false,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
                if inner_term {
                    terminated = true;
                }
            }
            HirStmt::Fail(expr, _) => {
                let msg_ty = compile_expr(
                    expr,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, msg_ty, WasmType::I32);
                func.instruction(&Instruction::GlobalSet(
                    async_helpers.fail_globals.message_idx,
                ));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::GlobalSet(
                    async_helpers.fail_globals.status_idx,
                ));

                // Inside an `attempt`, raising a failure unwinds to the handler
                // block instead of returning from the function.
                if let Some(depth) = control_stack
                    .iter()
                    .rev()
                    .position(|frame| *frame == ControlFrame::AttemptHandler)
                {
                    func.instruction(&Instruction::Br(depth as u32));
                } else {
                    match return_type {
                        Some(WasmType::I64) => {
                            func.instruction(&Instruction::I64Const(0));
                        }
                        Some(WasmType::I32) => {
                            func.instruction(&Instruction::I32Const(0));
                        }
                        Some(WasmType::F64) => {
                            func.instruction(&Instruction::F64Const(0.0.into()));
                        }
                        None => {}
                    }
                    func.instruction(&Instruction::Return);
                }
                terminated = true;
            }
            HirStmt::Attempt(attempt_stmt) => {
                compile_attempt_stmt(
                    attempt_stmt,
                    is_top_level,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    return_type,
                    repeat_id,
                    struct_depth,
                    call_depth,
                )?;
            }
            HirStmt::FnDecl(f) => {
                if let Some(anon_name) = anon_map.get(&f.span)
                    && let Some(&table_idx) = table_indices.get(anon_name)
                    && let Some(&(local_idx, _, _)) = locals.get(&f.name)
                {
                    func.instruction(&Instruction::I32Const(table_idx as i32));
                    func.instruction(&Instruction::LocalSet(local_idx));
                }
            }
        }

        // Statement-boundary failure propagation. Skipped once the path is
        // already terminated, since `Return`/`Break` leave no code to guard.
        if !terminated && stmt_may_fail(stmt) {
            emit_failure_check(func, control_stack, async_helpers.fail_globals, return_type);
        }
    }

    Ok(terminated)
}

/// Compiles an `if ... elif ... else` statement block.
#[allow(clippy::too_many_arguments)]
fn compile_if_stmt(
    if_stmt: &HirIfStmt,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    let cond_ty = compile_expr(
        &if_stmt.condition,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        struct_depth,
        call_depth,
    )?;
    coerce_to_bool(func, cond_ty);

    control_stack.push(ControlFrame::Block);
    func.instruction(&Instruction::If(BlockType::Empty));
    compile_stmts(
        &if_stmt.then_branch,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        return_type,
        false,
        repeat_id,
        struct_depth,
        call_depth,
    )?;

    let has_elifs = !if_stmt.elif_branches.is_empty();
    let has_else = if_stmt.else_branch.is_some();

    if has_elifs || has_else {
        func.instruction(&Instruction::Else);

        let mut elif_count = 0;
        for (elif_cond, elif_body) in &if_stmt.elif_branches {
            let e_cond_ty = compile_expr(
                elif_cond,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_to_bool(func, e_cond_ty);

            control_stack.push(ControlFrame::Block);
            func.instruction(&Instruction::If(BlockType::Empty));
            compile_stmts(
                elif_body,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                return_type,
                false,
                repeat_id,
                struct_depth,
                call_depth,
            )?;
            func.instruction(&Instruction::Else);
            elif_count += 1;
        }

        if let Some(else_branch) = &if_stmt.else_branch {
            compile_stmts(
                else_branch,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                return_type,
                false,
                repeat_id,
                struct_depth,
                call_depth,
            )?;
        }

        for _ in 0..elif_count {
            control_stack.pop();
            func.instruction(&Instruction::End);
        }
    }

    control_stack.pop();
    func.instruction(&Instruction::End);
    Ok(())
}

/// Compiles a `while condition { body }` loop.
#[allow(clippy::too_many_arguments)]
fn compile_while_stmt(
    cond: &HirExpr,
    loop_body: &[HirStmt],
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    control_stack.push(ControlFrame::LoopBreak);
    func.instruction(&Instruction::Block(BlockType::Empty));

    control_stack.push(ControlFrame::LoopContinue);
    func.instruction(&Instruction::Loop(BlockType::Empty));

    let cond_ty = compile_expr(
        cond,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        struct_depth,
        call_depth,
    )?;
    coerce_to_bool(func, cond_ty);
    func.instruction(&Instruction::I32Eqz);

    let break_depth = control_stack
        .iter()
        .rev()
        .position(|f| *f == ControlFrame::LoopBreak)
        .unwrap() as u32;
    func.instruction(&Instruction::BrIf(break_depth));

    compile_stmts(
        loop_body,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        return_type,
        false,
        repeat_id,
        struct_depth,
        call_depth,
    )?;

    let loop_depth = control_stack
        .iter()
        .rev()
        .position(|f| *f == ControlFrame::LoopContinue)
        .unwrap() as u32;
    func.instruction(&Instruction::Br(loop_depth));

    control_stack.pop();
    func.instruction(&Instruction::End);
    control_stack.pop();
    func.instruction(&Instruction::End);

    Ok(())
}

/// Compiles an unconditional `loop { body }` structure.
#[allow(clippy::too_many_arguments)]
fn compile_loop_stmt(
    loop_body: &[HirStmt],
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    control_stack.push(ControlFrame::LoopBreak);
    func.instruction(&Instruction::Block(BlockType::Empty));

    control_stack.push(ControlFrame::LoopContinue);
    func.instruction(&Instruction::Loop(BlockType::Empty));

    compile_stmts(
        loop_body,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        return_type,
        false,
        repeat_id,
        struct_depth,
        call_depth,
    )?;

    let loop_depth = control_stack
        .iter()
        .rev()
        .position(|f| *f == ControlFrame::LoopContinue)
        .unwrap() as u32;
    func.instruction(&Instruction::Br(loop_depth));

    control_stack.pop();
    func.instruction(&Instruction::End);
    control_stack.pop();
    func.instruction(&Instruction::End);

    Ok(())
}

/// Compiles a counted `repeat count [as i] { body }` loop.
#[allow(clippy::too_many_arguments)]
fn compile_repeat_stmt(
    count: &HirExpr,
    maybe_index: Option<&str>,
    loop_body: &[HirStmt],
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    *repeat_id += 1;
    let id = *repeat_id;
    let limit_slot = locals[&format!("__repeat_limit_{id}")].0;
    let idx_slot = locals[&format!("__repeat_idx_{id}")].0;

    let count_ty = compile_expr(
        count,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        struct_depth,
        call_depth,
    )?;
    coerce_type(func, count_ty, WasmType::I64);
    func.instruction(&Instruction::LocalSet(limit_slot));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(idx_slot));

    control_stack.push(ControlFrame::LoopBreak);
    func.instruction(&Instruction::Block(BlockType::Empty));

    control_stack.push(ControlFrame::LoopContinue);
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(idx_slot));
    func.instruction(&Instruction::LocalGet(limit_slot));
    func.instruction(&Instruction::I64LtS);
    func.instruction(&Instruction::I32Eqz);
    let break_depth = control_stack
        .iter()
        .rev()
        .position(|f| *f == ControlFrame::LoopBreak)
        .unwrap() as u32;
    func.instruction(&Instruction::BrIf(break_depth));

    if let Some(name) = maybe_index {
        let user_idx_slot = locals[name].0;
        func.instruction(&Instruction::LocalGet(idx_slot));
        func.instruction(&Instruction::LocalSet(user_idx_slot));
    }

    control_stack.push(ControlFrame::RepeatStep);
    func.instruction(&Instruction::Block(BlockType::Empty));
    compile_stmts(
        loop_body,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        return_type,
        false,
        repeat_id,
        struct_depth,
        call_depth,
    )?;
    control_stack.pop();
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(idx_slot));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(idx_slot));

    let loop_depth = control_stack
        .iter()
        .rev()
        .position(|f| *f == ControlFrame::LoopContinue)
        .unwrap() as u32;
    func.instruction(&Instruction::Br(loop_depth));

    control_stack.pop();
    func.instruction(&Instruction::End);
    control_stack.pop();
    func.instruction(&Instruction::End);

    Ok(())
}

/// Compiles an `each item in iterable` statement.
#[allow(clippy::too_many_arguments)]
fn compile_each_stmt(
    bindings: &[String],
    iterable: &HirExpr,
    loop_body: &[HirStmt],
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    *repeat_id += 1;
    let id = *repeat_id;
    let seq_slot = locals[&format!("__each_seq_{id}")].0;
    let cur_slot = locals[&format!("__each_cursor_{id}")].0;
    let len_slot = locals[&format!("__each_len_{id}")].0;

    let is_range = infer_expr_is_range(iterable, locals, functions, structs, table_indices);
    // A `Dict` iterates its entries in insertion order, not its storage slots: the
    // element count comes from the dict header and each entry yields its key and,
    // with two bindings, its value.
    let is_dict = matches!(
        infer_expr_kind(iterable, locals, functions, table_indices, anon_map),
        LocalKind::Dict(_)
    );

    let iter_ty = compile_expr(
        iterable,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        struct_depth,
        call_depth,
    )?;
    coerce_type(func, iter_ty, WasmType::I32);
    func.instruction(&Instruction::LocalSet(seq_slot));

    func.instruction(&Instruction::LocalGet(seq_slot));
    if is_range {
        let (range_len_idx, _, _) = functions["__aipo_range_len"];
        func.instruction(&Instruction::Call(range_len_idx));
    } else if is_dict {
        let (dict_len_idx, _, _) = functions["__aipo_dict_len"];
        func.instruction(&Instruction::Call(dict_len_idx));
    } else {
        let (list_len_idx, _, _) = functions["__aipo_list_len"];
        func.instruction(&Instruction::Call(list_len_idx));
    }
    func.instruction(&Instruction::LocalSet(len_slot));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(cur_slot));

    control_stack.push(ControlFrame::LoopBreak);
    func.instruction(&Instruction::Block(BlockType::Empty));

    control_stack.push(ControlFrame::LoopContinue);
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(cur_slot));
    func.instruction(&Instruction::LocalGet(len_slot));
    func.instruction(&Instruction::I64LtS);
    func.instruction(&Instruction::I32Eqz);
    let break_depth = control_stack
        .iter()
        .rev()
        .position(|f| *f == ControlFrame::LoopBreak)
        .unwrap() as u32;
    func.instruction(&Instruction::BrIf(break_depth));

    // Canon binds one name to the natural element. With two names the first is the
    // key (a `Dict` key, or the positional index for a List/Range) and the second
    // is the value.
    if bindings.len() == 1 {
        let slot = locals[&bindings[0]].0;
        if is_dict {
            // One binding takes the key, matching canon's primary iteration.
            let (dict_key_idx, _, _) = functions["__aipo_dict_key_at"];
            func.instruction(&Instruction::LocalGet(seq_slot));
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(dict_key_idx));
            // The key is a raw `i64` from the entry, but its slot is `i32` for a
            // String key so the printer treats it as text.
            func.instruction(&Instruction::I32WrapI64);
        } else if is_range {
            func.instruction(&Instruction::LocalGet(seq_slot));
            func.instruction(&Instruction::I64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }));
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I64Add);
        } else {
            let (list_get_idx, _, _) = functions["__aipo_list_get"];
            func.instruction(&Instruction::LocalGet(seq_slot));
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(list_get_idx));
        }
        func.instruction(&Instruction::LocalSet(slot));
    } else if bindings.len() >= 2 {
        let key_slot = locals[&bindings[0]].0;
        let value_slot = locals[&bindings[1]].0;
        if is_dict {
            let (dict_key_idx, _, _) = functions["__aipo_dict_key_at"];
            let (dict_value_idx, _, _) = functions["__aipo_dict_value_at"];
            func.instruction(&Instruction::LocalGet(seq_slot));
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(dict_key_idx));
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::LocalSet(key_slot));
            func.instruction(&Instruction::LocalGet(seq_slot));
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(dict_value_idx));
            func.instruction(&Instruction::LocalSet(value_slot));
            // The second binding holds the value kind the dict carries.
            let _ = &value_slot;
            control_stack.push(ControlFrame::RepeatStep);
            func.instruction(&Instruction::Block(BlockType::Empty));
            compile_stmts(
                loop_body,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                return_type,
                false,
                repeat_id,
                struct_depth,
                call_depth,
            )?;
            control_stack.pop();
            func.instruction(&Instruction::End);
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I64Const(1));
            func.instruction(&Instruction::I64Add);
            func.instruction(&Instruction::LocalSet(cur_slot));
            let loop_depth = control_stack
                .iter()
                .rev()
                .position(|f| *f == ControlFrame::LoopContinue)
                .unwrap() as u32;
            func.instruction(&Instruction::Br(loop_depth));
            control_stack.pop();
            func.instruction(&Instruction::End);
            control_stack.pop();
            func.instruction(&Instruction::End);
            return Ok(());
        }
        func.instruction(&Instruction::LocalGet(cur_slot));
        func.instruction(&Instruction::LocalSet(key_slot));

        if is_range {
            func.instruction(&Instruction::LocalGet(seq_slot));
            func.instruction(&Instruction::I64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }));
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I64Add);
        } else {
            let (list_get_idx, _, _) = functions["__aipo_list_get"];
            func.instruction(&Instruction::LocalGet(seq_slot));
            func.instruction(&Instruction::LocalGet(cur_slot));
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(list_get_idx));
        }
        func.instruction(&Instruction::LocalSet(value_slot));
    }

    control_stack.push(ControlFrame::RepeatStep);
    func.instruction(&Instruction::Block(BlockType::Empty));
    compile_stmts(
        loop_body,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        return_type,
        false,
        repeat_id,
        struct_depth,
        call_depth,
    )?;
    control_stack.pop();
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(cur_slot));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(cur_slot));

    let loop_depth = control_stack
        .iter()
        .rev()
        .position(|f| *f == ControlFrame::LoopContinue)
        .unwrap() as u32;
    func.instruction(&Instruction::Br(loop_depth));

    control_stack.pop();
    func.instruction(&Instruction::End);
    control_stack.pop();
    func.instruction(&Instruction::End);

    Ok(())
}

/// Compiles a `match target when ... else ...` statement.
///
/// Each `when` arm lists one or more literal patterns compared by equality
/// against the target value; the first matching arm runs. Comparison uses the
/// type of the target as the reference, and literals are coerced to it.
#[allow(clippy::too_many_arguments)]
fn compile_match_stmt(
    match_stmt: &HirMatchStmt,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    *repeat_id += 1;
    let id = *repeat_id;
    let target_slot = locals[&format!("__match_target_{id}")].0;

    let target_ty = compile_expr(
        &match_stmt.target,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        struct_depth,
        call_depth,
    )?;

    // The target lives in an i64 scratch slot so it can be re-read once per arm.
    // A float keeps its IEEE-754 bits (`reinterpret`), not a truncation: an arm
    // such as `when 1.5` must not collapse onto `1`.
    if target_ty == WasmType::F64 {
        func.instruction(&Instruction::I64ReinterpretF64);
    } else {
        coerce_type(func, target_ty, WasmType::I64);
    }
    func.instruction(&Instruction::LocalSet(target_slot));

    // One block wraps the whole match so `br` skips the remaining arms.
    control_stack.push(ControlFrame::Block);
    func.instruction(&Instruction::Block(BlockType::Empty));

    for arm in &match_stmt.when_arms {
        // Each arm may list several patterns. Build a nested
        // `if a then 1 else (if b then 1 else ... 0)` chain so exactly one boolean
        // is left on the stack, then use it to guard the arm body.
        if arm.patterns.is_empty() {
            func.instruction(&Instruction::I32Const(0));
        } else {
            for pattern in &arm.patterns {
                match pattern {
                    aipo_hir::HirMatchPattern::Value(pattern) => {
                        compile_match_condition(
                            pattern,
                            target_slot,
                            target_ty,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                    }
                    aipo_hir::HirMatchPattern::Destructure(_) => {
                        func.instruction(&Instruction::LocalGet(target_slot));
                        func.instruction(&Instruction::I64Const(0));
                        func.instruction(&Instruction::I64Ne);
                    }
                    aipo_hir::HirMatchPattern::Variant {
                        enum_name,
                        variant_name,
                        ..
                    } => {
                        let full_name = match enum_name {
                            Some(e) => format!("{e}.{variant_name}"),
                            None => variant_name.clone(),
                        };
                        let expected_ptr = static_strings.get(&full_name).copied().unwrap_or(0);
                        func.instruction(&Instruction::LocalGet(target_slot));
                        func.instruction(&Instruction::I64Const(0));
                        func.instruction(&Instruction::I64Ne);

                        func.instruction(&Instruction::LocalGet(target_slot));
                        func.instruction(&Instruction::I32WrapI64);
                        func.instruction(&Instruction::I32Load(MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                        func.instruction(&Instruction::I32Const(expected_ptr));
                        func.instruction(&Instruction::I32Eq);
                        func.instruction(&Instruction::I32And);
                    }
                }
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::Else);
            }
            // All patterns failed.
            func.instruction(&Instruction::I32Const(0));
            // Close one `if` per pattern, innermost first.
            for _ in &arm.patterns {
                func.instruction(&Instruction::End);
            }
        }

        func.instruction(&Instruction::If(BlockType::Empty));
        for pattern in &arm.patterns {
            match pattern {
                aipo_hir::HirMatchPattern::Destructure(fields) => {
                    for f_name in fields {
                        let f_slot = locals[f_name].0;
                        let (f_offset, f_ty) = structs
                            .values()
                            .find_map(|s| s.fields.get(f_name).copied())
                            .unwrap_or((0, WasmType::I64));
                        func.instruction(&Instruction::LocalGet(target_slot));
                        func.instruction(&Instruction::I32WrapI64);
                        match f_ty {
                            WasmType::I32 => {
                                func.instruction(&Instruction::I32Load(MemArg {
                                    offset: f_offset as u64,
                                    align: 2,
                                    memory_index: 0,
                                }));
                                func.instruction(&Instruction::I64ExtendI32U);
                            }
                            WasmType::F64 => {
                                func.instruction(&Instruction::F64Load(MemArg {
                                    offset: f_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            _ => {
                                func.instruction(&Instruction::I64Load(MemArg {
                                    offset: f_offset as u64,
                                    align: 3,
                                    memory_index: 0,
                                }));
                            }
                        }
                        func.instruction(&Instruction::LocalSet(f_slot));
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
                    match payload {
                        aipo_hir::HirVariantPatternPayload::Unit => {}
                        aipo_hir::HirVariantPatternPayload::Tuple(ids) => {
                            for (idx, id) in ids.iter().enumerate() {
                                let f_slot = locals[id].0;
                                let f_offset = (8 + idx * 8) as u64;
                                func.instruction(&Instruction::LocalGet(target_slot));
                                func.instruction(&Instruction::I32WrapI64);
                                func.instruction(&Instruction::I64Load(MemArg {
                                    offset: f_offset,
                                    align: 3,
                                    memory_index: 0,
                                }));
                                func.instruction(&Instruction::LocalSet(f_slot));
                            }
                        }
                        aipo_hir::HirVariantPatternPayload::Struct(ids) => {
                            for (idx, id) in ids.iter().enumerate() {
                                let f_slot = locals[id].0;
                                let f_offset = if let Some(layout) = structs.get(&full_name) {
                                    layout
                                        .fields
                                        .get(id)
                                        .map(|(off, _)| *off as u64)
                                        .unwrap_or((8 + idx * 8) as u64)
                                } else {
                                    (8 + idx * 8) as u64
                                };
                                func.instruction(&Instruction::LocalGet(target_slot));
                                func.instruction(&Instruction::I32WrapI64);
                                func.instruction(&Instruction::I64Load(MemArg {
                                    offset: f_offset,
                                    align: 3,
                                    memory_index: 0,
                                }));
                                func.instruction(&Instruction::LocalSet(f_slot));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(guard) = &arm.guard {
            let guard_ty = compile_expr(
                guard,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_to_bool(func, guard_ty);
            func.instruction(&Instruction::If(BlockType::Empty));
            let term = compile_stmts(
                &arm.body,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                return_type,
                false,
                repeat_id,
                struct_depth,
                call_depth,
            )?;
            if !term {
                func.instruction(&Instruction::Br(2));
            }
            func.instruction(&Instruction::End);
        } else {
            let term = compile_stmts(
                &arm.body,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                return_type,
                false,
                repeat_id,
                struct_depth,
                call_depth,
            )?;
            if !term {
                func.instruction(&Instruction::Br(1));
            }
        }
        func.instruction(&Instruction::End);
    }

    if let Some(else_body) = &match_stmt.else_arm {
        compile_stmts(
            else_body,
            func,
            locals,
            functions,
            control_stack,
            structs,
            static_strings,
            table_indices,
            anon_map,
            indirect_sigs,
            alloc_func_idx,
            async_helpers,
            return_type,
            false,
            repeat_id,
            struct_depth,
            call_depth,
        )?;
    }

    control_stack.pop();
    func.instruction(&Instruction::End);
    Ok(())
}

/// Compiles a single `when` pattern, leaving a boolean `i32` on the stack.
///
/// String patterns compare pointers, so they use a raw pointer equality check
/// against the static string pool entry the compiler already allocated.
#[allow(clippy::too_many_arguments)]
fn compile_match_condition(
    pattern: &HirExpr,
    target_slot: u32,
    _target_ty: WasmType,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    if let HirExpr::Literal(Literal::String(text, _), _) = pattern {
        let ptr = static_strings.get(text.as_str()).copied().ok_or(
            WasmCompileError::UnsupportedExpr {
                message: "string pattern is missing from the static pool".into(),
                span: pattern.span(),
            },
        )?;
        // The target is staged in an `i64` scratch slot, so the pointer has to be
        // narrowed before the comparison. Comparing an `i64` against an `i32`
        // constant produces a module the Wasm validator rejects outright.
        let (string_eq_idx, _, _) = functions["__aipo_str_eq"];
        func.instruction(&Instruction::LocalGet(target_slot));
        func.instruction(&Instruction::I32WrapI64);
        func.instruction(&Instruction::I32Const(ptr));
        // Pattern matching compares *values*, not addresses: two equal strings
        // built at runtime live at different offsets in linear memory.
        func.instruction(&Instruction::Call(string_eq_idx));
    } else {
        let pat_ty = compile_expr(
            pattern,
            func,
            locals,
            functions,
            control_stack,
            structs,
            static_strings,
            table_indices,
            anon_map,
            indirect_sigs,
            alloc_func_idx,
            async_helpers,
            struct_depth,
            call_depth,
        )?;
        // Both sides are staged as `i64` bits, so a float pattern compares its
        // reinterpeted bits instead of truncating to an integer.
        if pat_ty == WasmType::F64 {
            func.instruction(&Instruction::I64ReinterpretF64);
        } else {
            coerce_type(func, pat_ty, WasmType::I64);
        }
        func.instruction(&Instruction::LocalGet(target_slot));
        func.instruction(&Instruction::I64Eq);
    }
    Ok(())
}

/// Compiles `attempt { body } failed [err] { handler }`.
///
/// The body runs inside a block marked as the target for failure propagation, so
/// a statement-boundary failure check inside the body branches out instead of
/// returning from the function. Whether the body fell through or unwound is
/// then decided by the failure flag, which the handler clears before running.
#[allow(clippy::too_many_arguments)]
fn compile_attempt_stmt(
    attempt: &HirAttemptStmt,
    is_top_level: bool,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    return_type: Option<WasmType>,
    repeat_id: &mut u32,
    struct_depth: usize,
    call_depth: usize,
) -> Result<(), WasmCompileError> {
    let fg = async_helpers.fail_globals;

    control_stack.push(ControlFrame::AttemptHandler);
    func.instruction(&Instruction::Block(BlockType::Empty));

    compile_stmts(
        &attempt.body,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        // The guard block carries no result, so its body must not treat a trailing
        // expression as the function's return value. The boundary flag is inherited:
        // a field write inside an entry `attempt` is still a statement boundary.
        None,
        is_top_level,
        repeat_id,
        struct_depth,
        call_depth,
    )?;

    control_stack.pop();
    func.instruction(&Instruction::End);

    // A non-zero status means the body unwound here with a pending failure.
    func.instruction(&Instruction::GlobalGet(fg.status_idx));
    func.instruction(&Instruction::If(BlockType::Empty));

    // Clear the failure flag before the handler body runs, so the handler is
    // free to raise its own failure.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::GlobalSet(fg.status_idx));
    // Recovery commits the writes made before the attempt, so the journal stops
    // here rather than restoring them at the enclosing boundary.
    for slot in [async_helpers.journal, async_helpers.journal2] {
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::GlobalSet(slot.live));
    }

    // Bind the error variable, if the source named one.
    if let Some(err_name) = &attempt.error_binding
        && let Some(&(slot, _, _)) = locals.get(err_name)
    {
        func.instruction(&Instruction::GlobalGet(fg.message_idx));
        func.instruction(&Instruction::LocalSet(slot));
    }

    compile_stmts(
        &attempt.handler,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        return_type,
        false,
        repeat_id,
        struct_depth,
        call_depth,
    )?;

    func.instruction(&Instruction::End);
    Ok(())
}

/// Compresses a slice of local types into run-length encoded `(count, ValType)` pairs.
fn compress_locals(types: &[WasmType]) -> Vec<(u32, ValType)> {
    let mut compressed = Vec::new();
    for &ty in types {
        let val_ty: ValType = ty.into();
        if let Some((count, last_ty)) = compressed.last_mut()
            && *last_ty == val_ty
        {
            *count += 1;
            continue;
        }
        compressed.push((1, val_ty));
    }
    compressed
}

fn is_string_or_format_expr(
    expr: &HirExpr,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
) -> bool {
    match expr {
        HirExpr::Literal(Literal::String(..), _) => true,
        HirExpr::Call(callee, args, _) => {
            if let HirExpr::Identifier(name, _) = &**callee
                && name == "String"
                && args.len() == 1
            {
                return true;
            }
            false
        }
        HirExpr::Binary(BinaryOp::Add, left, right, _) => {
            is_string_or_format_expr(left, locals) || is_string_or_format_expr(right, locals)
        }
        HirExpr::Identifier(name, _) => {
            if let Some((_, _, kind)) = locals.get(name) {
                matches!(kind, LocalKind::String)
            } else {
                false
            }
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_print_arg(
    expr: &HirExpr,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    struct_depth: usize,
    call_depth: usize,
    io: HostIoHelpers,
) -> Result<(), WasmCompileError> {
    match expr {
        HirExpr::Binary(BinaryOp::Add, left, right, _)
            if is_string_or_format_expr(left, locals)
                || is_string_or_format_expr(right, locals) =>
        {
            compile_print_arg(
                left,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
                io,
            )?;
            compile_print_arg(
                right,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
                io,
            )
        }
        HirExpr::Call(callee, args, _)
            if matches!(&**callee, HirExpr::Identifier(name, _) if name == "String")
                && args.len() == 1 =>
        {
            compile_print_arg(
                &args[0].value,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
                io,
            )
        }
        _ => {
            let actual_ty = compile_expr(
                expr,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            let kind = infer_expr_kind(expr, locals, functions, table_indices, anon_map);
            // A call inside the argument may have raised a failure. Check it
            // before the value is rendered, so a statement such as
            // `io.println(f(may_fail()))` unwinds instead of printing a zero.
            if expr_may_fail(expr) {
                let staged = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
                match actual_ty {
                    WasmType::F64 => {
                        func.instruction(&Instruction::I64ReinterpretF64);
                        func.instruction(&Instruction::LocalSet(staged));
                        emit_failure_check(
                            func,
                            control_stack,
                            async_helpers.fail_globals,
                            async_helpers.function_return,
                        );
                        func.instruction(&Instruction::LocalGet(staged));
                        func.instruction(&Instruction::F64ReinterpretI64);
                    }
                    WasmType::I32 => {
                        func.instruction(&Instruction::I64ExtendI32U);
                        func.instruction(&Instruction::LocalSet(staged));
                        emit_failure_check(
                            func,
                            control_stack,
                            async_helpers.fail_globals,
                            async_helpers.function_return,
                        );
                        func.instruction(&Instruction::LocalGet(staged));
                        func.instruction(&Instruction::I32WrapI64);
                    }
                    _ => {
                        func.instruction(&Instruction::LocalSet(staged));
                        emit_failure_check(
                            func,
                            control_stack,
                            async_helpers.fail_globals,
                            async_helpers.function_return,
                        );
                        func.instruction(&Instruction::LocalGet(staged));
                    }
                }
            }
            match kind {
                LocalKind::None => {
                    // `none` renders as the literal word, not as the sentinel the
                    // backend uses internally.
                    let none_ptr = static_strings.get("none").copied().unwrap_or_default();
                    func.instruction(&Instruction::Drop);
                    func.instruction(&Instruction::I32Const(none_ptr));
                    func.instruction(&Instruction::Call(io.print_str_idx));
                }
                LocalKind::List(element) => {
                    compile_print_list(
                        func,
                        actual_ty,
                        &element,
                        locals,
                        static_strings,
                        functions,
                        async_helpers,
                        call_depth,
                        io,
                    )?;
                }
                LocalKind::Dict(value) => {
                    compile_print_dict(
                        func,
                        actual_ty,
                        &value,
                        locals,
                        static_strings,
                        functions,
                        async_helpers,
                        call_depth,
                        io,
                    )?;
                }
                LocalKind::String => {
                    coerce_type(func, actual_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(io.print_str_idx));
                }
                LocalKind::Bool => {
                    coerce_type(func, actual_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(io.print_bool_idx));
                }
                _ => match actual_ty {
                    WasmType::F64 => {
                        func.instruction(&Instruction::Call(io.print_float_idx));
                    }
                    WasmType::I32 => {
                        func.instruction(&Instruction::Call(io.print_str_idx));
                    }
                    _ => {
                        func.instruction(&Instruction::Call(io.print_int_idx));
                    }
                },
            }
            Ok(())
        }
    }
}

/// Emits the rendering of a `List` value on top of the stack.
///
/// Canon renders a list as `[a, b, c]`. The handle is already on the stack; this
/// walks it, printing each element through the same kind-directed dispatch the
/// scalar path uses, separated by `, `.
#[allow(clippy::too_many_arguments)]
fn compile_print_list(
    func: &mut Function,
    value_ty: WasmType,
    element: &LocalKind,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    static_strings: &HashMap<String, i32>,
    functions: &FunctionTable,
    async_helpers: AsyncHelpers,
    call_depth: usize,
    io: HostIoHelpers,
) -> Result<(), WasmCompileError> {
    let (list_len_idx, _, _) = functions["__aipo_list_len"];
    let (list_get_idx, _, _) = functions["__aipo_list_get"];

    // The handle is already on the stack; stage it and iterate.
    let handle = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
    let cursor = locals[&format!("__i64_temp_{}", (call_depth + 1).min(7))].0;
    let len_slot = locals[&format!("__i64_temp_{}", (call_depth + 2).min(7))].0;

    // Architecture: the handle travels as a pointer, but the staging locals are
    // shared with `_i64` scratch, so widen before storing.
    coerce_type(func, value_ty, WasmType::I32);
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::LocalSet(handle));

    func.instruction(&Instruction::LocalGet(handle));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::LocalSet(len_slot));

    let open = static_strings.get("[").copied().unwrap_or_default();
    let close = static_strings.get("]").copied().unwrap_or_default();
    let sep = static_strings.get(", ").copied().unwrap_or_default();

    func.instruction(&Instruction::I32Const(open));
    func.instruction(&Instruction::Call(io.print_str_idx));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(cursor));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::LocalGet(len_slot));
    func.instruction(&Instruction::I64GeS);
    func.instruction(&Instruction::BrIf(1));

    // A separator precedes every element but the first.
    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(sep));
    func.instruction(&Instruction::Call(io.print_str_idx));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(handle));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::Call(list_get_idx));

    // Elements are untyped in the handle, but the list's element kind says how to
    // render them: a list of strings must print text, not the pointer it stores.
    match element {
        LocalKind::String => {
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(io.print_str_idx));
        }
        LocalKind::Bool => {
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(io.print_bool_idx));
        }
        LocalKind::Float => {
            func.instruction(&Instruction::F64ReinterpretI64);
            func.instruction(&Instruction::Call(io.print_float_idx));
        }
        _ => {
            func.instruction(&Instruction::Call(io.print_int_idx));
        }
    }

    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(cursor));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(close));
    func.instruction(&Instruction::Call(io.print_str_idx));
    let _ = async_helpers;
    Ok(())
}

/// Emits the rendering of a `Dict` value on top of the stack.
///
/// Canon renders a dict as `{k1: v1, k2: v2}` in insertion order.
#[allow(clippy::too_many_arguments)]
fn compile_print_dict(
    func: &mut Function,
    value_ty: WasmType,
    value_kind: &LocalKind,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    static_strings: &HashMap<String, i32>,
    functions: &FunctionTable,
    async_helpers: AsyncHelpers,
    call_depth: usize,
    io: HostIoHelpers,
) -> Result<(), WasmCompileError> {
    let (dict_len_idx, _, _) = functions["__aipo_dict_len"];
    let (dict_key_idx, _, _) = functions["__aipo_dict_key_at"];
    let (dict_value_idx, _, _) = functions["__aipo_dict_value_at"];

    let handle = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
    let len_slot = locals[&format!("__i64_temp_{}", (call_depth + 1).min(7))].0;
    let cursor = locals[&format!("__i64_temp_{}", (call_depth + 2).min(7))].0;

    // The handle travels as a pointer, but the staging locals are shared with the
    // `i64` scratch pool, so widen before storing.
    coerce_type(func, value_ty, WasmType::I32);
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::LocalSet(handle));
    func.instruction(&Instruction::LocalGet(handle));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::Call(dict_len_idx));
    func.instruction(&Instruction::LocalSet(len_slot));

    let open = static_strings.get("{").copied().unwrap_or_default();
    let close = static_strings.get("}").copied().unwrap_or_default();
    let sep = static_strings.get(", ").copied().unwrap_or_default();
    let colon = static_strings.get(": ").copied().unwrap_or_default();

    func.instruction(&Instruction::I32Const(open));
    func.instruction(&Instruction::Call(io.print_str_idx));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(cursor));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::LocalGet(len_slot));
    func.instruction(&Instruction::I64GeS);
    func.instruction(&Instruction::BrIf(1));

    // A separator precedes every pair but the first.
    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(sep));
    func.instruction(&Instruction::Call(io.print_str_idx));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(handle));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::Call(dict_key_idx));
    // Keys are stored as their raw value; a String key is a pool pointer.
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::Call(io.print_str_idx));

    func.instruction(&Instruction::I32Const(colon));
    func.instruction(&Instruction::Call(io.print_str_idx));

    func.instruction(&Instruction::LocalGet(handle));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::Call(dict_value_idx));
    match value_kind {
        LocalKind::String => {
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(io.print_str_idx));
        }
        LocalKind::Bool => {
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::Call(io.print_bool_idx));
        }
        LocalKind::Float => {
            func.instruction(&Instruction::F64ReinterpretI64);
            func.instruction(&Instruction::Call(io.print_float_idx));
        }
        _ => {
            func.instruction(&Instruction::Call(io.print_int_idx));
        }
    }

    func.instruction(&Instruction::LocalGet(cursor));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(cursor));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(close));
    func.instruction(&Instruction::Call(io.print_str_idx));
    let _ = async_helpers;
    Ok(())
}

/// Compiles an expression and emits its WebAssembly instructions, returning its result type.
#[allow(clippy::too_many_arguments)]
fn compile_expr(
    expr: &HirExpr,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    struct_depth: usize,
    call_depth: usize,
) -> Result<WasmType, WasmCompileError> {
    match expr {
        HirExpr::Literal(lit, span) => match lit {
            Literal::Int(raw) => {
                let val =
                    parse_int_literal(raw).ok_or_else(|| WasmCompileError::InvalidLiteral {
                        message: format!("invalid integer literal `{raw}`"),
                        span: *span,
                    })?;
                func.instruction(&Instruction::I64Const(val));
                Ok(WasmType::I64)
            }
            Literal::Float(raw) => {
                let val =
                    parse_float_literal(raw).ok_or_else(|| WasmCompileError::InvalidLiteral {
                        message: format!("invalid float literal `{raw}`"),
                        span: *span,
                    })?;
                func.instruction(&Instruction::F64Const(val.into()));
                Ok(WasmType::F64)
            }
            Literal::Bool(b) => {
                func.instruction(&Instruction::I32Const(if *b { 1 } else { 0 }));
                Ok(WasmType::I32)
            }
            Literal::String(text, _) => {
                if let Some(&offset) = static_strings.get(text) {
                    func.instruction(&Instruction::I32Const(offset));
                    Ok(WasmType::I32)
                } else {
                    Err(WasmCompileError::InvalidLiteral {
                        message: format!("unregistered static string `{text}`"),
                        span: *span,
                    })
                }
            }
            Literal::None => {
                func.instruction(&Instruction::I64Const(0));
                Ok(WasmType::I64)
            }
        },
        HirExpr::Identifier(name, span) => {
            if let Some(&(idx, ty, _)) = locals.get(name) {
                func.instruction(&Instruction::LocalGet(idx));
                Ok(ty)
            } else if let Some(&table_idx) = table_indices.get(name) {
                func.instruction(&Instruction::I32Const(table_idx as i32));
                Ok(WasmType::I32)
            } else if functions.contains_key(name) {
                func.instruction(&Instruction::I32Const(0));
                Ok(WasmType::I32)
            } else if let Some(&(global_idx, ty, _)) = functions.module_globals.get(name) {
                func.instruction(&Instruction::GlobalGet(global_idx));
                Ok(ty)
            } else if functions.declared_vars.contains(name) {
                // A captured binding in a nested scope is not addressable here; the
                // module-scope case is handled above through its global.
                func.instruction(&Instruction::I64Const(0));
                Ok(WasmType::I64)
            } else {
                Err(WasmCompileError::UnknownVariable {
                    name: name.clone(),
                    span: *span,
                })
            }
        }
        HirExpr::Fn(func_expr) => {
            if let Some(anon_name) = anon_map.get(&func_expr.span) {
                if let Some(&table_idx) = table_indices.get(anon_name) {
                    func.instruction(&Instruction::I32Const(table_idx as i32));
                    Ok(WasmType::I32)
                } else {
                    Err(WasmCompileError::UnsupportedExpr {
                        message: format!("anonymous function `{anon_name}` missing from table"),
                        span: func_expr.span,
                    })
                }
            } else {
                Err(WasmCompileError::UnsupportedExpr {
                    message: "unregistered anonymous function".into(),
                    span: func_expr.span,
                })
            }
        }
        HirExpr::Unary(op, inner, span) => {
            let inner_ty = compile_expr(
                inner,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            match op {
                UnaryOp::Neg => match inner_ty {
                    WasmType::I64 => {
                        func.instruction(&Instruction::I64Const(-1));
                        func.instruction(&Instruction::I64Mul);
                        Ok(WasmType::I64)
                    }
                    WasmType::F64 => {
                        func.instruction(&Instruction::F64Neg);
                        Ok(WasmType::F64)
                    }
                    WasmType::I32 => {
                        func.instruction(&Instruction::I32Const(-1));
                        func.instruction(&Instruction::I32Mul);
                        Ok(WasmType::I32)
                    }
                },
                UnaryOp::Pos => Ok(inner_ty),
                UnaryOp::Not => match inner_ty {
                    WasmType::I32 => {
                        func.instruction(&Instruction::I32Eqz);
                        Ok(WasmType::I32)
                    }
                    WasmType::I64 => {
                        func.instruction(&Instruction::I64Eqz);
                        Ok(WasmType::I32)
                    }
                    WasmType::F64 => Err(WasmCompileError::TypeMismatch {
                        expected: "Int or Bool".into(),
                        found: "Float".into(),
                        span: *span,
                    }),
                },
            }
        }
        HirExpr::Binary(op, left, right, span) => {
            if matches!(op, BinaryOp::Is | BinaryOp::IsNullable) {
                let kind = infer_expr_kind(left, locals, functions, table_indices, anon_map);
                let HirExpr::Identifier(type_name, _) = &**right else {
                    return Err(WasmCompileError::UnsupportedExpr {
                        message:
                            "Wasm type tests require an unshadowed static type; use --engine=vm"
                                .into(),
                        span: *span,
                    });
                };
                let fundamental = matches!(
                    type_name.as_str(),
                    "Int"
                        | "Float"
                        | "Bool"
                        | "Byte"
                        | "String"
                        | "List"
                        | "Dict"
                        | "Bytes"
                        | "Set"
                        | "Duration"
                );
                let nominal = structs.contains_key(type_name)
                    || structs.keys().any(|name| {
                        name.split_once('.')
                            .is_some_and(|(parent, _)| parent == type_name)
                    });
                if locals.contains_key(type_name)
                    || functions.contains_key(type_name)
                    || functions.global_bindings.contains(type_name)
                    || (fundamental && functions.declared_types.contains(type_name))
                    || !(fundamental || nominal)
                {
                    return Err(WasmCompileError::UnsupportedExpr {
                        message: "Wasm cannot resolve a dynamic, aliased or shadowed type target; use --engine=vm".into(),
                        span: *span,
                    });
                }
                if kind == LocalKind::Unknown {
                    return Err(WasmCompileError::UnsupportedExpr {
                        message:
                            "Wasm cannot prove the semantic type of this value; use --engine=vm"
                                .into(),
                        span: *span,
                    });
                }
                // `narrowed` is the static verdict; `runtime_none_check` asks the
                // emitted code to also rule out the `none` sentinel, which only a
                // nullable contract carries.
                let (narrowed, runtime_none_check) = match (&kind, type_name.as_str()) {
                    (LocalKind::Int, "Int")
                    | (LocalKind::Float, "Float")
                    | (LocalKind::Bool, "Bool")
                    | (LocalKind::String, "String")
                    | (LocalKind::List(_), "List")
                    | (LocalKind::Dict(_), "Dict")
                    | (LocalKind::Bytes, "Bytes") => (true, false),
                    (LocalKind::Struct(name), expected) => (
                        name == expected
                            || name
                                .split_once('.')
                                .is_some_and(|(parent, _)| parent == expected),
                        false,
                    ),
                    (LocalKind::None, _) => (*op == BinaryOp::IsNullable, false),
                    // A `T?` value is a `T` or `none`, so `is T` keeps the static
                    // answer and defers the sentinel to runtime.
                    (LocalKind::Nullable(inner), expected) => {
                        let inner_matches = match (&**inner, expected) {
                            (LocalKind::Int, "Int")
                            | (LocalKind::Float, "Float")
                            | (LocalKind::Bool, "Bool")
                            | (LocalKind::String, "String")
                            | (LocalKind::List(_), "List")
                            | (LocalKind::Dict(_), "Dict")
                            | (LocalKind::Bytes, "Bytes") => true,
                            (LocalKind::Struct(name), expected) => {
                                name == expected
                                    || name
                                        .split_once('.')
                                        .is_some_and(|(parent, _)| parent == expected)
                            }
                            _ => false,
                        };
                        (inner_matches, true)
                    }
                    _ => (false, false),
                };
                // Evaluate the left side once, preserving its effects, even for a constant proof.
                let left_ty = compile_expr(
                    left,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                if runtime_none_check && narrowed {
                    // `none` is the zero `i64` sentinel, so the test is "not none".
                    coerce_type(func, left_ty, WasmType::I64);
                    func.instruction(&Instruction::I64Eqz);
                    func.instruction(&Instruction::I32Eqz);
                    return Ok(WasmType::I32);
                }
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::I32Const(i32::from(narrowed)));
                return Ok(WasmType::I32);
            }

            let left_ty = infer_expr_type(left, locals, functions, structs, table_indices)?;
            let right_ty = infer_expr_type(right, locals, functions, structs, table_indices)?;

            match op {
                BinaryOp::Div => {
                    let l_ty = compile_expr(
                        left,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, l_ty, WasmType::F64);
                    let r_ty = compile_expr(
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::F64);
                    func.instruction(&Instruction::F64Div);
                    Ok(WasmType::F64)
                }
                BinaryOp::IntDiv => {
                    let l_ty = compile_expr(
                        left,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, l_ty, WasmType::I64);
                    let r_ty = compile_expr(
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I64);
                    func.instruction(&Instruction::I64DivS);
                    Ok(WasmType::I64)
                }
                BinaryOp::Mod => {
                    let l_ty = compile_expr(
                        left,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, l_ty, WasmType::I64);
                    let r_ty = compile_expr(
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I64);
                    func.instruction(&Instruction::I64RemS);
                    Ok(WasmType::I64)
                }
                BinaryOp::Add => {
                    let l_kind = infer_expr_kind(left, locals, functions, table_indices, anon_map);
                    let r_kind = infer_expr_kind(right, locals, functions, table_indices, anon_map);
                    if matches!(l_kind, LocalKind::String) || matches!(r_kind, LocalKind::String) {
                        let (str_concat_idx, _, _) = functions["__aipo_str_concat"];
                        let (int_to_str_idx, _, _) = functions["__aipo_int_to_str"];
                        let (bool_to_str_idx, _, _) = functions["__aipo_bool_to_str"];

                        let l_ty = compile_expr(
                            left,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        match l_kind {
                            LocalKind::String => {
                                coerce_type(func, l_ty, WasmType::I32);
                            }
                            LocalKind::Int => {
                                coerce_type(func, l_ty, WasmType::I64);
                                func.instruction(&Instruction::Call(int_to_str_idx));
                            }
                            LocalKind::Bool => {
                                coerce_type(func, l_ty, WasmType::I32);
                                func.instruction(&Instruction::Call(bool_to_str_idx));
                            }
                            _ => {
                                if l_ty == WasmType::I64 {
                                    func.instruction(&Instruction::Call(int_to_str_idx));
                                } else {
                                    coerce_type(func, l_ty, WasmType::I32);
                                }
                            }
                        }

                        let r_ty = compile_expr(
                            right,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        match r_kind {
                            LocalKind::String => {
                                coerce_type(func, r_ty, WasmType::I32);
                            }
                            LocalKind::Int => {
                                coerce_type(func, r_ty, WasmType::I64);
                                func.instruction(&Instruction::Call(int_to_str_idx));
                            }
                            LocalKind::Bool => {
                                coerce_type(func, r_ty, WasmType::I32);
                                func.instruction(&Instruction::Call(bool_to_str_idx));
                            }
                            _ => {
                                if r_ty == WasmType::I64 {
                                    func.instruction(&Instruction::Call(int_to_str_idx));
                                } else {
                                    coerce_type(func, r_ty, WasmType::I32);
                                }
                            }
                        }

                        func.instruction(&Instruction::Call(str_concat_idx));
                        return Ok(WasmType::I32);
                    }

                    let target_ty = if left_ty == WasmType::F64 || right_ty == WasmType::F64 {
                        WasmType::F64
                    } else {
                        WasmType::I64
                    };
                    let l_ty = compile_expr(
                        left,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, l_ty, target_ty);
                    let r_ty = compile_expr(
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, target_ty);

                    if target_ty == WasmType::F64 {
                        func.instruction(&Instruction::F64Add);
                    } else {
                        func.instruction(&Instruction::I64Add);
                    }
                    Ok(target_ty)
                }
                BinaryOp::Sub | BinaryOp::Mul => {
                    let target_ty = if left_ty == WasmType::F64 || right_ty == WasmType::F64 {
                        WasmType::F64
                    } else {
                        WasmType::I64
                    };
                    let l_ty = compile_expr(
                        left,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, l_ty, target_ty);
                    let r_ty = compile_expr(
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, target_ty);

                    match (op, target_ty) {
                        (BinaryOp::Sub, WasmType::F64) => func.instruction(&Instruction::F64Sub),
                        (BinaryOp::Sub, _) => func.instruction(&Instruction::I64Sub),
                        (BinaryOp::Mul, WasmType::F64) => func.instruction(&Instruction::F64Mul),
                        (BinaryOp::Mul, _) => func.instruction(&Instruction::I64Mul),
                        _ => unreachable!(),
                    };
                    Ok(target_ty)
                }
                BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual => {
                    let l_kind = infer_expr_kind(left, locals, functions, table_indices, anon_map);
                    let r_kind = infer_expr_kind(right, locals, functions, table_indices, anon_map);
                    if matches!(l_kind, LocalKind::String)
                        && matches!(r_kind, LocalKind::String)
                        && matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
                    {
                        let (str_eq_idx, _, _) = functions["__aipo_str_eq"];
                        let l_ty = compile_expr(
                            left,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, l_ty, WasmType::I32);
                        let r_ty = compile_expr(
                            right,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, r_ty, WasmType::I32);
                        func.instruction(&Instruction::Call(str_eq_idx));
                        if *op == BinaryOp::NotEqual {
                            func.instruction(&Instruction::I32Eqz);
                        }
                        return Ok(WasmType::I32);
                    }

                    // Structs compare by value, not by address: `left == right`
                    // asks whether the two instances hold the same fields. The
                    // layouts are known, so the comparison is unrolled per field
                    // instead of allocating a runtime walker.
                    if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
                        && matches!(l_kind, LocalKind::Struct(_))
                        && matches!(r_kind, LocalKind::Struct(_))
                    {
                        let layout = match (&l_kind, &r_kind) {
                            (LocalKind::Struct(a), LocalKind::Struct(b)) if a == b => {
                                structs.get(a)
                            }
                            _ => None,
                        };
                        if let Some(layout) = layout {
                            let mut ordered: Vec<(u32, WasmType)> =
                                layout.fields.values().copied().collect();
                            ordered.sort_unstable_by_key(|(offset, _)| *offset);

                            let left_ptr = locals[&format!("__call_temp_{}", call_depth.min(7))].0;
                            let right_ptr =
                                locals[&format!("__call_temp_{}", (call_depth + 1).min(7))].0;

                            let l_ty = compile_expr(
                                left,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, l_ty, WasmType::I32);
                            func.instruction(&Instruction::LocalSet(left_ptr));
                            let r_ty = compile_expr(
                                right,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, r_ty, WasmType::I32);
                            func.instruction(&Instruction::LocalSet(right_ptr));

                            if ordered.is_empty() {
                                func.instruction(&Instruction::I32Const(1));
                            } else {
                                for (position, (offset, field_ty)) in ordered.iter().enumerate() {
                                    func.instruction(&Instruction::LocalGet(left_ptr));
                                    match field_ty {
                                        WasmType::F64 => {
                                            func.instruction(&Instruction::F64Load(MemArg {
                                                offset: u64::from(*offset),
                                                align: 3,
                                                memory_index: 0,
                                            }))
                                        }
                                        WasmType::I32 => {
                                            func.instruction(&Instruction::I32Load(MemArg {
                                                offset: u64::from(*offset),
                                                align: 2,
                                                memory_index: 0,
                                            }))
                                        }
                                        WasmType::I64 => {
                                            func.instruction(&Instruction::I64Load(MemArg {
                                                offset: u64::from(*offset),
                                                align: 3,
                                                memory_index: 0,
                                            }))
                                        }
                                    };
                                    func.instruction(&Instruction::LocalGet(right_ptr));
                                    match field_ty {
                                        WasmType::F64 => {
                                            func.instruction(&Instruction::F64Load(MemArg {
                                                offset: u64::from(*offset),
                                                align: 3,
                                                memory_index: 0,
                                            }))
                                        }
                                        WasmType::I32 => {
                                            func.instruction(&Instruction::I32Load(MemArg {
                                                offset: u64::from(*offset),
                                                align: 2,
                                                memory_index: 0,
                                            }))
                                        }
                                        WasmType::I64 => {
                                            func.instruction(&Instruction::I64Load(MemArg {
                                                offset: u64::from(*offset),
                                                align: 3,
                                                memory_index: 0,
                                            }))
                                        }
                                    };
                                    match field_ty {
                                        WasmType::F64 => {
                                            func.instruction(&Instruction::F64Eq);
                                        }
                                        WasmType::I32 => {
                                            func.instruction(&Instruction::I32Eq);
                                        }
                                        WasmType::I64 => {
                                            func.instruction(&Instruction::I64Eq);
                                        }
                                    }
                                    // Accumulate after each field comparison so the stack
                                    // always holds exactly one running boolean.
                                    if position > 0 {
                                        func.instruction(&Instruction::I32And);
                                    }
                                }
                            }
                            if *op == BinaryOp::NotEqual {
                                func.instruction(&Instruction::I32Eqz);
                            }
                            return Ok(WasmType::I32);
                        }
                    }

                    let compare_ty = if left_ty == WasmType::F64 || right_ty == WasmType::F64 {
                        WasmType::F64
                    } else {
                        WasmType::I64
                    };
                    let l_ty = compile_expr(
                        left,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, l_ty, compare_ty);
                    let r_ty = compile_expr(
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, compare_ty);

                    match (op, compare_ty) {
                        (BinaryOp::Equal, WasmType::F64) => func.instruction(&Instruction::F64Eq),
                        (BinaryOp::Equal, _) => func.instruction(&Instruction::I64Eq),
                        (BinaryOp::NotEqual, WasmType::F64) => {
                            func.instruction(&Instruction::F64Ne)
                        }
                        (BinaryOp::NotEqual, _) => func.instruction(&Instruction::I64Ne),
                        (BinaryOp::Less, WasmType::F64) => func.instruction(&Instruction::F64Lt),
                        (BinaryOp::Less, _) => func.instruction(&Instruction::I64LtS),
                        (BinaryOp::LessEqual, WasmType::F64) => {
                            func.instruction(&Instruction::F64Le)
                        }
                        (BinaryOp::LessEqual, _) => func.instruction(&Instruction::I64LeS),
                        (BinaryOp::Greater, WasmType::F64) => func.instruction(&Instruction::F64Gt),
                        (BinaryOp::Greater, _) => func.instruction(&Instruction::I64GtS),
                        (BinaryOp::GreaterEqual, WasmType::F64) => {
                            func.instruction(&Instruction::F64Ge)
                        }
                        (BinaryOp::GreaterEqual, _) => func.instruction(&Instruction::I64GeS),
                        _ => unreachable!(),
                    };
                    Ok(WasmType::I32)
                }
                BinaryOp::And => {
                    compile_logical_short_circuit(
                        left,
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                        false,
                    )?;
                    Ok(WasmType::I32)
                }
                BinaryOp::Or => {
                    compile_logical_short_circuit(
                        left,
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                        true,
                    )?;
                    Ok(WasmType::I32)
                }
                BinaryOp::Range => {
                    let l_ty = compile_expr(
                        left,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, l_ty, WasmType::I64);
                    let r_ty = compile_expr(
                        right,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I64);
                    let (range_fn_idx, _, _) = functions["__aipo_range_new"];
                    func.instruction(&Instruction::Call(range_fn_idx));
                    Ok(WasmType::I32)
                }
                other => Err(WasmCompileError::UnsupportedExpr {
                    message: format!("operator `{other:?}` is not yet supported in Wasm backend"),
                    span: *span,
                }),
            }
        }
        HirExpr::If(cond, then_expr, else_expr, _) => {
            let res_ty = infer_expr_type(then_expr, locals, functions, structs, table_indices)?;
            let cond_ty = compile_expr(
                cond,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_to_bool(func, cond_ty);

            control_stack.push(ControlFrame::Block);
            func.instruction(&Instruction::If(BlockType::Result(res_ty.into())));

            let t_ty = compile_expr(
                then_expr,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_type(func, t_ty, res_ty);

            func.instruction(&Instruction::Else);

            let e_ty = compile_expr(
                else_expr,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_type(func, e_ty, res_ty);

            control_stack.pop();
            func.instruction(&Instruction::End);

            Ok(res_ty)
        }
        HirExpr::Call(callee, args, span) => {
            // Case 0: Built-in host I/O call: `print`, `println`, `io.print`, `io.println`
            let (is_io_call, is_println) = match &**callee {
                HirExpr::Identifier(name, _) if name == "print" => (true, false),
                HirExpr::Identifier(name, _) if name == "println" => (true, true),
                HirExpr::Dot(base, member, _) if member == "print" || member == "println" => {
                    if let HirExpr::Identifier(base_name, _) = &**base {
                        if base_name == "io" {
                            (true, member == "println")
                        } else {
                            (false, false)
                        }
                    } else {
                        (false, false)
                    }
                }
                _ => (false, false),
            };

            if is_io_call && let Some(io) = async_helpers.host_io {
                for arg in args {
                    compile_print_arg(
                        &arg.value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                        io,
                    )?;
                }
                if is_println {
                    func.instruction(&Instruction::Call(io.println_idx));
                }
                func.instruction(&Instruction::I64Const(0));
                return Ok(WasmType::I64);
            }

            // Case 0.4: Global `len(value)` builtin. The Wasm backend already
            // implements the `.len` property for every supported container;
            // lowering the call to that property keeps one implementation.
            if let HirExpr::Identifier(name, _) = &**callee
                && name == "len"
                && args.len() == 1
                && args[0].name.is_none()
                && functions.builtin_is_unshadowed(name)
            {
                let prop = HirExpr::Dot(Box::new(args[0].value.clone()), "len".to_string(), *span);
                return compile_expr(
                    &prop,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                );
            }

            // Case 0.45: Global `fail(value)` native. Sets the hybrid failure status/message
            // and yields a zero default so callers can propagate or `or_else`.
            if let HirExpr::Identifier(name, _) = &**callee
                && name == "fail"
                && args.len() == 1
                && args[0].name.is_none()
                && functions.builtin_is_unshadowed(name)
            {
                let arg = &args[0].value;
                let arg_ty = compile_expr(
                    arg,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                // If the argument is a string pointer, store it as the message;
                // otherwise render is not available, so store 0 (or the provided i32).
                coerce_type(func, arg_ty, WasmType::I32);
                func.instruction(&Instruction::GlobalSet(
                    async_helpers.fail_globals.message_idx,
                ));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::GlobalSet(
                    async_helpers.fail_globals.status_idx,
                ));
                // `fail(...)` announces a pending failure through the globals; it
                // still has to leave a value on the stack, because the caller may
                // place it in an expression slot (`if` result, `return` operand,
                // argument). The value is a zero of the result representation and
                // is never observed, since every boundary checks the flag first.
                func.instruction(&Instruction::I64Const(0));
                return Ok(WasmType::I64);
            }

            // Case 0.47: `math.<fn>(...)` module calls.
            if let HirExpr::Dot(base, method_name, _) = &**callee
                && let HirExpr::Identifier(mod_name, _) = &**base
                && mod_name == "math"
                && !locals.contains_key(mod_name)
            {
                let all_int = args.iter().all(|a| {
                    matches!(
                        infer_expr_kind(&a.value, locals, functions, table_indices, anon_map),
                        LocalKind::Int
                    )
                });

                // Generic f64 path for other methods.
                match method_name.as_str() {
                    "abs" => {
                        if args.len() != 1 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let at = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, at, WasmType::F64);
                        func.instruction(&Instruction::F64Abs);
                    }
                    "min" => {
                        if args.len() != 2 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let a0 = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, a0, WasmType::F64);
                        let a1 = compile_expr(
                            &args[1].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, a1, WasmType::F64);
                        func.instruction(&Instruction::F64Min);
                    }
                    "max" => {
                        if args.len() != 2 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let a0 = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, a0, WasmType::F64);
                        let a1 = compile_expr(
                            &args[1].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, a1, WasmType::F64);
                        func.instruction(&Instruction::F64Max);
                    }
                    "floor" => {
                        if args.len() != 1 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let at = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, at, WasmType::F64);
                        func.instruction(&Instruction::F64Floor);
                    }
                    "ceil" => {
                        if args.len() != 1 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let at = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, at, WasmType::F64);
                        func.instruction(&Instruction::F64Ceil);
                    }
                    "round" => {
                        if args.len() != 1 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let at = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, at, WasmType::F64);
                        let temp = locals[&format!("__f64_temp_{}", call_depth.min(7))].0;
                        func.instruction(&Instruction::LocalSet(temp));
                        func.instruction(&Instruction::LocalGet(temp));
                        func.instruction(&Instruction::F64Const(0.0.into()));
                        func.instruction(&Instruction::F64Ge);
                        func.instruction(&Instruction::If(BlockType::Result(ValType::F64)));
                        func.instruction(&Instruction::LocalGet(temp));
                        func.instruction(&Instruction::F64Const(0.5.into()));
                        func.instruction(&Instruction::F64Add);
                        func.instruction(&Instruction::F64Floor);
                        func.instruction(&Instruction::Else);
                        func.instruction(&Instruction::LocalGet(temp));
                        func.instruction(&Instruction::F64Const(0.5.into()));
                        func.instruction(&Instruction::F64Sub);
                        func.instruction(&Instruction::F64Ceil);
                        func.instruction(&Instruction::End);
                    }
                    "truncate" => {
                        if args.len() != 1 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let at = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, at, WasmType::F64);
                        func.instruction(&Instruction::F64Trunc);
                    }
                    "sqrt" => {
                        if args.len() != 1 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let at = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, at, WasmType::F64);
                        func.instruction(&Instruction::F64Sqrt);
                    }
                    "clamp" => {
                        if args.len() != 3 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        let a0 = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, a0, WasmType::F64);
                        let a1 = compile_expr(
                            &args[1].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, a1, WasmType::F64);
                        let a2 = compile_expr(
                            &args[2].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, a2, WasmType::F64);
                        // Canon treats inverted bounds as a value outside a range: a
                        // recoverable failure, so `or_else` can supply a fallback. The
                        // clamp itself is still min-then-max, which is harmless once the
                        // failure flag is set.
                        let clamp_value = locals[&format!("__f64_temp_{}", call_depth.min(7))].0;
                        let clamp_low =
                            locals[&format!("__f64_temp_{}", (call_depth + 1).min(7))].0;
                        let clamp_high =
                            locals[&format!("__f64_temp_{}", (call_depth + 2).min(7))].0;
                        func.instruction(&Instruction::LocalSet(clamp_high));
                        func.instruction(&Instruction::LocalSet(clamp_low));
                        func.instruction(&Instruction::LocalSet(clamp_value));
                        func.instruction(&Instruction::LocalGet(clamp_low));
                        func.instruction(&Instruction::LocalGet(clamp_high));
                        func.instruction(&Instruction::F64Gt);
                        func.instruction(&Instruction::If(BlockType::Empty));
                        let (prefix_ptr, quote_ptr) = (
                            static_strings
                                .get(INT_PARSE_PREFIX)
                                .copied()
                                .unwrap_or_default(),
                            static_strings.get("\"").copied().unwrap_or_default(),
                        );
                        let _ = (prefix_ptr, quote_ptr);
                        func.instruction(&Instruction::I32Const(
                            static_strings
                                .get("math.clamp bounds are inverted")
                                .copied()
                                .unwrap_or_default(),
                        ));
                        func.instruction(&Instruction::GlobalSet(
                            async_helpers.fail_globals.message_idx,
                        ));
                        func.instruction(&Instruction::I32Const(1));
                        func.instruction(&Instruction::GlobalSet(
                            async_helpers.fail_globals.status_idx,
                        ));
                        func.instruction(&Instruction::End);
                        func.instruction(&Instruction::LocalGet(clamp_value));
                        func.instruction(&Instruction::LocalGet(clamp_low));
                        func.instruction(&Instruction::F64Max);
                        func.instruction(&Instruction::LocalGet(clamp_high));
                        func.instruction(&Instruction::F64Min);
                    }
                    "pow" => {
                        if args.len() != 2 {
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "bad arity".into(),
                                span: *span,
                            });
                        }
                        if all_int {
                            // Integer exponentiation by squaring on `i64` scratch
                            // locals. `math.pow(2, 10)` is an integer expression in
                            // canon, so it must stay exact instead of round-tripping
                            // through `f64`.
                            let bty = compile_expr(
                                &args[0].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, bty, WasmType::I64);
                            let base_temp =
                                locals[&format!("__i64_temp_{}", (call_depth + 1).min(7))].0;
                            func.instruction(&Instruction::LocalSet(base_temp));
                            let ety = compile_expr(
                                &args[1].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, ety, WasmType::I64);
                            let exp_temp = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
                            func.instruction(&Instruction::LocalSet(exp_temp));
                            let res_temp =
                                locals[&format!("__i64_temp_{}", (call_depth + 2).min(7))].0;
                            func.instruction(&Instruction::I64Const(1));
                            func.instruction(&Instruction::LocalSet(res_temp));

                            // A negative exponent has no exact integer result.
                            func.instruction(&Instruction::LocalGet(exp_temp));
                            func.instruction(&Instruction::I64Const(0));
                            func.instruction(&Instruction::I64LtS);
                            func.instruction(&Instruction::If(BlockType::Empty));
                            func.instruction(&Instruction::I64Const(0));
                            func.instruction(&Instruction::LocalSet(res_temp));
                            func.instruction(&Instruction::Else);
                            // while exp > 0 { if exp & 1 { res *= base } base *= base; exp >>= 1 }
                            func.instruction(&Instruction::Block(BlockType::Empty));
                            func.instruction(&Instruction::Loop(BlockType::Empty));
                            func.instruction(&Instruction::LocalGet(exp_temp));
                            func.instruction(&Instruction::I64Eqz);
                            func.instruction(&Instruction::BrIf(1));

                            func.instruction(&Instruction::LocalGet(exp_temp));
                            func.instruction(&Instruction::I64Const(1));
                            func.instruction(&Instruction::I64And);
                            // `i64.and` yields `i64`, but a Wasm `if` needs an `i32`
                            // condition; comparing against zero narrows it.
                            func.instruction(&Instruction::I64Const(0));
                            func.instruction(&Instruction::I64Ne);
                            func.instruction(&Instruction::If(BlockType::Empty));
                            func.instruction(&Instruction::LocalGet(res_temp));
                            func.instruction(&Instruction::LocalGet(base_temp));
                            func.instruction(&Instruction::I64Mul);
                            func.instruction(&Instruction::LocalSet(res_temp));
                            func.instruction(&Instruction::End);

                            func.instruction(&Instruction::LocalGet(base_temp));
                            func.instruction(&Instruction::LocalGet(base_temp));
                            func.instruction(&Instruction::I64Mul);
                            func.instruction(&Instruction::LocalSet(base_temp));

                            func.instruction(&Instruction::LocalGet(exp_temp));
                            func.instruction(&Instruction::I64Const(1));
                            func.instruction(&Instruction::I64ShrU);
                            func.instruction(&Instruction::LocalSet(exp_temp));
                            func.instruction(&Instruction::Br(0));
                            func.instruction(&Instruction::End);
                            func.instruction(&Instruction::End);
                            func.instruction(&Instruction::End);

                            func.instruction(&Instruction::LocalGet(res_temp));
                        } else {
                            let _a0 = compile_expr(
                                &args[0].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            let _a1 = compile_expr(
                                &args[1].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            return Err(WasmCompileError::UnsupportedExpr {
                                message: "`math.pow` with a non-integer operand is not implemented in the Wasm backend; use --engine=vm".into(),
                                span: *span,
                            });
                        }
                    }
                    _ => {
                        for a in args {
                            let _ = compile_expr(
                                &a.value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                        }
                        func.instruction(&Instruction::F64Const(0.0.into()));
                    }
                }

                // Convert back to int for int-preserving ops. `floor`, `ceil`,
                // `round` and `truncate` answer an Int whatever the argument was; the
                // others keep `Int` only when every operand already was one.
                let always_int = matches!(
                    method_name.as_str(),
                    "floor" | "ceil" | "round" | "truncate"
                );
                let int_preserving = always_int
                    || (all_int && matches!(method_name.as_str(), "abs" | "min" | "max" | "clamp"));
                if method_name == "pow" && all_int {
                    // already i64
                } else if int_preserving {
                    func.instruction(&Instruction::I64TruncF64S);
                }

                if method_name == "pow" && all_int {
                    return Ok(WasmType::I64);
                }
                return Ok(if int_preserving {
                    WasmType::I64
                } else {
                    WasmType::F64
                });
            }

            // pow handled separately below? We returned error above; remove pow from this path by handling before.
            // Case 0.455: `string.slice(text, start, end)` walks code points and
            // clamps its bounds, exactly like an indexing slice does.
            if let HirExpr::Dot(base, method_name, _) = &**callee
                && let HirExpr::Identifier(mod_name, _) = &**base
                && mod_name == "string"
                && !locals.contains_key(mod_name)
                && method_name == "slice"
                && args.len() == 3
            {
                let (str_slice_idx, _, _) = functions["__aipo_str_slice"];
                let text_ty = compile_expr(
                    &args[0].value,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, text_ty, WasmType::I32);
                for arg in &args[1..] {
                    // A bound is an `i64` in canon; the helper walks with `i32`
                    // indices, so the value saturates instead of wrapping.
                    let bound_ty = compile_expr(
                        &arg.value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, bound_ty, WasmType::I64);
                    let bound_temp = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
                    func.instruction(&Instruction::LocalSet(bound_temp));
                    func.instruction(&Instruction::LocalGet(bound_temp));
                    func.instruction(&Instruction::I64Const(i64::from(i32::MAX)));
                    func.instruction(&Instruction::I64GtS);
                    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                    func.instruction(&Instruction::I32Const(i32::MAX));
                    func.instruction(&Instruction::Else);
                    func.instruction(&Instruction::LocalGet(bound_temp));
                    func.instruction(&Instruction::I64Const(i64::from(i32::MIN)));
                    func.instruction(&Instruction::I64LtS);
                    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                    func.instruction(&Instruction::I32Const(i32::MIN));
                    func.instruction(&Instruction::Else);
                    func.instruction(&Instruction::LocalGet(bound_temp));
                    func.instruction(&Instruction::I32WrapI64);
                    func.instruction(&Instruction::End);
                    func.instruction(&Instruction::End);
                }
                func.instruction(&Instruction::Call(str_slice_idx));
                return Ok(WasmType::I32);
            }

            // Case 0.459: `string.trim/upper/lower/find` — the helpers that carry
            // their own implementation rather than the shared approximation below.
            if let HirExpr::Dot(base, method_name, _) = &**callee
                && let HirExpr::Identifier(mod_name, _) = &**base
                && mod_name == "string"
                && !locals.contains_key(mod_name)
            {
                let implemented = matches!(
                    method_name.as_str(),
                    "trim"
                        | "upper"
                        | "lower"
                        | "find"
                        | "len"
                        | "byte_len"
                        | "starts_with"
                        | "ends_with"
                        | "contains"
                        | "reverse"
                        | "capitalize"
                        | "split"
                        | "join"
                        | "replace"
                );
                if implemented {
                    let arity_ok = match method_name.as_str() {
                        "trim" | "upper" | "lower" | "len" | "byte_len" | "reverse"
                        | "capitalize" => args.len() == 1,
                        "replace" => args.len() == 3,
                        _ => args.len() == 2,
                    };
                    if !arity_ok {
                        return Err(WasmCompileError::UnsupportedExpr {
                            message: format!("bad arity for `string.{method_name}`"),
                            span: *span,
                        });
                    }
                    let text_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, text_ty, WasmType::I32);
                    match method_name.as_str() {
                        "trim" => {
                            let (idx, _, _) = functions["__aipo_str_trim"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "len" => {
                            let (idx, _, _) = functions["__aipo_str_len"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I64);
                        }
                        "byte_len" => {
                            // The pool header already stores the UTF-8 byte count.
                            func.instruction(&Instruction::I32Load(MemArg {
                                offset: 0,
                                align: 2,
                                memory_index: 0,
                            }));
                            func.instruction(&Instruction::I64ExtendI32U);
                            return Ok(WasmType::I64);
                        }
                        "upper" | "lower" => {
                            let (idx, _, _) = functions["__aipo_str_case"];
                            func.instruction(&Instruction::I32Const(i32::from(
                                method_name == "upper",
                            )));
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "reverse" => {
                            let (idx, _, _) = functions["__aipo_str_reverse"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "split" => {
                            let needle_ty = compile_expr(
                                &args[1].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, needle_ty, WasmType::I32);
                            let (idx, _, _) = functions["__aipo_str_split"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "join" => {
                            // `string.join(sep, listable)`: the separator is the
                            // receiver position in the module call.
                            let list_ty = compile_expr(
                                &args[1].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, list_ty, WasmType::I32);
                            let (idx, _, _) = functions["__aipo_str_join"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "replace" => {
                            for position in [1usize, 2] {
                                let arg_ty = compile_expr(
                                    &args[position].value,
                                    func,
                                    locals,
                                    functions,
                                    control_stack,
                                    structs,
                                    static_strings,
                                    table_indices,
                                    anon_map,
                                    indirect_sigs,
                                    alloc_func_idx,
                                    async_helpers,
                                    struct_depth,
                                    call_depth,
                                )?;
                                coerce_type(func, arg_ty, WasmType::I32);
                            }
                            let (idx, _, _) = functions["__aipo_str_replace"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "capitalize" => {
                            // Fold the head up and the tail down: the canonical
                            // `capitalize` keeps only the first letter capital.
                            let (case_idx, _, _) = functions["__aipo_str_case"];
                            let (slice_idx, _, _) = functions["__aipo_str_slice"];
                            let head_slot = locals[&format!("__call_temp_{}", call_depth.min(7))].0;
                            func.instruction(&Instruction::LocalSet(head_slot));
                            // head = upper(first code point)
                            func.instruction(&Instruction::LocalGet(head_slot));
                            func.instruction(&Instruction::I32Const(0));
                            func.instruction(&Instruction::I32Const(1));
                            func.instruction(&Instruction::Call(slice_idx));
                            func.instruction(&Instruction::I32Const(1));
                            func.instruction(&Instruction::Call(case_idx));
                            // tail = lower(rest)
                            func.instruction(&Instruction::LocalGet(head_slot));
                            func.instruction(&Instruction::I32Const(1));
                            func.instruction(&Instruction::I32Const(i32::MAX));
                            func.instruction(&Instruction::Call(slice_idx));
                            func.instruction(&Instruction::I32Const(0));
                            func.instruction(&Instruction::Call(case_idx));
                            let (concat_idx, _, _) = functions["__aipo_str_concat"];
                            func.instruction(&Instruction::Call(concat_idx));
                            return Ok(WasmType::I32);
                        }
                        "contains" => {
                            // `contains` is "found somewhere", so a `-1` answer is
                            // the only negative one.
                            let needle_ty = compile_expr(
                                &args[1].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, needle_ty, WasmType::I32);
                            let (idx, _, _) = functions["__aipo_str_find"];
                            func.instruction(&Instruction::Call(idx));
                            // `find` answers the `none` sentinel when absent.
                            func.instruction(&Instruction::I64Eqz);
                            func.instruction(&Instruction::I32Eqz);
                            return Ok(WasmType::I32);
                        }
                        _ => {
                            let needle_ty = compile_expr(
                                &args[1].value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, needle_ty, WasmType::I32);
                            let name = if method_name == "starts_with" {
                                "__aipo_str_starts"
                            } else if method_name == "ends_with" {
                                "__aipo_str_ends"
                            } else {
                                "__aipo_str_find"
                            };
                            let (idx, _, _) = functions[name];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(if name == "__aipo_str_find" {
                                WasmType::I64
                            } else {
                                WasmType::I32
                            });
                        }
                    }
                }
            }

            // Case 0.46: `string.<fn>(...)` module calls (approximated).
            if let HirExpr::Dot(base, method_name, _) = &**callee
                && let HirExpr::Identifier(mod_name, _) = &**base
                && mod_name == "string"
                && !locals.contains_key(mod_name)
            {
                // compile all args
                let mut first_str_temp: Option<u32> = None;
                for a in args {
                    let at = compile_expr(
                        &a.value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    let kind =
                        infer_expr_kind(&a.value, locals, functions, table_indices, anon_map);
                    if matches!(kind, LocalKind::String) {
                        coerce_type(func, at, WasmType::I32);
                        if first_str_temp.is_none() {
                            let temp = locals[&format!("__call_temp_{}", call_depth.min(7))].0;
                            func.instruction(&Instruction::LocalSet(temp));
                            first_str_temp = Some(temp);
                        } else {
                            func.instruction(&Instruction::Drop);
                        }
                    } else if matches!(kind, LocalKind::Int) {
                        coerce_type(func, at, WasmType::I64);
                        func.instruction(&Instruction::Drop);
                    } else if matches!(kind, LocalKind::Bool) {
                        coerce_type(func, at, WasmType::I32);
                        func.instruction(&Instruction::Drop);
                    } else {
                        // default coerce to i32
                        coerce_type(func, at, WasmType::I32);
                        func.instruction(&Instruction::Drop);
                    }
                }
                // Determine return type
                let returns_int = matches!(method_name.as_str(), "len" | "byte_len" | "find");
                let returns_bool = matches!(
                    method_name.as_str(),
                    "contains" | "starts_with" | "ends_with"
                );
                let returns_list = matches!(method_name.as_str(), "split");
                if returns_int {
                    func.instruction(&Instruction::I64Const(0));
                    return Ok(WasmType::I64);
                }
                if returns_bool {
                    func.instruction(&Instruction::I32Const(1));
                    return Ok(WasmType::I32);
                }
                if returns_list {
                    // return empty list
                    let (list_new_idx, _, _) = functions["__aipo_list_new"];
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::Call(list_new_idx));
                    return Ok(WasmType::I32);
                }
                // string-returning: return first string arg if captured
                if let Some(temp) = first_str_temp {
                    func.instruction(&Instruction::LocalGet(temp));
                } else {
                    func.instruction(&Instruction::I32Const(0));
                }
                return Ok(WasmType::I32);
            }

            // Case 0.45: `task.<fn>(...)` module calls (approximated).
            if let HirExpr::Dot(base, method_name, _) = &**callee
                && let HirExpr::Identifier(mod_name, _) = &**base
                && mod_name == "task"
                && !locals.contains_key(mod_name)
            {
                // `task.all`/`task.race` consume their argument list below, so it is
                // not discarded here.
                let handles_args =
                    matches!(method_name.as_str(), "all" | "race") && !args.is_empty();
                if !handles_args {
                    for a in args {
                        let at = compile_expr(
                            &a.value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        let kind =
                            infer_expr_kind(&a.value, locals, functions, table_indices, anon_map);
                        // A `task.*` argument is only ever consumed by the stub that
                        // drives the call; its representation is irrelevant here, so
                        // every kind collapses onto the same coercion-and-drop.
                        let arg_ty = match kind {
                            LocalKind::Int => WasmType::I64,
                            _ => WasmType::I32,
                        };
                        coerce_type(func, at, arg_ty);
                        func.instruction(&Instruction::Drop);
                    }
                }
                let returns_list = matches!(method_name.as_str(), "all" | "race");
                if method_name == "group" {
                    func.instruction(&Instruction::I32Const(0));
                    return Ok(WasmType::I32);
                }
                if returns_list {
                    // `task.all`/`task.race` drive every task in the argument list and
                    // collect the values, so the result is a List of what each returned.
                    if args.len() == 1 {
                        let (list_new_idx, _, _) = functions["__aipo_list_new"];
                        let (list_len_idx, _, _) = functions["__aipo_list_len"];
                        let (list_get_idx, _, _) = functions["__aipo_list_get"];
                        let (list_add_idx, _, _) = functions["__aipo_list_add"];
                        let tasks_ty = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, tasks_ty, WasmType::I32);
                        let tasks_slot = locals[&format!("__call_temp_{}", call_depth.min(7))].0;
                        func.instruction(&Instruction::LocalSet(tasks_slot));
                        func.instruction(&Instruction::I32Const(0));
                        func.instruction(&Instruction::Call(list_new_idx));
                        let out_slot = locals[&format!("__list_temp_{}", struct_depth.min(7))].0;
                        func.instruction(&Instruction::LocalSet(out_slot));
                        let idx_slot = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
                        let len_slot = locals[&format!("__i64_temp_{}", (call_depth + 1).min(7))].0;
                        func.instruction(&Instruction::I32Const(0));
                        func.instruction(&Instruction::I64ExtendI32U);
                        func.instruction(&Instruction::LocalSet(idx_slot));
                        func.instruction(&Instruction::LocalGet(tasks_slot));
                        func.instruction(&Instruction::Call(list_len_idx));
                        func.instruction(&Instruction::LocalSet(len_slot));
                        func.instruction(&Instruction::Block(BlockType::Empty));
                        func.instruction(&Instruction::Loop(BlockType::Empty));
                        func.instruction(&Instruction::LocalGet(idx_slot));
                        func.instruction(&Instruction::LocalGet(len_slot));
                        func.instruction(&Instruction::I64GeS);
                        func.instruction(&Instruction::BrIf(1));
                        func.instruction(&Instruction::LocalGet(out_slot));
                        func.instruction(&Instruction::LocalGet(tasks_slot));
                        func.instruction(&Instruction::LocalGet(idx_slot));
                        func.instruction(&Instruction::I32WrapI64);
                        func.instruction(&Instruction::Call(list_get_idx));
                        func.instruction(&Instruction::I32WrapI64);
                        func.instruction(&Instruction::Call(async_helpers.await_idx));
                        func.instruction(&Instruction::Call(list_add_idx));
                        func.instruction(&Instruction::Drop);
                        func.instruction(&Instruction::LocalGet(idx_slot));
                        func.instruction(&Instruction::I64Const(1));
                        func.instruction(&Instruction::I64Add);
                        func.instruction(&Instruction::LocalSet(idx_slot));
                        func.instruction(&Instruction::Br(0));
                        func.instruction(&Instruction::End);
                        func.instruction(&Instruction::End);
                        func.instruction(&Instruction::LocalGet(out_slot));
                        return Ok(WasmType::I32);
                    }
                    let (list_new_idx, _, _) = functions["__aipo_list_new"];
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::Call(list_new_idx));
                    return Ok(WasmType::I32);
                }
                if matches!(method_name.as_str(), "spawn") {
                    // `task.spawn(f, [a, b])` starts the task eagerly. The function's
                    // table index and up to two of its arguments travel as the task
                    // body's payload, which is what `__aipo_task_create` stores.
                    let (callee_name, table_idx) = match args.first().map(|a| &a.value) {
                        Some(HirExpr::Identifier(name, _)) => {
                            // An async function's table entry is its body: the wrapper
                            // spawns, the body runs inside the task.
                            let body_name = format!("__async_body_{name}");
                            match table_indices
                                .get(&body_name)
                                .or_else(|| table_indices.get(name.as_str()))
                            {
                                Some(&index) => (Some(name.clone()), Some(index)),
                                None => (Some(name.clone()), None),
                            }
                        }
                        _ => (None, None),
                    };
                    let Some(table_idx) = table_idx else {
                        // A dynamic target cannot be spawned by this backend.
                        func.instruction(&Instruction::I32Const(0));
                        return Ok(WasmType::I32);
                    };
                    let _ = callee_name;
                    // The argument count and values come from the list literal in the
                    // second position.
                    let (mut arg_count, mut arg0, mut arg1) = (0i32, 0i64, 0i64);
                    if let Some(HirExpr::List(items, _)) = args.get(1).map(|a| &a.value) {
                        arg_count = i32::try_from(items.len().min(2)).unwrap_or(2);
                        if let Some(HirExpr::Literal(Literal::Int(raw), _)) = items.first() {
                            arg0 = raw.parse().unwrap_or(0);
                        }
                        if let Some(HirExpr::Literal(Literal::Int(raw), _)) = items.get(1) {
                            arg1 = raw.parse().unwrap_or(0);
                        }
                    }
                    func.instruction(&Instruction::I32Const(table_idx as i32));
                    func.instruction(&Instruction::I32Const(arg_count));
                    func.instruction(&Instruction::I64Const(arg0));
                    func.instruction(&Instruction::I64Const(arg1));
                    let (create_idx, _, _) = functions["__aipo_task_create"];
                    func.instruction(&Instruction::Call(create_idx));
                    return Ok(WasmType::I32);
                }
                // sleep, cancel, timeout -> i64/none
                func.instruction(&Instruction::I64Const(0));
                return Ok(WasmType::I64);
            }

            // Case 0.44: `time.<fn>(...)` module calls (approximated).
            if let HirExpr::Dot(base, _method_name, _) = &**callee
                && let HirExpr::Identifier(mod_name, _) = &**base
                && mod_name == "time"
                && !locals.contains_key(mod_name)
            {
                for a in args {
                    let at = compile_expr(
                        &a.value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    let kind =
                        infer_expr_kind(&a.value, locals, functions, table_indices, anon_map);
                    if matches!(kind, LocalKind::Int) {
                        coerce_type(func, at, WasmType::I64);
                        func.instruction(&Instruction::Drop);
                    } else {
                        coerce_type(func, at, WasmType::I32);
                        func.instruction(&Instruction::Drop);
                    }
                }
                // now/monotonic return a time value; represent as f64 seconds 0.0
                func.instruction(&Instruction::F64Const(0.0.into()));
                return Ok(WasmType::F64);
            }

            // Case 0.5: Enum tuple variant constructor call: `Enum.Variant(...)`
            if let HirExpr::Dot(receiver, variant_name, _) = &**callee
                && let HirExpr::Identifier(enum_name, _) = &**receiver
            {
                let candidate_variant = format!("{enum_name}.{variant_name}");
                if let Some(layout) = structs.get(&candidate_variant)
                    && let Some(&str_ptr) = static_strings.get(&candidate_variant)
                {
                    let temp_name = format!("__struct_temp_{}", struct_depth.min(7));
                    let struct_temp = locals[&temp_name].0;

                    func.instruction(&Instruction::I32Const(layout.size as i32));
                    func.instruction(&Instruction::Call(alloc_func_idx));
                    func.instruction(&Instruction::LocalSet(struct_temp));

                    func.instruction(&Instruction::LocalGet(struct_temp));
                    func.instruction(&Instruction::I32Const(str_ptr));
                    func.instruction(&Instruction::I32Store(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    for (i, arg) in args.iter().enumerate() {
                        func.instruction(&Instruction::LocalGet(struct_temp));
                        let arg_ty = compile_expr(
                            &arg.value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth + 1,
                            call_depth,
                        )?;
                        coerce_type(func, arg_ty, WasmType::I64);
                        func.instruction(&Instruction::I64Store(MemArg {
                            offset: (8 + i * 8) as u64,
                            align: 3,
                            memory_index: 0,
                        }));
                    }

                    func.instruction(&Instruction::LocalGet(struct_temp));
                    return Ok(WasmType::I32);
                }
            }

            // Case 0.7: Zero-arg `.len()` method call on a collection or string.
            // In Aipo `l.len()` binds the same `len` member the property path
            // resolves, so compiling the callee as a property and then emitting
            // `call_indirect` on the length would invoke garbage (or recurse
            // into table entry 0 for an empty list). Compile it as the property.
            if let HirExpr::Dot(receiver, method_name, dot_span) = &**callee
                && method_name == "len"
                && args.is_empty()
            {
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(
                    receiver_kind,
                    LocalKind::List(_)
                        | LocalKind::Dict(_)
                        | LocalKind::Range
                        | LocalKind::String
                        | LocalKind::Bytes
                ) {
                    let prop = HirExpr::Dot(receiver.clone(), "len".to_string(), *dot_span);
                    return compile_expr(
                        &prop,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    );
                }
            }

            // Case 0.8: List `.add(...)` dynamic append
            if let HirExpr::Dot(receiver, method_name, _) = &**callee
                && method_name == "add"
                && args.len() == 1
            {
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                let is_struct_method = match &receiver_kind {
                    LocalKind::Struct(s) => functions.contains_key(&format!("{s}.add")),
                    _ => false,
                };
                if !is_struct_method {
                    let (list_add_idx, _, _) = functions["__aipo_list_add"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let val_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, val_ty, WasmType::I64);
                    func.instruction(&Instruction::Call(list_add_idx));
                    return Ok(WasmType::I64);
                }
            }

            // Case 0.81: List and collection predicates/accessors.
            if let HirExpr::Dot(receiver, method_name, _) = &**callee {
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if method_name == "is_empty"
                    && args.is_empty()
                    && matches!(
                        receiver_kind,
                        LocalKind::List(_)
                            | LocalKind::Dict(_)
                            | LocalKind::Range
                            | LocalKind::String
                            | LocalKind::Bytes
                    )
                {
                    let len_prop = HirExpr::Dot(receiver.clone(), "len".to_string(), *span);
                    let zero = HirExpr::Literal(Literal::Int("0".to_string()), *span);
                    let cmp =
                        HirExpr::Binary(BinaryOp::Equal, Box::new(len_prop), Box::new(zero), *span);
                    return compile_expr(
                        &cmp,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    );
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && (method_name == "first" || method_name == "last")
                    && args.is_empty()
                {
                    let (list_get_idx, _, _) = functions["__aipo_list_get"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    if method_name == "first" {
                        func.instruction(&Instruction::I32Const(0));
                    } else {
                        let temp = locals["__dot_temp"].0;
                        func.instruction(&Instruction::LocalSet(temp));
                        func.instruction(&Instruction::LocalGet(temp));
                        func.instruction(&Instruction::LocalGet(temp));
                        let (list_len_idx, _, _) = functions["__aipo_list_len"];
                        func.instruction(&Instruction::Call(list_len_idx));
                        func.instruction(&Instruction::I32WrapI64);
                        func.instruction(&Instruction::I32Const(1));
                        func.instruction(&Instruction::I32Sub);
                    }
                    func.instruction(&Instruction::Call(list_get_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "contains"
                    && args.len() == 1
                {
                    let (list_contains_idx, _, _) = functions["__aipo_list_contains"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let val_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, val_ty, WasmType::I64);
                    func.instruction(&Instruction::Call(list_contains_idx));
                    return Ok(WasmType::I32);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "insert"
                    && args.len() == 2
                {
                    let (list_insert_idx, _, _) = functions["__aipo_list_insert"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let idx_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, idx_ty, WasmType::I32);
                    let val_ty = compile_expr(
                        &args[1].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, val_ty, WasmType::I64);
                    func.instruction(&Instruction::Call(list_insert_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "remove_last"
                    && args.is_empty()
                {
                    let (list_remove_last_idx, _, _) = functions["__aipo_list_remove_last"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(list_remove_last_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "remove_at"
                    && args.len() == 1
                {
                    let (list_remove_at_idx, _, _) = functions["__aipo_list_remove_at"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let idx_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, idx_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(list_remove_at_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "remove"
                    && args.len() == 1
                {
                    let (list_remove_idx, _, _) = functions["__aipo_list_remove"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let val_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, val_ty, WasmType::I64);
                    func.instruction(&Instruction::Call(list_remove_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "clear"
                    && args.is_empty()
                {
                    let (list_clear_idx, _, _) = functions["__aipo_list_clear"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(list_clear_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && (method_name == "count" || method_name == "find")
                    && args.len() == 1
                {
                    let helper_idx = if method_name == "count" {
                        functions["__aipo_list_count"].0
                    } else {
                        functions["__aipo_list_find"].0
                    };
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let val_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, val_ty, WasmType::I64);
                    func.instruction(&Instruction::Call(helper_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "reverse"
                    && args.is_empty()
                {
                    let (list_reverse_idx, _, _) = functions["__aipo_list_reverse"];
                    let (list_slice_idx, _, _) = functions["__aipo_list_slice"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32Const(i32::MAX));
                    func.instruction(&Instruction::Call(list_slice_idx));
                    func.instruction(&Instruction::Call(list_reverse_idx));
                    return Ok(WasmType::I32);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && (method_name == "transform" || method_name == "map")
                    && args.len() == 1
                {
                    let (list_transform_idx, _, _) = functions["__aipo_list_transform"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let cb_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, cb_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(list_transform_idx));
                    return Ok(WasmType::I32);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "filter"
                    && args.len() == 1
                {
                    let (list_filter_idx, _, _) = functions["__aipo_list_filter"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let cb_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, cb_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(list_filter_idx));
                    return Ok(WasmType::I32);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "sort"
                    && args.is_empty()
                {
                    let (list_sort_idx, _, _) = functions["__aipo_list_sort"];
                    let (list_slice_idx, _, _) = functions["__aipo_list_slice"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    // Canon's `sort` yields a new ordered list and leaves the receiver
                    // as it was, so sorting happens on a window over the whole list.
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32Const(i32::MAX));
                    func.instruction(&Instruction::Call(list_slice_idx));
                    func.instruction(&Instruction::Call(list_sort_idx));
                    return Ok(WasmType::I32);
                }
                if matches!(receiver_kind, LocalKind::List(_))
                    && method_name == "sort_by"
                    && args.len() == 1
                {
                    let (list_sort_by_idx, _, _) = functions["__aipo_list_sort_by"];
                    let (list_slice_idx, _, _) = functions["__aipo_list_slice"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32Const(i32::MAX));
                    func.instruction(&Instruction::Call(list_slice_idx));
                    let cb_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, cb_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(list_sort_by_idx));
                    return Ok(WasmType::I32);
                }
            }

            // Case 0.82: Dict `.has(...)` key presence check
            if let HirExpr::Dot(receiver, method_name, _) = &**callee
                && method_name == "has"
                && args.len() == 1
            {
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(receiver_kind, LocalKind::Dict(_)) {
                    let (dict_has_idx, _, _) = functions["__aipo_dict_has"];
                    let (string_hash_idx, _, _) = functions["__aipo_string_hash"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);

                    let key_expr = &args[0].value;
                    let key_kind =
                        infer_expr_kind(key_expr, locals, functions, table_indices, anon_map);
                    let k_ty = compile_expr(
                        key_expr,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    if matches!(key_kind, LocalKind::String) {
                        coerce_type(func, k_ty, WasmType::I32);
                        func.instruction(&Instruction::Call(string_hash_idx));
                    } else {
                        coerce_type(func, k_ty, WasmType::I64);
                    }
                    func.instruction(&Instruction::Call(dict_has_idx));
                    return Ok(WasmType::I32);
                }
            }

            // Case 0.83: Dict `.is_empty()`, `.remove(key)`, `.keys()`, `.values()`
            if let HirExpr::Dot(receiver, method_name, _) = &**callee {
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(receiver_kind, LocalKind::Dict(_))
                    && matches!(
                        method_name.as_str(),
                        "is_empty" | "remove" | "keys" | "values"
                    )
                {
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);

                    match method_name.as_str() {
                        "is_empty" => {
                            let (idx, _, _) = functions["__aipo_dict_is_empty"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "remove" => {
                            let (idx, _, _) = functions["__aipo_dict_remove"];
                            let (string_hash_idx, _, _) = functions["__aipo_string_hash"];
                            let key_expr = &args[0].value;
                            let key_kind = infer_expr_kind(
                                key_expr,
                                locals,
                                functions,
                                table_indices,
                                anon_map,
                            );
                            let k_ty = compile_expr(
                                key_expr,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            if matches!(key_kind, LocalKind::String) {
                                coerce_type(func, k_ty, WasmType::I32);
                                func.instruction(&Instruction::Call(string_hash_idx));
                            } else {
                                coerce_type(func, k_ty, WasmType::I64);
                            }
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I64);
                        }
                        "keys" => {
                            let (idx, _, _) = functions["__aipo_dict_keys"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        "values" => {
                            let (idx, _, _) = functions["__aipo_dict_values"];
                            func.instruction(&Instruction::Call(idx));
                            return Ok(WasmType::I32);
                        }
                        _ => unreachable!(),
                    }
                }
            }

            // Case 0.85: Bytes methods
            if let HirExpr::Dot(receiver, method_name, _) = &**callee {
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(receiver_kind, LocalKind::Bytes) {
                    if (method_name == "decode" || method_name == "decode_utf8") && args.is_empty()
                    {
                        let r_ty = compile_expr(
                            receiver,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, r_ty, WasmType::I32);
                        return Ok(WasmType::I32);
                    }
                    if (method_name == "read_u8"
                        || method_name == "read_i8"
                        || method_name == "read_i16"
                        || method_name == "read_i16_le"
                        || method_name == "read_u16"
                        || method_name == "read_u16_le"
                        || method_name == "read_i32"
                        || method_name == "read_i32_le"
                        || method_name == "read_u32"
                        || method_name == "read_u32_le"
                        || method_name == "read_i64"
                        || method_name == "read_i64_le"
                        || method_name == "read_u64"
                        || method_name == "read_u64_le")
                        && args.len() == 1
                    {
                        let r_ty = compile_expr(
                            receiver,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, r_ty, WasmType::I32);
                        let off_ty = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, off_ty, WasmType::I32);
                        func.instruction(&Instruction::I32Add);
                        match method_name.as_str() {
                            "read_u8" => func.instruction(&Instruction::I64Load8U(MemArg {
                                offset: 4,
                                align: 0,
                                memory_index: 0,
                            })),
                            "read_i8" => func.instruction(&Instruction::I64Load8S(MemArg {
                                offset: 4,
                                align: 0,
                                memory_index: 0,
                            })),
                            "read_u16" | "read_u16_le" => {
                                func.instruction(&Instruction::I64Load16U(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }))
                            }
                            "read_i16" | "read_i16_le" => {
                                func.instruction(&Instruction::I64Load16S(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }))
                            }
                            "read_u32" | "read_u32_le" => {
                                func.instruction(&Instruction::I64Load32U(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }))
                            }
                            "read_i32" | "read_i32_le" => {
                                func.instruction(&Instruction::I64Load32S(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }))
                            }
                            "read_i64" | "read_i64_le" | "read_u64" | "read_u64_le" => func
                                .instruction(&Instruction::I64Load(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                })),
                            _ => unreachable!(),
                        };
                        return Ok(WasmType::I64);
                    }
                    if (method_name == "write_u8"
                        || method_name == "write_i8"
                        || method_name == "write_i16"
                        || method_name == "write_i16_le"
                        || method_name == "write_u16"
                        || method_name == "write_u16_le"
                        || method_name == "write_i32"
                        || method_name == "write_i32_le"
                        || method_name == "write_u32"
                        || method_name == "write_u32_le"
                        || method_name == "write_i64"
                        || method_name == "write_i64_le"
                        || method_name == "write_u64"
                        || method_name == "write_u64_le")
                        && args.len() == 2
                    {
                        let r_ty = compile_expr(
                            receiver,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, r_ty, WasmType::I32);
                        let off_ty = compile_expr(
                            &args[0].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        coerce_type(func, off_ty, WasmType::I32);
                        func.instruction(&Instruction::I32Add);
                        let val_ty = compile_expr(
                            &args[1].value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        match method_name.as_str() {
                            "write_u8" | "write_i8" => {
                                coerce_type(func, val_ty, WasmType::I64);
                                func.instruction(&Instruction::I32WrapI64);
                                func.instruction(&Instruction::I32Store8(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }));
                            }
                            "write_i16" | "write_i16_le" | "write_u16" | "write_u16_le" => {
                                coerce_type(func, val_ty, WasmType::I64);
                                func.instruction(&Instruction::I32WrapI64);
                                func.instruction(&Instruction::I32Store16(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }));
                            }
                            "write_i32" | "write_i32_le" | "write_u32" | "write_u32_le" => {
                                coerce_type(func, val_ty, WasmType::I64);
                                func.instruction(&Instruction::I32WrapI64);
                                func.instruction(&Instruction::I32Store(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }));
                            }
                            "write_i64" | "write_i64_le" | "write_u64" | "write_u64_le" => {
                                coerce_type(func, val_ty, WasmType::I64);
                                func.instruction(&Instruction::I64Store(MemArg {
                                    offset: 4,
                                    align: 0,
                                    memory_index: 0,
                                }));
                            }
                            _ => unreachable!(),
                        };
                        func.instruction(&Instruction::I64Const(0));
                        return Ok(WasmType::I64);
                    }
                }
            }

            // Case 0.9: Struct method call: `receiver.method(...)` or static `Type.method(...)`
            if let HirExpr::Dot(receiver, method_name, _) = &**callee {
                // A receiver that names a module binding is an instance, not a
                // type: the binding's name is in scope, so it shadows the type.
                let static_type_call = if let HirExpr::Identifier(rec_name, _) = &**receiver {
                    if structs.contains_key(rec_name)
                        && !locals.contains_key(rec_name)
                        && !functions.module_globals.contains_key(rec_name)
                    {
                        Some(rec_name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(type_name) = static_type_call {
                    let full_name = format!("{type_name}.{method_name}");
                    if let Some(&(func_idx, _, ref fn_type)) = functions.get(&full_name) {
                        let fn_type = fn_type.clone();
                        for (arg, &expected_ty) in args.iter().zip(fn_type.params.iter()) {
                            let actual_ty = compile_expr(
                                &arg.value,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, actual_ty, expected_ty);
                        }
                        func.instruction(&Instruction::Call(func_idx));
                        if fn_type.results.is_empty() {
                            func.instruction(&Instruction::I64Const(0));
                            return Ok(WasmType::I64);
                        } else {
                            return Ok(fn_type.results[0]);
                        }
                    }
                } else {
                    let target_struct_name = match &**receiver {
                        HirExpr::Identifier(rec_name, _) => {
                            locals.get(rec_name).and_then(|(_, _, kind)| match kind {
                                LocalKind::Struct(s_name) => Some(s_name.clone()),
                                _ => None,
                            })
                        }
                        HirExpr::Construct(s_name, ..) => Some(s_name.clone()),
                        _ => None,
                    }
                    .or_else(|| {
                        let candidates: Vec<_> = structs
                            .keys()
                            .filter(|s| functions.contains_key(&format!("{s}.{method_name}")))
                            .cloned()
                            .collect();
                        if candidates.len() == 1 {
                            Some(candidates[0].clone())
                        } else {
                            None
                        }
                    });

                    if let Some(s_name) = target_struct_name {
                        let full_name = format!("{s_name}.{method_name}");
                        if let Some(&(func_idx, _, ref fn_type)) = functions.get(&full_name) {
                            let fn_type = fn_type.clone();
                            // Argument 0 is receiver `self: i32`
                            let r_ty = compile_expr(
                                receiver,
                                func,
                                locals,
                                functions,
                                control_stack,
                                structs,
                                static_strings,
                                table_indices,
                                anon_map,
                                indirect_sigs,
                                alloc_func_idx,
                                async_helpers,
                                struct_depth,
                                call_depth,
                            )?;
                            coerce_type(func, r_ty, WasmType::I32);

                            // Remaining arguments
                            let expected_params = if fn_type.params.len() > 1 {
                                &fn_type.params[1..]
                            } else {
                                &[]
                            };
                            for (arg, &expected_ty) in args.iter().zip(expected_params.iter()) {
                                let actual_ty = compile_expr(
                                    &arg.value,
                                    func,
                                    locals,
                                    functions,
                                    control_stack,
                                    structs,
                                    static_strings,
                                    table_indices,
                                    anon_map,
                                    indirect_sigs,
                                    alloc_func_idx,
                                    async_helpers,
                                    struct_depth,
                                    call_depth,
                                )?;
                                coerce_type(func, actual_ty, expected_ty);
                            }

                            func.instruction(&Instruction::Call(func_idx));
                            if fn_type.results.is_empty() {
                                func.instruction(&Instruction::I64Const(0));
                                return Ok(WasmType::I64);
                            } else {
                                return Ok(fn_type.results[0]);
                            }
                        }
                    }
                }
            }

            // Case 1: Direct function call
            if let HirExpr::Identifier(func_name, _) = &**callee {
                if func_name == "String" && args.len() == 1 {
                    let (int_to_str_idx, _, _) = functions["__aipo_int_to_str"];
                    let (bool_to_str_idx, _, _) = functions["__aipo_bool_to_str"];
                    let arg = &args[0].value;
                    let arg_kind = infer_expr_kind(arg, locals, functions, table_indices, anon_map);
                    let arg_ty = compile_expr(
                        arg,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    match arg_kind {
                        LocalKind::String => {
                            coerce_type(func, arg_ty, WasmType::I32);
                        }
                        LocalKind::Int => {
                            coerce_type(func, arg_ty, WasmType::I64);
                            func.instruction(&Instruction::Call(int_to_str_idx));
                        }
                        LocalKind::Bool => {
                            coerce_type(func, arg_ty, WasmType::I32);
                            func.instruction(&Instruction::Call(bool_to_str_idx));
                        }
                        _ => {
                            if arg_ty == WasmType::I64 {
                                func.instruction(&Instruction::Call(int_to_str_idx));
                            } else {
                                coerce_type(func, arg_ty, WasmType::I32);
                            }
                        }
                    }
                    return Ok(WasmType::I32);
                }
                if func_name == "Int" && args.len() == 1 {
                    let (str_to_int_idx, _, _) = functions["__aipo_str_to_int"];
                    let arg = &args[0].value;
                    let arg_kind = infer_expr_kind(arg, locals, functions, table_indices, anon_map);
                    let arg_ty = compile_expr(
                        arg,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    match arg_kind {
                        LocalKind::Int => {
                            coerce_type(func, arg_ty, WasmType::I64);
                        }
                        LocalKind::Float => {
                            coerce_type(func, arg_ty, WasmType::F64);
                            func.instruction(&Instruction::I64TruncF64S);
                        }
                        LocalKind::Bool => {
                            coerce_type(func, arg_ty, WasmType::I32);
                            func.instruction(&Instruction::I64ExtendI32U);
                        }
                        LocalKind::String => {
                            coerce_type(func, arg_ty, WasmType::I32);
                            func.instruction(&Instruction::Call(str_to_int_idx));
                        }
                        _ => {
                            if arg_ty == WasmType::F64 {
                                func.instruction(&Instruction::I64TruncF64S);
                            } else if arg_ty == WasmType::I32 {
                                func.instruction(&Instruction::I64ExtendI32U);
                            } else {
                                coerce_type(func, arg_ty, WasmType::I64);
                            }
                        }
                    }
                    return Ok(WasmType::I64);
                }
                if func_name == "Float" && args.len() == 1 {
                    let (str_to_int_idx, _, _) = functions["__aipo_str_to_int"];
                    let arg = &args[0].value;
                    let arg_kind = infer_expr_kind(arg, locals, functions, table_indices, anon_map);
                    let arg_ty = compile_expr(
                        arg,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    match arg_kind {
                        LocalKind::Float => {
                            coerce_type(func, arg_ty, WasmType::F64);
                        }
                        LocalKind::Int => {
                            coerce_type(func, arg_ty, WasmType::I64);
                            func.instruction(&Instruction::F64ConvertI64S);
                        }
                        LocalKind::Bool => {
                            coerce_type(func, arg_ty, WasmType::I32);
                            func.instruction(&Instruction::F64ConvertI32S);
                        }
                        LocalKind::String => {
                            coerce_type(func, arg_ty, WasmType::I32);
                            func.instruction(&Instruction::Call(str_to_int_idx));
                            func.instruction(&Instruction::F64ConvertI64S);
                        }
                        _ => {
                            if arg_ty == WasmType::I64 {
                                func.instruction(&Instruction::F64ConvertI64S);
                            } else if arg_ty == WasmType::I32 {
                                func.instruction(&Instruction::F64ConvertI32S);
                            } else {
                                coerce_type(func, arg_ty, WasmType::F64);
                            }
                        }
                    }
                    return Ok(WasmType::F64);
                }
                if func_name == "Byte" && args.len() == 1 {
                    let (str_to_int_idx, _, _) = functions["__aipo_str_to_int"];
                    let arg = &args[0].value;
                    let arg_kind = infer_expr_kind(arg, locals, functions, table_indices, anon_map);
                    let arg_ty = compile_expr(
                        arg,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    match arg_kind {
                        LocalKind::Int => {
                            coerce_type(func, arg_ty, WasmType::I64);
                        }
                        LocalKind::Float => {
                            coerce_type(func, arg_ty, WasmType::F64);
                            func.instruction(&Instruction::I64TruncF64S);
                        }
                        LocalKind::String => {
                            coerce_type(func, arg_ty, WasmType::I32);
                            func.instruction(&Instruction::Call(str_to_int_idx));
                        }
                        _ => {
                            if arg_ty == WasmType::F64 {
                                func.instruction(&Instruction::I64TruncF64S);
                            } else if arg_ty == WasmType::I32 {
                                func.instruction(&Instruction::I64ExtendI32U);
                            } else {
                                coerce_type(func, arg_ty, WasmType::I64);
                            }
                        }
                    }
                    return Ok(WasmType::I64);
                }
                if func_name == "Bytes" && args.len() == 1 {
                    let (bytes_new_idx, _, _) = functions["__aipo_bytes_new"];
                    let len_ty = compile_expr(
                        &args[0].value,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, len_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(bytes_new_idx));
                    return Ok(WasmType::I32);
                }
                if !locals.contains_key(func_name) && functions.contains_key(func_name) {
                    let (func_idx, _, ref fn_type) = functions[func_name];
                    // Named arguments are reordered into declaration order before
                    // anything is evaluated. `scale(factor = 5, x = 4)` otherwise passed
                    // 5 as `x`, which is what made the result 25 instead of 20.
                    let ordered: Vec<Option<&HirCallArg>> = {
                        let names = functions.param_names.get(func_name);
                        (0..fn_type.params.len())
                            .map(|position| {
                                let by_name = names.and_then(|names| names.get(position)).and_then(
                                    |param_name| {
                                        args.iter().find(|arg| {
                                            arg.name.as_deref() == Some(param_name.as_str())
                                        })
                                    },
                                );
                                by_name.or_else(|| {
                                    // A positional argument keeps its written position,
                                    // counted over the unnamed prefix only.
                                    let positional_index =
                                        args.iter().take_while(|arg| arg.name.is_none()).count();
                                    if position < positional_index {
                                        args.get(position)
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect()
                    };
                    // Tolerate named/defaults arity mismatches by padding with zero defaults.
                    //
                    // A default may name an earlier parameter (`label(name, tag = name)`),
                    // so the parameters already passed become locals while the later
                    // defaults are compiled.
                    let nparams = fn_type.params.len();
                    let mut default_locals = locals.clone();
                    for (position, &expected_ty) in fn_type.params.iter().enumerate() {
                        match ordered.get(position).copied().flatten() {
                            Some(arg) => {
                                let actual_ty = compile_expr(
                                    &arg.value,
                                    func,
                                    &default_locals,
                                    functions,
                                    control_stack,
                                    structs,
                                    static_strings,
                                    table_indices,
                                    anon_map,
                                    indirect_sigs,
                                    alloc_func_idx,
                                    async_helpers,
                                    struct_depth,
                                    call_depth,
                                )?;
                                coerce_type(func, actual_ty, expected_ty);
                                if let Some(param_name) = functions
                                    .param_names
                                    .get(func_name)
                                    .and_then(|names| names.get(position))
                                {
                                    // A later default may name this parameter, so its value
                                    // is also kept in a scratch slot of the same
                                    // representation. The stack copy stays where the call
                                    // expects it.
                                    let slot = match expected_ty {
                                        WasmType::F64 => {
                                            locals[&format!("__f64_temp_{}", call_depth.min(7))].0
                                        }
                                        WasmType::I32 => {
                                            locals[&format!("__call_temp_{}", call_depth.min(7))].0
                                        }
                                        WasmType::I64 => {
                                            locals[&format!("__i64_temp_{}", call_depth.min(7))].0
                                        }
                                    };
                                    func.instruction(&Instruction::LocalTee(slot));
                                    default_locals.insert(
                                        param_name.clone(),
                                        (slot, expected_ty, LocalKind::Unknown),
                                    );
                                }
                            }
                            None => {
                                let default = functions
                                    .defaults
                                    .get(func_name)
                                    .and_then(|row| row.get(position))
                                    .and_then(|value| value.as_ref());
                                if let Some(default) = default {
                                    let d_ty = compile_expr(
                                        default,
                                        func,
                                        &default_locals,
                                        functions,
                                        control_stack,
                                        structs,
                                        static_strings,
                                        table_indices,
                                        anon_map,
                                        indirect_sigs,
                                        alloc_func_idx,
                                        async_helpers,
                                        struct_depth,
                                        call_depth,
                                    )?;
                                    coerce_type(func, d_ty, expected_ty);
                                    // A later default can name this parameter too
                                    // (`label(name = "anon", tag = name)`), so the value
                                    // this default produced stays reachable by name.
                                    if let Some(param_name) = functions
                                        .param_names
                                        .get(func_name)
                                        .and_then(|names| names.get(position))
                                    {
                                        let slot = match expected_ty {
                                            WasmType::F64 => {
                                                locals[&format!("__f64_temp_{}", call_depth.min(7))]
                                                    .0
                                            }
                                            WasmType::I32 => {
                                                locals
                                                    [&format!("__call_temp_{}", call_depth.min(7))]
                                                    .0
                                            }
                                            WasmType::I64 => {
                                                locals[&format!("__i64_temp_{}", call_depth.min(7))]
                                                    .0
                                            }
                                        };
                                        func.instruction(&Instruction::LocalTee(slot));
                                        default_locals.insert(
                                            param_name.clone(),
                                            (slot, expected_ty, LocalKind::Unknown),
                                        );
                                    }
                                } else {
                                    match expected_ty {
                                        WasmType::F64 => {
                                            func.instruction(&Instruction::F64Const(0.0.into()));
                                        }
                                        WasmType::I32 => {
                                            func.instruction(&Instruction::I32Const(0));
                                        }
                                        WasmType::I64 => {
                                            func.instruction(&Instruction::I64Const(0));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // If too many args, drop extras.
                    for arg in args.iter().skip(nparams) {
                        let at = compile_expr(
                            &arg.value,
                            func,
                            locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth,
                            call_depth,
                        )?;
                        let _ = at;
                        func.instruction(&Instruction::Drop);
                    }
                    func.instruction(&Instruction::Call(func_idx));
                    // A function with no results pushes nothing, and the caller's
                    // `Drop` (or any consumer) would then read the stack as if a
                    // value were there. Materialize the unit value the rest of the
                    // backend expects so call sites stay uniform.
                    return Ok(match fn_type.results.first().copied() {
                        Some(ty) => ty,
                        None => {
                            func.instruction(&Instruction::I64Const(0));
                            WasmType::I64
                        }
                    });
                }
            }

            // Case 2: Indirect Call (call_indirect) via Table 0
            let temp_name = format!("__call_temp_{}", call_depth.min(7));
            let call_temp = locals[&temp_name].0;
            let callee_ty = compile_expr(
                callee,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth + 1,
            )?;
            coerce_type(func, callee_ty, WasmType::I32);
            func.instruction(&Instruction::LocalSet(call_temp));

            // Determine target signature
            let (type_idx, expected_params, ret_ty) =
                if let HirExpr::Identifier(name, _) = &**callee {
                    if let Some((_, _, LocalKind::Fn(t_idx))) = locals.get(name) {
                        let fn_sig = &functions
                            .values()
                            .find(|(_, idx, _)| idx == t_idx)
                            .unwrap()
                            .2;
                        (
                            *t_idx,
                            fn_sig.params.clone(),
                            fn_sig.results.first().copied(),
                        )
                    } else if let Some((_, t_idx, fn_sig)) = functions.get(name) {
                        (
                            *t_idx,
                            fn_sig.params.clone(),
                            fn_sig.results.first().copied(),
                        )
                    } else {
                        let params = vec![WasmType::I64; args.len()];
                        let ret = Some(WasmType::I64);
                        let t_idx = indirect_sigs.get(&args.len()).copied().unwrap_or(0);
                        (t_idx, params, ret)
                    }
                } else if let HirExpr::Fn(func_expr) = &**callee {
                    let anon_name = &anon_map[&func_expr.span];
                    let (_, t_idx, fn_sig) = &functions[anon_name];
                    (
                        *t_idx,
                        fn_sig.params.clone(),
                        fn_sig.results.first().copied(),
                    )
                } else {
                    let params = vec![WasmType::I64; args.len()];
                    let ret = Some(WasmType::I64);
                    let t_idx = indirect_sigs.get(&args.len()).copied().unwrap_or(0);
                    (t_idx, params, ret)
                };

            // Evaluate arguments onto stack
            for (arg, expected_ty) in args.iter().zip(
                expected_params
                    .iter()
                    .chain(std::iter::repeat(&WasmType::I64)),
            ) {
                let actual_ty = compile_expr(
                    &arg.value,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth + 1,
                )?;
                coerce_type(func, actual_ty, *expected_ty);
            }

            // Push table index and emit CallIndirect
            func.instruction(&Instruction::LocalGet(call_temp));
            func.instruction(&Instruction::CallIndirect {
                type_index: type_idx,
                table_index: 0,
            });
            Ok(ret_ty.unwrap_or(WasmType::I64))
        }
        HirExpr::Dict(entries, _span) => {
            let (dict_new_idx, _, _) = functions["__aipo_dict_new"];
            let (dict_set_idx, _, _) = functions["__aipo_dict_set"];
            let (string_hash_idx, _, _) = functions["__aipo_string_hash"];

            let dict_temp_name = format!("__dict_temp_{}", struct_depth.min(7));
            let dict_temp = locals[&dict_temp_name].0;

            if entries.is_empty() {
                func.instruction(&Instruction::I32Const(4));
                func.instruction(&Instruction::Call(dict_new_idx));
                func.instruction(&Instruction::LocalSet(dict_temp));
                func.instruction(&Instruction::LocalGet(dict_temp));
                return Ok(WasmType::I32);
            }

            // Reserve one slot per entry; over-allocating is safe since the
            // entry count tracks actual use.
            func.instruction(&Instruction::I32Const(entries.len() as i32));
            func.instruction(&Instruction::Call(dict_new_idx));
            func.instruction(&Instruction::LocalSet(dict_temp));

            for (key, value) in entries {
                func.instruction(&Instruction::LocalGet(dict_temp));

                // The entry keeps both keys: the hash lookup compares, and the raw
                // value is what `keys()` and dict iteration hand back.
                compile_dict_key_pair(
                    key,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                let _ = (string_hash_idx,);

                let val_ty = compile_expr(
                    value,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, val_ty, WasmType::I64);
                func.instruction(&Instruction::Call(dict_set_idx));
                func.instruction(&Instruction::Drop);
            }

            func.instruction(&Instruction::LocalGet(dict_temp));
            Ok(WasmType::I32)
        }
        HirExpr::Await(inner, _span) => {
            // Compile the inner expression — should produce task_ptr (I32)
            let inner_ty = compile_expr(
                inner,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            // Coerce to I32 if needed (task handle is an I32 pointer)
            coerce_type(func, inner_ty, WasmType::I32);
            // Call __aipo_await(task_ptr) -> i64
            func.instruction(&Instruction::Call(async_helpers.await_idx));
            Ok(WasmType::I64)
        }
        HirExpr::Construct(type_name, fields, span) => {
            if let Some(struct_layout) = structs.get(type_name) {
                let temp_name = format!("__struct_temp_{}", struct_depth.min(7));
                let struct_temp = locals[&temp_name].0;

                // Allocate memory block for instance
                func.instruction(&Instruction::I32Const(struct_layout.size as i32));
                func.instruction(&Instruction::Call(alloc_func_idx));
                func.instruction(&Instruction::LocalSet(struct_temp));

                // If type_name is an enum variant, record the variant name string pointer at offset 0
                if let Some(&str_ptr) = static_strings.get(type_name) {
                    func.instruction(&Instruction::LocalGet(struct_temp));
                    func.instruction(&Instruction::I32Const(str_ptr));
                    func.instruction(&Instruction::I32Store(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                }

                // Initialize fields
                for (i, (maybe_name, field_expr)) in fields.iter().enumerate() {
                    let (field_offset, field_ty) = if let Some(name) = maybe_name {
                        struct_layout
                            .fields
                            .get(name)
                            .copied()
                            .unwrap_or(((i * 8) as u32, WasmType::I64))
                    } else {
                        // Positional initialization
                        ((i * 8) as u32, WasmType::I64)
                    };

                    func.instruction(&Instruction::LocalGet(struct_temp));
                    let val_ty = compile_expr(
                        field_expr,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth + 1,
                        call_depth,
                    )?;
                    coerce_type(func, val_ty, field_ty);
                    match field_ty {
                        WasmType::F64 => {
                            func.instruction(&Instruction::F64Store(MemArg {
                                offset: field_offset as u64,
                                align: 3,
                                memory_index: 0,
                            }));
                        }
                        WasmType::I32 => {
                            func.instruction(&Instruction::I32Store(MemArg {
                                offset: field_offset as u64,
                                align: 2,
                                memory_index: 0,
                            }));
                        }
                        WasmType::I64 => {
                            func.instruction(&Instruction::I64Store(MemArg {
                                offset: field_offset as u64,
                                align: 3,
                                memory_index: 0,
                            }));
                        }
                    }
                }

                // Canon: `Type{...}` runs `init` when the type declares one. The instance
                // is built first so `init` observes and overwrites the declared fields.
                //
                // Each argument is evaluated once into a scratch slot before the call. A
                // default may name an earlier parameter (`mirror = value`), so the
                // parameter names are in scope while defaults are compiled.
                let init_name = format!("{type_name}.init");
                if let Some(&(init_idx, _, ref init_sig)) = functions.get(&init_name) {
                    let init_sig = init_sig.clone();
                    let mut arg_locals = locals.clone();
                    let mut staged: Vec<Option<u32>> = Vec::new();
                    // Construction entries may be named and out of order, so a parameter
                    // resolves by its own name first and only then by declaration
                    // position. Reading by position alone passed `name`'s expression as
                    // `id` when the caller wrote `T{name = "bo", id = 2}`.
                    let init_param_names = functions
                        .param_names
                        .get(&init_name)
                        .cloned()
                        .unwrap_or_default();
                    for (position, &expected_ty) in init_sig.params.iter().enumerate().skip(1) {
                        let named = init_param_names.get(position).and_then(|param_name| {
                            fields.iter().find_map(|(field_name, expression)| {
                                (field_name.as_deref() == Some(param_name.as_str()))
                                    .then_some(expression)
                            })
                        });
                        let written = named
                            .or_else(|| fields.get(position - 1).map(|(_, expression)| expression));
                        let default = functions
                            .defaults
                            .get(&init_name)
                            .and_then(|row| row.get(position))
                            .and_then(|value| value.as_ref());
                        let Some(expression) = written.or(default) else {
                            staged.push(None);
                            continue;
                        };
                        let arg_ty = compile_expr(
                            expression,
                            func,
                            &arg_locals,
                            functions,
                            control_stack,
                            structs,
                            static_strings,
                            table_indices,
                            anon_map,
                            indirect_sigs,
                            alloc_func_idx,
                            async_helpers,
                            struct_depth + 1,
                            call_depth,
                        )?;
                        coerce_type(func, arg_ty, expected_ty);
                        // Each argument needs its own slot: sharing one would let a later
                        // argument overwrite an earlier one before the call.
                        let scratch =
                            locals[&format!("__i64_temp_{}", (call_depth + position).min(7))].0;
                        func.instruction(&Instruction::LocalSet(scratch));
                        staged.push(Some(scratch));
                        arg_locals.insert(
                            functions
                                .param_names
                                .get(&init_name)
                                .and_then(|names| names.get(position))
                                .cloned()
                                .unwrap_or_default(),
                            (scratch, expected_ty, LocalKind::Unknown),
                        );
                    }

                    // Receiver first, then every argument in declaration order.
                    func.instruction(&Instruction::LocalGet(struct_temp));
                    for (position, &expected_ty) in init_sig.params.iter().enumerate().skip(1) {
                        match staged[position - 1] {
                            Some(slot) => {
                                func.instruction(&Instruction::LocalGet(slot));
                            }
                            None => match expected_ty {
                                WasmType::F64 => {
                                    func.instruction(&Instruction::F64Const(0.0.into()));
                                }
                                WasmType::I32 => {
                                    func.instruction(&Instruction::I32Const(0));
                                }
                                WasmType::I64 => {
                                    func.instruction(&Instruction::I64Const(0));
                                }
                            },
                        }
                    }
                    func.instruction(&Instruction::Call(init_idx));
                    if !init_sig.results.is_empty() {
                        func.instruction(&Instruction::Drop);
                    }
                }

                // Canon verifies `invariant()` at the end of construction, observing
                // exactly what `init` assigned. A violated invariant traps, because the
                // instance must never be published in a state canon forbids.
                let invariant_name = format!("{type_name}.invariant");
                if let Some(&(inv_idx, _, _)) = functions.get(&invariant_name) {
                    func.instruction(&Instruction::LocalGet(struct_temp));
                    func.instruction(&Instruction::Call(inv_idx));
                    func.instruction(&Instruction::I32Eqz);
                    func.instruction(&Instruction::If(BlockType::Empty));
                    func.instruction(&Instruction::Unreachable);
                    func.instruction(&Instruction::End);
                }

                // Return instance pointer
                func.instruction(&Instruction::LocalGet(struct_temp));
                Ok(WasmType::I32)
            } else {
                Err(WasmCompileError::UnsupportedExpr {
                    message: format!("unknown struct type `{type_name}`"),
                    span: *span,
                })
            }
        }
        HirExpr::Dot(receiver, field_name, span) => {
            if let HirExpr::Identifier(rec_name, _) = &**receiver {
                let candidate_variant = format!("{rec_name}.{field_name}");
                if let Some(variant_layout) = structs.get(&candidate_variant)
                    && let Some(&str_ptr) = static_strings.get(&candidate_variant)
                {
                    let temp_name = format!("__struct_temp_{}", struct_depth.min(7));
                    let struct_temp = locals[&temp_name].0;

                    func.instruction(&Instruction::I32Const(variant_layout.size as i32));
                    func.instruction(&Instruction::Call(alloc_func_idx));
                    func.instruction(&Instruction::LocalSet(struct_temp));

                    func.instruction(&Instruction::LocalGet(struct_temp));
                    func.instruction(&Instruction::I32Const(str_ptr));
                    func.instruction(&Instruction::I32Store(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    func.instruction(&Instruction::LocalGet(struct_temp));
                    return Ok(WasmType::I32);
                }
            }

            let receiver_struct_name = match &**receiver {
                HirExpr::Identifier(name, _) => {
                    locals.get(name).and_then(|(_, _, kind)| match kind {
                        LocalKind::Struct(s_name) => Some(s_name.as_str()),
                        _ => None,
                    })
                }
                _ => None,
            };

            // The kind comes from `infer_expr_kind` rather than from `locals` alone,
            // because a module binding lives in a global and a call result has no
            // local at all. Reading only `locals` classified `w.len()` on a
            // module-scope string as a byte length.
            let is_string_receiver = matches!(
                infer_expr_kind(receiver, locals, functions, table_indices, anon_map),
                LocalKind::String
            );

            if field_name == "len"
                && (is_string_receiver
                    || (receiver_struct_name.is_none()
                        && !structs.values().any(|s| s.fields.contains_key("len"))))
            {
                // Lists and Ranges expose their element count through the
                // runtime helpers rather than the static string length path.
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(receiver_kind, LocalKind::List(_)) {
                    let (list_len_idx, _, _) = functions["__aipo_list_len"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(list_len_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::Dict(_)) {
                    let (dict_len_idx, _, _) = functions["__aipo_dict_len"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(dict_len_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::Range) {
                    let (range_len_idx, _, _) = functions["__aipo_range_len"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(range_len_idx));
                    return Ok(WasmType::I64);
                }
                if matches!(receiver_kind, LocalKind::Bytes) {
                    let (bytes_len_idx, _, _) = functions["__aipo_bytes_len"];
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(bytes_len_idx));
                    return Ok(WasmType::I64);
                }

                let r_ty = compile_expr(
                    receiver,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, r_ty, WasmType::I32);
                if is_string_receiver {
                    // `len` on a `String` counts code points, not UTF-8 bytes;
                    // the pool header stores the byte length.
                    let (str_len_idx, _, _) = functions["__aipo_str_len"];
                    func.instruction(&Instruction::Call(str_len_idx));
                    return Ok(WasmType::I64);
                }
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::I64ExtendI32U);
                Ok(WasmType::I64)
            } else {
                let field_info = if let Some(s_name) = receiver_struct_name {
                    structs
                        .get(s_name)
                        .and_then(|s| s.fields.get(field_name).copied())
                } else {
                    structs
                        .values()
                        .find_map(|s| s.fields.get(field_name).copied())
                };

                if let Some((field_offset, ty)) = field_info {
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    match ty {
                        WasmType::F64 => {
                            func.instruction(&Instruction::F64Load(MemArg {
                                offset: field_offset as u64,
                                align: 3,
                                memory_index: 0,
                            }));
                            Ok(WasmType::F64)
                        }
                        WasmType::I32 => {
                            func.instruction(&Instruction::I32Load(MemArg {
                                offset: field_offset as u64,
                                align: 2,
                                memory_index: 0,
                            }));
                            Ok(WasmType::I32)
                        }
                        WasmType::I64 => {
                            func.instruction(&Instruction::I64Load(MemArg {
                                offset: field_offset as u64,
                                align: 3,
                                memory_index: 0,
                            }));
                            Ok(WasmType::I64)
                        }
                    }
                } else if field_name == "message" {
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    Ok(WasmType::I32)
                } else if field_name == "total_seconds" {
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::F64);
                    Ok(WasmType::F64)
                } else if field_name == "wait" || field_name == "all" || field_name == "spawn" {
                    let r_ty = compile_expr(
                        receiver,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, r_ty, WasmType::I32);
                    let (list_new_idx, _, _) = functions["__aipo_list_new"];
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::Call(list_new_idx));
                    Ok(WasmType::I32)
                } else {
                    Err(WasmCompileError::UnsupportedExpr {
                        message: format!("unknown field `{field_name}` on struct"),
                        span: *span,
                    })
                }
            }
        }
        HirExpr::OrElse(left, right, _) => {
            // `a or_else b`: evaluate `a`; if a failure is pending, clear the
            // flag and evaluate `b` instead.
            //
            // Both branches store through an i64 scratch local and agree on an
            // i64 result on the stack, so the `if` type-checks regardless of
            // which branch runs.
            let result_ty = infer_expr_type(left, locals, functions, structs, table_indices)?;
            let fg = async_helpers.fail_globals;
            // Staging slot for the primary value. Indexed by call nesting, which
            // bounds how deeply `or_else` can appear inside itself; using a
            // struct-nesting counter here could exceed the eight allocated slots
            // and reference a local the function never declared.
            let or_else_depth = call_depth;

            let l_ty = compile_expr(
                left,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_type(func, l_ty, WasmType::I64);

            // Save the primary value, then branch on the failure flag.
            func.instruction(&Instruction::LocalSet(
                locals[&format!("__or_else_temp_{}", or_else_depth.min(7))].0,
            ));
            func.instruction(&Instruction::GlobalGet(fg.status_idx));
            func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));

            // Failure pending: clear the flag and use the fallback.
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::GlobalSet(fg.status_idx));
            let r_ty = compile_expr(
                right,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_type(func, r_ty, WasmType::I64);

            func.instruction(&Instruction::Else);
            // No failure: use the saved primary value.
            func.instruction(&Instruction::LocalGet(
                locals[&format!("__or_else_temp_{}", or_else_depth.min(7))].0,
            ));
            func.instruction(&Instruction::End);

            // Both branches produced i64; narrow to the inferred result type.
            coerce_type(func, WasmType::I64, result_ty);
            Ok(result_ty)
        }
        HirExpr::List(elements, _span) => {
            let (list_new_idx, _, _) = functions["__aipo_list_new"];
            let (list_set_idx, _, _) = functions["__aipo_list_set"];

            let list_temp_name = format!("__list_temp_{}", struct_depth.min(7));
            let list_temp = locals[&list_temp_name].0;

            if elements.is_empty() {
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::Call(list_new_idx));
                func.instruction(&Instruction::LocalSet(list_temp));
                func.instruction(&Instruction::LocalGet(list_temp));
                return Ok(WasmType::I32);
            }

            // Determine the element type so every slot can be coerced uniformly.
            let _elem_ty = elements
                .iter()
                .map(|e| infer_expr_type(e, locals, functions, structs, table_indices))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .reduce(|acc, ty| {
                    if acc == WasmType::F64 || ty == WasmType::F64 {
                        WasmType::F64
                    } else {
                        acc
                    }
                })
                .unwrap_or(WasmType::I64);

            func.instruction(&Instruction::I32Const(elements.len() as i32));
            func.instruction(&Instruction::Call(list_new_idx));
            func.instruction(&Instruction::LocalSet(list_temp));

            for (i, element) in elements.iter().enumerate() {
                func.instruction(&Instruction::LocalGet(list_temp));
                func.instruction(&Instruction::I32Const(i as i32));
                let val_ty = compile_expr(
                    element,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, val_ty, WasmType::I64);
                func.instruction(&Instruction::Call(list_set_idx));
                func.instruction(&Instruction::Drop);
            }

            func.instruction(&Instruction::LocalGet(list_temp));
            Ok(WasmType::I32)
        }
        HirExpr::Index(target, index, span) => {
            let target_kind = infer_expr_kind(target, locals, functions, table_indices, anon_map);

            // A Range index reads a bound; a Dict index looks the key up.
            if matches!(target_kind, LocalKind::Range) {
                let t_ty = compile_expr(
                    target,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, t_ty, WasmType::I32);
                let i_ty = compile_expr(
                    index,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, i_ty, WasmType::I32);
                let (range_get_idx, _, _) = functions["__aipo_range_get"];
                func.instruction(&Instruction::Call(range_get_idx));
                return Ok(WasmType::I64);
            }

            if matches!(target_kind, LocalKind::Dict(_)) {
                let t_ty = compile_expr(
                    target,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, t_ty, WasmType::I32);

                let index_kind = infer_expr_kind(index, locals, functions, table_indices, anon_map);
                if matches!(index_kind, LocalKind::String) {
                    let i_ty = compile_expr(
                        index,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, i_ty, WasmType::I32);
                    let (string_hash_idx, _, _) = functions["__aipo_string_hash"];
                    func.instruction(&Instruction::Call(string_hash_idx));
                } else {
                    let i_ty = compile_expr(
                        index,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, i_ty, WasmType::I64);
                }

                let (dict_get_idx, _, _) = functions["__aipo_dict_get"];
                func.instruction(&Instruction::Call(dict_get_idx));
                return Ok(WasmType::I64);
            }

            if matches!(target_kind, LocalKind::Bytes) {
                // `bytes[a..b]` yields a view over a byte window; `bytes[i]` reads
                // one byte. Canon treats both as Bytes operations, so the range
                // form must not fall through to the integer index path.
                if let HirExpr::Binary(BinaryOp::Range, start, end, _) = &**index {
                    let t_ty = compile_expr(
                        target,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, t_ty, WasmType::I32);
                    let (bytes_slice_idx, _, _) = functions["__aipo_bytes_slice"];
                    let s_ty = compile_expr(
                        start,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, s_ty, WasmType::I32);
                    let e_ty = compile_expr(
                        end,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, e_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(bytes_slice_idx));
                    return Ok(WasmType::I32);
                }
                let t_ty = compile_expr(
                    target,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, t_ty, WasmType::I32);
                let i_ty = compile_expr(
                    index,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, i_ty, WasmType::I32);
                let (bytes_get_idx, _, _) = functions["__aipo_bytes_get"];
                func.instruction(&Instruction::Call(bytes_get_idx));
                return Ok(WasmType::I64);
            }

            // A `String` indexed by a code point yields a one-character string, and
            // a range yields a substring. Both are value operations on the pool.
            if matches!(target_kind, LocalKind::String) {
                let is_range = matches!(&**index, HirExpr::Binary(BinaryOp::Range, ..));
                let t_ty = compile_expr(
                    target,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, t_ty, WasmType::I32);
                if let HirExpr::Binary(BinaryOp::Range, start, end, _) = &**index {
                    let (str_slice_idx, _, _) = functions["__aipo_str_slice"];
                    let s_ty = compile_expr(
                        start,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, s_ty, WasmType::I32);
                    let e_ty = compile_expr(
                        end,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    // An omitted end bound is the largest safe integer, which does
                    // not fit `i32`; saturate so the helper clamps to the length.
                    coerce_type(func, e_ty, WasmType::I64);
                    let bound_temp = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
                    func.instruction(&Instruction::LocalSet(bound_temp));
                    func.instruction(&Instruction::LocalGet(bound_temp));
                    func.instruction(&Instruction::I64Const(i64::from(i32::MAX)));
                    func.instruction(&Instruction::I64GtS);
                    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                    func.instruction(&Instruction::I32Const(i32::MAX));
                    func.instruction(&Instruction::Else);
                    func.instruction(&Instruction::LocalGet(bound_temp));
                    func.instruction(&Instruction::I32WrapI64);
                    func.instruction(&Instruction::End);
                    func.instruction(&Instruction::Call(str_slice_idx));
                } else {
                    let (str_char_idx, _, _) = functions["__aipo_str_char"];
                    let i_ty = compile_expr(
                        index,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth,
                        call_depth,
                    )?;
                    coerce_type(func, i_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(str_char_idx));
                }
                let _ = is_range;
                return Ok(WasmType::I32);
            }

            // A `List` sliced by a range yields a new list. Falling through to the
            // integer index path would read `list_get(list, <range pointer>)` and
            // trap, so the range form is handled before the element read.
            if matches!(target_kind, LocalKind::List(_))
                && let HirExpr::Binary(BinaryOp::Range, start, end, _) = &**index
            {
                let t_ty = compile_expr(
                    target,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, t_ty, WasmType::I32);
                let (list_slice_idx, _, _) = functions["__aipo_list_slice"];
                let s_ty = compile_expr(
                    start,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                coerce_type(func, s_ty, WasmType::I32);
                let e_ty = compile_expr(
                    end,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth,
                )?;
                // An omitted end bound is the largest safe integer, which does not
                // fit `i32`: saturate so the slice helper clamps to the length
                // instead of reading a wrapped negative index.
                coerce_type(func, e_ty, WasmType::I64);
                let bound_temp = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
                func.instruction(&Instruction::LocalSet(bound_temp));
                func.instruction(&Instruction::LocalGet(bound_temp));
                func.instruction(&Instruction::I64Const(i64::from(i32::MAX)));
                func.instruction(&Instruction::I64GtS);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                func.instruction(&Instruction::I32Const(i32::MAX));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::LocalGet(bound_temp));
                func.instruction(&Instruction::I32WrapI64);
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::Call(list_slice_idx));
                return Ok(WasmType::I32);
            }

            let _ = span;
            let t_ty = compile_expr(
                target,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_type(func, t_ty, WasmType::I32);
            let i_ty = compile_expr(
                index,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth,
            )?;
            coerce_type(func, i_ty, WasmType::I32);
            let (list_get_idx, _, _) = functions["__aipo_list_get"];
            func.instruction(&Instruction::Call(list_get_idx));
            Ok(WasmType::I64)
        }
        HirExpr::Try(inner, _) => {
            // `expr?` reads the operand once, then propagates a pending failure to
            // the nearest handler. The staging local must match the operand's own
            // representation: storing an `i64` in an `i32` scratch produces a module
            // the validator rejects.
            let produced = infer_expr_type(inner, locals, functions, structs, table_indices)?;
            if produced == WasmType::F64 {
                // A float result is staged by its bits so a single `i64` slot serves
                // every representation.
                let temp = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
                let inner_ty = compile_expr(
                    inner,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth + 1,
                )?;
                coerce_type(func, inner_ty, WasmType::F64);
                func.instruction(&Instruction::I64ReinterpretF64);
                func.instruction(&Instruction::LocalSet(temp));
                emit_failure_check(
                    func,
                    control_stack,
                    async_helpers.fail_globals,
                    async_helpers.function_return,
                );
                func.instruction(&Instruction::LocalGet(temp));
                func.instruction(&Instruction::F64ReinterpretI64);
                return Ok(WasmType::F64);
            }
            let temp = locals[&format!("__i64_temp_{}", call_depth.min(7))].0;
            let inner_ty = compile_expr(
                inner,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth + 1,
            )?;
            coerce_type(func, inner_ty, WasmType::I64);
            func.instruction(&Instruction::LocalSet(temp));
            emit_failure_check(
                func,
                control_stack,
                async_helpers.fail_globals,
                async_helpers.function_return,
            );
            func.instruction(&Instruction::LocalGet(temp));
            Ok(WasmType::I64)
        }
        HirExpr::QuestionDot(base, member, _) => {
            let base_temp_name = format!("__call_temp_{}", call_depth.min(7));
            let base_temp = locals[&base_temp_name].0;
            let base_ty = compile_expr(
                base,
                func,
                locals,
                functions,
                control_stack,
                structs,
                static_strings,
                table_indices,
                anon_map,
                indirect_sigs,
                alloc_func_idx,
                async_helpers,
                struct_depth,
                call_depth + 1,
            )?;
            coerce_type(func, base_ty, WasmType::I32);
            func.instruction(&Instruction::LocalSet(base_temp));
            func.instruction(&Instruction::LocalGet(base_temp));
            func.instruction(&Instruction::I32Eqz);
            func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
            func.instruction(&Instruction::I64Const(0));
            func.instruction(&Instruction::Else);

            let base_kind = infer_expr_kind(base, locals, functions, table_indices, anon_map);
            let field_info = match base_kind {
                LocalKind::Struct(ref s_name) => structs
                    .get(s_name)
                    .and_then(|s| s.fields.get(member).copied()),
                _ => structs.values().find_map(|s| s.fields.get(member).copied()),
            };

            if let Some((field_offset, field_ty)) = field_info {
                func.instruction(&Instruction::LocalGet(base_temp));
                match field_ty {
                    WasmType::I32 => {
                        func.instruction(&Instruction::I32Load(MemArg {
                            offset: field_offset as u64,
                            align: 2,
                            memory_index: 0,
                        }));
                        func.instruction(&Instruction::I64ExtendI32U);
                    }
                    WasmType::F64 => {
                        func.instruction(&Instruction::F64Load(MemArg {
                            offset: field_offset as u64,
                            align: 3,
                            memory_index: 0,
                        }));
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                    _ => {
                        func.instruction(&Instruction::I64Load(MemArg {
                            offset: field_offset as u64,
                            align: 3,
                            memory_index: 0,
                        }));
                    }
                }
            } else {
                func.instruction(&Instruction::I64Const(0));
            }
            func.instruction(&Instruction::End);
            Ok(WasmType::I64)
        }
        HirExpr::With(base, updates, span) => {
            let base_kind = infer_expr_kind(base, locals, functions, table_indices, anon_map);
            let s_name = match base_kind {
                LocalKind::Struct(ref name) => Some(name.clone()),
                _ => None,
            };
            let struct_layout = if let Some(ref name) = s_name {
                structs.get(name).cloned()
            } else {
                structs
                    .values()
                    .find(|layout| updates.iter().all(|(f, _)| layout.fields.contains_key(f)))
                    .cloned()
            };

            if let Some(struct_layout) = struct_layout {
                let base_temp_name = format!("__call_temp_{}", call_depth.min(7));
                let base_temp = locals[&base_temp_name].0;
                let target_temp_name = format!("__struct_temp_{}", struct_depth.min(7));
                let target_temp = locals[&target_temp_name].0;

                let base_ty = compile_expr(
                    base,
                    func,
                    locals,
                    functions,
                    control_stack,
                    structs,
                    static_strings,
                    table_indices,
                    anon_map,
                    indirect_sigs,
                    alloc_func_idx,
                    async_helpers,
                    struct_depth,
                    call_depth + 1,
                )?;
                coerce_type(func, base_ty, WasmType::I32);
                func.instruction(&Instruction::LocalSet(base_temp));

                func.instruction(&Instruction::I32Const(struct_layout.size as i32));
                func.instruction(&Instruction::Call(alloc_func_idx));
                func.instruction(&Instruction::LocalSet(target_temp));

                let num_words = struct_layout.size.div_ceil(8);
                for word in 0..num_words {
                    let offset = (word * 8) as u64;
                    func.instruction(&Instruction::LocalGet(target_temp));
                    func.instruction(&Instruction::LocalGet(base_temp));
                    func.instruction(&Instruction::I64Load(MemArg {
                        offset,
                        align: 3,
                        memory_index: 0,
                    }));
                    func.instruction(&Instruction::I64Store(MemArg {
                        offset,
                        align: 3,
                        memory_index: 0,
                    }));
                }

                for (field_name, field_expr) in updates {
                    let (field_offset, field_ty) = struct_layout
                        .fields
                        .get(field_name)
                        .copied()
                        .unwrap_or((0, WasmType::I64));

                    func.instruction(&Instruction::LocalGet(target_temp));
                    let val_ty = compile_expr(
                        field_expr,
                        func,
                        locals,
                        functions,
                        control_stack,
                        structs,
                        static_strings,
                        table_indices,
                        anon_map,
                        indirect_sigs,
                        alloc_func_idx,
                        async_helpers,
                        struct_depth + 1,
                        call_depth,
                    )?;
                    coerce_type(func, val_ty, field_ty);

                    match field_ty {
                        WasmType::F64 => {
                            func.instruction(&Instruction::F64Store(MemArg {
                                offset: field_offset as u64,
                                align: 3,
                                memory_index: 0,
                            }));
                        }
                        WasmType::I32 => {
                            func.instruction(&Instruction::I32Store(MemArg {
                                offset: field_offset as u64,
                                align: 2,
                                memory_index: 0,
                            }));
                        }
                        _ => {
                            func.instruction(&Instruction::I64Store(MemArg {
                                offset: field_offset as u64,
                                align: 3,
                                memory_index: 0,
                            }));
                        }
                    }
                }

                func.instruction(&Instruction::LocalGet(target_temp));
                Ok(WasmType::I32)
            } else {
                Err(WasmCompileError::UnsupportedExpr {
                    message: "cannot determine struct layout for `with` expression".to_string(),
                    span: *span,
                })
            }
        }
    }
}

/// Emits an arithmetic binary operation on top of stack.
fn emit_binary_op(
    func: &mut Function,
    op: BinaryOp,
    target_ty: WasmType,
    expr_ty: WasmType,
    span: SourceSpan,
) -> Result<(), WasmCompileError> {
    coerce_type(func, expr_ty, target_ty);
    match (op, target_ty) {
        (BinaryOp::Add, WasmType::F64) => func.instruction(&Instruction::F64Add),
        (BinaryOp::Add, _) => func.instruction(&Instruction::I64Add),
        (BinaryOp::Sub, WasmType::F64) => func.instruction(&Instruction::F64Sub),
        (BinaryOp::Sub, _) => func.instruction(&Instruction::I64Sub),
        (BinaryOp::Mul, WasmType::F64) => func.instruction(&Instruction::F64Mul),
        (BinaryOp::Mul, _) => func.instruction(&Instruction::I64Mul),
        (BinaryOp::IntDiv, _) => func.instruction(&Instruction::I64DivS),
        (BinaryOp::Div, _) => func.instruction(&Instruction::F64Div),
        (BinaryOp::Mod, _) => func.instruction(&Instruction::I64RemS),
        _ => {
            return Err(WasmCompileError::UnsupportedExpr {
                message: format!("compound assignment with `{op:?}` not supported"),
                span,
            });
        }
    };
    Ok(())
}

/// Emits `left and right` / `left or right` with short-circuit semantics.
///
/// The right operand is only evaluated when the result depends on it:
/// - `and` evaluates `right` only when `left` is true.
/// - `or` evaluates `right` only when `left` is false.
///
/// The result is a normalized `i32` boolean (0 or 1).
#[allow(clippy::too_many_arguments)]
fn compile_logical_short_circuit(
    left: &HirExpr,
    right: &HirExpr,
    func: &mut Function,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    control_stack: &mut Vec<ControlFrame>,
    structs: &HashMap<String, StructLayout>,
    static_strings: &HashMap<String, i32>,
    table_indices: &HashMap<String, u32>,
    anon_map: &HashMap<SourceSpan, String>,
    indirect_sigs: &HashMap<usize, u32>,
    alloc_func_idx: u32,
    async_helpers: AsyncHelpers,
    struct_depth: usize,
    call_depth: usize,
    is_or: bool,
) -> Result<(), WasmCompileError> {
    // Evaluate the left operand, normalized to 0/1.
    let l_ty = compile_expr(
        left,
        func,
        locals,
        functions,
        control_stack,
        structs,
        static_strings,
        table_indices,
        anon_map,
        indirect_sigs,
        alloc_func_idx,
        async_helpers,
        struct_depth,
        call_depth,
    )?;
    coerce_to_bool(func, l_ty);

    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    if is_or {
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::Else);
        let r_ty = compile_expr(
            right,
            func,
            locals,
            functions,
            control_stack,
            structs,
            static_strings,
            table_indices,
            anon_map,
            indirect_sigs,
            alloc_func_idx,
            async_helpers,
            struct_depth,
            call_depth,
        )?;
        coerce_to_bool(func, r_ty);
    } else {
        let r_ty = compile_expr(
            right,
            func,
            locals,
            functions,
            control_stack,
            structs,
            static_strings,
            table_indices,
            anon_map,
            indirect_sigs,
            alloc_func_idx,
            async_helpers,
            struct_depth,
            call_depth,
        )?;
        coerce_to_bool(func, r_ty);
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::I32Const(0));
    }
    func.instruction(&Instruction::End);
    Ok(())
}

/// Emits a statement-boundary failure check.
///
/// When the failure status global is non-zero the function either branches to
/// the nearest enclosing `attempt` handler, or - when there is none - returns
/// immediately with a zero-valued dummy of the expected return type. This
/// mirrors the stack VM's `CheckFailure` opcode together with its handler stack.
fn emit_failure_check(
    func: &mut Function,
    control_stack: &[ControlFrame],
    fail_globals: FailureGlobals,
    return_type: Option<WasmType>,
) {
    func.instruction(&Instruction::GlobalGet(fail_globals.status_idx));
    func.instruction(&Instruction::If(BlockType::Empty));

    if let Some(depth) = control_stack
        .iter()
        .rev()
        .position(|frame| *frame == ControlFrame::AttemptHandler)
    {
        // A pending failure unwinds to the nearest `attempt` handler block.
        //
        // The `If` emitted just above is itself a nesting level that
        // `control_stack` does not track, so the handler sits one frame further
        // out than its index suggests. Branching to `depth` alone would target
        // this very `If` and fall straight through to the next statement.
        func.instruction(&Instruction::Br(depth as u32 + 1));
    } else {
        match return_type {
            Some(WasmType::I64) => {
                func.instruction(&Instruction::I64Const(0));
            }
            Some(WasmType::I32) => {
                func.instruction(&Instruction::I32Const(0));
            }
            Some(WasmType::F64) => {
                func.instruction(&Instruction::F64Const(0.0.into()));
            }
            None => {}
        }
        func.instruction(&Instruction::Return);
    }
    func.instruction(&Instruction::End);
}

/// Coerces a value on top of the Wasm stack to boolean `i32` (0 or 1).
fn coerce_to_bool(func: &mut Function, ty: WasmType) {
    match ty {
        WasmType::I32 => {}
        WasmType::I64 => {
            func.instruction(&Instruction::I64Const(0));
            func.instruction(&Instruction::I64Ne);
        }
        WasmType::F64 => {
            func.instruction(&Instruction::F64Const(0.0.into()));
            func.instruction(&Instruction::F64Ne);
        }
    }
}

/// Coerces a value on top of the Wasm stack from `from` type to `to` type.
fn coerce_type(func: &mut Function, from: WasmType, to: WasmType) {
    if from == to {
        return;
    }
    match (from, to) {
        (WasmType::I64, WasmType::F64) => {
            func.instruction(&Instruction::F64ConvertI64S);
        }
        (WasmType::I32, WasmType::I64) => {
            func.instruction(&Instruction::I64ExtendI32U);
        }
        (WasmType::I32, WasmType::F64) => {
            func.instruction(&Instruction::F64ConvertI32S);
        }
        (WasmType::I64, WasmType::I32) => {
            func.instruction(&Instruction::I32WrapI64);
        }
        (WasmType::F64, WasmType::I64) => {
            func.instruction(&Instruction::I64TruncF64S);
        }
        (WasmType::F64, WasmType::I32) => {
            func.instruction(&Instruction::I32TruncF64S);
        }
        (WasmType::I32, WasmType::I32)
        | (WasmType::I64, WasmType::I64)
        | (WasmType::F64, WasmType::F64) => {}
    }
}

/// Returns `true` when evaluating a statement can leave a `Failure` pending.
///
/// The check mirrors the stack VM's `CheckFailure`, which fires at every
/// statement boundary. Keeping it conservative (any call may fail) preserves
/// semantics; the emitted guard is only two instructions, so the cost is
/// confined to statements that can actually raise.
fn stmt_may_fail(stmt: &HirStmt) -> bool {
    match stmt {
        HirStmt::Fail(..) => true,
        HirStmt::Attempt(..) => false,
        HirStmt::Let(_, expr, _) | HirStmt::Var(_, expr, _) => expr_may_fail(expr),
        HirStmt::Return(Some(expr), _) => expr_may_fail(expr),
        HirStmt::Return(None, _) => false,
        HirStmt::Assign(_, expr, _) => expr_may_fail(expr),
        HirStmt::Expr(expr) => expr_may_fail(expr),
        HirStmt::CompoundAssign(..) => true,
        HirStmt::If(if_stmt) => {
            expr_may_fail(&if_stmt.condition)
                || stmts_may_fail(&if_stmt.then_branch)
                || if_stmt
                    .elif_branches
                    .iter()
                    .any(|(cond, body)| expr_may_fail(cond) || stmts_may_fail(body))
                || if_stmt
                    .else_branch
                    .as_ref()
                    .is_some_and(|body| stmts_may_fail(body))
        }
        HirStmt::Match(match_stmt) => {
            expr_may_fail(&match_stmt.target)
                || match_stmt.when_arms.iter().any(|arm| {
                    arm.patterns.iter().any(|p| match p {
                        aipo_hir::HirMatchPattern::Value(e) => expr_may_fail(e),
                        aipo_hir::HirMatchPattern::Destructure(_) => true,
                        aipo_hir::HirMatchPattern::Variant { .. } => true,
                    }) || arm.guard.as_ref().map(expr_may_fail).unwrap_or(false)
                        || stmts_may_fail(&arm.body)
                })
                || match_stmt
                    .else_arm
                    .as_ref()
                    .is_some_and(|body| stmts_may_fail(body))
        }
        HirStmt::While(cond, body, _) => expr_may_fail(cond) || stmts_may_fail(body),
        HirStmt::Loop(body, _) | HirStmt::AwaitDo(body, _) => stmts_may_fail(body),
        HirStmt::Repeat(count, _, body, _) => expr_may_fail(count) || stmts_may_fail(body),
        HirStmt::Each(_, iterable, body, _) => expr_may_fail(iterable) || stmts_may_fail(body),
        HirStmt::Break(_) | HirStmt::Continue(_) => false,
        HirStmt::FnDecl(decl) => {
            stmts_may_fail(&decl.body) || decl.params.iter().any(|p| p.default.is_some())
        }
    }
}

/// Returns `true` when any statement in the slice can leave a `Failure` pending.
fn stmts_may_fail(stmts: &[HirStmt]) -> bool {
    stmts.iter().any(stmt_may_fail)
}

/// Returns `true` when evaluating an expression can leave a `Failure` pending.
fn expr_may_fail(expr: &HirExpr) -> bool {
    match expr {
        // `or_else` consumes a failure itself, so it only propagates when its
        // fallback can raise.
        HirExpr::OrElse(_, fallback, _) => expr_may_fail(fallback),
        HirExpr::Call(callee, args, _) => {
            // Calls into the module may reach `fail`; the runtime helpers and
            // the primitive conversions below are pure and never raise.
            let is_pure_builtin = matches!(&**callee, HirExpr::Identifier(name, _)
                if name == "String"
                    || name.starts_with("__aipo_")
                    || name == "int"
                    || name == "float"
                    || name == "bool"
                    || name == "str"
                    || name == "len");
            // A `math` helper answers a number and never raises (except `clamp`
            // with inverted bounds, which is checked separately).
            let is_pure_module = matches!(&**callee, HirExpr::Dot(receiver, method, _)
                if matches!(&**receiver, HirExpr::Identifier(module, _) if module == "math")
                    && method != "clamp");
            let is_pure_string = matches!(&**callee, HirExpr::Dot(receiver, method, _)
                if matches!(&**receiver, HirExpr::Identifier(module, _) if module == "string")
                    && !matches!(method.as_str(), "to_int"));
            (!is_pure_builtin && !is_pure_module && !is_pure_string)
                || args.iter().any(|a| expr_may_fail(&a.value))
        }
        HirExpr::Await(inner, _) => expr_may_fail(inner),
        HirExpr::Try(..) => true,
        // `with` copies then writes; the write can fault on a `fixed` field, so treat the
        // whole update as failing rather than assuming the base's fields stay writable.
        HirExpr::With(base, _, _) => expr_may_fail(base),
        HirExpr::Literal(..) | HirExpr::Identifier(..) | HirExpr::Fn(..) => false,
        HirExpr::Unary(_, inner, _)
        | HirExpr::Index(inner, _, _)
        | HirExpr::Dot(inner, _, _)
        | HirExpr::QuestionDot(inner, _, _) => expr_may_fail(inner),
        HirExpr::Binary(_, lhs, rhs, _) => expr_may_fail(lhs) || expr_may_fail(rhs),
        HirExpr::List(items, _) => items.iter().any(expr_may_fail),
        HirExpr::Dict(entries, _) => entries
            .iter()
            .any(|(k, v)| expr_may_fail(k) || expr_may_fail(v)),
        HirExpr::Construct(_, fields, _) => fields.iter().any(|(_, v)| expr_may_fail(v)),
        HirExpr::If(cond, then_expr, else_expr, _) => {
            expr_may_fail(cond) || expr_may_fail(then_expr) || expr_may_fail(else_expr)
        }
    }
}

/// Returns `true` when the expression is statically known to evaluate to a `Range`.
///
/// Only the shapes the backend can resolve without runtime type information are
/// recognised: a local initialised from a `..` expression, or a call to
/// `__aipo_range_new`.
fn infer_expr_is_range(
    expr: &HirExpr,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    _structs: &HashMap<String, StructLayout>,
    table_indices: &HashMap<String, u32>,
) -> bool {
    match expr {
        HirExpr::Binary(BinaryOp::Range, ..) => true,
        HirExpr::Identifier(name, _) => locals
            .get(name)
            .is_some_and(|(_, _, kind)| matches!(kind, LocalKind::Range)),
        HirExpr::If(_, then_expr, else_expr, _) => {
            infer_expr_is_range(then_expr, locals, functions, _structs, table_indices)
                && infer_expr_is_range(else_expr, locals, functions, _structs, table_indices)
        }
        _ => {
            let _ = (functions, table_indices, _structs);
            false
        }
    }
}

/// Infers the Wasm type of an HIR expression without emitting instructions.
fn infer_expr_type(
    expr: &HirExpr,
    locals: &HashMap<String, (u32, WasmType, LocalKind)>,
    functions: &FunctionTable,
    structs: &HashMap<String, StructLayout>,
    table_indices: &HashMap<String, u32>,
) -> Result<WasmType, WasmCompileError> {
    match expr {
        HirExpr::Literal(lit, _) => match lit {
            Literal::Int(_) => Ok(WasmType::I64),
            Literal::Float(_) => Ok(WasmType::F64),
            Literal::Bool(_) => Ok(WasmType::I32),
            Literal::String(..) => Ok(WasmType::I32),
            Literal::None => Ok(WasmType::I64),
        },
        HirExpr::Identifier(name, _span) => {
            if let Some((_, ty, _)) = locals.get(name) {
                Ok(*ty)
            } else if table_indices.contains_key(name) {
                Ok(WasmType::I32)
            } else {
                Ok(WasmType::I64)
            }
        }
        HirExpr::Fn(..) => Ok(WasmType::I32),
        HirExpr::Unary(op, inner, _) => match op {
            UnaryOp::Neg | UnaryOp::Pos => {
                infer_expr_type(inner, locals, functions, structs, table_indices)
            }
            UnaryOp::Not => Ok(WasmType::I32),
        },
        HirExpr::Binary(op, left, right, _) => match op {
            BinaryOp::Div => Ok(WasmType::F64),
            BinaryOp::IntDiv | BinaryOp::Mod => Ok(WasmType::I64),
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual
            | BinaryOp::And
            | BinaryOp::Or => Ok(WasmType::I32),
            BinaryOp::Range => Ok(WasmType::I32),
            BinaryOp::Is | BinaryOp::IsNullable => Ok(WasmType::I32),
            BinaryOp::Add => {
                let left_ty = infer_expr_type(left, locals, functions, structs, table_indices)?;
                let right_ty = infer_expr_type(right, locals, functions, structs, table_indices)?;
                if left_ty == WasmType::I32 || right_ty == WasmType::I32 {
                    Ok(WasmType::I32)
                } else if left_ty == WasmType::F64 || right_ty == WasmType::F64 {
                    Ok(WasmType::F64)
                } else {
                    Ok(WasmType::I64)
                }
            }
            BinaryOp::Sub | BinaryOp::Mul => {
                let left_ty = infer_expr_type(left, locals, functions, structs, table_indices)?;
                let right_ty = infer_expr_type(right, locals, functions, structs, table_indices)?;
                if left_ty == WasmType::F64 || right_ty == WasmType::F64 {
                    Ok(WasmType::F64)
                } else {
                    Ok(WasmType::I64)
                }
            }
            other => Err(WasmCompileError::UnsupportedExpr {
                message: format!("operator `{other:?}` is not supported in Wasm backend"),
                span: expr.span(),
            }),
        },
        HirExpr::If(_, then_expr, _, _) => {
            infer_expr_type(then_expr, locals, functions, structs, table_indices)
        }
        HirExpr::Await(..) => Ok(WasmType::I64),
        HirExpr::Try(inner, _) => infer_expr_type(inner, locals, functions, structs, table_indices),
        HirExpr::QuestionDot(..) => Ok(WasmType::I64),
        HirExpr::With(..) => Ok(WasmType::I32),
        HirExpr::Construct(..) => Ok(WasmType::I32),
        HirExpr::List(..) | HirExpr::Dict(..) => Ok(WasmType::I32),
        HirExpr::Index(target, index, _) => {
            // A range index yields the container's own handle, not an element.
            // `Bytes`/`List` handles are `i32`; only an element read is `i64`.
            if matches!(&**index, HirExpr::Binary(BinaryOp::Range, ..)) {
                let is_string = matches!(&**target, HirExpr::Literal(Literal::String(..), _));
                if !is_string {
                    return Ok(WasmType::I32);
                }
            }
            Ok(WasmType::I64)
        }
        HirExpr::OrElse(left, right, _) => {
            let l_ty = infer_expr_type(left, locals, functions, structs, table_indices)?;
            let r_ty = infer_expr_type(right, locals, functions, structs, table_indices)?;
            // Both branches contribute the result type; prefer the wider one so
            // the fallback is not truncated.
            if l_ty == WasmType::F64 || r_ty == WasmType::F64 {
                Ok(WasmType::F64)
            } else if l_ty == WasmType::I32 || r_ty == WasmType::I32 {
                Ok(WasmType::I32)
            } else {
                Ok(WasmType::I64)
            }
        }
        HirExpr::Dot(receiver, field_name, _) => {
            let receiver_struct_name = match &**receiver {
                HirExpr::Identifier(name, _) => {
                    locals.get(name).and_then(|(_, _, kind)| match kind {
                        LocalKind::Struct(s_name) => Some(s_name.as_str()),
                        _ => None,
                    })
                }
                _ => None,
            };
            if field_name == "len"
                && (receiver_struct_name.is_none()
                    && !structs.values().any(|s| s.fields.contains_key("len")))
            {
                Ok(WasmType::I64)
            } else {
                let field_info = if let Some(s_name) = receiver_struct_name {
                    structs
                        .get(s_name)
                        .and_then(|s| s.fields.get(field_name).copied())
                } else {
                    structs
                        .values()
                        .find_map(|s| s.fields.get(field_name).copied())
                };
                Ok(field_info.map(|(_, ty)| ty).unwrap_or(WasmType::I64))
            }
        }
        HirExpr::Call(callee, _, _) => {
            if let HirExpr::Dot(receiver, method_name, _) = &**callee
                && let HirExpr::Identifier(rec_name, _) = &**receiver
                && rec_name == "task"
            {
                match method_name.as_str() {
                    "spawn" => return Ok(WasmType::I32),
                    "sleep" | "cancel" | "race" | "all" => return Ok(WasmType::I64),
                    _ => {}
                }
                if method_name == "add" || method_name == "has" {
                    return Ok(if method_name == "has" {
                        WasmType::I32
                    } else {
                        WasmType::I64
                    });
                }
                if method_name == "decode" || method_name == "decode_utf8" {
                    return Ok(WasmType::I32);
                }
                for s_name in structs.keys() {
                    let full_name = format!("{s_name}.{method_name}");
                    if let Some((_, _, fn_type)) = functions.get(&full_name) {
                        return Ok(fn_type.results.first().copied().unwrap_or(WasmType::I64));
                    }
                }
            }
            if let HirExpr::Dot(receiver, method_name, _) = &**callee
                && let HirExpr::Identifier(module, _) = &**receiver
                && !locals.contains_key(module)
                && module == "math"
            {
                // `math` helpers take and return scalars; only the integer-valued
                // ones keep an `i64` representation. Reporting `i64` for all of
                // them made `math.sqrt(...)` print as an integer.
                return Ok(match method_name.as_str() {
                    "floor" | "ceil" | "truncate" | "round" => WasmType::I64,
                    "abs" | "min" | "max" | "clamp" | "pow" => WasmType::I64,
                    _ => WasmType::F64,
                });
            }
            if let HirExpr::Identifier(func_name, _) = &**callee {
                if func_name == "String" || func_name == "Bytes" {
                    return Ok(WasmType::I32);
                }
                if let Some((_, _, fn_type)) = functions.get(func_name)
                    && let Some(&ret) = fn_type.results.first()
                {
                    return Ok(ret);
                }
            }
            Ok(WasmType::I64)
        }
    }
}
