//! VM-side adapter for the host ABI (`aipo-host`).
//!
//! `aipo-host` owns the contracts a host and a script share; this module is the one place
//! that consumes them, so the value model, the fault model and the capability gate have a
//! single conversion point instead of each call site inventing its own.
//!
//! Three conversions live here:
//!
//! - **Values.** A [`HostValue`] becomes a [`Value`] and back. Host values are plain data, so
//!   the conversion never hands a script a Rust reference; a `String` is normalized to NFC as
//!   it becomes a language value, because NFC belongs to the language's `String` boundary and
//!   the host side deliberately does not duplicate that rule.
//! - **Faults.** A [`HostFault`] becomes a [`VmFault`], never a panic and never a recoverable
//!   `Failure`: canon classifies a denied capability, a stale handle and an escaped binding as
//!   programming errors at the boundary.
//! - **Capabilities.** [`HostContext`] holds the deny-by-default [`CapabilitySet`] the running
//!   profile granted plus the host objects behind their handles. A privileged operation calls
//!   [`HostContext::require`] first, so a missing capability is always the same fault.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use aipo_host::{Capability, CapabilitySet, Handle, HandleTable, HostFault, HostValue};
#[cfg(feature = "unicode")]
use unicode_normalization::UnicodeNormalization;

use crate::fault::VmFault;
use crate::value::{Value, check_finite_float, check_safe_int};

/// A typed failure returned by a filesystem provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesystemError {
    /// The requested path is not present in the provider.
    NotFound {
        /// Path that was not found.
        path: String,
    },
    /// The provider could not complete the requested operation.
    Provider {
        /// Operation that failed, such as `read_text` or `roots`.
        operation: String,
        /// Provider-defined explanation.
        message: String,
    },
    /// The path does not satisfy the provider's path policy.
    InvalidPath {
        /// Rejected path.
        path: String,
        /// Policy explanation.
        reason: String,
    },
    /// A configured root does not satisfy the provider's root policy.
    InvalidRoot {
        /// Rejected root.
        root: String,
        /// Policy explanation.
        reason: String,
    },
}

impl fmt::Display for FilesystemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { path } => write!(formatter, "filesystem path not found: {path}"),
            Self::Provider { operation, message } => {
                write!(
                    formatter,
                    "filesystem provider failed during {operation}: {message}"
                )
            }
            Self::InvalidPath { path, reason } => {
                write!(formatter, "invalid filesystem path '{path}': {reason}")
            }
            Self::InvalidRoot { root, reason } => {
                write!(formatter, "invalid filesystem root '{root}': {reason}")
            }
        }
    }
}

impl std::error::Error for FilesystemError {}

impl FilesystemError {
    /// Creates a missing-file error.
    #[must_use]
    pub fn not_found(path: impl Into<String>) -> Self {
        Self::NotFound { path: path.into() }
    }

    /// Creates a provider-operation error.
    #[must_use]
    pub fn provider(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Provider {
            operation: operation.into(),
            message: message.into(),
        }
    }

