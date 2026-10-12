//! Capability-aware filesystem operations for Aipo hosts.
//!
//! This module exposes `fs.read_text`, `fs.write_text`, and `fs.roots`. `read_text` and
//! `write_text` are asynchronous and return a VM-managed task; `roots` is synchronous. All three
//! consult the provider installed in the current VM's [`HostContext`](aipo_vm::HostContext) and
//! require separate capabilities: `filesystem.read`, `filesystem.write`, and `filesystem.roots`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::string::normalize_nfc_str;
use aipo_host::Capability;
use aipo_host::ahs::{FunctionSchema, HostSchema, ModuleSchema, ParamSchema, TypeRef};
use aipo_runtime::{NativeFunctionMeta, NativeRegistry};
use aipo_vm::{
    DictMap, FailureValue, FilesystemError, FilesystemSource, Value, Vm, VmError, VmFault,
};

/// Maximum text size accepted by the deterministic map provider.
pub const MAX_MAP_READ_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// A deterministic, in-memory filesystem provider.
///
/// Roots are stored as a sorted set and files as an ordered map. The provider owns the lexical
/// root policy: a path must be a valid, normalized path inside one configured root. Relative
/// paths are accepted when exactly one root makes their resolution unambiguous. The map has no
/// symlinks; a real host provider must apply the equivalent no-escape policy before reading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapFilesystemSource {
    roots: BTreeSet<String>,
    files: BTreeMap<String, String>,
}

impl MapFilesystemSource {
    /// Creates a provider from roots and path/content entries.
    ///
    /// Inputs are checked when the provider is read or when a file is inserted. Use
    /// [`Self::try_new`] when construction itself must reject an invalid policy.
    #[must_use]
    pub fn new<I, K, J, P, V>(roots: I, files: J) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
        J: IntoIterator<Item = (P, V)>,
        P: Into<String>,
        V: Into<String>,
    {
        Self {
            roots: roots.into_iter().map(Into::into).collect(),
            files: files
                .into_iter()
                .map(|(path, content)| (path.into(), content.into()))
                .collect(),
        }
    }

    /// Creates a provider from roots and path/content entries.
    #[must_use]
    pub fn from_roots_and_files<I, K, J, P, V>(roots: I, files: J) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
        J: IntoIterator<Item = (P, V)>,
        P: Into<String>,
        V: Into<String>,
    {
        Self::new(roots, files)
    }

