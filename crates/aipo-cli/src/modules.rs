//! Module resolution for the Wave 1 surface: one `.aipo` file is one module.
//!
//! Canon fixes the surface: `import m`, `import m: names`, `import m as alias`, `export`,
//! acyclic structural resolution and **eager init once**. A module is loaded from
//! `<module-name>.aipo` next to the importing file, its declarations are added to the
//! program and its top-level statements run exactly once, before the importer's.
//!
//! Everything is private by default, so a name declared by an imported module and not listed
//! in its `export` declaration is renamed to a module-qualified global (`m::name`) together
//! with the references inside its own module. The importer therefore cannot resolve it and
//! gets an ordinary unknown-name diagnostic — privacy falls out of name resolution instead of
//! a separate authorization pass.
//!
//! Namespaced access (`import m as alias`) is desugared at this level: `alias.name` becomes a
//! reference to the module's declaration of `name`, which is exactly what the alias means.

use aipo_diagnostics::{Diagnostic, DiagnosticCode};
use aipo_hir::*;
use aipo_package::{PackageId, PackagePathMap};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A program after every imported module has been merged in.
pub(crate) struct ResolvedProgram {
    /// Merged program ready for semantic analysis and lowering.
    pub program: HirProgram,
    /// Names the importing surface may reference (aliases, module names, imported names).
    pub imported_names: HashSet<String>,
    /// Diagnostics produced while resolving imports.
    pub diagnostics: Vec<Diagnostic>,
}

/// What one `import` declaration binds in the importing module.
#[derive(Debug, Clone)]
struct ImportBinding {
    /// Module that owns the declarations.
    module: String,
    /// Local alias, when the import introduced one.
    alias: Option<String>,
    /// Explicitly requested names, when the import selected them.
    names: Vec<String>,
}

impl ImportBinding {
    fn local_namespace(&self) -> &str {
        self.alias.as_deref().unwrap_or_else(|| {
            self.module
                .rsplit_once('.')
                .map_or(self.module.as_str(), |(_, local)| local)
        })
    }
}

/// A loaded module and the metadata needed to merge it.
struct LoadedModule {
    name: String,
    program: HirProgram,
    /// Declared names visible to importers.
    exported: HashSet<String>,
    /// Declared names that stay private to this module.
    private: HashSet<String>,
    /// Imports declared by this module.
    imports: Vec<ImportBinding>,
}

