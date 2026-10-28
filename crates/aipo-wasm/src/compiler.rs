//! WebAssembly compiler lowering Aipo HIR to standard Wasm binary modules (ADP-013).

use crate::emitter::WasmEmitter;
use crate::error::WasmCompileError;
use crate::types::{WasmFnType, WasmType};
use aipo_ast::{BinaryOp, Literal, UnaryOp};
use aipo_hir::{
    HirAttemptStmt, HirExpr, HirFunctionDecl, HirIfStmt, HirItem, HirMatchStmt, HirParam,
    HirProgram, HirStmt, HirStructDecl,
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
    /// A `List` value produced by a list literal.
    List,
    /// A `Dict` value produced by a dict literal.
    Dict,
    /// A `Bytes` value produced by `Bytes(len)`.
    Bytes,
}

/// Function symbols and module bindings relevant to static semantic proofs.
struct FunctionTable {
    symbols: HashMap<String, (u32, u32, WasmFnType)>,
    global_bindings: HashSet<String>,
    declared_types: HashSet<String>,
}

impl FunctionTable {
    fn new(program: &HirProgram) -> Self {
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

fn annotated_kind(
    annotation: Option<&aipo_ast::TypeAnnotation>,
    structs: &HashMap<String, StructLayout>,
) -> LocalKind {
    let Some(annotation) = annotation.filter(|annotation| !annotation.is_nullable) else {
        return LocalKind::Unknown;
    };
    match annotation.name.as_str() {
        "Int" => LocalKind::Int,
        "Float" => LocalKind::Float,
        "Bool" => LocalKind::Bool,
        "String" => LocalKind::String,
        "List" => LocalKind::List,
        "Dict" => LocalKind::Dict,
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
    refine_struct_layouts(program, &mut structs);

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

    // Function 14: `__aipo_dict_set(dict: i32, key: i64, value: i64) -> i64`
    let dict_set_fn_type = WasmFnType::new(
        vec![WasmType::I32, WasmType::I64, WasmType::I64],
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

    let async_helpers = AsyncHelpers {
        task_create_idx: task_create_func_idx,
        await_idx: await_func_idx,
        host_io,
        fail_globals: failure_globals,
    };

    // Pass 1: Collect signatures of all declared and anonymous functions
    let mut next_func_idx = emitter.next_func_idx();
    for func in &func_decls {
        let params = func.params.iter().map(param_wasm_type).collect::<Vec<_>>();
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
        let fn_type = WasmFnType::new(params, ret.into_iter().collect());
        let type_idx = emitter.add_type(fn_type.clone());
        let func_idx = next_func_idx;
        next_func_idx += 1;
        functions.insert(func.name.clone(), (func_idx, type_idx, fn_type));
    }

    // Register async wrapper function signatures (returning task_ptr: i32)
    let mut async_wrapper_infos = Vec::new();
    for (name, inner_body_name, params) in &async_wrappers {
        let params_wasm = params.iter().map(param_wasm_type).collect::<Vec<_>>();
        let fn_type = WasmFnType::new(params_wasm, vec![WasmType::I32]);
        let type_idx = emitter.add_type(fn_type.clone());
        let func_idx = next_func_idx;
        next_func_idx += 1;
        functions.insert(name.clone(), (func_idx, type_idx, fn_type));
        async_wrapper_infos.push((
            name.clone(),
            inner_body_name.clone(),
            params.len(),
            type_idx,
        ));
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
    for (name, inner_body_name, param_count, type_idx) in async_wrapper_infos {
        let inner_table_idx = table_indices[&inner_body_name];
        let wrapper_fn = build_async_wrapper_function(
            inner_table_idx,
            param_count,
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
        let ty = if let Some(ref def) = f.default {
            match def {
                HirExpr::Literal(Literal::Float(..), _) => WasmType::F64,
                HirExpr::Literal(Literal::Bool(..), _) => WasmType::I32,
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
fn refine_struct_layouts(program: &HirProgram, structs: &mut HashMap<String, StructLayout>) {
    for item in &program.items {
        match item {
            HirItem::Fn(func) => {
                scan_stmts_for_constructs(&func.body, structs);
            }
            HirItem::Impl(imp) => {
                if let Some(init) = &imp.init {
                    scan_stmts_for_constructs(&init.body, structs);
                }
                if let Some(inv) = &imp.invariant {
                    scan_stmts_for_constructs(&inv.body, structs);
                }
                for m in &imp.methods {
                    scan_stmts_for_constructs(&m.body, structs);
                }
            }
            _ => {}
        }
    }
    scan_stmts_for_constructs(&program.statements, structs);
}

fn scan_stmts_for_constructs(stmts: &[HirStmt], structs: &mut HashMap<String, StructLayout>) {
    for stmt in stmts {
        match stmt {
            HirStmt::Let(_, expr, _) | HirStmt::Var(_, expr, _) | HirStmt::Expr(expr) => {
                scan_expr_for_constructs(expr, structs);
            }
            HirStmt::Assign(target, expr, _) | HirStmt::CompoundAssign(_, target, expr, _) => {
                scan_expr_for_constructs(target, structs);
                scan_expr_for_constructs(expr, structs);
            }
            HirStmt::Return(Some(expr), _) => {
                scan_expr_for_constructs(expr, structs);
            }
            HirStmt::If(s) => {
                scan_expr_for_constructs(&s.condition, structs);
                scan_stmts_for_constructs(&s.then_branch, structs);
                for (c, b) in &s.elif_branches {
                    scan_expr_for_constructs(c, structs);
                    scan_stmts_for_constructs(b, structs);
                }
                if let Some(eb) = &s.else_branch {
                    scan_stmts_for_constructs(eb, structs);
                }
            }
            HirStmt::While(cond, body, _) => {
                scan_expr_for_constructs(cond, structs);
                scan_stmts_for_constructs(body, structs);
            }
            HirStmt::Loop(body, _) => {
                scan_stmts_for_constructs(body, structs);
            }
            HirStmt::Repeat(count, _, body, _) => {
                scan_expr_for_constructs(count, structs);
                scan_stmts_for_constructs(body, structs);
            }
            HirStmt::Each(_, iterable, body, _) => {
                scan_expr_for_constructs(iterable, structs);
                scan_stmts_for_constructs(body, structs);
            }
            HirStmt::Fail(expr, _) => {
                scan_expr_for_constructs(expr, structs);
            }
            HirStmt::Attempt(a) => {
                scan_stmts_for_constructs(&a.body, structs);
                scan_stmts_for_constructs(&a.handler, structs);
            }
            HirStmt::Match(m) => {
                scan_expr_for_constructs(&m.target, structs);
                for arm in &m.when_arms {
                    for p in &arm.patterns {
                        if let aipo_hir::HirMatchPattern::Value(expr) = p {
                            scan_expr_for_constructs(expr, structs);
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        scan_expr_for_constructs(guard, structs);
                    }
                    scan_stmts_for_constructs(&arm.body, structs);
                }
                if let Some(else_body) = &m.else_arm {
                    scan_stmts_for_constructs(else_body, structs);
                }
            }
            HirStmt::AwaitDo(stmts, _) => {
                scan_stmts_for_constructs(stmts, structs);
            }
            _ => {}
        }
    }
}

fn scan_expr_for_constructs(expr: &HirExpr, structs: &mut HashMap<String, StructLayout>) {
    match expr {
        HirExpr::Await(inner, _) => {
            scan_expr_for_constructs(inner, structs);
        }
        HirExpr::Construct(type_name, fields, _) => {
            let mut float_fields = Vec::new();
            for (maybe_name, field_expr) in fields {
                scan_expr_for_constructs(field_expr, structs);
                if let Some(name) = maybe_name
                    && matches!(field_expr, HirExpr::Literal(Literal::Float(..), _))
                {
                    float_fields.push(name.clone());
                }
            }
            if let Some(layout) = structs.get_mut(type_name) {
                for name in float_fields {
                    if let Some((_, field_ty)) = layout.fields.get_mut(&name) {
                        *field_ty = WasmType::F64;
                    }
                }
            }
        }
        HirExpr::Binary(_, left, right, _) => {
            scan_expr_for_constructs(left, structs);
            scan_expr_for_constructs(right, structs);
        }
        HirExpr::Unary(_, inner, _) => {
            scan_expr_for_constructs(inner, structs);
        }
        HirExpr::If(cond, then_e, else_e, _) => {
            scan_expr_for_constructs(cond, structs);
            scan_expr_for_constructs(then_e, structs);
            scan_expr_for_constructs(else_e, structs);
        }
        HirExpr::Call(callee, args, _) => {
            scan_expr_for_constructs(callee, structs);
            for arg in args {
                scan_expr_for_constructs(&arg.value, structs);
            }
        }
        HirExpr::Dot(receiver, _, _) => {
            scan_expr_for_constructs(receiver, structs);
        }
        HirExpr::List(elements, _) => {
            for elem in elements {
                scan_expr_for_constructs(elem, structs);
            }
        }
        HirExpr::Dict(entries, _) => {
            for (key, value) in entries {
                scan_expr_for_constructs(key, structs);
                scan_expr_for_constructs(value, structs);
            }
        }
        HirExpr::Index(target, index, _) => {
            scan_expr_for_constructs(target, structs);
            scan_expr_for_constructs(index, structs);
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
/// followed by `capacity` fixed-width 16-byte entries (`key: i64`,
/// `value: i64`). Entries are filled in order and searched linearly.
///
/// `ponytail:` linear scan is `O(n)` per lookup. Replace with an open-addressing
/// table once dictionaries above a few dozen entries show up in benchmarks.
fn build_dict_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // bytes = 8 (8-byte aligned entry count header) + capacity * 16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Shl);
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
    // locals: 0=dict, 1=key, 2=value, 3=index, 4=entry_count
    let mut func = Function::new([(2, ValType::I32)]);

    // entry_count = dict[0]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    // Scan for an existing key, remembering the first free slot.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));

    // block $done
    func.instruction(&Instruction::Block(BlockType::Empty));
    // loop
    func.instruction(&Instruction::Loop(BlockType::Empty));

    // if index == entry_count then leave the loop
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // entry key = dict[4 + index*16]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
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
    // key matched: overwrite the value in place.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    // Return `value` immediately without appending or altering entry_count.
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // index += 1
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Append at index.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Shl);
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
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // entry_count = index + 1
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(2));
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
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
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

/// Builds a wrapper function for an `async fn` that creates a `Task` handle and returns it (`task_ptr: i32`).
fn build_async_wrapper_function(
    inner_table_idx: u32,
    param_count: usize,
    task_create_func_idx: u32,
) -> Function {
    let mut func = Function::new([]);
    // arg 0: fn_table_idx
    func.instruction(&Instruction::I32Const(inner_table_idx as i32));

    // arg 1: arg_count
    func.instruction(&Instruction::I32Const(param_count as i32));

    // arg 2: arg0
    if param_count > 0 {
        func.instruction(&Instruction::LocalGet(0));
    } else {
        func.instruction(&Instruction::I64Const(0));
    }

    // arg 3: arg1
    if param_count > 1 {
        func.instruction(&Instruction::LocalGet(1));
    } else {
        func.instruction(&Instruction::I64Const(0));
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
        _ => {}
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
            "String" | "Bytes" => WasmType::I32,
            _ => WasmType::I64,
        }
    } else {
        WasmType::I64
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
                for arm in &match_stmt.when_arms {
                    for pat in &arm.patterns {
                        match pat {
                            aipo_hir::HirMatchPattern::Destructure(fields) => {
                                for f in fields {
                                    if !locals.contains_key(f) {
                                        let slot = (params_count + declared_locals.len()) as u32;
                                        declared_locals.push(WasmType::I64);
                                        locals.insert(
                                            f.clone(),
                                            (slot, WasmType::I64, LocalKind::Unknown),
                                        );
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
                                        locals.insert(
                                            f.clone(),
                                            (slot, WasmType::I64, LocalKind::Unknown),
                                        );
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
                    (seq_idx, WasmType::I32, LocalKind::List),
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
                for name in bindings {
                    let slot = (params_count + declared_locals.len()) as u32;
                    declared_locals.push(WasmType::I64);
                    locals.insert(name.clone(), (slot, WasmType::I64, LocalKind::Unknown));
                }
                let _ = iterable;
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
        HirExpr::List(..) => LocalKind::List,
        HirExpr::Dict(..) => LocalKind::Dict,
        HirExpr::With(base, ..) => {
            infer_expr_kind(base, locals, functions, table_indices, anon_map)
        }
        HirExpr::Try(inner, ..) => {
            infer_expr_kind(inner, locals, functions, table_indices, anon_map)
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
                let rec_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(rec_kind, LocalKind::Bytes)
                    && (method_name == "decode" || method_name == "decode_utf8")
                {
                    return LocalKind::String;
                }
                if let HirExpr::Identifier(rec_name, _) = &**receiver
                    && rec_name == "task"
                    && !locals.contains_key(rec_name)
                    && functions.builtin_is_unshadowed(rec_name)
                    && method_name == "spawn"
                {
                    return LocalKind::Task;
                }
            }
            if let HirExpr::Identifier(name, _) = &**callee
                && !locals.contains_key(name)
                && !functions.global_bindings.contains(name)
                && let Some((_, _, fn_type)) = functions.get(name)
                && fn_type.results.first() == Some(&WasmType::F64)
            {
                return LocalKind::Float;
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
    _func_name: &str,
    params: &[HirParam],
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

    let struct_target = if _func_name.contains('.') {
        _func_name.split('.').next()
    } else {
        None
    };

    // Map parameters to locals 0..params.len()
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

    // Build the Wasm function with its locals declaration
    let locals_spec = compress_locals(&declared_locals);
    let mut func = Function::new(locals_spec);
    let mut control_stack: Vec<ControlFrame> = Vec::new();
    let mut emit_repeat_id = 0;

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
        true,
        &mut emit_repeat_id,
        0,
        0,
    )?;

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
                    } else {
                        return Err(WasmCompileError::UnknownVariable {
                            name: name.clone(),
                            span: *span,
                        });
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
                    if matches!(coll_kind, LocalKind::Dict) {
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

                        let index_kind =
                            infer_expr_kind(index, locals, functions, table_indices, anon_map);
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
                    } else {
                        return Err(WasmCompileError::UnknownVariable {
                            name: name.clone(),
                            span: *span,
                        });
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

                    if matches!(coll_kind, LocalKind::Dict) {
                        let (dict_get_idx, _, _) = functions["__aipo_dict_get"];
                        let (dict_set_idx, _, _) = functions["__aipo_dict_set"];
                        let (string_hash_idx, _, _) = functions["__aipo_string_hash"];
                        let key_temp = locals["__or_else_temp_0"].0;
                        let val_temp = locals["__or_else_temp_1"].0;

                        let index_kind =
                            infer_expr_kind(index, locals, functions, table_indices, anon_map);
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
                        if matches!(index_kind, LocalKind::String) {
                            coerce_type(func, i_ty, WasmType::I32);
                            func.instruction(&Instruction::Call(string_hash_idx));
                        } else {
                            coerce_type(func, i_ty, WasmType::I64);
                        }
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

                        // Store new value
                        func.instruction(&Instruction::LocalGet(dot_temp));
                        func.instruction(&Instruction::LocalGet(key_temp));
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

    if let Some(first_binding) = bindings.first() {
        let elem_slot = locals[first_binding].0;
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
        func.instruction(&Instruction::LocalSet(elem_slot));
    }

    if let Some(second_binding) = bindings.get(1) {
        let idx_slot = locals[second_binding].0;
        func.instruction(&Instruction::LocalGet(cur_slot));
        func.instruction(&Instruction::LocalSet(idx_slot));
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
    coerce_type(func, target_ty, WasmType::I64);
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
    target_ty: WasmType,
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
        func.instruction(&Instruction::LocalGet(target_slot));
        func.instruction(&Instruction::I32Const(ptr));
        func.instruction(&Instruction::I32Eq);
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
        coerce_type(func, pat_ty, WasmType::I64);
        func.instruction(&Instruction::LocalGet(target_slot));
        match target_ty {
            WasmType::F64 => {
                func.instruction(&Instruction::F64Eq);
            }
            _ => {
                func.instruction(&Instruction::I64Eq);
            }
        }
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
        return_type,
        false,
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
            match kind {
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
                let matches = match (&kind, type_name.as_str()) {
                    (LocalKind::Int, "Int")
                    | (LocalKind::Float, "Float")
                    | (LocalKind::Bool, "Bool")
                    | (LocalKind::String, "String")
                    | (LocalKind::List, "List")
                    | (LocalKind::Dict, "Dict") => true,
                    (LocalKind::Struct(name), expected) => {
                        name == expected
                            || name
                                .split_once('.')
                                .is_some_and(|(parent, _)| parent == expected)
                    }
                    (LocalKind::None, _) => *op == BinaryOp::IsNullable,
                    _ => false,
                };
                // Evaluate the left side once, preserving its effects, even for a constant proof.
                compile_expr(
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
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::I32Const(i32::from(matches)));
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
                    LocalKind::List
                        | LocalKind::Dict
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

            // Case 0.82: Dict `.has(...)` key presence check
            if let HirExpr::Dot(receiver, method_name, _) = &**callee
                && method_name == "has"
                && args.len() == 1
            {
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(receiver_kind, LocalKind::Dict) {
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
                let static_type_call = if let HirExpr::Identifier(rec_name, _) = &**receiver {
                    if structs.contains_key(rec_name) && !locals.contains_key(rec_name) {
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
                    if args.len() != fn_type.params.len() {
                        return Err(WasmCompileError::TypeMismatch {
                            expected: format!("{} arguments", fn_type.params.len()),
                            found: format!("{} arguments", args.len()),
                            span: *span,
                        });
                    }
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
                    return Ok(fn_type.results.first().copied().unwrap_or(WasmType::I64));
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

                // Keys are unboxed to an i64 that the dict search compares raw.
                // Float, Bool, and small numeric keys use their bit pattern;
                // strings hash to their FNV-1a value first.
                let key_kind = infer_expr_kind(key, locals, functions, table_indices, anon_map);
                if matches!(key_kind, LocalKind::String) {
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
                    coerce_type(func, key_ty, WasmType::I32);
                    func.instruction(&Instruction::Call(string_hash_idx));
                } else {
                    // Float keys round-trip through their bit pattern so a
                    // Float(1.5) and an Int(1.5-bits) never compare equal.
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
                    coerce_type(func, key_ty, WasmType::I64);
                }

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

            let is_string_receiver = match &**receiver {
                HirExpr::Identifier(name, _) => locals
                    .get(name)
                    .map(|(_, _, kind)| *kind == LocalKind::String)
                    .unwrap_or(false),
                HirExpr::Literal(Literal::String(..), _) => true,
                _ => false,
            };

            if field_name == "len"
                && (is_string_receiver
                    || (receiver_struct_name.is_none()
                        && !structs.values().any(|s| s.fields.contains_key("len"))))
            {
                // Lists and Ranges expose their element count through the
                // runtime helpers rather than the static string length path.
                let receiver_kind =
                    infer_expr_kind(receiver, locals, functions, table_indices, anon_map);
                if matches!(receiver_kind, LocalKind::List) {
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
                if matches!(receiver_kind, LocalKind::Dict) {
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
                locals[&format!("__or_else_temp_{}", struct_depth.min(7))].0,
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
                locals[&format!("__or_else_temp_{}", struct_depth.min(7))].0,
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

            if matches!(target_kind, LocalKind::Dict) {
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
            let temp_name = format!("__call_temp_{}", call_depth.min(7));
            let temp = locals[&temp_name].0;
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
            match inner_ty {
                WasmType::I64 | WasmType::I32 => {
                    func.instruction(&Instruction::LocalSet(temp));
                    emit_failure_check(func, control_stack, async_helpers.fail_globals, None);
                    func.instruction(&Instruction::LocalGet(temp));
                }
                _ => {
                    emit_failure_check(func, control_stack, async_helpers.fail_globals, None);
                }
            }
            Ok(inner_ty)
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
            !is_pure_builtin || args.iter().any(|a| expr_may_fail(&a.value))
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
        HirExpr::Identifier(name, span) => {
            if let Some((_, ty, _)) = locals.get(name) {
                Ok(*ty)
            } else if table_indices.contains_key(name) {
                Ok(WasmType::I32)
            } else {
                Err(WasmCompileError::UnknownVariable {
                    name: name.clone(),
                    span: *span,
                })
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
        HirExpr::Index(..) => Ok(WasmType::I64),
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