    /// Creates an empty provider.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a provider with roots and no files.
    #[must_use]
    pub fn from_roots<I, K>(roots: I) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        Self::new(roots, Vec::<(String, String)>::new())
    }

    /// Creates a provider after validating every root and file path.
    pub fn try_new<I, K, J, P, V>(roots: I, files: J) -> Result<Self, FilesystemError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
        J: IntoIterator<Item = (P, V)>,
        P: Into<String>,
        V: Into<String>,
    {
        let mut provider = Self::new(roots, Vec::<(String, String)>::new());
        provider.validate_roots()?;
        for (path, content) in files {
            provider.insert(path, content)?;
        }
        Ok(provider)
    }

    /// Validated variant of [`Self::from_roots_and_files`].
    pub fn try_from_roots_and_files<I, K, J, P, V>(
        roots: I,
        files: J,
    ) -> Result<Self, FilesystemError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
        J: IntoIterator<Item = (P, V)>,
        P: Into<String>,
        V: Into<String>,
    {
        Self::try_new(roots, files)
    }

    /// Inserts or replaces a file after applying the provider's root policy.
    pub fn insert<P, V>(&mut self, path: P, content: V) -> Result<Option<String>, FilesystemError>
    where
        P: Into<String>,
        V: Into<String>,
    {
        let roots = self.validate_roots()?;
        let path = path.into();
        let canonical = normalize_path(&path, false)?;
        let key = qualify_path(&canonical, &roots, &path)?;
        let content = content.into();
        if content.len() > MAX_MAP_READ_TEXT_BYTES {
            return Err(FilesystemError::Provider {
                operation: "read_text".to_string(),
                message: format!("file exceeds the {MAX_MAP_READ_TEXT_BYTES}-byte text limit"),
            });
        }
        Ok(self.files.insert(key, content))
    }

    /// Alias for [`Self::insert`].
    pub fn insert_file<P, V>(
        &mut self,
        path: P,
        content: V,
    ) -> Result<Option<String>, FilesystemError>
    where
        P: Into<String>,
        V: Into<String>,
    {
        self.insert(path, content)
    }

    /// Removes a file from the deterministic map.
    pub fn remove(&mut self, path: &str) -> Result<Option<String>, FilesystemError> {
        let roots = self.validate_roots()?;
        let canonical = normalize_path(path, false)?;
        let key = qualify_path(&canonical, &roots, path)?;
        Ok(self.files.remove(&key))
    }

    /// Reads a text path through this provider.
    pub fn read_text(&self, path: &str) -> Result<Option<String>, FilesystemError> {
        self.read_text_impl(path)
    }

    /// Returns the sorted roots exposed by this provider.
    pub fn roots(&self) -> Result<Vec<String>, FilesystemError> {
        self.validate_roots()
    }

    /// Returns the sorted roots exposed by this provider.
    pub fn root_paths(&self) -> Result<Vec<String>, FilesystemError> {
        self.validate_roots()
    }

    /// Returns the number of files in the deterministic map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the deterministic map has no files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn validate_roots(&self) -> Result<Vec<String>, FilesystemError> {
        let mut roots = BTreeSet::new();
        for root in &self.roots {
            let normalized = normalize_path(root, true).map_err(|error| match error {
                FilesystemError::InvalidPath { reason, .. } => FilesystemError::InvalidRoot {
                    root: root.clone(),
                    reason,
                },
                other => other,
            })?;
            roots.insert(normalized);
        }
        Ok(roots.into_iter().collect())
    }

    fn read_text_impl(&self, path: &str) -> Result<Option<String>, FilesystemError> {
        let roots = self.validate_roots()?;
        if roots.is_empty() {
            return Err(FilesystemError::InvalidRoot {
                root: String::new(),
                reason: "provider has no configured roots".to_string(),
            });
        }
        let canonical = normalize_path(path, false)?;
        let absolute = canonical.starts_with('/');
        let candidates = if absolute {
            if !roots.iter().any(|root| path_in_root(&canonical, root)) {
                return Err(FilesystemError::InvalidPath {
                    path: path.to_string(),
                    reason: "path is outside the configured roots".to_string(),
                });
            }
            vec![canonical.clone()]
        } else if roots.len() == 1 {
            let root = &roots[0];
            if self.files.contains_key(&canonical) {
                vec![canonical.clone()]
            } else {
                vec![join_root(root, &canonical)]
            }
        } else {
            let matches = roots
                .iter()
                .filter_map(|root| {
                    let candidate = join_root(root, &canonical);
                    self.files.get(&candidate).map(|_| candidate)
                })
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(FilesystemError::InvalidPath {
                    path: path.to_string(),
                    reason: "relative path is ambiguous across multiple roots".to_string(),
                });
            }
            matches
        };
        for candidate in candidates {
            if let Some(content) = self.files.get(&candidate) {
                if content.len() > MAX_MAP_READ_TEXT_BYTES {
                    return Err(FilesystemError::Provider {
                        operation: "read_text".to_string(),
                        message: format!(
                            "file exceeds the {MAX_MAP_READ_TEXT_BYTES}-byte text limit"
                        ),
                    });
                }
                return Ok(Some(content.clone()));
            }
        }
        Ok(None)
    }
}

impl FilesystemSource for MapFilesystemSource {
    fn read_text(&self, path: &str) -> Result<Option<String>, FilesystemError> {
        self.read_text_impl(path)
    }

    fn write_text(&mut self, path: &str, content: &str) -> Result<(), FilesystemError> {
        self.insert(path, content)
            .map(|_| ())
            .map_err(|error| match error {
                FilesystemError::Provider { message, .. } => FilesystemError::Provider {
                    operation: "write_text".to_string(),
                    message,
                },
                other => other,
            })
    }

    fn roots(&self) -> Result<Vec<String>, FilesystemError> {
        self.validate_roots()
    }
}

/// Installs a filesystem provider in one VM without granting any capability.
pub fn install_filesystem_source<S>(vm: &mut Vm, source: S)
where
    S: FilesystemSource + 'static,
{
    vm.host_context().install_filesystem_source(source);
}

/// Alias for [`install_filesystem_source`].
pub fn install_filesystem<S>(vm: &mut Vm, source: S)
where
    S: FilesystemSource + 'static,
{
    install_filesystem_source(vm, source);
}