/// Merges the entry file with every module it imports.
///
/// Returns the merged program, the names the importing surface may reference, and any
/// diagnostics about missing modules, cycles or unknown exported names.
pub(crate) fn resolve(
    entry: &Path,
    entry_program: HirProgram,
    package_paths: Option<&PackagePathMap>,
) -> ResolvedProgram {
    let mut diagnostics = Vec::new();
    let mut loaded: Vec<LoadedModule> = Vec::new();
    let mut visiting: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let base_dir = entry
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let entry_name = entry.file_stem().map_or_else(
        || entry.display().to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );

    load_imports(
        &entry_program,
        &base_dir,
        package_paths,
        &mut loaded,
        &mut visiting,
        &mut seen,
        &mut diagnostics,
    );

    // The entry file imports through the same rules as any module, so its bindings take part
    // in export validation even though the entry itself is not a loaded dependency.
    let entry_bindings = import_bindings(&entry_program);

    // Requested names must actually be exported by the module that declares them.
    let exported_by_module: HashMap<&str, &HashSet<String>> = loaded
        .iter()
        .map(|module| (module.name.as_str(), &module.exported))
        .collect();
    let importers = loaded
        .iter()
        .map(|module| (module.name.as_str(), &module.imports))
        .chain(std::iter::once((entry_name.as_str(), &entry_bindings)));
    for (importer, bindings) in importers {
        for binding in bindings {
            let Some(exported) = exported_by_module.get(binding.module.as_str()) else {
                continue;
            };
            for requested in &binding.names {
                if !exported.contains(requested) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_SEM_EXPORT_UNKNOWN,
                            format!(
                                "module '{}' does not export '{}'",
                                binding.module, requested
                            ),
                        )
                        .with_note(format!("while resolving imports of {importer}")),
                    );
                }
            }
        }
    }

    let mut imported_names = HashSet::new();
    for binding in loaded
        .iter()
        .flat_map(|module| module.imports.iter())
        .chain(entry_bindings.iter())
    {
        // Only the module name (or its alias) needs a surface entry: names selected by
        // `import m: name` are already global because the declaring module's items were merged
        // into the program, and `alias.member` was rewritten to the declaration itself.
        imported_names.insert(binding.local_namespace().to_string());
    }

    let mut items: Vec<HirItem> = Vec::new();
    let mut statements: Vec<HirStmt> = Vec::new();

    // Dependencies were loaded first, so their `export`ed declarations and initializers come
    // first: every module is initialized exactly once, before the modules that import it.
    for module in &loaded {
        let scope = ModuleScope {
            private_names: module.private.clone(),
            prefix: module.name.clone(),
            aliases: namespace_bindings(&module.imports, &exported_by_module),
            // Only names owned by other modules are rewritten; an alias that collides with a
            // local declaration must keep meaning the local declaration.
            local_names: declared_names(&module.program),
            violations: RefCell::new(Vec::new()),
        };

        for item in module.program.items.clone() {
            match item {
                HirItem::Import(_) | HirItem::Export(_) => {}
                HirItem::Fn(mut decl) => {
                    decl.name = scope.rewrite_identifier(decl.name);
                    rewrite_function(&mut decl, &scope);
                    items.push(HirItem::Fn(decl));
                }
                HirItem::Struct(mut decl) => {
                    decl.name = scope.rewrite_identifier(decl.name);
                    for field in &mut decl.fields {
                        if let Some(default) = field.default.take() {
                            field.default = Some(rewrite_expr(default, &scope));
                        }
                    }
                    items.push(HirItem::Struct(decl));
                }
                HirItem::Impl(mut block) => {
                    block.target = scope.rewrite_identifier(block.target);
                    rewrite_impl(&mut block, &scope);
                    items.push(HirItem::Impl(block));
                }
                other => items.push(other),
            }
        }

        for statement in module.program.statements.clone() {
            statements.push(rewrite_stmt(statement, &scope));
        }
        scope.report_violations(&module.name, &mut diagnostics);
    }

    let span = entry_program.span;
    let mut entry_items = entry_program.items;
    let entry_scope = ModuleScope {
        private_names: HashSet::new(),
        prefix: String::new(),
        aliases: namespace_bindings(&entry_bindings, &exported_by_module),
        local_names: declared_names(&HirProgram {
            items: entry_items.clone(),
            statements: entry_program.statements.clone(),
            span,
        }),
        violations: RefCell::new(Vec::new()),
    };
    entry_items.retain(|item| !matches!(item, HirItem::Import(_) | HirItem::Export(_)));
    for item in entry_items {
        match item {
            HirItem::Fn(mut decl) => {
                rewrite_function(&mut decl, &entry_scope);
                items.push(HirItem::Fn(decl));
            }
            HirItem::Struct(mut decl) => {
                for field in &mut decl.fields {
                    if let Some(default) = field.default.take() {
                        field.default = Some(rewrite_expr(default, &entry_scope));
                    }
                }
                items.push(HirItem::Struct(decl));
            }
            HirItem::Impl(mut block) => {
                rewrite_impl(&mut block, &entry_scope);
                items.push(HirItem::Impl(block));
            }
            other => items.push(other),
        }
    }
    let entry_statements = entry_program.statements;
    for statement in entry_statements {
        statements.push(rewrite_stmt(statement, &entry_scope));
    }
    entry_scope.report_violations(&entry_name, &mut diagnostics);

    let program = HirProgram {
        items,
        statements,
        span,
    };

    ResolvedProgram {
        program,
        imported_names,
        diagnostics,
    }
}

