//! Call protocol: callee dispatch, frames, arity, natives, conversions,
//! bound methods. Failure short-circuiting follows Model B.
use super::Vm;
use crate::convert::convert_via_type;
use crate::fault::{VmError, VmFault};
use crate::frame::CallFrame;
use crate::value::{MethodKind, Value};
use aipo_bytecode::BytecodeModule;
use std::rc::Rc;

impl Vm {
    /// Resolves and starts a call whose callee and arguments are the top
    /// `arg_count + 1` operand stack entries.
    ///
    /// Frame-based callees (functions, closures, struct methods) push a call frame;
    /// natives, type conversions and native bound methods complete inline and leave
    /// their result on the stack.
    pub(super) fn begin_call(
        &mut self,
        module: &BytecodeModule,
        arg_count: usize,
    ) -> Result<(), VmError> {
        if self.stack.len() < arg_count + 1 {
            return Err(VmFault::StackUnderflow.into());
        }
        if self.metrics_enabled {
            self.metrics.calls = self.metrics.calls.saturating_add(1);
            self.metrics.call_argument_copies = self
                .metrics
                .call_argument_copies
                .saturating_add(arg_count as u64);
        }

        let callee_idx = self.stack.len() - 1 - arg_count;
        let callee = self.stack[callee_idx].clone();

        // Failure propagation (Model B):
        if callee.is_failure() {
            self.stack.truncate(callee_idx);
            self.push(callee)?;
            return Ok(());
        }
        let is_failure_inspector = match &callee {
            Value::Native(native) => {
                native.name == "expect.failure" || native.name == "testing.failure"
            }
            _ => false,
        };
        if !is_failure_inspector {
            for i in 0..arg_count {
                if self.stack[callee_idx + 1 + i].is_failure() {
                    let failure = self.stack[callee_idx + 1 + i].clone();
                    self.stack.truncate(callee_idx);
                    self.push(failure)?;
                    return Ok(());
                }
            }
        }

        match callee {
            Value::Function {
                entry_ip,
                arity,
                is_async,
            } => {
                if self.metrics_enabled {
                    self.metrics.function_calls = self.metrics.function_calls.saturating_add(1);
                }
                self.check_arity(arg_count, arity as usize)?;
                if is_async {
                    // Calling an async function spawns it eagerly and yields
                    // its `Task`; awaiting stays explicit (`await`, joins).
                    let callee = Value::Function {
                        entry_ip,
                        arity,
                        is_async,
                    };
                    let args = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let id = self.spawn_task(callee, args, None)?;
                    self.stack.truncate(callee_idx);
                    self.push(Value::Task(id))?;
                    return Ok(());
                }
                let journal_start = self.mutation_journal.len();
                Self::push_plain_frame(
                    &mut self.frames,
                    &mut self.upvalue_frames,
                    journal_start,
                    self.ip,
                    callee_idx,
                    arg_count,
                    None,
                );
                self.refresh_frame_base();
                self.ip = entry_ip as usize;
            }
            Value::Closure(closure) => {
                if self.metrics_enabled {
                    self.metrics.function_calls = self.metrics.function_calls.saturating_add(1);
                }
                self.check_arity(arg_count, closure.arity)?;
                if closure.is_async {
                    let callee = Value::Closure(closure.clone());
                    let args = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let id = self.spawn_task(callee, args, None)?;
                    self.stack.truncate(callee_idx);
                    self.push(Value::Task(id))?;
                    return Ok(());
                }
                let journal_start = self.mutation_journal.len();
                Self::push_plain_frame(
                    &mut self.frames,
                    &mut self.upvalue_frames,
                    journal_start,
                    self.ip,
                    callee_idx,
                    arg_count,
                    // A closure that captures nothing shares the empty frame; only a
                    // capturing one needs its own vector.
                    if closure.upvalues.is_empty() {
                        None
                    } else {
                        Some(Rc::new(closure.upvalues.clone()))
                    },
                );
                self.refresh_frame_base();
                self.ip = closure.entry_ip;
            }
            Value::Native(native) => {
                let name = native.name.as_str();
                let arity = native.arity;
                let func = native.func;
                if self.metrics_enabled {
                    self.metrics.native_calls = self.metrics.native_calls.saturating_add(1);
                }
                if name.is_empty() || name.trim() != name {
                    return Err(VmFault::TypeMismatch {
                        expected: "non-empty canonical native name".to_string(),
                        actual: if name.is_empty() {
                            "<empty>".to_string()
                        } else {
                            name.to_string()
                        },
                    }
                    .into());
                }
                self.check_arity(arg_count, arity)?;
                if let Some(entry) = self.host_natives.get(name).copied() {
                    if arity != entry.arity {
                        return Err(VmFault::TypeMismatch {
                            expected: format!("native value arity {}", entry.arity),
                            actual: format!("native value arity {arity}"),
                        }
                        .into());
                    }
                    self.check_arity(arg_count, entry.arity)?;
                    let args = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    let original_stack = self.stack.clone();
                    let result = if entry.is_async {
                        self.spawn_host_task(entry.callback, args).map(Value::Task)
                    } else {
                        (entry.callback)(self, &args)
                    };
                    return match result {
                        Ok(value) => {
                            self.stack.truncate(callee_idx);
                            if let Err(error) = self.push(value) {
                                self.stack = original_stack;
                                return Err(error.into());
                            }
                            Ok(())
                        }
                        Err(error) => {
                            self.stack = original_stack;
                            Err(error)
                        }
                    };
                }
                if matches!(
                    name,
                    "task.spawn"
                        | "task.sleep"
                        | "task.all"
                        | "task.race"
                        | "task.timeout"
                        | "task.cancel"
                        | "task.group"
                ) {
                    let args = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    return self.task_call(name, callee_idx, &args);
                }
                if matches!(name, "test" | "testing.test") {
                    return self.test_call(module, callee_idx, arg_count);
                }
                let result = func(&self.stack[callee_idx + 1..callee_idx + 1 + arg_count])?;
                self.stack.truncate(callee_idx);
                self.push(result)?;
            }
            Value::Type(tag) => {
                let result =
                    convert_via_type(tag, &self.stack[callee_idx + 1..callee_idx + 1 + arg_count])?;
                self.stack.truncate(callee_idx);
                self.push(result)?;
            }
            Value::UserType(name) => {
                return Err(VmFault::TypeMismatch {
                    expected: "function or callable".to_string(),
                    actual: format!("Type {name}"),
                }
                .into());
            }
            Value::BoundMethod(bm) => {
                if self.metrics_enabled {
                    self.metrics.bound_method_calls =
                        self.metrics.bound_method_calls.saturating_add(1);
                }
                self.check_arity(arg_count, bm.arity)?;
                self.ensure_mutation_allowed(&bm.receiver, &bm.name)?;
                // `Sequence` and `Group` methods need the scheduler or the
                // driving interpreter, so they dispatch before the kind match.
                if matches!(&bm.receiver, Value::Sequence(_)) {
                    let args = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    return self.sequence_method(module, &bm.name, &bm.receiver, &args, callee_idx);
                }
                if matches!(&bm.receiver, Value::Group(_)) {
                    let args = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                    return self.group_method(&bm.name, &bm.receiver, &args, callee_idx);
                }
                match bm.kind {
                    MethodKind::Native(func) => {
                        let result = func(
                            &bm.receiver,
                            &self.stack[callee_idx + 1..callee_idx + 1 + arg_count],
                        )?;
                        self.stack.truncate(callee_idx);
                        self.push(result)?;
                    }
                    MethodKind::Function {
                        entry_ip,
                        total_arity,
                        is_async,
                    } => {
                        if arg_count + 1 != total_arity {
                            return Err(VmFault::TypeMismatch {
                                expected: format!("{} arguments for {}", total_arity - 1, bm.name),
                                actual: format!("{arg_count} arguments"),
                            }
                            .into());
                        }
                        if is_async {
                            // An `async fn` method is an `async fn`: calling it yields a
                            // `Task` whose body carries the receiver as argument 0.
                            let callee = Value::Function {
                                entry_ip: entry_ip as u32,
                                arity: total_arity as u16,
                                is_async,
                            };
                            let mut args = Vec::with_capacity(total_arity);
                            args.push(bm.receiver.clone());
                            args.extend(
                                self.stack[callee_idx + 1..callee_idx + 1 + arg_count]
                                    .iter()
                                    .cloned(),
                            );
                            let id = self.spawn_task(callee, args, None)?;
                            self.stack.truncate(callee_idx);
                            self.push(Value::Task(id))?;
                            return Ok(());
                        }
                        // The receiver takes the callee slot and becomes argument 0.
                        self.stack[callee_idx] = bm.receiver.clone();
                        let journal_start = self.mutation_journal.len();
                        self.frames.push(CallFrame::method(
                            self.ip,
                            callee_idx,
                            arg_count + 1,
                            journal_start,
                        ));
                        self.upvalue_frames.push(None);
                        self.refresh_frame_base();
                        self.ip = entry_ip;
                    }
                    MethodKind::HigherOrder => {
                        let args = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                        self.stack.truncate(callee_idx);
                        let result = self.higher_order(module, &bm.name, &bm.receiver, &args)?;
                        self.push(result)?;
                    }
                }
            }
            Value::StructMethod(sm) => {
                if self.metrics_enabled {
                    self.metrics.bound_method_calls =
                        self.metrics.bound_method_calls.saturating_add(1);
                }
                let total_arity = sm.total_arity as usize;
                if arg_count + 1 != total_arity {
                    return Err(VmFault::TypeMismatch {
                        expected: format!("{} arguments", total_arity.saturating_sub(1)),
                        actual: format!("{arg_count} arguments"),
                    }
                    .into());
                }
                if sm.is_async {
                    let callee = Value::Function {
                        entry_ip: sm.entry_ip,
                        arity: total_arity as u16,
                        is_async: sm.is_async,
                    };
                    let mut args = Vec::with_capacity(total_arity);
                    args.push(Value::Struct(sm.receiver.clone()));
                    args.extend(
                        self.stack[callee_idx + 1..callee_idx + 1 + arg_count]
                            .iter()
                            .cloned(),
                    );
                    let id = self.spawn_task(callee, args, None)?;
                    self.stack.truncate(callee_idx);
                    self.push(Value::Task(id))?;
                    return Ok(());
                }
                self.stack[callee_idx] = Value::Struct(sm.receiver.clone());
                let journal_start = self.mutation_journal.len();
                self.frames.push(CallFrame::method(
                    self.ip,
                    callee_idx,
                    arg_count + 1,
                    journal_start,
                ));
                self.upvalue_frames.push(None);
                self.refresh_frame_base();
                self.ip = sm.entry_ip as usize;
            }
            other => {
                return Err(VmFault::NotCallable {
                    type_name: other.type_name().to_string(),
                }
                .into());
            }
        }

        Ok(())
    }