fn capability(path: &str) -> Result<Capability, VmError> {
    Capability::parse(path).map_err(|error| {
        VmFault::CorruptedBytecode {
            offset: 0,
            reason: format!("invalid filesystem capability: {error}"),
        }
        .into()
    })
}

fn require_capability(vm: &Vm, path: &str, operation: &str) -> Result<(), VmError> {
    let capability = capability(path)?;
    Ok(vm.host().require(&capability, operation)?)
}

fn require_provider(vm: &Vm, path: &str, operation: &str) -> Result<(), VmError> {
    require_capability(vm, path, operation)?;
    if vm.host().filesystem_source().is_none() {
        return Err(VmFault::CapabilityDenied {
            capability: path.to_string(),
            operation: operation.to_string(),
        }
        .into());
    }
    Ok(())
}

fn read_path(args: &[Value], operation: &str) -> Result<String, VmError> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: format!("1 argument for {operation}"),
            actual: format!("{} arguments", args.len()),
        }
        .into());
    }
    match &args[0] {
        Value::String(path) => Ok(path.as_str().to_string()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("String path for {operation}"),
            actual: other.type_name().to_string(),
        }
        .into()),
    }
}

fn write_args(args: &[Value], operation: &str) -> Result<(String, String), VmError> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: format!("2 arguments for {operation}"),
            actual: format!("{} arguments", args.len()),
        }
        .into());
    }
    let path = match &args[0] {
        Value::String(path) => path.as_str().to_string(),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: format!("String path for {operation}"),
                actual: other.type_name().to_string(),
            }
            .into());
        }
    };
    let content = match &args[1] {
        Value::String(text) => text.as_str().to_string(),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: format!("String content for {operation}"),
                actual: other.type_name().to_string(),
            }
            .into());
        }
    };
    Ok((path, content))
}

fn failure(message: impl Into<String>) -> Value {
    Value::Failure(Rc::new(FailureValue::new(message.into())))
}

fn provider_failure(error: &FilesystemError) -> Value {
    failure(error.to_string())
}

fn provider_contract_fault(error: &FilesystemError, operation: &str) -> VmError {
    VmFault::TypeMismatch {
        expected: format!("a valid {operation} provider result"),
        actual: error.to_string(),
    }
    .into()
}

/// Starts an asynchronous `fs.read_text(path)` call.
///
/// The returned task owns a copy of the path. Provider access and the read operation happen
/// only when the scheduler drives the task.
pub fn read_text(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = read_path(args, "fs.read_text")?;
    require_capability(vm, Capability::FILESYSTEM_READ, "fs.read_text")?;
    vm.create_host_task(read_text_task, vec![Value::String(Rc::new(path))])
}

/// Executes the hosted `fs.read_text` operation.
pub fn read_text_task(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let path = read_path(args, "fs.read_text")?;
    require_provider(vm, Capability::FILESYSTEM_READ, "fs.read_text")?;
    let Some(source) = vm.host().filesystem_source() else {
        return Err(VmFault::CapabilityDenied {
            capability: Capability::FILESYSTEM_READ.to_string(),
            operation: "fs.read_text".to_string(),
        }
        .into());
    };
    match source.read_text(&path) {
        Ok(Some(text)) => Ok(Value::String(Rc::new(normalize_nfc_str(&text)))),
        Ok(None) | Err(FilesystemError::NotFound { .. }) => {
            Ok(failure(format!("fs.read_text: file not found: {path}")))
        }
        Err(error @ FilesystemError::Provider { .. }) => Ok(provider_failure(&error)),
        Err(error) => Err(provider_contract_fault(&error, "fs.read_text")),
    }
}

/// Starts an asynchronous `fs.write_text(path, content)` call.
///
/// The returned task owns copies of the path and content. Provider access and the write happen
/// only when the scheduler drives the task.
pub fn write_text(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (path, content) = write_args(args, "fs.write_text")?;
    require_capability(vm, Capability::FILESYSTEM_WRITE, "fs.write_text")?;
    vm.create_host_task(
        write_text_task,
        vec![
            Value::String(Rc::new(path)),
            Value::String(Rc::new(content)),
        ],
    )
}