/// Loads every module reachable from `program`, dependencies before dependents.
fn load_imports(
    program: &HirProgram,
    base_dir: &Path,
    package_paths: Option<&PackagePathMap>,
    loaded: &mut Vec<LoadedModule>,
    visiting: &mut Vec<String>,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in &program.items {
        let HirItem::Import(import) = item else {
            continue;
        };
        let name = import.module_name.clone();
        if seen.contains(&name) {
            continue;
        }
        if visiting.iter().any(|module| module == &name) {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::AIPO_SEM_IMPORT_CYCLE,
                format!("import cycle detected through module '{name}'"),
            ));
            continue;
        }

        let path = match import_module_path(&name, base_dir, package_paths) {
            Ok(path) => path,
            Err(message) => {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_UNKNOWN_MODULE,
                    message,
                ));
                continue;
            }
        };
        let module_base_dir = path.parent().unwrap_or(base_dir);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_UNKNOWN_MODULE,
                    format!("cannot load module '{name}': {error}"),
                ));
                continue;
            }
        };

        let source = aipo_source::Source::new(
            aipo_source::SourceId::next(),
            path.display().to_string(),
            &text,
        );
        let (parsed, parse_diagnostics) = aipo_syntax::parse(&source);
        for diagnostic in parse_diagnostics {
            if diagnostic.severity == aipo_diagnostics::Severity::Error {
                diagnostics.push(diagnostic);
            }
        }
        let module_program = aipo_hir::lower(parsed);

        visiting.push(name.clone());
        load_imports(
            &module_program,
            module_base_dir,
            package_paths,
            loaded,
            visiting,
            seen,
            diagnostics,
        );
        visiting.pop();
        seen.insert(name.clone());

        let declared = declared_names(&module_program);
        let exported: HashSet<String> = module_program
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Export(export) => Some(export.names.iter().cloned()),
                _ => None,
            })
            .flatten()
            .filter(|name| declared.contains(name))
            .collect();
        let private = declared
            .iter()
            .filter(|name| !exported.contains(*name))
            .cloned()
            .collect();

        let imports = import_bindings(&module_program);

        loaded.push(LoadedModule {
            name,
            program: module_program,
            exported,
            private,
            imports,
        });
    }
}

fn import_module_path(
    module: &str,
    base_dir: &Path,
    package_paths: Option<&PackagePathMap>,
) -> Result<PathBuf, String> {
    if package_paths.is_some() && module.contains('.') {
        let coordinate = PackageId::parse(module)
            .map_err(|error| format!("cannot load module '{module}': {error}"))?;
        return package_paths
            .and_then(|paths| paths.get(&coordinate))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "cannot load module '{module}': package is not present in the resolved package graph"
                )
            });
    }
    Ok(base_dir.join(format!("{module}.aipo")))
}

