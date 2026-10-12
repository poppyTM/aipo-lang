//! Method binding, mutation-during-iteration guards, and the higher-order
//! collection methods that call back into Aipo code.
use super::helpers::{collection_identity, compare_keys, iterable_items};
use super::{MUTATING_METHODS, MethodNative, Vm};
use crate::fault::{VmError, VmFault};
use crate::value::{MethodKind, Value};
use aipo_bytecode::BytecodeModule;
use std::cell::RefCell;
use std::rc::Rc;
impl Vm {
    pub(super) fn lookup_method_native(
        &self,
        type_name: &str,
        method: &str,
    ) -> Option<(usize, MethodNative)> {
        self.method_natives_by_type
            .get(type_name)
            .and_then(|methods| methods.get(method))
            .copied()
    }

    /// Binds a method of `receiver` by name, if one exists.
    ///
    /// The bound value carries the *plain* method name: the mutation guard and the
    /// higher-order dispatch both key off canonical method names (`add`, `filter`, …),
    /// independent of the receiver type.
    pub(super) fn bind_method(&self, receiver: &Value, method: &str) -> Option<Value> {
        let type_name = receiver.type_name();
        // Sequence methods dispatch through the scheduler-aware path in
        // `begin_call` (pure stages append, driving stages evaluate).
        if matches!(receiver, Value::Sequence(_)) {
            if let Some((_, arity)) = super::task::SEQUENCE_METHODS
                .iter()
                .find(|(name, _)| *name == method)
            {
                return Some(Value::bound_method(
                    method,
                    *arity,
                    receiver.clone(),
                    MethodKind::HigherOrder,
                ));
            }
            return None;
        }
        // Group methods (`spawn`, `wait`) run through the scheduler too.
        if matches!(receiver, Value::Group(_)) {
            let arity = match method {
                "spawn" => 2,
                "wait" => 0,
                _ => return None,
            };
            return Some(Value::bound_method(
                method,
                arity,
                receiver.clone(),
                MethodKind::HigherOrder,
            ));
        }
        if let Some((arity, func)) = self.lookup_method_native(type_name, method) {
            return Some(Value::bound_method(
                method,
                arity,
                receiver.clone(),
                MethodKind::Native(func),
            ));
        }
        if matches!(
            method,
            "filter" | "transform" | "map" | "sort_by" | "any" | "all" | "flat_map"
        ) {
            return Some(Value::bound_method(
                method,
                1,
                receiver.clone(),
                MethodKind::HigherOrder,
            ));
        }
        if method == "reduce" {
            return Some(Value::bound_method(
                method,
                2,
                receiver.clone(),
                MethodKind::HigherOrder,
            ));
        }
        None
    }

    /// Rejects structural mutation of a collection that is currently being iterated.
    pub(super) fn ensure_mutation_allowed(
        &self,
        receiver: &Value,
        method: &str,
    ) -> Result<(), VmFault> {
        if !MUTATING_METHODS.contains(&method) {
            return Ok(());
        }
        match collection_identity(receiver) {
            Some(id) if self.active_iterations.contains(&id) => {
                Err(VmFault::MutationDuringIteration)
            }
            _ => Ok(()),
        }
    }