/// Executes the hosted `fs.write_text` operation.
pub fn write_text_task(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (path, content) = write_args(args, "fs.write_text")?;
    require_provider(vm, Capability::FILESYSTEM_WRITE, "fs.write_text")?;
    let Some(source) = vm.host_context().filesystem_source_mut() else {
        return Err(VmFault::CapabilityDenied {
            capability: Capability::FILESYSTEM_WRITE.to_string(),
            operation: "fs.write_text".to_string(),
        }
        .into());
    };
    match source.write_text(&path, &content) {
        Ok(()) => Ok(Value::None),
        Err(error @ FilesystemError::Provider { .. })
        | Err(error @ FilesystemError::NotFound { .. }) => Ok(provider_failure(&error)),
        Err(error) => Err(provider_contract_fault(&error, "fs.write_text")),
    }
}

/// Returns the provider's roots as an ordered `List<String>`.
pub fn roots(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if !args.is_empty() {
        return Err(VmFault::TypeMismatch {
            expected: "0 arguments for fs.roots".to_string(),
            actual: format!("{} arguments", args.len()),
        }
        .into());
    }
    require_provider(vm, Capability::FILESYSTEM_ROOTS, "fs.roots")?;
    let Some(source) = vm.host().filesystem_source() else {
        return Err(VmFault::CapabilityDenied {
            capability: Capability::FILESYSTEM_ROOTS.to_string(),
            operation: "fs.roots".to_string(),
        }
        .into());
    };
    match source.roots() {
        Ok(values) => {
            let mut normalized = BTreeSet::new();
            for value in values {
                let root = normalize_path(&value, true)
                    .map_err(|error| provider_contract_fault(&error, "fs.roots"))?;
                normalized.insert(root);
            }
            let roots: Vec<_> = normalized
                .into_iter()
                .map(|root| Value::String(Rc::new(root)))
                .collect();
            Ok(Value::List(Rc::new(RefCell::new((roots).into()))))
        }
        Err(error @ FilesystemError::Provider { .. })
        | Err(error @ FilesystemError::NotFound { .. }) => Ok(provider_failure(&error)),
        Err(error) => Err(provider_contract_fault(&error, "fs.roots")),
    }
}

fn unavailable(_args: &[Value]) -> Result<Value, VmFault> {
    Err(VmFault::CorruptedBytecode {
        offset: 0,
        reason: "filesystem native was called without its VM callback".to_string(),
    })
}

/// Creates the canonical `fs` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("read_text".to_string())),
            Value::native("fs.read_text", 1, unavailable),
        ),
        (
            Value::String(Rc::new("write_text".to_string())),
            Value::native("fs.write_text", 2, unavailable),
        ),
        (
            Value::String(Rc::new("roots".to_string())),
            Value::native("fs.roots", 0, unavailable),
        ),
    ];
    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}

/// Registers the VM callbacks for the filesystem module.
pub fn register_natives(vm: &mut Vm) {
    vm.register_host_native("fs.read_text", 1, read_text);
    vm.register_host_native("fs.write_text", 2, write_text);
    vm.register_host_native("fs.roots", 0, roots);
}

/// Registers filesystem metadata in the native catalog.
pub fn register_metadata(registry: &mut NativeRegistry) {
    registry.register(
        NativeFunctionMeta::new(
            "read_text",
            1,
            Some("fs"),
            "Asynchronously reads a text file inside an exposed filesystem root.",
        )
        .with_async()
        .with_capabilities([Capability::FILESYSTEM_READ]),
    );
    registry.register(
        NativeFunctionMeta::new(
            "write_text",
            2,
            Some("fs"),
            "Asynchronously writes text to a file inside an exposed filesystem root.",
        )
        .with_async()
        .with_capabilities([Capability::FILESYSTEM_WRITE]),
    );
    registry.register(
        NativeFunctionMeta::new(
            "roots",
            0,
            Some("fs"),
            "Returns the sorted filesystem roots exposed by the current host.",
        )
        .with_capabilities([Capability::FILESYSTEM_ROOTS]),
    );
}