/// The `import` declarations a program carries, in source order.
fn import_bindings(program: &HirProgram) -> Vec<ImportBinding> {
    program
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Import(import) => Some(ImportBinding {
                module: import.module_name.clone(),
                alias: import.alias.clone(),
                names: import.names.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Names a module declares at its top level: functions, structs and top-level variables.
///
/// Top-level `var`/`let` bindings count because a module's public surface is not limited to
/// callables — an importer reads an exported variable through the module namespace.
fn declared_names(program: &HirProgram) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in &program.items {
        match item {
            HirItem::Fn(decl) => {
                names.insert(decl.name.clone());
            }
            HirItem::Struct(decl) => {
                names.insert(decl.name.clone());
            }
            HirItem::Impl(block) => {
                names.insert(block.target.clone());
            }
            _ => {}
        }
    }
    for statement in &program.statements {
        match statement {
            HirStmt::Let(name, ..) | HirStmt::Var(name, ..) => {
                names.insert(name.clone());
            }
            _ => {}
        }
    }
    names
}

/// A module namespace this scope may go through, with the names it exposes.
struct NamespaceBinding {
    /// Module the namespace names.
    module: String,
    /// Names the module actually exports, so a private one can be rejected.
    exported: HashSet<String>,
}

/// One rejected `alias.member` access, reported after the module has been rewritten.
struct AliasViolation {
    module: String,
    member: String,
}

/// Name-rewriting rules for one module.
struct ModuleScope {
    /// Declarations that stay private, renamed to `<module>::<name>`.
    private_names: HashSet<String>,
    /// Owning module name, used as the prefix for private declarations.
    prefix: String,
    /// Namespace bindings this module's imports introduce, keyed by the local name.
    aliases: HashMap<String, NamespaceBinding>,
    /// Names locally declared, which shadow anything an import could introduce.
    local_names: HashSet<String>,
    /// Access to a private member, collected while rewriting and reported afterwards.
    violations: RefCell<Vec<AliasViolation>>,
}

impl ModuleScope {
    /// Rewrites a plain identifier reference.
    fn rewrite_identifier(&self, name: String) -> String {
        if self.private_names.contains(&name) {
            format!("{}::{name}", self.prefix)
        } else {
            name
        }
    }

    /// Rewrites `alias.member`, returning the resolved reference when the receiver is a
    /// namespace and `member` belongs to that module's public surface.
    ///
    /// Reachability through a namespace is deliberately narrower than reachability from
    /// inside the module: `alias.private_name` resolves to nothing and is reported, so
    /// privacy cannot be bypassed by importing the namespace instead of the names.
    fn rewrite_alias_access(&self, receiver: &HirExpr, member: &str) -> Option<String> {
        let HirExpr::Identifier(name, _) = receiver else {
            return None;
        };
        if self.local_names.contains(name) {
            return None;
        }
        let binding = self.aliases.get(name)?;
        if !binding.exported.contains(member) {
            self.violations.borrow_mut().push(AliasViolation {
                module: binding.module.clone(),
                member: member.to_string(),
            });
            return None;
        }
        Some(self.rewrite_identifier(member.to_string()))
    }

    /// Reports every rejected `alias.member` access into `diagnostics`.
    ///
    /// A call site is rewritten twice (`alias.fn(...)` is first treated as a namespace call and
    /// then walked as an ordinary expression), so the same access can be collected more than
    /// once; reporting is deduplicated so each rejected access produces one diagnostic.
    fn report_violations(&self, imported_from: &str, diagnostics: &mut Vec<Diagnostic>) {
        let mut reported: HashSet<(String, String)> = HashSet::new();
        for violation in self.violations.borrow().iter() {
            if !reported.insert((violation.module.clone(), violation.member.clone())) {
                continue;
            }
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_SEM_EXPORT_UNKNOWN,
                    format!(
                        "module '{}' does not export '{}'",
                        violation.module, violation.member
                    ),
                )
                .with_note(format!("while resolving imports of {imported_from}")),
            );
        }
    }
}

/// Builds the namespace bindings an importing surface gets from its imports.
///
/// `import module` and `import module as alias` both bind a namespace, under the module's final
/// segment or under the alias. `import module: names` binds only the selected names, so it
/// introduces no namespace — which is why it is skipped here.
fn namespace_bindings(
    bindings: &[ImportBinding],
    exported_by_module: &HashMap<&str, &HashSet<String>>,
) -> HashMap<String, NamespaceBinding> {
    let mut namespaces = HashMap::new();
    for binding in bindings {
        let local = match &binding.alias {
            Some(alias) => alias.clone(),
            None if binding.names.is_empty() => binding.local_namespace().to_string(),
            None => continue,
        };
        let exported = exported_by_module
            .get(binding.module.as_str())
            .map_or_else(HashSet::new, |names| (*names).clone());
        namespaces.insert(
            local,
            NamespaceBinding {
                module: binding.module.clone(),
                exported,
            },
        );
    }
    namespaces
}