    /// Creates an invalid-path error.
    #[must_use]
    pub fn invalid_path(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidPath {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Creates an invalid-root error.
    #[must_use]
    pub fn invalid_root(root: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidRoot {
            root: root.into(),
            reason: reason.into(),
        }
    }
}

/// Descriptive alias for [`FilesystemError`].
pub type FilesystemSourceError = FilesystemError;

/// A filesystem policy and data source owned by one VM host context.
///
/// Implementations return owned strings and never expose provider-owned references. The VM
/// stores the trait object per VM; this module deliberately does not choose a filesystem
/// implementation or perform host I/O.
pub trait FilesystemSource {
    /// Reads text from `path`, returning `Ok(None)` when the path is absent.
    fn read_text(&self, path: &str) -> Result<Option<String>, FilesystemError>;

    /// Writes `content` to `path`, creating or replacing it inside an exposed root.
    ///
    /// # Errors
    ///
    /// [`FilesystemError`] when the path violates the provider's policy or the write fails.
    /// Read-only providers keep this default, which reports a write as a recoverable provider
    /// failure instead of panicking.
    fn write_text(&mut self, path: &str, content: &str) -> Result<(), FilesystemError> {
        let _ = (path, content);
        Err(FilesystemError::Provider {
            operation: "write_text".to_string(),
            message: "filesystem provider is read-only".to_string(),
        })
    }

    /// Returns the roots exposed by this provider in deterministic order.
    fn roots(&self) -> Result<Vec<String>, FilesystemError>;
}

impl<T> FilesystemSource for Box<T>
where
    T: FilesystemSource + ?Sized,
{
    fn read_text(&self, path: &str) -> Result<Option<String>, FilesystemError> {
        (**self).read_text(path)
    }

    fn write_text(&mut self, path: &str, content: &str) -> Result<(), FilesystemError> {
        (**self).write_text(path, content)
    }

    fn roots(&self) -> Result<Vec<String>, FilesystemError> {
        (**self).roots()
    }
}

/// A source of environment values owned by one VM host context.
///
/// The source is consulted only by capability-aware host callbacks. It returns an owned value so
/// no provider-owned reference or allocation escapes the read boundary.
pub trait EnvironmentSource {
    /// Returns a copy of the value stored under `name`, if present.
    fn get(&self, name: &str) -> Option<String>;
}

impl<T> EnvironmentSource for Box<T>
where
    T: EnvironmentSource + ?Sized,
{
    fn get(&self, name: &str) -> Option<String> {
        (**self).get(name)
    }
}

/// Converts a host value into a language value.
///
/// # Errors
///
/// [`VmFault`] when the snapshot cannot be a language value: an `Int` outside ±(2^53−1), a
/// non-finite `Float`, or a handle the boundary does not model.
pub fn host_value_to_value(host: &HostValue) -> Result<Value, VmFault> {
    match host {
        HostValue::None => Ok(Value::None),
        HostValue::Bool(value) => Ok(Value::Bool(*value)),
        HostValue::Int(value) => check_safe_int(*value).map(Value::Int),
        HostValue::Float(value) => check_finite_float(*value).map(Value::Float),
        // The host side keeps `HostValue::String` a raw snapshot (see its docs), so NFC is
        // applied here, where a snapshot becomes a language `String`.
        #[cfg(feature = "unicode")]
        HostValue::String(text) => Ok(Value::String(Rc::new(text.nfc().collect()))),
        #[cfg(not(feature = "unicode"))]
        HostValue::String(text) => Ok(Value::String(Rc::new(text.clone()))),
        HostValue::Bytes(bytes) => Ok(Value::Bytes(Rc::new(RefCell::new(bytes.clone())))),
        HostValue::Handle(handle) => Ok(Value::HostHandle(*handle)),
    }
}

/// Converts a language value into a host value, when the value may cross the boundary.
///
/// Returns `None` for a value the host ABI does not model (a function, a struct instance, a
/// collection), because canon says a host receives plain data plus handles: anything richer
/// must be described by the AHS instead of pushed through by reference.
#[must_use]
pub fn value_to_host_value(value: &Value) -> Option<HostValue> {
    match value {
        Value::None | Value::Unset => Some(HostValue::None),
        Value::Bool(inner) => Some(HostValue::Bool(*inner)),
        Value::Int(inner) => Some(HostValue::Int(*inner)),
        Value::Float(inner) => Some(HostValue::Float(*inner)),
        Value::String(inner) => Some(HostValue::String(inner.to_string())),
        Value::Bytes(inner) => Some(HostValue::Bytes(inner.borrow().clone())),
        Value::HostHandle(handle) => Some(HostValue::Handle(*handle)),
        _ => None,
    }
}

/// Maps a host-boundary fault onto the VM's fault model.
///
/// The mapping is total and one-way: the VM never turns a fault back into a recoverable
/// `Failure`, because canon puts every one of these in the fault column.
#[must_use]
pub fn host_fault_to_vm_fault(fault: &HostFault) -> VmFault {
    match fault {
        HostFault::CapabilityDenied {
            capability,
            operation,
        } => VmFault::CapabilityDenied {
            capability: capability.name().to_string(),
            operation: operation.clone(),
        },
        HostFault::StaleHandle { handle } => VmFault::StaleHandle {
            handle: handle.to_string(),
        },
        HostFault::ScopeEscape { handle, binding } => VmFault::ScopeEscape {
            handle: handle.to_string(),
            binding: binding.clone(),
        },
        // A host value that cannot satisfy its contract and a rejected host surface are both
        // boundary type problems, which the VM already reports as a type mismatch.
        HostFault::InvalidHostValue { detail } => VmFault::TypeMismatch {
            expected: "host value satisfying its declared contract".to_string(),
            actual: detail.clone(),
        },
        HostFault::Schema { problems } => VmFault::TypeMismatch {
            expected: "a consistent host surface description".to_string(),
            actual: problems
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        },
    }
}

/// Identifier of one open host scope.
///
/// A scope is the region a host callback runs in. Handles minted while it is open belong to
/// it, which is how canon's rule — a binding created inside a scoped callback cannot leave
/// it — becomes something the host cannot widen by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(u64);

impl ScopeId {
    /// The scope's ordinal, for diagnostics and tests.
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// The host services a running VM may reach.
///
/// Holds what the profile granted and the host objects those services hand out. A host object
/// is reachable only through a [`Handle`], and the table is the only thing that resolves one,
/// so a script cannot address host memory directly.
#[derive(Default)]
pub struct HostContext {
    granted: CapabilitySet,
    objects: HandleTable<HostValue>,
    userdata: HandleTable<Box<dyn std::any::Any>>,
    /// Open scopes, innermost last.
    open_scopes: Vec<ScopeId>,
    /// Handles each scope minted, so closing it can release exactly those.
    minted: HashMap<ScopeId, Vec<Handle>>,
    /// Handles released by closing a scope: remembered so a publication attempt can report the
    /// specific fault canon names instead of a plain stale read.
    escaped: HashSet<Handle>,
    /// Next scope ordinal.
    next_scope: u64,
    /// Environment provider installed for this VM, if any.
    environment: Option<Box<dyn EnvironmentSource>>,
    /// Filesystem provider installed for this VM, if any.
    filesystem: Option<Box<dyn FilesystemSource>>,
}

impl fmt::Debug for HostContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostContext")
            .field("granted", &self.granted)
            .field("objects", &self.objects)
            .field("userdata_count", &self.userdata.len())
            .field(
                "environment",
                &self.environment.as_ref().map(|_| "installed"),
            )
            .field("filesystem", &self.filesystem.as_ref().map(|_| "installed"))
            .field("open_scopes", &self.open_scopes)
            .field("next_scope", &self.next_scope)
            .finish()
    }
}

impl HostContext {
    /// A context that grants nothing: canon's default for a sandboxed host.
    #[must_use]
    pub fn denied() -> Self {
        Self::default()
    }

