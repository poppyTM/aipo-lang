//! Description of the globally visible language surface.

use crate::symbol::SymbolKind;
use std::collections::{HashMap, HashSet};

/// The set of names that are visible at the root scope before any user declaration.
///
/// The analyzer itself knows only the *language* surface: the fundamental values
/// (`none`, `true`, `false`) and the core type values (`Int`, `Float`, `Byte`, `String`,
/// `Bool`, `List`, `Dict`, `Bytes`, `Range`). Everything the standard library adds is
/// supplied by the caller through [`crate::check_with_prelude`], so `aipo-sema` never has
/// to depend on `aipo-stdlib` and the two can never drift silently: the composition root
/// (the CLI) derives the surface from the *same* registration the VM executes with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreludeSurface {
    globals: HashMap<String, SymbolKind>,
    mutable_globals: HashSet<String>,
    host_modules: HashMap<String, HashMap<String, HostFunction>>,
}

/// Function signature supplied by a host schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFunction {
    /// Parameters in declaration order.
    pub params: Vec<HostParameter>,
    /// Whether the function accepts arguments beyond `params`.
    ///
    /// A host surface may describe only the checked prefix of an open-ended
    /// function (`path.join`, `log.info`), in which case `params.len()` is a
    /// lower bound and must not be enforced as a maximum.
    pub variadic: bool,
}

/// Parameter signature supplied by a host schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostParameter {
    /// Parameter name.
    pub name: String,
    /// Type contract name.
    pub ty: String,
    /// Whether `none` satisfies the contract.
    pub nullable: bool,
    /// Whether the parameter may be omitted.
    pub optional: bool,
}

impl PreludeSurface {
    /// Creates an empty surface.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the surface of the Aipo language itself, without the standard library.
    #[must_use]
    pub fn fundamental() -> Self {
        let mut surface = Self::new();
        for name in ["none", "true", "false"] {
            surface.add_variable(name);
        }
        for name in [
            "Int", "Float", "Byte", "String", "Bool", "List", "Dict", "Bytes", "Range", "Set",
            "Duration",
        ] {
            surface.add_variable(name);
        }
        surface
    }

    /// Adds a variable-like or type-value global.
    pub fn add_variable(&mut self, name: &str) {
        self.mutable_globals.remove(name);
        self.globals.insert(name.to_string(), SymbolKind::Variable);
    }

    /// Adds an existing mutable guest binding, for persistent compilation sessions.
    pub fn add_mutable_variable(&mut self, name: &str) {
        self.add_variable(name);
        self.mutable_globals.insert(name.into());
    }
    /// Whether a supplied binding can be reassigned.
    pub fn is_mutable(&self, name: &str) -> bool {
        self.mutable_globals.contains(name)
    }

    /// Adds a global function with its accepted argument counts.
    pub fn add_function(&mut self, name: &str, min_args: usize, max_args: usize) {
        self.globals.insert(
            name.to_string(),
            SymbolKind::Function { min_args, max_args },
        );
    }

    /// Returns whether `name` is part of the surface.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.globals.contains_key(name)
    }

    /// Registers a host module and its function signatures.
    pub fn add_host_module(&mut self, module: &str, functions: HashMap<String, HostFunction>) {
        self.add_variable(module);
        self.host_modules.insert(module.to_string(), functions);
    }

    /// Looks up a host function signature if `module` is a registered host module.
    #[must_use]
    pub fn host_function(&self, module: &str, function: &str) -> Option<&HostFunction> {
        self.host_modules.get(module)?.get(function)
    }

    /// Returns whether `module` was registered as a host module.
    #[must_use]
    pub fn is_host_module(&self, module: &str) -> bool {
        self.host_modules.contains_key(module)
    }

    /// Iterates over the surface entries.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SymbolKind)> {
        self.globals.iter()
    }

    /// Iterates over host module signatures.
    pub fn host_modules(&self) -> impl Iterator<Item = (&String, &HashMap<String, HostFunction>)> {
        self.host_modules.iter()
    }

    /// Number of names in the surface.
    #[must_use]
    pub fn len(&self) -> usize {
        self.globals.len()
    }

    /// Returns whether the surface is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.globals.is_empty()
    }
}
