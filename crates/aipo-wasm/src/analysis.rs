//! Static analysis the Wasm backend performs before emitting code.
//!
//! Wasm has no tagged values, so the representation of an expression has to be
//! decided at compile time. These passes recover what the IR does not carry:
//! which semantic kind a parameter holds, which variables a closure captures,
//! what a function returns, and whether a statement can raise a `Failure`.

pub(super) fn annotated_kind(
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

use super::compiler::{FunctionTable, LocalKind, StructLayout};
use crate::error::WasmCompileError;
use crate::types::WasmType;
use aipo_ast::{BinaryOp, Literal, UnaryOp};
use aipo_hir::{HirExpr, HirFunctionDecl, HirItem, HirParam, HirProgram, HirStmt, HirStructDecl};
use aipo_source::SourceSpan;
use std::collections::{HashMap, HashSet};
use wasm_encoder::{Function, Instruction};

///
/// An unannotated parameter takes its representation from the arguments (`i32` for
/// a `String`, `List`, `Dict` or struct), but those four share one representation.
/// The body still has to know which one it received: `len(value)` counts bytes on a
/// `String` and elements on a `List`. This pass reads the argument kinds at every
/// call site until the answer stops moving.
pub(super) fn collect_param_kinds(
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
pub(super) fn walk_param_kinds(
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
pub(super) fn walk_param_kinds_expr(
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

/// Infers the high-level kind of an expression for local tracking.
pub(super) fn infer_expr_kind(
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
                if matches!(method_name.as_str(), "len" | "byte_len") {
                    return LocalKind::Int;
                }
                // `string.find` answers an `Int` or `none`; it stays an Int here so a
                // found index prints as a number. The absent case is the `none`
                // sentinel, which `String`/`io.println` render as the word only when
                // the value is statically `none`.
                if method_name == "find" {
                    return LocalKind::Nullable(Box::new(LocalKind::Int));
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

/// Infers the Wasm type of an HIR expression without emitting instructions.
pub(super) fn infer_expr_type(
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

/// Infers an expression kind against a kind-only environment.
pub(super) fn kind_with_env(
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
pub(super) fn collect_anon_functions(
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

pub(super) fn collect_stmts_anon_functions(
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

pub(super) fn collect_expr_anon_functions(
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
pub(super) fn register_struct_layout(
    decl: &HirStructDecl,
    structs: &mut HashMap<String, StructLayout>,
) {
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
    let fixed: HashSet<String> = decl
        .fields
        .iter()
        .filter(|f| f.is_fixed)
        .map(|f| f.name.clone())
        .collect();
    structs.insert(
        decl.name.clone(),
        StructLayout {
            size,
            fields,
            fixed,
        },
    );
}

/// Scans program expressions to refine field types in `structs` based on constructor literals.
pub(super) fn refine_struct_layouts(
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

pub(super) fn scan_stmts_for_constructs(
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

pub(super) fn scan_expr_for_constructs(
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
pub(super) fn scan_program_for_io(program: &HirProgram) -> bool {
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

pub(super) fn scan_stmts_for_io(stmts: &[HirStmt]) -> bool {
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

pub(super) fn scan_expr_for_io(expr: &HirExpr) -> bool {
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
/// Builds a wrapper function for an `async fn` that creates a `Task` handle and returns it (`task_ptr: i32`).
pub(super) fn build_async_wrapper_function(
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
pub(super) fn collect_program_strings(
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

pub(super) fn collect_stmts_strings(
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

pub(super) fn add_static_string(
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

pub(super) fn collect_expr_strings(
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
pub(super) fn struct_field_kind(
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
pub(super) fn param_wasm_type(param: &HirParam) -> WasmType {
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
pub(super) fn merge_hint(current: Option<WasmType>, observed: WasmType) -> Option<WasmType> {
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
pub(super) fn static_expr_type(
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
pub(super) fn collect_param_hints(
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
pub(super) fn static_return_type(
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

pub(super) fn static_return_type_in_stmts(
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

pub(super) fn merge_return(
    current: Option<WasmType>,
    observed: Option<WasmType>,
) -> Option<WasmType> {
    match (current, observed) {
        (None, other) | (other, None) => other,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => Some(WasmType::I64),
    }
}

/// Records the argument types observed at every direct call inside `stmts`.
pub(super) fn observe_stmts(
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

pub(super) fn observe_stmts_inner(
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

pub(super) fn observe_expr(
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