    /// A context with exactly the given capabilities granted.
    #[must_use]
    pub fn with_capabilities(granted: CapabilitySet) -> Self {
        Self {
            granted,
            objects: HandleTable::new(),
            userdata: HandleTable::new(),
            open_scopes: Vec::new(),
            minted: HashMap::new(),
            escaped: HashSet::new(),
            next_scope: 1,
            environment: None,
            filesystem: None,
        }
    }

    /// The capabilities this profile granted.
    #[must_use]
    pub fn granted(&self) -> &CapabilitySet {
        &self.granted
    }

    /// Grants one more capability to the running profile (host-side only).
    pub fn grant(&mut self, capability: Capability) -> bool {
        self.granted.grant(capability)
    }

    /// Whether a privileged operation is allowed.
    #[must_use]
    pub fn allows(&self, capability: &Capability) -> bool {
        self.granted.allows(capability)
    }

    /// Checks a capability, mapping the denial onto the VM fault model.
    ///
    /// # Errors
    ///
    /// [`VmFault::CapabilityDenied`] naming the capability and the operation.
    pub fn require(&self, capability: &Capability, operation: &str) -> Result<(), VmFault> {
        self.granted
            .require(capability, operation)
            .map_err(|fault| host_fault_to_vm_fault(&fault))
    }

    /// Installs the environment provider owned by this VM.
    pub fn install_environment_source<S>(&mut self, source: S)
    where
        S: EnvironmentSource + 'static,
    {
        self.environment = Some(Box::new(source));
    }

    /// Installs the environment provider owned by this VM.
    pub fn install_environment<S>(&mut self, source: S)
    where
        S: EnvironmentSource + 'static,
    {
        self.install_environment_source(source);
    }

    /// Removes the environment provider owned by this VM.
    pub fn revoke_environment(&mut self) {
        self.environment = None;
    }