/// Builds the AHS description consumed by host tooling.
#[must_use]
pub fn schema() -> HostSchema {
    HostSchema {
        host: "aipo-stdlib".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        modules: vec![ModuleSchema {
            name: "fs".to_string(),
            docs: Some(
                "Host-only asynchronous text reads and writes plus synchronous root discovery."
                    .to_string(),
            ),
            capabilities: Vec::new(),
            types: Vec::new(),
            handles: Vec::new(),
            values: Vec::new(),
            functions: vec![
                FunctionSchema {
                    name: "read_text".to_string(),
                    docs: Some("Reads a text file and returns a Task[String].".to_string()),
                    params: vec![ParamSchema {
                        name: "path".to_string(),
                        ty: type_ref("String"),
                        is_mut: false,
                        is_optional: false,
                        docs: Some("Provider-owned root-qualified path.".to_string()),
                    }],
                    returns: Some(type_ref("Task")),
                    is_async: true,
                    mutates_receiver: false,
                    capabilities: vec![Capability::FILESYSTEM_READ.to_string()],
                    subject: None,
                    deprecated: None,
                },
                FunctionSchema {
                    name: "write_text".to_string(),
                    docs: Some("Writes text to a file and returns a Task[Unit].".to_string()),
                    params: vec![
                        ParamSchema {
                            name: "path".to_string(),
                            ty: type_ref("String"),
                            is_mut: false,
                            is_optional: false,
                            docs: Some("Provider-owned root-qualified path.".to_string()),
                        },
                        ParamSchema {
                            name: "content".to_string(),
                            ty: type_ref("String"),
                            is_mut: false,
                            is_optional: false,
                            docs: Some("Text to write to the path.".to_string()),
                        },
                    ],
                    returns: Some(type_ref("Task")),
                    is_async: true,
                    mutates_receiver: false,
                    capabilities: vec![Capability::FILESYSTEM_WRITE.to_string()],
                    subject: None,
                    deprecated: None,
                },
                FunctionSchema {
                    name: "roots".to_string(),
                    docs: Some("Returns the provider's roots as List[String].".to_string()),
                    params: Vec::new(),
                    returns: Some(type_ref("List")),
                    is_async: false,
                    mutates_receiver: false,
                    capabilities: vec![Capability::FILESYSTEM_ROOTS.to_string()],
                    subject: None,
                    deprecated: None,
                },
            ],
        }],
    }
}

/// Alias for [`schema`].
#[must_use]
pub fn fs_schema() -> HostSchema {
    schema()
}

fn type_ref(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        nullable: false,
        args: Vec::new(),
    }
}

fn normalize_path(path: &str, _root: bool) -> Result<String, FilesystemError> {
    if path.is_empty() {
        return Err(FilesystemError::InvalidPath {
            path: path.to_string(),
            reason: "path is empty".to_string(),
        });
    }
    if path.contains('\0') || path.contains('\\') || path.chars().any(char::is_control) {
        return Err(FilesystemError::InvalidPath {
            path: path.to_string(),
            reason: "path contains a control character or backslash".to_string(),
        });
    }
    let absolute = path.starts_with('/');
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                let Some(last) = components.last() else {
                    if absolute {
                        return Err(FilesystemError::InvalidPath {
                            path: path.to_string(),
                            reason: "path escapes its root".to_string(),
                        });
                    }
                    return Err(FilesystemError::InvalidPath {
                        path: path.to_string(),
                        reason: "relative path escapes its root".to_string(),
                    });
                };
                if *last == ".." {
                    return Err(FilesystemError::InvalidPath {
                        path: path.to_string(),
                        reason: "path contains too many parent segments".to_string(),
                    });
                }
                components.pop();
            }
            other => components.push(other),
        }
    }
    if absolute {
        return Ok(if components.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", components.join("/"))
        });
    }
    Ok(if components.is_empty() {
        ".".to_string()
    } else {
        components.join("/")
    })
}

