//! Native function and built-in module registration catalog.

use std::collections::HashMap;

/// Metadata describing a native function or method provided by the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFunctionMeta {
    /// Function identifier.
    pub name: String,
    /// Expected parameter count.
    pub arity: usize,
    /// Parent module path (`None` for Prelude functions).
    pub module: Option<String>,
    /// Brief documentation of purpose.
    pub doc: String,
    /// Capability paths required by the native function, for catalog consumers.
    pub capabilities: Vec<String>,
    /// Whether calling the native produces a `Task` instead of a value.
    pub is_async: bool,
    /// Whether `arity` is a *lower bound* instead of an exact count.
    ///
    /// The VM ships natives whose arity is deliberately unchecked
    /// (`Value::native(name, usize::MAX, f)`) because the implementation accepts an
    /// open tail: `path.join` concatenates any number of parts, `log.info` takes an
    /// optional fields dict, `expect.approx` an optional tolerance. A consumer that
    /// enforces `arity` as a maximum would reject those calls, so the catalog has to
    /// say which entries are open-ended.
    pub is_variadic: bool,
    /// Names of the parameters in declaration order.
    ///
    /// When empty, consumers synthesize `arg0`, `arg1`, ... . Supplying real names
    /// enables named-argument calls (`json.stringify(value, pretty = true)`).
    pub param_names: Vec<String>,
    /// Number of trailing parameters that may be omitted.
    ///
    /// A value of `N` marks the last `N` entries of `param_names`/arity as
    /// optional, so the minimum accepted argument count is `arity - N`.
    pub optional_params: usize,
}

impl NativeFunctionMeta {
    /// Creates new native function metadata.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        arity: usize,
        module: Option<&str>,
        doc: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            arity,
            module: module.map(ToString::to_string),
            doc: doc.into(),
            capabilities: Vec::new(),
            is_async: false,
            is_variadic: false,
            param_names: Vec::new(),
            optional_params: 0,
        }
    }

    /// Marks the native as asynchronous.
    #[must_use]
    pub fn with_async(mut self) -> Self {
        self.is_async = true;
        self
    }

    /// Marks `arity` as a lower bound: the native accepts additional arguments.
    #[must_use]
    pub fn with_variadic(mut self) -> Self {
        self.is_variadic = true;
        self
    }

    /// Sets the parameter names in declaration order.
    #[must_use]
    pub fn with_param_names<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.param_names = names.into_iter().map(Into::into).collect();
        self
    }

    /// Marks the last `count` parameters as optional.
    #[must_use]
    pub fn with_optional_params(mut self, count: usize) -> Self {
        self.optional_params = count;
        self
    }

    /// Adds the capability paths declared by this native function.
    #[must_use]
    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.capabilities = capabilities
            .into_iter()
            .map(|capability| capability.as_ref().to_string())
            .collect();
        self
    }
}

/// Catalog of native functions registered for prelude and built-in modules.
///
/// Introspection/tooling catalog only (auditoria R-2): the VM dispatches natives
/// by name in `aipo-vm/src/vm/call.rs` and never consults this registry on the
/// hot path. `get()` therefore allocates on lookup; that is acceptable because
/// this type serves documentation, CLI help and tests — not per-call dispatch.
#[derive(Debug, Default, Clone)]
pub struct NativeRegistry {
    functions: HashMap<(Option<String>, String), NativeFunctionMeta>,
}

impl NativeRegistry {
    /// Constructs a new empty native registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a native function.
    ///
    /// Returns `true` when an entry with the same `(module, name)` was replaced, and
    /// `false` on a fresh insert, so callers can detect collisions instead of losing
    /// the previous metadata silently (auditoria N-3). An empty module string is
    /// normalized to `None`, the Prelude slot (auditoria N-7); names are matched
    /// exactly (case-sensitive).
    pub fn register(&mut self, mut meta: NativeFunctionMeta) -> bool {
        if meta.module.as_deref() == Some("") {
            meta.module = None;
        }
        let key = (meta.module.clone(), meta.name.clone());
        self.functions.insert(key, meta).is_some()
    }

    /// Looks up a native function by optional module path and name.
    ///
    /// The module comparison is exact and case-sensitive.
    #[must_use]
    pub fn get(&self, module: Option<&str>, name: &str) -> Option<&NativeFunctionMeta> {
        let key = (module.map(ToString::to_string), name.to_string());
        self.functions.get(&key)
    }

    /// Lists all native functions belonging to the global Prelude, ordered by name.
    #[must_use]
    pub fn list_prelude(&self) -> Vec<&NativeFunctionMeta> {
        let mut found: Vec<&NativeFunctionMeta> = self
            .functions
            .iter()
            .filter(|((m, _), _)| m.is_none())
            .map(|(_, meta)| meta)
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    /// Lists all native functions belonging to a specific built-in module, ordered by name.
    #[must_use]
    pub fn list_module(&self, module: &str) -> Vec<&NativeFunctionMeta> {
        let mut found: Vec<&NativeFunctionMeta> = self
            .functions
            .iter()
            .filter(|((m, _), _)| m.as_deref() == Some(module))
            .map(|(_, meta)| meta)
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    /// Returns a list of distinct built-in module names registered.
    #[must_use]
    pub fn list_modules(&self) -> Vec<String> {
        let mut mods: Vec<_> = self
            .functions
            .keys()
            .filter_map(|(m, _)| m.clone())
            .collect();
        mods.sort();
        mods.dedup();
        mods
    }
}