    /// Removes the environment provider owned by this VM.
    pub fn revoke_environment_source(&mut self) {
        self.revoke_environment();
    }

    /// Returns the environment provider owned by this VM for read-only access.
    #[must_use]
    pub fn environment_source(&self) -> Option<&dyn EnvironmentSource> {
        self.environment.as_deref()
    }

    /// Returns the environment provider owned by this VM for read-only access.
    #[must_use]
    pub fn environment(&self) -> Option<&dyn EnvironmentSource> {
        self.environment_source()
    }

    /// Whether this VM has an environment provider installed.
    #[must_use]
    pub fn has_environment_source(&self) -> bool {
        self.environment.is_some()
    }

    /// Installs the filesystem provider owned by this VM.
    pub fn install_filesystem_source<S>(&mut self, source: S)
    where
        S: FilesystemSource + 'static,
    {
        self.filesystem = Some(Box::new(source));
    }

    /// Alias for [`Self::install_filesystem_source`].
    pub fn install_filesystem<S>(&mut self, source: S)
    where
        S: FilesystemSource + 'static,
    {
        self.install_filesystem_source(source);
    }

    /// Removes the filesystem provider owned by this VM.
    pub fn revoke_filesystem(&mut self) {
        self.filesystem = None;
    }

    /// Alias for [`Self::revoke_filesystem`].
    pub fn revoke_filesystem_source(&mut self) {
        self.revoke_filesystem();
    }

    /// Returns the filesystem provider owned by this VM for read-only access.
    #[must_use]
    pub fn filesystem_source(&self) -> Option<&dyn FilesystemSource> {
        self.filesystem.as_deref()
    }

    /// Returns the filesystem provider owned by this VM for mutable access.
    ///
    /// Writes go through here so a provider can enforce its own path policy on mutation. The
    /// borrow is exclusive and short-lived: a host callback holds it only for one operation.
    pub fn filesystem_source_mut(&mut self) -> Option<&mut (dyn FilesystemSource + 'static)> {
        self.filesystem.as_deref_mut()
    }

    /// Alias for [`Self::filesystem_source`].
    #[must_use]
    pub fn filesystem(&self) -> Option<&dyn FilesystemSource> {
        self.filesystem_source()
    }

    /// Whether this VM has a filesystem provider installed.
    #[must_use]
    pub fn has_filesystem_source(&self) -> bool {
        self.filesystem.is_some()
    }

    /// Gives the script ownership of a host value and returns its handle.
    ///
    /// A handle minted while a scope is open belongs to that scope; with no scope open it is
    /// unconstrained, which is what a host that hands out a value outside any callback intends.
    pub fn hand_out(&mut self, value: HostValue) -> Handle {
        let handle = self.objects.insert(value);
        if let Some(scope) = self.open_scopes.last().copied() {
            self.minted.entry(scope).or_default().push(handle);
        }
        handle
    }

    /// The value behind a handle, or `None` when the handle is stale.
    #[must_use]
    pub fn lookup(&self, handle: Handle) -> Option<&HostValue> {
        self.objects.get(handle)
    }

    /// The value behind a handle, or the stale-handle fault.
    ///
    /// # Errors
    ///
    /// [`VmFault::StaleHandle`] when the handle was released or belongs to another generation.
    pub fn resolve(&self, handle: Handle) -> Result<&HostValue, VmFault> {
        self.objects
            .require(handle)
            .map_err(|fault| host_fault_to_vm_fault(&fault))
    }

    /// Releases a host object, invalidating every handle minted for its slot.
    pub fn release(&mut self, handle: Handle) -> Option<HostValue> {
        self.objects.remove(handle)
    }

    /// Number of live host objects, for diagnostics and tests.
    #[must_use]
    pub fn live_objects(&self) -> usize {
        self.objects.len()
    }

    /// Opens a host scope and returns its identifier.
    pub fn open_scope(&mut self) -> ScopeId {
        let scope = ScopeId(self.next_scope);
        self.next_scope += 1;
        self.open_scopes.push(scope);
        scope
    }

    /// Whether a scope is still open.
    #[must_use]
    pub fn scope_is_open(&self, scope: ScopeId) -> bool {
        self.open_scopes.contains(&scope)
    }