fn path_in_root(path: &str, root: &str) -> bool {
    if root == "/" {
        return path.starts_with('/');
    }
    if root == "." {
        return !path.starts_with('/');
    }
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn join_root(root: &str, path: &str) -> String {
    if root == "/" {
        format!("/{path}")
    } else if root == "." {
        path.to_string()
    } else {
        format!("{root}/{path}")
    }
}

fn qualify_path(
    canonical: &str,
    roots: &[String],
    original: &str,
) -> Result<String, FilesystemError> {
    if canonical.starts_with('/') {
        if roots.iter().any(|root| path_in_root(canonical, root)) {
            return Ok(canonical.to_string());
        }
        return Err(FilesystemError::InvalidPath {
            path: original.to_string(),
            reason: "path is outside the configured roots".to_string(),
        });
    }
    match roots {
        [root] => Ok(join_root(root, canonical)),
        [] => Err(FilesystemError::InvalidRoot {
            root: String::new(),
            reason: "provider has no configured roots".to_string(),
        }),
        _ => Err(FilesystemError::InvalidPath {
            path: original.to_string(),
            reason: "relative path is ambiguous across multiple roots".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipo_bytecode::{BytecodeModule, Constant, OpCode};
    use aipo_diagnostics::DiagnosticCode;
    use aipo_vm::VmError;

    fn make_vm() -> (Vm, NativeRegistry) {
        let mut vm = Vm::new();
        let mut registry = NativeRegistry::new();
        crate::register_stdlib(&mut vm, &mut registry);
        (vm, registry)
    }

    fn grant(vm: &mut Vm, capability: &str) {
        vm.host_context()
            .grant(Capability::parse(capability).expect("test capability"));
    }

    fn source() -> MapFilesystemSource {
        MapFilesystemSource::try_new(
            ["/workspace", "/shared"],
            [
                ("/workspace/config.txt", "configuration"),
                ("/shared/notes.txt", "notes"),
            ],
        )
        .expect("valid deterministic provider")
    }

    struct FailingSource {
        error: FilesystemError,
    }

    impl FilesystemSource for FailingSource {
        fn read_text(&self, _path: &str) -> Result<Option<String>, FilesystemError> {
            Err(self.error.clone())
        }

        fn roots(&self) -> Result<Vec<String>, FilesystemError> {
            Ok(Vec::new())
        }
    }

    fn string(value: &str) -> Value {
        Value::String(Rc::new(value.to_string()))
    }

    fn read_module(path: &str, await_result: bool) -> BytecodeModule {
        let mut module = BytecodeModule::new();
        module.names = vec!["fs".to_string(), "read_text".to_string()];
        module.constants = vec![Constant::String(path.to_string())];
        module.code = vec![
            OpCode::GetGlobal as u8,
            0,
            0,
            OpCode::GetField as u8,
            0,
            1,
            OpCode::Constant as u8,
            0,
            0,
            OpCode::Call as u8,
            1,
        ];
        if await_result {
            module.code.push(OpCode::Await as u8);
        }
        module.code.push(OpCode::Return as u8);
        module
    }

    fn roots_module() -> BytecodeModule {
        let mut module = BytecodeModule::new();
        module.names = vec!["fs".to_string(), "roots".to_string()];
        module.code = vec![
            OpCode::GetGlobal as u8,
            0,
            0,
            OpCode::GetField as u8,
            0,
            1,
            OpCode::Call as u8,
            0,
            OpCode::Return as u8,
        ];
        module
    }

    #[test]
    fn test_read_text_returns_task_and_await_returns_copied_text() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_READ);

        let task = read_text(&mut vm, &[string("/workspace/config.txt")])
            .expect("read call returns a task");
        assert!(matches!(task, Value::Task(_)));

        let result = vm
            .run(&read_module("/workspace/config.txt", true))
            .expect("await succeeds");
        assert_eq!(result, string("configuration"));
    }

    #[test]
    fn test_missing_file_is_a_recoverable_failure() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_READ);

        let result = read_text_task(&mut vm, &[string("/workspace/missing.txt")])
            .expect("provider failure is a value");
        assert!(matches!(result, Value::Failure(_)));
    }

    #[test]
    fn test_provider_error_is_recoverable_failure() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(
            &mut vm,
            FailingSource {
                error: FilesystemError::Provider {
                    operation: "read_text".to_string(),
                    message: "device unavailable".to_string(),
                },
            },
        );
        grant(&mut vm, Capability::FILESYSTEM_READ);
        let result =
            read_text_task(&mut vm, &[string("value")]).expect("provider error is a value");
        assert!(matches!(result, Value::Failure(_)));
    }

    #[test]
    fn test_invalid_provider_contract_is_a_typed_fault() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(
            &mut vm,
            FailingSource {
                error: FilesystemError::InvalidPath {
                    path: "value".to_string(),
                    reason: "provider rejected path".to_string(),
                },
            },
        );
        grant(&mut vm, Capability::FILESYSTEM_READ);
        let error = read_text_task(&mut vm, &[string("value")])
            .expect_err("invalid provider contract is not recoverable data");
        assert!(matches!(
            error,
            VmError::Fault(VmFault::TypeMismatch { .. })
        ));
    }

    #[test]
    fn test_read_and_roots_require_their_capabilities() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());

        let read_error = read_text(&mut vm, &[string("/workspace/config.txt")])
            .expect_err("read capability is required");
        assert_eq!(
            read_error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
        let roots_error = roots(&mut vm, &[]).expect_err("roots capability is required");
        assert_eq!(
            roots_error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_read_grant_does_not_grant_roots() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_READ);
        let error = roots(&mut vm, &[]).expect_err("roots requires its own grant");
        assert_eq!(
            error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_missing_provider_is_denied_for_roots() {
        let (mut vm, _registry) = make_vm();
        grant(&mut vm, Capability::FILESYSTEM_ROOTS);
        let error = roots(&mut vm, &[]).expect_err("roots provider is required");
        assert_eq!(
            error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_missing_provider_is_a_capability_fault_after_task_creation() {
        let (mut vm, _registry) = make_vm();
        grant(&mut vm, Capability::FILESYSTEM_READ);
        let task = read_text(&mut vm, &[string("/workspace/config.txt")])
            .expect("capability check happens at call time");
        assert!(matches!(task, Value::Task(_)));
        let error = read_text_task(&mut vm, &[string("/workspace/config.txt")])
            .expect_err("provider is required by the hosted callback");
        assert_eq!(
            error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_read_text_call_is_denied_before_creating_a_task() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        let error = vm
            .run(&read_module("/workspace/config.txt", true))
            .expect_err("read capability is required at call time");
        assert_eq!(
            error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_read_text_provider_denial_occurs_when_task_is_awaited() {
        let (mut vm, _registry) = make_vm();
        grant(&mut vm, Capability::FILESYSTEM_READ);
        let error = vm
            .run(&read_module("/workspace/config.txt", true))
            .expect_err("provider denial faults when the task runs");
        assert_eq!(
            error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_roots_are_sorted_and_capability_is_separate() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_ROOTS);
        let result = roots(&mut vm, &[]).expect("roots succeeds");
        let Value::List(values) = result else {
            panic!("roots must return a list");
        };
        assert_eq!(
            values.borrow().clone(),
            vec![string("/shared"), string("/workspace")]
        );
    }

    #[test]
    fn test_providers_are_isolated_per_vm() {
        let (mut first, _first_registry) = make_vm();
        let (mut second, _second_registry) = make_vm();
        install_filesystem_source(
            &mut first,
            MapFilesystemSource::try_new(["/first"], [("/first/value", "one")])
                .expect("first provider"),
        );
        install_filesystem_source(
            &mut second,
            MapFilesystemSource::try_new(["/second"], [("/second/value", "two")])
                .expect("second provider"),
        );
        grant(&mut first, Capability::FILESYSTEM_READ);
        grant(&mut second, Capability::FILESYSTEM_READ);
        assert_eq!(
            read_text_task(&mut first, &[string("/first/value")]).expect("first read"),
            string("one")
        );
        assert_eq!(
            read_text_task(&mut second, &[string("/second/value")]).expect("second read"),
            string("two")
        );
    }

    #[test]
    fn test_root_policy_rejects_escape_and_invalid_roots() {
        assert!(
            MapFilesystemSource::try_new(["../escape"], Vec::<(String, String)>::new()).is_err()
        );
        let provider = source();
        let error = provider
            .read_text("../outside")
            .expect_err("parent traversal is rejected");
        assert!(matches!(error, FilesystemError::InvalidPath { .. }));
        let outside = provider
            .read_text("/outside/value")
            .expect_err("absolute paths outside roots are rejected");
        assert!(matches!(outside, FilesystemError::InvalidPath { .. }));
    }

    #[test]
    fn test_metadata_and_schema_declare_exact_contracts() {
        let (_vm, registry) = make_vm();
        let read_meta = registry
            .get(Some("fs"), "read_text")
            .expect("read metadata");
        assert!(read_meta.is_async);
        assert_eq!(
            read_meta.capabilities,
            vec![Capability::FILESYSTEM_READ.to_string()]
        );
        let write_meta = registry
            .get(Some("fs"), "write_text")
            .expect("write metadata");
        assert!(write_meta.is_async);
        assert_eq!(
            write_meta.capabilities,
            vec![Capability::FILESYSTEM_WRITE.to_string()]
        );
        let roots_meta = registry.get(Some("fs"), "roots").expect("roots metadata");
        assert!(!roots_meta.is_async);
        assert_eq!(
            roots_meta.capabilities,
            vec![Capability::FILESYSTEM_ROOTS.to_string()]
        );

        let schema = schema();
        assert!(schema.validate().is_ok());
        assert!(
            schema
                .function("fs", "read_text")
                .expect("read schema")
                .is_async
        );
        assert!(
            schema
                .function("fs", "write_text")
                .expect("write schema")
                .is_async
        );
        assert_eq!(
            schema
                .declared_capabilities()
                .iter()
                .map(|capability| capability.name())
                .collect::<Vec<_>>(),
            vec!["filesystem.read", "filesystem.roots", "filesystem.write"]
        );
    }

    #[test]
    fn test_bytecode_roots_call_uses_the_current_vm_provider() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_ROOTS);
        let result = vm.run(&roots_module()).expect("roots bytecode succeeds");
        let Value::List(values) = result else {
            panic!("roots must return a list");
        };
        assert_eq!(values.borrow().len(), 2);
    }

    #[test]
    fn test_read_text_type_error_is_a_fault() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_READ);
        let error = read_text(&mut vm, &[Value::Int(1)]).expect_err("path must be String");
        assert!(matches!(
            error,
            VmError::Fault(VmFault::TypeMismatch { .. })
        ));
    }

    #[test]
    fn test_write_text_writes_and_resolves_to_none() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_WRITE);
        grant(&mut vm, Capability::FILESYSTEM_READ);

        let task = write_text(&mut vm, &[string("/workspace/new.txt"), string("hello")])
            .expect("write call returns a task");
        assert!(matches!(task, Value::Task(_)));

        let result = write_text_task(&mut vm, &[string("/workspace/new.txt"), string("hello")])
            .expect("write succeeds");
        assert_eq!(result, Value::None);

        let written = read_text_task(&mut vm, &[string("/workspace/new.txt")])
            .expect("written file is readable");
        assert_eq!(written, string("hello"));
    }

    #[test]
    fn test_write_text_overwrites_existing_file() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_WRITE);
        grant(&mut vm, Capability::FILESYSTEM_READ);

        write_text_task(
            &mut vm,
            &[string("/workspace/config.txt"), string("changed")],
        )
        .expect("overwrite succeeds");
        assert_eq!(
            read_text_task(&mut vm, &[string("/workspace/config.txt")]).expect("read back"),
            string("changed")
        );
    }

    #[test]
    fn test_write_text_requires_its_own_capability() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_READ);

        let error = write_text(&mut vm, &[string("/workspace/x"), string("y")])
            .expect_err("write requires filesystem.write");
        assert_eq!(
            error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_write_text_missing_provider_is_denied_after_task_creation() {
        let (mut vm, _registry) = make_vm();
        grant(&mut vm, Capability::FILESYSTEM_WRITE);

        let task = write_text(&mut vm, &[string("/workspace/x"), string("y")])
            .expect("capability check happens at call time");
        assert!(matches!(task, Value::Task(_)));
        let error = write_text_task(&mut vm, &[string("/workspace/x"), string("y")])
            .expect_err("provider is required by the hosted callback");
        assert_eq!(
            error.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_write_text_readonly_provider_is_a_recoverable_failure() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(
            &mut vm,
            FailingSource {
                error: FilesystemError::NotFound {
                    path: "unused".to_string(),
                },
            },
        );
        grant(&mut vm, Capability::FILESYSTEM_WRITE);

        let result = write_text_task(&mut vm, &[string("/workspace/x"), string("y")])
            .expect("a read-only provider reports a recoverable failure");
        assert!(matches!(result, Value::Failure(_)));
    }

    #[test]
    fn test_write_text_outside_root_is_a_contract_fault() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_WRITE);

        let error = write_text_task(&mut vm, &[string("/etc/passwd"), string("x")])
            .expect_err("paths outside the roots are not writable");
        assert!(matches!(
            error,
            VmError::Fault(VmFault::TypeMismatch { .. })
        ));
    }

    #[test]
    fn test_write_text_type_error_is_a_fault() {
        let (mut vm, _registry) = make_vm();
        install_filesystem_source(&mut vm, source());
        grant(&mut vm, Capability::FILESYSTEM_WRITE);

        let bad_path =
            write_text(&mut vm, &[Value::Int(1), string("y")]).expect_err("path must be String");
        assert!(matches!(
            bad_path,
            VmError::Fault(VmFault::TypeMismatch { .. })
        ));
        let bad_content = write_text(&mut vm, &[string("/workspace/x"), Value::Int(1)])
            .expect_err("content must be String");
        assert!(matches!(
            bad_content,
            VmError::Fault(VmFault::TypeMismatch { .. })
        ));
        let bad_arity = write_text(&mut vm, &[string("/workspace/x")])
            .expect_err("write_text needs two arguments");
        assert!(matches!(
            bad_arity,
            VmError::Fault(VmFault::TypeMismatch { .. })
        ));
    }
}