    pub(super) fn check_arity(&self, provided: usize, expected: usize) -> Result<(), VmFault> {
        crate::contract::check_arity(provided, expected)
    }

    /// Calls an Aipo callable from host code (used by higher-order collection methods).
    ///
    /// Runs the interpreter until the invoked frame returns, so user functions and
    /// closures can be applied while the outer computation is still in progress.
    /// Blocking inside the callback faults (`AIPO_RT_AWAIT_IN_CALLBACK`): the
    /// host Rust stack cannot suspend, so suspension signals propagate through.
    pub fn invoke(
        &mut self,
        module: &BytecodeModule,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, VmError> {
        self.invoke_depth += 1;
        let result = self.invoke_inner(module, callee, args);
        self.invoke_depth -= 1;
        result
    }

    fn invoke_inner(
        &mut self,
        module: &BytecodeModule,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, VmError> {
        let prev_state = self.push_module_execution(module)?;
        let result = self.invoke_body(module, callee, args);
        self.pop_module_execution(prev_state);
        result
    }

    fn invoke_body(
        &mut self,
        module: &BytecodeModule,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, VmError> {
        let stack_base = self.stack.len();
        let frame_base = self.frames.len();
        let journal_base = self.mutation_journal.len();
        self.push(callee)?;
        for arg in args {
            self.push(arg.clone())?;
        }
        if let Err(err) = self.begin_call(module, args.len()) {
            self.stack.truncate(stack_base);
            self.rollback_mutations(journal_base);
            return Err(err);
        }
        while self.frames.len() > frame_base {
            match self.step(module) {
                Ok(true) => break,
                Ok(false) => {}
                Err(err) => {
                    self.stack.truncate(stack_base);
                    self.frames.truncate(frame_base);
                    self.refresh_frame_base();
                    self.upvalue_frames.truncate(frame_base);
                    self.rollback_mutations(journal_base);
                    return Err(err);
                }
            }
        }
        self.refresh_frame_base();
        let result = self.pop()?;
        self.stack.truncate(stack_base);
        self.upvalue_frames.truncate(frame_base);
        self.mutation_journal.truncate(journal_base);
        Ok(result)
    }

    fn test_call(
        &mut self,
        module: &BytecodeModule,
        callee_idx: usize,
        arg_count: usize,
    ) -> Result<(), VmError> {
        self.check_arity(arg_count, 2)?;
        let name_val = &self.stack[callee_idx + 1];
        let test_name = match name_val {
            Value::String(s) => s.to_string(),
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "String test name".to_string(),
                    actual: other.type_name().to_string(),
                }
                .into());
            }
        };

        let callback = self.stack[callee_idx + 2].clone();
        let arity = match &callback {
            Value::Function { arity, .. } => *arity as usize,
            Value::Closure(c) => c.arity,
            Value::Native(n) => n.arity,
            Value::StructMethod(sm) => (sm.total_arity as usize).saturating_sub(1),
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "callable test body".to_string(),
                    actual: other.type_name().to_string(),
                }
                .into());
            }
        };

        let mode = self.test_mode.clone();
        match mode {
            crate::vm::TestMode::Discover(list) => {
                list.borrow_mut().push(test_name);
                self.stack.truncate(callee_idx);
                self.push(Value::None)?;
                Ok(())
            }
            crate::vm::TestMode::Execute { target, ran } => {
                if test_name == target {
                    ran.set(true);
                    self.stack.truncate(callee_idx);
                    self.push(callback)?;
                    if arity == 1 {
                        self.push(Value::None)?;
                        self.begin_call(module, 1)
                    } else {
                        self.begin_call(module, 0)
                    }
                } else {
                    self.stack.truncate(callee_idx);
                    self.push(Value::None)?;
                    Ok(())
                }
            }
            crate::vm::TestMode::Disabled => {
                self.stack.truncate(callee_idx);
                self.push(callback)?;
                if arity == 1 {
                    self.push(Value::None)?;
                    self.begin_call(module, 1)
                } else {
                    self.begin_call(module, 0)
                }
            }
        }
    }
}