    /// Gives the script a generational handle to an arbitrary host object.
    ///
    /// If a scope is currently open, the handle is bound to that scope and will be
    /// released when the scope closes.
    pub fn hand_out_userdata<T: 'static>(&mut self, value: T) -> Handle {
        let handle = self.userdata.insert(Box::new(value));
        if let Some(scope) = self.open_scopes.last().copied() {
            self.minted.entry(scope).or_default().push(handle);
        }
        handle
    }

    /// Resolves a live generational handle to an immutable reference to a host object.
    ///
    /// # Errors
    ///
    /// [`VmFault::ScopeEscape`] if the handle belonged to a scope that has closed.
    /// [`VmFault::StaleHandle`] if the handle was released or belongs to another generation.
    /// [`VmFault::TypeMismatch`] if the host object is not of type `T`.
    pub fn resolve_userdata<T: 'static>(&self, handle: Handle) -> Result<&T, VmFault> {
        if self.escaped.contains(&handle) {
            return Err(host_fault_to_vm_fault(&HostFault::ScopeEscape {
                handle,
                binding: format!("userdata<{}>", std::any::type_name::<T>()),
            }));
        }
        let any_box = self
            .userdata
            .require(handle)
            .map_err(|fault| host_fault_to_vm_fault(&fault))?;
        any_box
            .downcast_ref::<T>()
            .ok_or_else(|| VmFault::TypeMismatch {
                expected: std::any::type_name::<T>().to_string(),
                actual: "different userdata type".to_string(),
            })
    }

    /// Resolves a live generational handle to a mutable reference to a host object.
    ///
    /// # Errors
    ///
    /// [`VmFault::ScopeEscape`] if the handle belonged to a scope that has closed.
    /// [`VmFault::StaleHandle`] if the handle was released or belongs to another generation.
    /// [`VmFault::TypeMismatch`] if the host object is not of type `T`.
    pub fn resolve_userdata_mut<T: 'static>(&mut self, handle: Handle) -> Result<&mut T, VmFault> {
        if self.escaped.contains(&handle) {
            return Err(host_fault_to_vm_fault(&HostFault::ScopeEscape {
                handle,
                binding: format!("userdata<{}>", std::any::type_name::<T>()),
            }));
        }
        let any_box = self
            .userdata
            .get_mut(handle)
            .ok_or_else(|| host_fault_to_vm_fault(&HostFault::StaleHandle { handle }))?;
        any_box
            .downcast_mut::<T>()
            .ok_or_else(|| VmFault::TypeMismatch {
                expected: std::any::type_name::<T>().to_string(),
                actual: "different userdata type".to_string(),
            })
    }

    /// Releases a host userdata object, invalidating every handle minted for its slot.
    pub fn release_userdata(&mut self, handle: Handle) -> Option<Box<dyn std::any::Any>> {
        self.userdata.remove(handle)
    }

    /// Number of live host userdata objects.
    #[must_use]
    pub fn live_userdata(&self) -> usize {
        self.userdata.len()
    }

    /// Whether `handle` addresses a live userdata object.
    #[must_use]
    pub fn contains_userdata(&self, handle: Handle) -> bool {
        self.userdata.contains(handle)
    }

    /// Closes a host scope, releasing everything it minted.
    ///
    /// The handles go through the table, so each slot's generation advances and none of them
    /// can resolve to host memory again — the same rule a manual release follows. Remembering
    /// them separately is what lets a later publication attempt report the escape specifically
    /// instead of a generic stale read.
    pub fn close_scope(&mut self, scope: ScopeId) {
        let Some(handles) = self.minted.remove(&scope) else {
            return;
        };
        self.open_scopes.retain(|open| *open != scope);
        for handle in handles {
            self.objects.remove(handle);
            self.userdata.remove(handle);
            self.escaped.insert(handle);
        }
    }

    /// Whether any scope has closed, and so whether a publication check can possibly fail.
    ///
    /// A publication point consults this first: with no closed scope there is nothing to catch
    /// and the walk is skipped entirely.
    #[must_use]
    pub fn has_escapes(&self) -> bool {
        !self.escaped.is_empty()
    }

    /// Checks one handle that is about to become reachable outside the scope it was minted in.
    ///
    /// # Errors
    ///
    /// [`VmFault::ScopeEscape`] when the handle belonged to a scope that has since closed.
    pub fn check_publication(&self, handle: Handle, binding: &str) -> Result<(), VmFault> {
        if self.escaped.contains(&handle) {
            return Err(host_fault_to_vm_fault(&HostFault::ScopeEscape {
                handle,
                binding: binding.to_string(),
            }));
        }
        Ok(())
    }

    /// Checks every handle a value carries, at the point the value becomes heap-reachable.
    ///
    /// Containers are walked because canon names `BuildList` and `BuildDict` as publication
    /// points: a handle smuggled into a list literal has already left its scope even if the
    /// list is never stored anywhere. The walk tracks visited containers so a self-referential
    /// value terminates instead of recursing forever.
    ///
    /// # Errors
    ///
    /// [`VmFault::ScopeEscape`] when any reached handle escaped a closed scope.
    pub fn ensure_publishable(&self, value: &Value, site: &str) -> Result<(), VmFault> {
        let mut visited: HashSet<usize> = HashSet::new();
        self.walk(value, site, &mut visited)
    }

    fn walk(&self, value: &Value, site: &str, visited: &mut HashSet<usize>) -> Result<(), VmFault> {
        match value {
            Value::HostHandle(handle) => self.check_publication(*handle, site),
            Value::List(items) | Value::Set(items) => {
                if !visited.insert(Rc::as_ptr(items) as usize) {
                    return Ok(());
                }
                items
                    .borrow()
                    .iter()
                    .try_for_each(|item| self.walk(item, site, visited))
            }
            Value::Dict(entries) => {
                if !visited.insert(Rc::as_ptr(entries) as usize) {
                    return Ok(());
                }
                for (key, entry) in entries.borrow().entries() {
                    self.walk(key, site, visited)?;
                    self.walk(entry, site, visited)?;
                }
                Ok(())
            }
            Value::Struct(instance) => {
                if !visited.insert(Rc::as_ptr(instance) as usize) {
                    return Ok(());
                }
                for (_, field) in &instance.borrow().fields {
                    self.walk(field, site, visited)?;
                }
                Ok(())
            }
            Value::Closure(closure) => {
                for cell in &closure.upvalues {
                    if visited.insert(Rc::as_ptr(cell) as usize) {
                        self.walk(&cell.borrow(), site, visited)?;
                    }
                }
                Ok(())
            }
            Value::BoundMethod(bm) => self.walk(&bm.receiver, site, visited),
            Value::StructMethod(sm) => {
                self.walk(&Value::Struct(sm.receiver.clone()), site, visited)
            }
            Value::Sequence(pipeline) => {
                if !visited.insert(Rc::as_ptr(pipeline) as usize) {
                    return Ok(());
                }
                if let Some(cached) = pipeline.cached.borrow().as_ref() {
                    for item in cached {
                        self.walk(item, site, visited)?;
                    }
                }
                match &pipeline.source {
                    crate::value::SequenceSource::List(items)
                    | crate::value::SequenceSource::Dict(items)
                    | crate::value::SequenceSource::Set(items) => {
                        items
                            .iter()
                            .try_for_each(|item| self.walk(item, site, visited))?;
                    }
                    crate::value::SequenceSource::Range { .. } => {}
                }
                for op in &pipeline.ops {
                    match op {
                        crate::value::SeqOp::Map(v)
                        | crate::value::SeqOp::Filter(v)
                        | crate::value::SeqOp::FlatMap(v)
                        | crate::value::SeqOp::Find(v)
                        | crate::value::SeqOp::Any(v)
                        | crate::value::SeqOp::All(v)
                        | crate::value::SeqOp::GroupBy(v) => self.walk(v, site, visited)?,
                        crate::value::SeqOp::Count(Some(v)) => self.walk(v, site, visited)?,
                        crate::value::SeqOp::Reduce { initial, func } => {
                            self.walk(initial, site, visited)?;
                            self.walk(func, site, visited)?;
                        }
                        crate::value::SeqOp::Zip(items) | crate::value::SeqOp::Chain(items) => {
                            for item in items {
                                self.walk(item, site, visited)?;
                            }
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            // Everything else is a plain value or an opaque host-owned kind that cannot hold a handle.
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{DictMap, StructInstance};

    fn capability(path: &str) -> Capability {
        Capability::parse(path).expect("test capability is valid")
    }

    /// A handle that addresses nothing, for the tests that only need the identity.
    fn escaped_handle() -> Handle {
        let mut table: HandleTable<HostValue> = HandleTable::new();
        table.insert(HostValue::None)
    }

    #[test]
    fn test_host_values_round_trip() {
        for host in [
            HostValue::None,
            HostValue::Bool(true),
            HostValue::Int(42),
            HostValue::Float(1.5),
            HostValue::string("aipo"),
            HostValue::Bytes(vec![1, 2, 3]),
        ] {
            let value = host_value_to_value(&host).expect("converts");
            let back = value_to_host_value(&value).expect("converts back");
            assert_eq!(back, host);
        }
    }

    #[test]
    fn test_out_of_range_host_value_is_a_fault_not_a_wrap() {
        // The `aipo-host` constructors already refuse these, so a value this wrong can only
        // arrive from a host that bypassed them; the VM must still refuse to wrap it.
        let fault = host_value_to_value(&HostValue::Int(i64::MAX)).expect_err("out of range");
        assert_eq!(
            fault.diagnostic_code(),
            aipo_diagnostics::DiagnosticCode::AIPO_RT_OVERFLOW
        );
    }

    #[test]
    fn test_string_snapshot_is_normalized_at_this_boundary() {
        // `e` + combining acute becomes the precomposed character, NFC being the language's
        // String boundary rule.
        let decomposed = "e\u{0301}".to_string();
        let value = host_value_to_value(&HostValue::String(decomposed)).expect("converts");
        assert_eq!(value, Value::String(Rc::new("\u{00e9}".to_string())));
    }

    #[test]
    fn test_richer_values_do_not_cross_by_reference() {
        // A collection has no host-value form: the AHS describes it instead, so a host never
        // receives script memory by reference.
        let list = Value::List(Rc::new(RefCell::new(vec![Value::Int(1)])));
        assert_eq!(value_to_host_value(&list), None);
    }

    #[test]
    fn test_denied_capability_is_a_fault_with_the_canon_code() {
        let context = HostContext::denied();
        let fault = context
            .require(&capability("clock.wall"), "time.now")
            .expect_err("denied by default");
        assert_eq!(
            fault.diagnostic_code(),
            aipo_diagnostics::DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
        assert!(fault.to_string().contains("time.now"));
    }

    #[test]
    fn test_granting_an_ancestor_allows_the_leaf() {
        let mut context = HostContext::denied();
        context.grant(capability("clock"));
        assert!(
            context
                .require(&capability("clock.monotonic"), "time.monotonic")
                .is_ok()
        );
        assert!(
            context
                .require(&capability("clock.wall"), "time.now")
                .is_ok()
        );
    }

    #[test]
    fn test_stale_handle_is_a_fault_and_the_holder_is_unchanged() {
        let mut context = HostContext::denied();
        let handle = context.hand_out(HostValue::string("entity"));
        assert_eq!(context.live_objects(), 1);
        context.release(handle);
        assert_eq!(context.live_objects(), 0);
        let fault = context.resolve(handle).expect_err("stale");
        assert_eq!(
            fault.diagnostic_code(),
            aipo_diagnostics::DiagnosticCode::AIPO_RT_STALE_HANDLE
        );
    }

    #[test]
    fn test_closing_a_scope_releases_its_handles() {
        let mut context = HostContext::denied();
        let scope = context.open_scope();
        let handle = context.hand_out(HostValue::string("entity"));
        assert!(context.scope_is_open(scope));
        assert_eq!(context.live_objects(), 1);

        context.close_scope(scope);
        assert!(!context.scope_is_open(scope));
        // Released through the table, so the slot's generation advanced: this handle can never
        // resolve to host memory again, which is the use-after-free the table exists to stop.
        assert_eq!(context.live_objects(), 0);
        let fault = context.resolve(handle).expect_err("no longer resolvable");
        assert_eq!(
            fault.diagnostic_code(),
            aipo_diagnostics::DiagnosticCode::AIPO_RT_STALE_HANDLE
        );
    }

    #[test]
    fn test_a_handle_minted_in_a_closed_scope_cannot_be_published() {
        let mut context = HostContext::denied();
        let scope = context.open_scope();
        let handle = context.hand_out(HostValue::string("entity"));
        context.close_scope(scope);

        let fault = context
            .check_publication(handle, "global 'held'")
            .expect_err("escaped");
        assert_eq!(
            fault.diagnostic_code(),
            aipo_diagnostics::DiagnosticCode::AIPO_RT_SCOPE_ESCAPE
        );
        let rendered = fault.to_string();
        assert!(rendered.contains("global 'held'"), "{rendered}");
    }

    #[test]
    fn test_a_live_scoped_handle_is_publishable_while_the_scope_is_open() {
        // Canon constrains a binding that *leaves* the scope, not one used inside it.
        let mut context = HostContext::denied();
        let scope = context.open_scope();
        let handle = context.hand_out(HostValue::Int(7));
        assert!(context.check_publication(handle, "field 'x'").is_ok());
        context.close_scope(scope);
    }

    #[test]
    fn test_a_handle_inside_a_container_is_caught_at_the_container_site() {
        let mut context = HostContext::denied();
        let scope = context.open_scope();
        let handle = context.hand_out(HostValue::string("entity"));
        context.close_scope(scope);

        let list = Value::List(Rc::new(RefCell::new(vec![
            Value::Int(1),
            Value::HostHandle(handle),
        ])));
        let fault = context
            .ensure_publishable(&list, "list element")
            .expect_err("escaped through the list");
        assert_eq!(
            fault.diagnostic_code(),
            aipo_diagnostics::DiagnosticCode::AIPO_RT_SCOPE_ESCAPE
        );

        // The same handle behind a dict value is caught too.
        let dict = Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(vec![(
            Value::String(Rc::new("held".to_string())),
            Value::HostHandle(handle),
        )]))));
        assert!(context.ensure_publishable(&dict, "dict entry").is_err());
    }

    #[test]
    fn test_a_handle_buried_in_a_struct_field_is_caught() {
        let mut context = HostContext::denied();
        let scope = context.open_scope();
        let handle = context.hand_out(HostValue::string("entity"));
        context.close_scope(scope);

        let instance = StructInstance {
            type_name: "Holder".to_string(),
            fields: vec![("held".to_string(), Value::HostHandle(handle))],
            fixed_fields: HashSet::new(),
            under_construction: false,
        };
        let value = Value::Struct(Rc::new(RefCell::new(instance)));
        assert!(context.ensure_publishable(&value, "field 'held'").is_err());
    }

    #[test]
    fn test_a_container_without_handles_is_publishable() {
        let context = HostContext::denied();
        let value = Value::List(Rc::new(RefCell::new(vec![
            Value::Int(1),
            Value::String(Rc::new("text".to_string())),
        ])));
        assert!(context.ensure_publishable(&value, "global 'items'").is_ok());
    }

    #[test]
    fn test_a_self_referential_container_terminates() {
        // `List.add(l, l)` is legal, so the escape walk must not depend on the value being a
        // tree. Visiting each container once is what bounds it.
        let context = HostContext::denied();
        let inner: Rc<RefCell<Vec<Value>>> = Rc::new(RefCell::new(Vec::new()));
        let snapshot = Value::List(Rc::clone(&inner));
        inner.borrow_mut().push(snapshot);
        assert!(
            context
                .ensure_publishable(&Value::List(inner), "global 'loop'")
                .is_ok()
        );
    }

    #[test]
    fn test_host_faults_map_onto_the_vm_fault_model() {
        let cases = [
            (
                HostFault::CapabilityDenied {
                    capability: capability("filesystem.read"),
                    operation: "fs.read".to_string(),
                },
                aipo_diagnostics::DiagnosticCode::AIPO_RT_CAPABILITY_DENIED,
            ),
            (
                HostFault::ScopeEscape {
                    handle: escaped_handle(),
                    binding: "held".to_string(),
                },
                aipo_diagnostics::DiagnosticCode::AIPO_RT_SCOPE_ESCAPE,
            ),
            (
                HostFault::InvalidHostValue {
                    detail: "not a number".to_string(),
                },
                aipo_diagnostics::DiagnosticCode::AIPO_RT_TYPE_MISMATCH,
            ),
        ];
        for (fault, expected) in cases {
            assert_eq!(host_fault_to_vm_fault(&fault).diagnostic_code(), expected);
        }
    }
}