    /// Executes the canonical higher-order collection methods (`filter`, `transform`, `map`,
    /// `sort_by`, `any`, `all`, `flat_map`, `reduce`) by applying the Aipo callable through [`Vm::invoke`].
    pub(super) fn higher_order(
        &mut self,
        module: &BytecodeModule,
        method: &str,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, VmError> {
        let items = iterable_items(receiver)?;
        let guard = collection_identity(receiver);
        if let Some(id) = guard {
            self.active_iterations.push(id);
            self.iteration_revisions.push((
                receiver.clone(),
                receiver.structural_revision(),
                self.frames.len(),
            ));
        }

        let outcome = match method {
            "filter" => {
                let callable = args.first().cloned().unwrap_or(Value::None);
                let mut kept = Vec::new();
                let mut error = None;
                for item in items {
                    match self.invoke(module, callable.clone(), std::slice::from_ref(&item)) {
                        Ok(Value::Bool(true)) => kept.push(item),
                        Ok(Value::Bool(false)) => {}
                        Ok(other) => {
                            error = Some(
                                VmFault::TypeMismatch {
                                    expected: "Bool predicate result".to_string(),
                                    actual: other.type_name().to_string(),
                                }
                                .into(),
                            );
                            break;
                        }
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                match error {
                    Some(err) => Err(err),
                    None => Ok(Value::List(Rc::new(RefCell::new((kept).into())))),
                }
            }
            "transform" | "map" => {
                let callable = args.first().cloned().unwrap_or(Value::None);
                let mut mapped = Vec::new();
                let mut error = None;
                for item in items {
                    match self.invoke(module, callable.clone(), &[item]) {
                        Ok(value) => mapped.push(value),
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                match error {
                    Some(err) => Err(err),
                    None => Ok(Value::List(Rc::new(RefCell::new((mapped).into())))),
                }
            }
            "any" => {
                let callable = args.first().cloned().unwrap_or(Value::None);
                let mut found = false;
                let mut error = None;
                for item in items {
                    match self.invoke(module, callable.clone(), std::slice::from_ref(&item)) {
                        Ok(Value::Bool(b)) => {
                            if b {
                                found = true;
                                break;
                            }
                        }
                        Ok(other) => {
                            error = Some(
                                VmFault::TypeMismatch {
                                    expected: "Bool predicate result".to_string(),
                                    actual: other.type_name().to_string(),
                                }
                                .into(),
                            );
                            break;
                        }
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                match error {
                    Some(err) => Err(err),
                    None => Ok(Value::Bool(found)),
                }
            }
            "all" => {
                let callable = args.first().cloned().unwrap_or(Value::None);
                let mut all_true = true;
                let mut error = None;
                for item in items {
                    match self.invoke(module, callable.clone(), std::slice::from_ref(&item)) {
                        Ok(Value::Bool(b)) => {
                            if !b {
                                all_true = false;
                                break;
                            }
                        }
                        Ok(other) => {
                            error = Some(
                                VmFault::TypeMismatch {
                                    expected: "Bool predicate result".to_string(),
                                    actual: other.type_name().to_string(),
                                }
                                .into(),
                            );
                            break;
                        }
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                match error {
                    Some(err) => Err(err),
                    None => Ok(Value::Bool(all_true)),
                }
            }
            "flat_map" => {
                let callable = args.first().cloned().unwrap_or(Value::None);
                let mut flat = Vec::new();
                let mut error = None;
                for item in items {
                    match self.invoke(module, callable.clone(), &[item]) {
                        Ok(Value::List(sub_list)) => {
                            flat.extend(sub_list.borrow().clone());
                        }
                        Ok(other) => {
                            error = Some(
                                VmFault::TypeMismatch {
                                    expected: "List result from flat_map callback".to_string(),
                                    actual: other.type_name().to_string(),
                                }
                                .into(),
                            );
                            break;
                        }
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                match error {
                    Some(err) => Err(err),
                    None => Ok(Value::List(Rc::new(RefCell::new((flat).into())))),
                }
            }
            "reduce" => {
                if args.len() < 2 {
                    Err(VmFault::TypeMismatch {
                        expected: "2 arguments for reduce: initial, callable".to_string(),
                        actual: format!("{} arguments", args.len()),
                    }
                    .into())
                } else {
                    let mut acc = args[0].clone();
                    let callable = &args[1];
                    if acc.is_failure() {
                        Ok(acc)
                    } else {
                        let mut error = None;
                        for item in items {
                            match self.invoke(module, callable.clone(), &[acc.clone(), item]) {
                                Ok(next) => {
                                    acc = next;
                                    if acc.is_failure() {
                                        break;
                                    }
                                }
                                Err(err) => {
                                    error = Some(err);
                                    break;
                                }
                            }
                        }
                        match error {
                            Some(err) => Err(err),
                            None => Ok(acc),
                        }
                    }
                }
            }
            "sort_by" => {
                let callable = args.first().cloned().unwrap_or(Value::None);
                let mut keyed: Vec<(Value, Value)> = Vec::with_capacity(items.len());
                let mut error = None;
                for item in items {
                    match self.invoke(module, callable.clone(), std::slice::from_ref(&item)) {
                        Ok(key) => keyed.push((key, item)),
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                if let Some(err) = error {
                    Err(err)
                } else {
                    let mut failed = false;
                    keyed.sort_by(|(a, _), (b, _)| match compare_keys(a, b) {
                        Some(ordering) => ordering,
                        None => {
                            failed = true;
                            std::cmp::Ordering::Equal
                        }
                    });
                    if failed {
                        Err(VmError::Fault(VmFault::TypeMismatch {
                            expected: "comparable sort keys".to_string(),
                            actual: "mixed non-comparable keys".to_string(),
                        }))
                    } else {
                        let sorted: Vec<_> = keyed.into_iter().map(|(_, item)| item).collect();
                        Ok(Value::List(Rc::new(RefCell::new((sorted).into()))))
                    }
                }
            }
            other => Err(VmError::Fault(VmFault::TypeMismatch {
                expected: "collection method".to_string(),
                actual: other.to_string(),
            })),
        };

        let changed = guard.is_some()
            && self
                .iteration_revisions
                .last()
                .is_some_and(|(value, revision, _)| value.structural_revision() != *revision);
        if guard.is_some() {
            self.active_iterations.pop();
            self.iteration_revisions.pop();
        }
        if changed {
            return Err(VmFault::MutationDuringIteration.into());
        }
        // `filter`/`transform`/`sort_by` over a `Set` return a `Set`
        // (deduplicated, insertion order); over anything else a `List`.
        match (&receiver, outcome) {
            (Value::Set(_), Ok(Value::List(items))) => {
                let mut unique = Vec::new();
                for item in items.borrow().iter() {
                    if !unique.iter().any(|seen| seen == item) {
                        unique.push(item.clone());
                    }
                }
                Ok(Value::Set(Rc::new(RefCell::new((unique).into()))))
            }
            (_, outcome) => outcome,
        }
    }
}
