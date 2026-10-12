//! Persistent compilation and execution for REPLs and reloadable hosts.
use crate::{
    analyze_full_with_catalog, emit_diagnostics, register_module_symbols, standard_environment,
};
use aipo_bytecode::{BytecodeModule, append_unit};
use aipo_diagnostics::{MessageFormat, Severity};
use aipo_sema::PreludeSurface;
use aipo_source::{Source, SourceId};
use aipo_vm::{Value, Vm};
use std::io::Write;
use std::path::Path;

/// Persistent globals and linked function bodies, independent of frontend input history.
pub struct Session {
    vm: Vm,
    linked: BytecodeModule,
    generation: u64,
    catalog: Vec<aipo_hir::HirItem>,
    mutable: std::collections::HashSet<String>,
    owners: std::collections::HashMap<String, std::path::PathBuf>,
}
impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
impl Session {
    /// Creates a session with the canonical standard environment.
    pub fn new() -> Self {
        Self {
            vm: standard_environment().0,
            linked: BytecodeModule::new(),
            generation: 0,
            catalog: Vec::new(),
            mutable: std::collections::HashSet::new(),
            owners: std::collections::HashMap::new(),
        }
    }
    /// Read access to globals for completion and embedding.
    pub fn globals(&self) -> &std::collections::HashMap<String, Value> {
        &self.vm.globals
    }
    /// Successful compilation-unit generation; faults do not advance it.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Configures an instruction limit per evaluation.
    pub fn set_instruction_budget(&mut self, limit: Option<u64>) {
        self.vm.set_max_instructions(limit);
    }
    /// Evaluates exactly one new unit, preserving old definitions without replaying old I/O.
    /// A failed unit restores guest definitions and reachable heap objects in place.
    pub fn eval(
        &mut self,
        text: &str,
        path: &Path,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> u8 {
        let catalog_declared: std::collections::HashSet<String> = self
            .catalog
            .iter()
            .filter_map(|item| match item {
                aipo_hir::HirItem::Fn(f) => Some(f.name.clone()),
                aipo_hir::HirItem::Struct(s) => Some(s.name.clone()),
                aipo_hir::HirItem::Enum(e) => Some(e.name.clone()),
                aipo_hir::HirItem::Interface(i) => Some(i.name.clone()),
                _ => None,
            })
            .collect();

        let mut surface = PreludeSurface::new();
        for (name, value) in &self.vm.globals {
            if catalog_declared.contains(name) {
                continue;
            }
            match value {
                Value::Function { arity, .. } => {
                    surface.add_function(name, *arity as usize, *arity as usize)
                }
                Value::Closure(value) => surface.add_function(name, value.arity, value.arity),
                _ => {
                    if self.mutable.contains(name) {
                        surface.add_mutable_variable(name);
                    } else {
                        surface.add_variable(name);
                    }
                }
            }
        }
        let source = Source::new(SourceId::next(), path.to_string_lossy(), text);
        self.vm.reset_instruction_count();
        let old_catalog = self.catalog.clone();
        let package_paths =
            match crate::resolve_local_package_paths(path, None, Some(text.as_bytes())) {
                Ok(paths) => paths,
                Err(error) => {
                    let _ = writeln!(err, "{}", crate::cli_error_message(error));
                    return 1;
                }
            };
        let compiled = analyze_full_with_catalog(
            &source,
            path,
            package_paths.as_ref(),
            Some(&surface),
            true,
            Some(&mut self.catalog),
        );
        emit_diagnostics(
            MessageFormat::Human,
            &source,
            &compiled.diagnostics,
            out,
            err,
        );
        if compiled
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
        {
            self.catalog = old_catalog;
            return 1;
        }
        let mut candidate = self.linked.clone();
        let entry = match append_unit(&mut candidate, &compiled.module) {
            Ok(entry) => entry,
            Err(errors) => {
                self.catalog = old_catalog;
                let _ = writeln!(err, "{}", errors.join("\n"));
                return 1;
            }
        };
        candidate.structs = compiled.module.structs.clone();
        let old_globals = self.vm.globals.clone();
        let snapshot = self.vm.snapshot_definitions();
        self.vm.replace_user_methods(&compiled.module);
        register_module_symbols(&mut self.vm, &compiled.module);
        match self.vm.run_persistent_at(&candidate, entry) {
            Ok(_) => {
                let (parsed, _) = aipo_syntax::parse(&source);
                let current = aipo_hir::lower(parsed);
                let changed: std::collections::HashSet<_> =
                    current.items.iter().filter_map(item_key).collect();
                for key in &changed {
                    self.owners.insert(key.clone(), path.to_path_buf());
                }
                for item in old_catalog {
                    if let aipo_hir::HirItem::Fn(function) = item
                        && !changed.contains(&format!("fn:{}", function.name))
                        && let Some(value) = old_globals.get(&function.name)
                    {
                        self.vm.define_global(function.name, value.clone());
                    }
                }
                for statement in current.statements {
                    match statement {
                        aipo_hir::HirStmt::Var(name, ..) => {
                            self.mutable.insert(name);
                        }
                        aipo_hir::HirStmt::Let(name, ..) => {
                            self.mutable.remove(&name);
                        }
                        _ => {}
                    }
                }
                self.linked = candidate;
                self.generation += 1;
                0
            }
            Err(error) => {
                self.catalog = old_catalog;
                self.vm.restore_definitions(snapshot);
                let _ = writeln!(err, "{error}");
                1
            }
        }
    }
    /// Reloads code while retaining existing non-callable globals; an optional host
    /// migration operates on the candidate state before it is committed.
    /// Guest state is rolled back when compilation, initialization or migration fails.
    pub fn reload_with(
        &mut self,
        text: &str,
        path: &Path,
        out: &mut dyn Write,
        err: &mut dyn Write,
        migrate: impl FnOnce(&mut std::collections::HashMap<String, Value>) -> Result<(), String>,
    ) -> u8 {
        let snapshot = self.vm.snapshot_definitions();
        let old = self.vm.globals.clone();
        let linked = self.linked.clone();
        let catalog = self.catalog.clone();
        let mutable = self.mutable.clone();
        let owners = self.owners.clone();
        let removed: std::collections::HashSet<_> = self
            .owners
            .iter()
            .filter(|(_, owner)| owner.as_path() == path)
            .map(|(key, _)| key.clone())
            .collect();
        self.catalog
            .retain(|item| item_key(item).is_none_or(|key| !removed.contains(&key)));
        for key in &removed {
            if let Some((kind, name)) = key.split_once(':')
                && matches!(kind, "fn" | "struct" | "enum" | "interface")
            {
                self.vm.remove_definition(name);
            }
            self.owners.remove(key);
        }
        let generation = self.generation;
        if self.eval(text, path, out, err) != 0 {
            self.vm.restore_definitions(snapshot);
            self.linked = linked;
            self.catalog = catalog;
            self.mutable = mutable;
            self.owners = owners;
            self.generation = generation;
            return 1;
        }
        for (name, value) in old {
            if !matches!(
                value,
                Value::Function { .. }
                    | Value::Closure(_)
                    | Value::Native(_)
                    | Value::UserType(_)
                    | Value::Type(_)
            ) && self.vm.globals.contains_key(&name)
            {
                self.vm.define_global(name, value);
            }
        }
        if let Err(message) =
            migrate(&mut self.vm.globals).and_then(|()| self.vm.validate_guest_layouts())
        {
            self.vm.restore_definitions(snapshot);
            self.linked = linked;
            self.catalog = catalog;
            self.mutable = mutable;
            self.owners = owners;
            self.generation = generation;
            let _ = writeln!(err, "migration failed: {message}");
            return 1;
        }
        for key in &removed {
            if !self.owners.contains_key(key)
                && let Some((kind, name)) = key.split_once(':')
                && matches!(kind, "fn" | "struct" | "enum" | "interface")
            {
                self.vm.remove_definition(name);
            }
        }
        // Refresh global slot caches after migration, never retain a stale lookup.
        if let Err(error) = self.vm.prepare_module_execution(&self.linked) {
            self.vm.restore_definitions(snapshot);
            self.linked = linked;
            self.catalog = catalog;
            self.mutable = mutable;
            self.owners = owners;
            self.generation = generation;
            let _ = writeln!(err, "{error}");
            return 1;
        }
        0
    }
    /// Clears all guest definitions and linked units.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
    /// Lists matching global names in deterministic order.
    pub fn complete(&self, prefix: &str) -> Vec<String> {
        let mut names: Vec<_> = self
            .vm
            .globals
            .keys()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect();
        names.sort();
        names
    }
}

pub(crate) fn item_key(item: &aipo_hir::HirItem) -> Option<String> {
    use aipo_hir::HirItem::*;
    Some(match item {
        Fn(value) => format!("fn:{}", value.name),
        Struct(value) => format!("struct:{}", value.name),
        Enum(value) => format!("enum:{}", value.name),
        Interface(value) => format!("interface:{}", value.name),
        Impl(value) => format!(
            "impl:{}:{}:{}:{}",
            value.target,
            value.init.is_some(),
            value.invariant.is_some(),
            value
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
        Batch(value) => format!("batch:{value:?}"),
        _ => return None,
    })
}