fn rewrite_impl(block: &mut HirImplBlock, scope: &ModuleScope) {
    if let Some(init) = &mut block.init {
        rewrite_function(init, scope);
    }
    // `InvariantHook` still holds AST expressions (`aipo_ast::Expr`), not HIR ones, so its
    // conditions are not renamed here. Invariants only reference the receiver's own fields,
    // which are never module-private, so no rewrite is required.
    for method in &mut block.methods {
        rewrite_function(method, scope);
    }
}

fn rewrite_function(decl: &mut HirFunctionDecl, scope: &ModuleScope) {
    for statement in &mut decl.body {
        *statement = rewrite_stmt(statement.clone(), scope);
    }
}

fn rewrite_block(statements: &mut [HirStmt], scope: &ModuleScope) {
    for statement in statements {
        *statement = rewrite_stmt(statement.clone(), scope);
    }
}

fn rewrite_stmt(statement: HirStmt, scope: &ModuleScope) -> HirStmt {
    match statement {
        HirStmt::Let(name, expr, span) => HirStmt::Let(
            scope.rewrite_identifier(name),
            rewrite_expr(expr, scope),
            span,
        ),
        HirStmt::Var(name, expr, span) => HirStmt::Var(
            scope.rewrite_identifier(name),
            rewrite_expr(expr, scope),
            span,
        ),
        HirStmt::Assign(target, value, span) => HirStmt::Assign(
            rewrite_expr(target, scope),
            rewrite_expr(value, scope),
            span,
        ),
        HirStmt::CompoundAssign(op, target, value, span) => HirStmt::CompoundAssign(
            op,
            rewrite_expr(target, scope),
            rewrite_expr(value, scope),
            span,
        ),
        HirStmt::If(mut stmt) => {
            stmt.condition = rewrite_expr(stmt.condition, scope);
            rewrite_block(&mut stmt.then_branch, scope);
            for (condition, body) in &mut stmt.elif_branches {
                *condition = rewrite_expr(condition.clone(), scope);
                rewrite_block(body, scope);
            }
            if let Some(branch) = &mut stmt.else_branch {
                rewrite_block(branch, scope);
            }
            HirStmt::If(stmt)
        }
        HirStmt::Match(mut stmt) => {
            stmt.target = rewrite_expr(stmt.target, scope);
            for arm in &mut stmt.when_arms {
                for pattern in &mut arm.patterns {
                    if let aipo_hir::HirMatchPattern::Value(expr) = pattern {
                        *expr = rewrite_expr(expr.clone(), scope);
                    }
                }
                if let Some(guard) = &mut arm.guard {
                    *guard = rewrite_expr(guard.clone(), scope);
                }
                rewrite_block(&mut arm.body, scope);
            }
            if let Some(arm) = &mut stmt.else_arm {
                rewrite_block(arm, scope);
            }
            HirStmt::Match(stmt)
        }
        HirStmt::Loop(mut body, span) => {
            rewrite_block(&mut body, scope);
            HirStmt::Loop(body, span)
        }
        HirStmt::While(condition, mut body, span) => {
            let condition = rewrite_expr(condition, scope);
            rewrite_block(&mut body, scope);
            HirStmt::While(condition, body, span)
        }
        HirStmt::Repeat(count, bind, mut body, span) => {
            let count = rewrite_expr(count, scope);
            rewrite_block(&mut body, scope);
            HirStmt::Repeat(count, bind, body, span)
        }
        HirStmt::Each(bindings, iterable, mut body, span) => {
            let iterable = rewrite_expr(iterable, scope);
            rewrite_block(&mut body, scope);
            HirStmt::Each(bindings, iterable, body, span)
        }
        HirStmt::Return(expr, span) => {
            HirStmt::Return(expr.map(|expr| rewrite_expr(expr, scope)), span)
        }
        HirStmt::Fail(expr, span) => HirStmt::Fail(rewrite_expr(expr, scope), span),
        HirStmt::Attempt(mut stmt) => {
            rewrite_block(&mut stmt.body, scope);
            rewrite_block(&mut stmt.handler, scope);
            HirStmt::Attempt(stmt)
        }
        HirStmt::Expr(expr) => HirStmt::Expr(rewrite_expr(expr, scope)),
        HirStmt::FnDecl(mut f) => {
            rewrite_function(&mut f, scope);
            HirStmt::FnDecl(f)
        }
        other => other,
    }
}

fn rewrite_expr(expr: HirExpr, scope: &ModuleScope) -> HirExpr {
    match expr {
        HirExpr::Identifier(name, span) => {
            HirExpr::Identifier(scope.rewrite_identifier(name), span)
        }
        HirExpr::Unary(op, operand, span) => {
            HirExpr::Unary(op, Box::new(rewrite_expr(*operand, scope)), span)
        }
        HirExpr::Binary(op, left, right, span) => HirExpr::Binary(
            op,
            Box::new(rewrite_expr(*left, scope)),
            Box::new(rewrite_expr(*right, scope)),
            span,
        ),
        HirExpr::Call(callee, args, span) => HirExpr::Call(
            Box::new(rewrite_alias_callee(*callee, scope)),
            args.into_iter()
                .map(|arg| HirCallArg {
                    name: arg.name,
                    value: rewrite_expr(arg.value, scope),
                    span: arg.span,
                })
                .collect(),
            span,
        ),
        HirExpr::Dot(receiver, member, span) => {
            match scope.rewrite_alias_access(&receiver, &member) {
                Some(resolved) => HirExpr::Identifier(resolved, span),
                None => HirExpr::Dot(Box::new(rewrite_expr(*receiver, scope)), member, span),
            }
        }
        HirExpr::QuestionDot(receiver, member, span) => {
            HirExpr::QuestionDot(Box::new(rewrite_expr(*receiver, scope)), member, span)
        }
        HirExpr::Index(receiver, index, span) => HirExpr::Index(
            Box::new(rewrite_expr(*receiver, scope)),
            Box::new(rewrite_expr(*index, scope)),
            span,
        ),
        HirExpr::List(items, span) => HirExpr::List(
            items
                .into_iter()
                .map(|item| rewrite_expr(item, scope))
                .collect(),
            span,
        ),
        HirExpr::Dict(pairs, span) => HirExpr::Dict(
            pairs
                .into_iter()
                .map(|(key, value)| (rewrite_expr(key, scope), rewrite_expr(value, scope)))
                .collect(),
            span,
        ),
        HirExpr::Construct(name, fields, span) => HirExpr::Construct(
            scope.rewrite_identifier(name),
            fields
                .into_iter()
                .map(|(field, value)| (field, rewrite_expr(value, scope)))
                .collect(),
            span,
        ),
        HirExpr::Fn(mut function) => {
            rewrite_block(&mut function.body, scope);
            HirExpr::Fn(function)
        }
        HirExpr::If(condition, then_branch, else_branch, span) => HirExpr::If(
            Box::new(rewrite_expr(*condition, scope)),
            Box::new(rewrite_expr(*then_branch, scope)),
            Box::new(rewrite_expr(*else_branch, scope)),
            span,
        ),
        HirExpr::OrElse(left, right, span) => HirExpr::OrElse(
            Box::new(rewrite_expr(*left, scope)),
            Box::new(rewrite_expr(*right, scope)),
            span,
        ),
        other => other,
    }
}

/// Rewrites a call callee that names an imported member (`alias.fn(...)`).
fn rewrite_alias_callee(callee: HirExpr, scope: &ModuleScope) -> HirExpr {
    if let HirExpr::Dot(receiver, member, span) = &callee
        && let Some(resolved) = scope.rewrite_alias_access(receiver, member)
    {
        return HirExpr::Identifier(resolved, *span);
    }
    rewrite_expr(callee, scope)
}
