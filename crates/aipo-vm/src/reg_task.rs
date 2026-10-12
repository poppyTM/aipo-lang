//! Cooperative async task subsystem for the register engine.
//!
//! Semantics mirror [`crate::vm`]'s stack-machine scheduler (`vm/task.rs`) exactly:
//! a deterministic FIFO run queue, a virtual tick that advances only past sleeps
//! and join deadlines, structured joins over [`Value::Task`] handles, and
//! suspension by swapping the running task's machine state into its task slot.
//!
//! The register engine executes activation trees through Rust recursion, so a
//! suspension point cooperatively unwinds that recursion: the innermost frame is
//! captured whole ([`RegActiveFrame`]) and every caller activation on the way out
//! contributes a [`RegFrameSlot`]. Resuming rebuilds the recursion from the
//! chain and re-executes the parked instruction, which then takes its fast path.

use crate::fault::VmFault;
use crate::reg_vm::{RegCallFrame, RegVm};
use crate::value::{FailureValue, Value};
use aipo_bytecode::{RegCompiledFunction, RegContract, RegInstruction};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

/// Identifier of the module entry script inside [`RegScheduler::tasks`].
pub const MAIN_TASK: u64 = 0;

/// Identifier of a structured join.
pub type JoinId = u64;

/// Result of one nested register call: its value, or a cooperative suspension
/// that must unwind the running activation immediately.
#[derive(Debug, Clone)]
pub enum RegCall {
    /// The call produced a value for the destination register.
    Value(Value),
    /// The running task parked; `run` must return without touching registers.
    Suspended,
}

/// Lifecycle of a scheduler-managed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegTaskStatus {
    /// Created, queued, never driven.
    Pending,
    /// Suspended until the virtual clock reaches `until` (inclusive wake).
    Sleeping {
        /// Virtual-clock tick of the wake deadline.
        until: u64,
    },
    /// Suspended on a task or join completing.
    Blocked(RegWaitTarget),
    /// Loaded on the machine and currently executing.
    Running,
    /// Completed with a value.
    Ready,
    /// Completed with a `Failure` value.
    Failed,
    /// Cancelled: driving or awaiting it faults.
    Cancelled,
}

/// What a blocked task waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegWaitTarget {
    /// Completion of one task.
    Task(u64),
    /// Completion of one structured join.
    Join(JoinId),
}

/// Terminal outcome of a task.
#[derive(Debug, Clone)]
pub enum RegTaskOutcome {
    /// Completed with a plain value.
    Ready(Value),
    /// Completed with a `Failure` value.
    Failed(Value),
    /// Cancelled before or during execution.
    Cancelled,
}

impl RegTaskOutcome {
    /// Classifies a returned value: `Failure` values become task failures.
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        if value.is_failure() {
            Self::Failed(value)
        } else {
            Self::Ready(value)
        }
    }

    /// Value a resuming waiter observes (`None` for cancellation, which faults).
    fn into_waiter_value(self) -> Option<Value> {
        match self {
            Self::Ready(value) | Self::Failed(value) => Some(value),
            Self::Cancelled => None,
        }
    }
}

/// Kind of a structured join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegJoinKind {
    /// All members complete; first failure fails the join.
    All,
    /// First member completion wins, value or failure.
    Race,
    /// Single member with a deadline; late completion cancels the member.
    Timeout,
    /// All group members complete; outcomes collected in completion order.
    GroupWait,
}

/// Pending or completed structured join.
#[derive(Debug)]
pub struct RegJoinState {
    kind: RegJoinKind,
    /// Member tasks (grows for group waits as the group spawns).
    members: Vec<u64>,
    /// Original member order (for `all` results).
    order: Vec<u64>,
    /// Members in completion order.
    completed: Vec<u64>,
    /// Waiting tasks (usually one; groups admit several).
    waiters: Vec<u64>,
    /// Deadline in ticks (`Timeout` only).
    deadline: Option<u64>,
    /// Group being waited on (`GroupWait` only).
    group: Option<u64>,
    /// Set once resolved; further polls replay the stored outcome.
    done: bool,
    /// Resolved result value.
    outcome: Option<Value>,
    /// Resolution faults the waiter (cancelled member) instead of resolving.
    fault: Option<VmFault>,
}

/// Live structured-concurrency group.
#[derive(Debug)]
pub struct RegGroupState {
    /// Every task spawned into the group, in spawn order.
    members: Vec<u64>,
}

/// Entry body of a spawned task that has not started yet.
#[derive(Debug)]
pub struct RegTaskEntry {
    /// Function-table entry of the body (receiver travels as argument 0).
    pub func: RegCompiledFunction,
    /// Captured arguments.
    pub args: Vec<Value>,
    /// Captured upvalue cells (closures only).
    pub upvalues: Vec<Rc<RefCell<Value>>>,
}

/// The innermost activation at a suspension point, captured whole.
#[derive(Debug)]
pub struct RegActiveFrame {
    /// Instruction array of the running function (cloned: resume must own it).
    pub code: Vec<RegInstruction>,
    /// Register file of the running function.
    pub registers: [Value; 256],
    /// Constant pool of the running function.
    pub constants: Vec<Value>,
    /// Contract descriptors of the running function.
    pub contracts: Vec<RegContract>,
    /// Resume point: the parked instruction re-executes on resume.
    pub pc: usize,
    /// Upvalue cells of the running frame.
    pub cells: Vec<Rc<RefCell<Value>>>,
}

/// A caller activation saved during the cooperative unwind.
#[derive(Debug)]
pub struct RegFrameSlot {
    /// Instruction array of the caller (cloned by its unwinding `run`).
    pub code: Vec<RegInstruction>,
    /// Caller register file at the call site.
    pub registers: [Value; 256],
    /// Caller constant pool.
    pub constants: Vec<Value>,
    /// Caller contract descriptors.
    pub contracts: Vec<RegContract>,
    /// Caller resume point (just after the call instruction).
    pub pc: usize,
    /// Caller register receiving the callee's result.
    pub dest: u8,
    /// Upvalue cells of the caller's frame.
    pub cells: Vec<Rc<RefCell<Value>>>,
}

/// Machine state of a parked task: its activation chain plus the stacks that
/// belong to it (the fiber swap).
#[derive(Debug)]
pub struct RegSuspendCapture {
    /// Saved caller activations, innermost caller first.
    pub chain: Vec<RegFrameSlot>,
    /// The active (innermost) frame's partial state.
    pub active: RegActiveFrame,
    /// Nested-call depth markers of this task.
    pub frames: Vec<RegCallFrame>,
    /// Failure-handler stack of this task.
    pub handler_stack: Vec<(usize, u8)>,
    /// Active iteration guards of this task.
    pub active_iterations: Vec<usize>,
}

/// Saved machine state of a suspended or queued task.
#[derive(Debug)]
pub struct RegTaskState {
    /// Lifecycle state.
    pub status: RegTaskStatus,
    /// Parked machine state while the task is not loaded.
    pub slot: Option<Box<RegSuspendCapture>>,
    /// Entry body of a task that has not started.
    pub entry: Option<RegTaskEntry>,
    /// Terminal payload for `Ready`/`Failed` tasks, observed by late awaiters.
    pub result: Option<Value>,
    /// Deadline for tasks that suspended on `task.sleep`, kept across the
    /// re-execution that resolves the call.
    pub sleeping_until: Option<u64>,
}

impl RegTaskState {
    /// Empty placeholder for a freshly spawned or freshly loaded task.
    fn empty() -> Self {
        Self {
            status: RegTaskStatus::Pending,
            slot: None,
            entry: None,
            result: None,
            sleeping_until: None,
        }
    }
}

/// Cooperative scheduler state of one [`RegVm`] session.
#[derive(Debug)]
pub struct RegScheduler {
    /// Every task by id, including the entry script ([`MAIN_TASK`]).
    pub(crate) tasks: HashMap<u64, RegTaskState>,
    /// Structured joins by id.
    pub(crate) joins: HashMap<JoinId, RegJoinState>,
    /// Structured groups by id.
    pub(crate) groups: HashMap<u64, RegGroupState>,
    /// Task-id waiters registered per awaited task.
    pub(crate) waiters: HashMap<u64, Vec<u64>>,
    /// Deterministic FIFO run queue.
    pub(crate) run_queue: VecDeque<u64>,
    /// Virtual clock in ticks (1s = 1000 ticks).
    pub(crate) tick: u64,
    /// Task loaded on the machine (`None` = bare entry script).
    pub(crate) current: Option<u64>,
    /// Next task id to hand out.
    pub(crate) next_task: u64,
    /// Next join id to hand out.
    pub(crate) next_join: u64,
    /// Next group id to hand out.
    pub(crate) next_group: u64,
    /// Settled outcome of the entry script.
    pub(crate) main_outcome: Option<RegTaskOutcome>,
}

impl Default for RegScheduler {
    fn default() -> Self {
        Self {
            tasks: HashMap::new(),
            joins: HashMap::new(),
            groups: HashMap::new(),
            waiters: HashMap::new(),
            run_queue: VecDeque::new(),
            tick: 0,
            current: None,
            // `MAIN_TASK` reserves id 0 for the entry script: spawned tasks
            // start at 1 exactly like the stack VM's scheduler.
            next_task: MAIN_TASK + 1,
            next_join: 0,
            next_group: 0,
            main_outcome: None,
        }
    }
}

/// Validated sleep amount: ticks, or a `Failure` value resolving inline.
enum Ticks {
    Ticks(u64),
    Failure(Value),
}

/// Validated join members: tasks, or a `Failure` list element resolving inline.
enum Members {
    Tasks(Vec<u64>),
    Failure(Value),
}

impl RegVm {
    // ------------------------------------------------------------------
    // Entry driving
    // ------------------------------------------------------------------

    /// Runs the entry body as the main task and drives the scheduler until the
    /// entry script settles. Spawned tasks the entry never awaited are dropped
    /// when it completes, matching the stack VM's `run` contract.
    ///
    /// # Errors
    /// Returns [`VmFault`] for runtime faults anywhere in the task graph.
    pub fn run_driven<F>(&mut self, body: F) -> Result<Value, VmFault>
    where
        F: FnOnce(&mut Self) -> Result<Value, VmFault>,
    {
        self.sched.current = None;
        let value = body(self)?;
        if self.pending_capture.is_some() {
            self.park_current_capture();
        } else {
            self.complete_current(RegTaskOutcome::from_value(value));
        }
        self.drive_main()
    }

    /// Drives the scheduler until the entry script's outcome is known.
    fn drive_main(&mut self) -> Result<Value, VmFault> {
        loop {
            if let Some(outcome) = self.sched.main_outcome.take() {
                return Ok(match outcome {
                    RegTaskOutcome::Ready(value) | RegTaskOutcome::Failed(value) => value,
                    // No `Task` handle for the entry script exists, so this is
                    // unreachable in practice; it keeps the mapping total.
                    RegTaskOutcome::Cancelled => Value::None,
                });
            }
            self.select_next()?;
            if self.sched.main_outcome.is_some() {
                continue;
            }
            if let Some(id) = self.sched.current {
                self.run_loaded_task(id)?;
            } else {
                // Nothing loaded and nothing settled: mirror the stack machine's
                // bare-step completion so the loop always terminates.
                self.complete_current(RegTaskOutcome::Ready(Value::None));
            }
        }
    }

    /// Runs the task loaded on the machine until it completes or parks again.
    fn run_loaded_task(&mut self, id: u64) -> Result<(), VmFault> {
        let (entry, slot) = {
            let state = self
                .sched
                .tasks
                .get_mut(&id)
                .expect("loaded task is tracked");
            state.status = RegTaskStatus::Running;
            (state.entry.take(), state.slot.take())
        };
        let outcome = if let Some(slot) = slot {
            self.resume_capture(*slot)
        } else if let Some(entry) = entry {
            let RegTaskEntry {
                func,
                args,
                upvalues,
            } = entry;
            match self.invoke_with_upvalues(&func, &args, upvalues, 0, true)? {
                RegCall::Value(value) => Ok(value),
                RegCall::Suspended => Ok(Value::None),
            }
        } else {
            Ok(Value::None)
        };
        if self.pending_capture.is_some() {
            self.park_current_capture();
            return Ok(());
        }
        match outcome {
            Ok(value) => {
                self.complete_current(RegTaskOutcome::from_value(value));
                Ok(())
            }
            Err(fault) => {
                // Cancellation is a task state, not a recoverable failure: the
                // task ends cancelled and its awaiters fault with `Cancelled`.
                // Every other fault keeps its code and aborts the program.
                let cancelled = matches!(
                    self.sched.tasks.get(&id).map(|state| state.status),
                    Some(RegTaskStatus::Cancelled)
                );
                if cancelled {
                    self.complete_current(RegTaskOutcome::Cancelled);
                    Ok(())
                } else {
                    Err(fault)
                }
            }
        }
    }

    /// Stores the in-flight cooperative capture into its task slot.
    pub(crate) fn park_current_capture(&mut self) {
        let capture = self
            .pending_capture
            .take()
            .expect("park without a pending capture");
        let id = self.sched.current.unwrap_or(MAIN_TASK);
        let state = self
            .sched
            .tasks
            .entry(id)
            .or_insert_with(RegTaskState::empty);
        state.slot = Some(capture);
    }

    /// Rebuilds the Rust activation chain of a parked task and continues it
    /// until it returns or parks again (leaving a new pending capture).
    fn resume_capture(&mut self, mut capture: RegSuspendCapture) -> Result<Value, VmFault> {
        self.frames = std::mem::take(&mut capture.frames);
        self.handler_stack = std::mem::take(&mut capture.handler_stack);
        self.active_iterations = std::mem::take(&mut capture.active_iterations);
        self.upvalue_frames.clear();
        let RegActiveFrame {
            code,
            registers,
            constants,
            contracts,
            pc,
            cells,
        } = capture.active;
        let mut code = code;
        let mut pc = pc;
        let mut chain = capture.chain.into_iter();
        self.registers = registers;
        self.constants = constants;
        self.contracts = contracts;
        self.upvalue_frames.push(cells);
        loop {
            self.pc = pc;
            let outcome = self.run_loop(&code);
            if self.pending_capture.is_some() {
                // Parked again: the fresh capture resumes innermost-first and
                // the not-yet-resumed callers stay on the chain behind it.
                if let Some(fresh) = self.pending_capture.as_mut() {
                    let rest: Vec<RegFrameSlot> = chain.collect();
                    fresh.chain.extend(rest);
                }
                return Ok(Value::None);
            }
            let value = outcome?;
            match chain.next() {
                None => return Ok(value),
                Some(slot) => {
                    self.upvalue_frames.pop();
                    self.registers = slot.registers;
                    self.constants = slot.constants;
                    self.contracts = slot.contracts;
                    self.registers[slot.dest as usize] = value;
                    self.upvalue_frames.push(slot.cells);
                    code = slot.code;
                    pc = slot.pc;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Scheduler core
    // ------------------------------------------------------------------

    /// Completes the loaded task (or the entry script) with `outcome`.
    /// Returns `true` when the entry script settled.
    pub(crate) fn complete_current(&mut self, outcome: RegTaskOutcome) -> bool {
        let id = self.sched.current.take().unwrap_or(MAIN_TASK);
        let outcome = match self.sched.tasks.get(&id) {
            Some(state) if state.status == RegTaskStatus::Cancelled => RegTaskOutcome::Cancelled,
            _ => outcome,
        };
        if id == MAIN_TASK {
            self.sched.main_outcome = Some(outcome);
            self.sched.tasks.remove(&MAIN_TASK);
            return true;
        }
        self.finish_task(id, outcome);
        false
    }

    /// Settles a task, wakes its direct waiters, and polls affected joins.
    fn finish_task(&mut self, id: u64, outcome: RegTaskOutcome) {
        {
            let state = self
                .sched
                .tasks
                .entry(id)
                .or_insert_with(RegTaskState::empty);
            match &outcome {
                RegTaskOutcome::Cancelled => state.status = RegTaskStatus::Cancelled,
                RegTaskOutcome::Ready(value) => {
                    state.status = RegTaskStatus::Ready;
                    state.result = Some(value.clone());
                }
                RegTaskOutcome::Failed(failure) => {
                    state.status = RegTaskStatus::Failed;
                    state.result = Some(failure.clone());
                }
            }
        }
        if let Some(waiters) = self.sched.waiters.remove(&id) {
            for waiter in waiters {
                self.wake_waiter(waiter);
            }
        }
        let affected: Vec<JoinId> = self
            .sched
            .joins
            .iter()
            .filter(|(_, join)| join.members.contains(&id))
            .map(|(join_id, _)| *join_id)
            .collect();
        for join_id in affected {
            if let Some(join) = self.sched.joins.get_mut(&join_id) {
                if !join.completed.contains(&id) {
                    join.completed.push(id);
                }
            }
            self.poll_join(join_id);
        }
    }

    /// Wakes a waiter whose target completed: it becomes runnable and resumes
    /// by re-executing its blocking instruction.
    fn wake_waiter(&mut self, waiter: u64) {
        if let Some(state) = self.sched.tasks.get_mut(&waiter) {
            if matches!(state.status, RegTaskStatus::Blocked(_)) {
                state.status = RegTaskStatus::Pending;
            } else {
                return;
            }
        } else {
            return;
        }
        if !self.sched.run_queue.contains(&waiter) {
            self.sched.run_queue.push_back(waiter);
        }
    }

    /// Picks the next runnable task, advancing virtual time past sleepers when
    /// nothing is runnable.
    fn select_next(&mut self) -> Result<(), VmFault> {
        // Fast path: the never-suspended main script with nothing queued.
        if self.sched.run_queue.is_empty() && !self.sched.tasks.contains_key(&MAIN_TASK) {
            return Ok(());
        }
        loop {
            self.poll_timeouts();
            let mut pick: Option<u64> = None;
            self.sched.run_queue.retain(|&id| {
                matches!(
                    self.sched.tasks.get(&id).map(|state| state.status),
                    Some(
                        RegTaskStatus::Pending
                            | RegTaskStatus::Blocked(_)
                            | RegTaskStatus::Running
                            | RegTaskStatus::Sleeping { .. }
                    )
                )
            });
            for &id in &self.sched.run_queue {
                match self.sched.tasks.get(&id).map(|state| state.status) {
                    Some(RegTaskStatus::Pending)
                    | Some(RegTaskStatus::Blocked(_))
                    | Some(RegTaskStatus::Running) => {
                        pick = Some(id);
                        break;
                    }
                    Some(RegTaskStatus::Sleeping { until }) if until <= self.sched.tick => {
                        pick = Some(id);
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(id) = pick {
                if let Some(pos) = self.sched.run_queue.iter().position(|&x| x == id) {
                    self.sched.run_queue.remove(pos);
                }
                if matches!(
                    self.sched.tasks.get(&id),
                    Some(state) if state.status == RegTaskStatus::Cancelled
                ) {
                    continue;
                }
                self.load_task(id);
                return Ok(());
            }
            let next_wake = self
                .sched
                .tasks
                .values()
                .filter_map(|state| match state.status {
                    RegTaskStatus::Sleeping { until } if until > self.sched.tick => Some(until),
                    _ => None,
                })
                .min();
            let next_timeout = self
                .sched
                .joins
                .values()
                .filter_map(|join| match join.deadline {
                    Some(deadline) if !join.done && deadline > self.sched.tick => Some(deadline),
                    _ => None,
                })
                .min();
            let next_wake = match (next_wake, next_timeout) {
                (Some(w), Some(t)) => Some(w.min(t)),
                (Some(w), None) => Some(w),
                (None, Some(t)) => Some(t),
                (None, None) => None,
            };
            match next_wake {
                Some(tick) => self.sched.tick = tick,
                None => {
                    let live = self.sched.tasks.values().any(|state| {
                        matches!(
                            state.status,
                            RegTaskStatus::Pending
                                | RegTaskStatus::Sleeping { .. }
                                | RegTaskStatus::Blocked(_)
                                | RegTaskStatus::Running
                        )
                    });
                    if live {
                        return Err(VmFault::CorruptedBytecode {
                            offset: 0,
                            reason: "scheduler deadlock: live tasks with nothing runnable"
                                .to_string(),
                        });
                    }
                    return Ok(());
                }
            }
        }
    }

    /// Loads a queued task onto the machine, marking it running. The machine's
    /// per-task stacks are cleared: a previous task's leftovers must never be
    /// visible to the loaded body (stale handlers would misroute failures).
    fn load_task(&mut self, id: u64) {
        let mut state = self
            .sched
            .tasks
            .remove(&id)
            .unwrap_or_else(RegTaskState::empty);
        self.registers = std::array::from_fn(|_| Value::None);
        self.register_stack.clear();
        self.constants.clear();
        self.contracts.clear();
        self.frames.clear();
        self.upvalue_frames.clear();
        self.handler_stack.clear();
        self.active_iterations.clear();
        state.status = RegTaskStatus::Running;
        self.sched.tasks.insert(id, state);
        self.sched.current = Some(id);
    }

    /// Resolves deadline joins whose time has passed.
    fn poll_timeouts(&mut self) {
        let expired: Vec<JoinId> = self
            .sched
            .joins
            .iter()
            .filter(|(_, join)| {
                !join.done
                    && join.kind == RegJoinKind::Timeout
                    && join
                        .deadline
                        .is_some_and(|deadline| self.sched.tick > deadline)
            })
            .map(|(join_id, _)| *join_id)
            .collect();
        for join_id in expired {
            if let Some(join) = self.sched.joins.get_mut(&join_id) {
                join.done = true;
                join.outcome = Some(Value::Failure(Rc::new(FailureValue::new(
                    "timeout".to_string(),
                ))));
            }
            let member = self
                .sched
                .joins
                .get(&join_id)
                .and_then(|join| join.members.first().copied());
            if let Some(member) = member {
                // The late member is cancelled; its other awaiters observe the
                // cancellation fault while this join reports a timeout.
                self.finish_task(member, RegTaskOutcome::Cancelled);
            }
            self.wake_join(join_id);
        }
    }

    /// Advances a join after a member completes; resolves and wakes waiters
    /// once the join's condition holds. Idempotent: completed joins replay.
    fn poll_join(&mut self, join_id: JoinId) {
        let done = self.sched.joins.get(&join_id).is_some_and(|join| join.done);
        if done {
            return;
        }
        enum Resolution {
            Pending,
            Value(Value),
            Fault(VmFault),
        }
        let resolution = match self.sched.joins.get(&join_id).map(|join| join.kind) {
            None => return,
            Some(RegJoinKind::All) => {
                let terminal = |id: &u64| {
                    matches!(
                        self.sched.tasks.get(id).map(|state| state.status),
                        Some(
                            RegTaskStatus::Ready | RegTaskStatus::Failed | RegTaskStatus::Cancelled
                        )
                    )
                };
                let join = &self.sched.joins[&join_id];
                if join.members.iter().any(|member| {
                    self.task_result(member)
                        .is_some_and(|o| matches!(o, RegTaskOutcome::Cancelled))
                }) {
                    Resolution::Fault(VmFault::Cancelled {
                        details: "a joined task was cancelled".to_string(),
                    })
                } else if join.members.iter().all(terminal) {
                    // First failure by completion order fails the join;
                    // otherwise results follow the original member order.
                    let first_failure =
                        self.completion_order(join_id)
                            .into_iter()
                            .find_map(|member| match self.task_result(&member) {
                                Some(RegTaskOutcome::Failed(failure)) => Some(failure),
                                _ => None,
                            });
                    if let Some(failure) = first_failure {
                        Resolution::Value(failure)
                    } else {
                        let values = join
                            .order
                            .iter()
                            .map(|member| {
                                self.task_result(member)
                                    .and_then(RegTaskOutcome::into_waiter_value)
                                    .unwrap_or(Value::None)
                            })
                            .collect();
                        Resolution::Value(Value::List(Rc::new(RefCell::new(values))))
                    }
                } else {
                    Resolution::Pending
                }
            }
            Some(RegJoinKind::Race) => {
                let winner = self
                    .completion_order(join_id)
                    .into_iter()
                    .find_map(|member| self.task_result(&member));
                match winner {
                    None => Resolution::Pending,
                    Some(RegTaskOutcome::Cancelled) => Resolution::Fault(VmFault::Cancelled {
                        details: "the winning task was cancelled".to_string(),
                    }),
                    Some(RegTaskOutcome::Ready(value) | RegTaskOutcome::Failed(value)) => {
                        Resolution::Value(value)
                    }
                }
            }
            Some(RegJoinKind::Timeout) => {
                let member = self.sched.joins[&join_id].members.first().copied();
                match member.and_then(|id| self.task_result(&id)) {
                    None => Resolution::Pending,
                    Some(RegTaskOutcome::Cancelled) => Resolution::Fault(VmFault::Cancelled {
                        details: "the timed task was cancelled".to_string(),
                    }),
                    Some(RegTaskOutcome::Ready(value) | RegTaskOutcome::Failed(value)) => {
                        Resolution::Value(value)
                    }
                }
            }
            Some(RegJoinKind::GroupWait) => {
                // Late spawns extend `members`, so completion requires every
                // currently known member to be terminal.
                let terminal = |id: &u64| {
                    matches!(
                        self.sched.tasks.get(id).map(|state| state.status),
                        Some(
                            RegTaskStatus::Ready | RegTaskStatus::Failed | RegTaskStatus::Cancelled
                        )
                    )
                };
                let all_terminal = self.sched.joins[&join_id].members.iter().all(terminal);
                let any_cancelled = self.sched.joins[&join_id].members.iter().any(|member| {
                    matches!(self.task_result(member), Some(RegTaskOutcome::Cancelled))
                });
                if any_cancelled {
                    Resolution::Fault(VmFault::Cancelled {
                        details: "a group member was cancelled".to_string(),
                    })
                } else if all_terminal {
                    let values = self
                        .completion_order(join_id)
                        .into_iter()
                        .map(|member| {
                            self.task_result(&member)
                                .and_then(RegTaskOutcome::into_waiter_value)
                                .unwrap_or(Value::None)
                        })
                        .collect();
                    Resolution::Value(Value::List(Rc::new(RefCell::new(values))))
                } else {
                    Resolution::Pending
                }
            }
        };
        match resolution {
            Resolution::Pending => {}
            Resolution::Value(value) => {
                if let Some(join) = self.sched.joins.get_mut(&join_id) {
                    join.done = true;
                    join.outcome = Some(value);
                }
                self.wake_join(join_id);
            }
            Resolution::Fault(fault) => {
                if let Some(join) = self.sched.joins.get_mut(&join_id) {
                    join.done = true;
                    join.fault = Some(fault);
                }
                self.wake_join(join_id);
            }
        }
    }

    /// Wakes every waiter of a resolved join.
    fn wake_join(&mut self, join_id: JoinId) {
        let waiters = self
            .sched
            .joins
            .get(&join_id)
            .map(|join| join.waiters.clone())
            .unwrap_or_default();
        for waiter in waiters {
            self.wake_waiter(waiter);
        }
    }

    /// Members in completion order: tasks already terminal when the join was
    /// created count as earliest, in member order, followed by observed
    /// completions. Late group spawns extend `members` and complete in place.
    fn completion_order(&self, join_id: JoinId) -> Vec<u64> {
        let join = &self.sched.joins[&join_id];
        let mut ordered: Vec<u64> = join
            .completed
            .iter()
            .filter(|member| join.members.contains(member))
            .copied()
            .collect();
        let mut missing: Vec<u64> = join
            .members
            .iter()
            .filter(|member| !ordered.contains(member))
            .copied()
            .collect();
        missing.append(&mut ordered);
        missing
    }

    /// Terminal outcome of a tracked task, if it already completed.
    fn task_result(&self, id: &u64) -> Option<RegTaskOutcome> {
        let state = self.sched.tasks.get(id)?;
        match state.status {
            RegTaskStatus::Ready => state.result.clone().map(RegTaskOutcome::Ready),
            RegTaskStatus::Failed => state.result.clone().map(RegTaskOutcome::Failed),
            RegTaskStatus::Cancelled => Some(RegTaskOutcome::Cancelled),
            _ => None,
        }
    }

    /// Whether `from` transitively awaits `target` through task waits or
    /// structured joins.
    fn blocks_on(&self, from: u64, target: u64) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(from);
        visited.insert(from);

        while let Some(cursor) = queue.pop_front() {
            match self.sched.tasks.get(&cursor).map(|state| state.status) {
                Some(RegTaskStatus::Blocked(RegWaitTarget::Task(next))) => {
                    if next == target {
                        return true;
                    }
                    if visited.insert(next) {
                        queue.push_back(next);
                    }
                }
                Some(RegTaskStatus::Blocked(RegWaitTarget::Join(join_id))) => {
                    if let Some(join) = self.sched.joins.get(&join_id) {
                        for &member in &join.members {
                            if member == target {
                                return true;
                            }
                            if visited.insert(member) {
                                queue.push_back(member);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Builds the await chain for cycle reports.
    fn await_chain(&self, me: u64, id: u64) -> Vec<u64> {
        let mut chain = vec![me, id];
        let mut cursor = id;
        let mut visited = 0;
        let max = self.sched.tasks.len() + 1;
        while visited <= max {
            visited += 1;
            match self.sched.tasks.get(&cursor).map(|state| state.status) {
                Some(RegTaskStatus::Blocked(RegWaitTarget::Task(next))) => {
                    chain.push(next);
                    if next == me {
                        break;
                    }
                    cursor = next;
                }
                _ => break,
            }
        }
        chain
    }

    // ------------------------------------------------------------------
    // Suspension
    // ------------------------------------------------------------------

    /// Parks the running task at the current instruction and cooperatively
    /// unwinds. A cancellation that landed mid-run faults instead of saving.
    ///
    /// # Errors
    /// Returns [`VmError::Suspended`] on success (the unwind signal) or
    /// [`VmFault::Cancelled`] when the task was cancelled.
    pub(crate) fn suspend_at_instruction(
        &mut self,
        code: &[RegInstruction],
        status: RegTaskStatus,
    ) -> Result<RegCall, VmFault> {
        let id = self.sched.current.unwrap_or(MAIN_TASK);
        if matches!(
            self.sched.tasks.get(&id),
            Some(state) if state.status == RegTaskStatus::Cancelled
        ) {
            return Err(VmFault::Cancelled {
                details: format!("task {id} was cancelled"),
            });
        }
        // Rewind so the parked instruction re-executes on resume and takes its
        // fast path (mirrors `suspend_at_call`).
        self.pc = self.pc.saturating_sub(1);
        let sleeping_until = match status {
            RegTaskStatus::Sleeping { until } => Some(until),
            _ => None,
        };
        let state = self
            .sched
            .tasks
            .entry(id)
            .or_insert_with(RegTaskState::empty);
        state.status = status;
        state.sleeping_until = sleeping_until;
        let active = RegActiveFrame {
            code: code.to_vec(),
            registers: std::mem::replace(&mut self.registers, std::array::from_fn(|_| Value::None)),
            constants: std::mem::take(&mut self.constants),
            contracts: std::mem::take(&mut self.contracts),
            pc: self.pc,
            cells: self.upvalue_frames.pop().unwrap_or_default(),
        };
        self.pending_capture = Some(Box::new(RegSuspendCapture {
            chain: Vec::new(),
            active,
            frames: std::mem::take(&mut self.frames),
            handler_stack: std::mem::take(&mut self.handler_stack),
            active_iterations: std::mem::take(&mut self.active_iterations),
        }));
        if matches!(status, RegTaskStatus::Sleeping { .. }) && !self.sched.run_queue.contains(&id) {
            self.sched.run_queue.push_back(id);
        }
        Ok(RegCall::Suspended)
    }

    // ------------------------------------------------------------------
    // Task natives
    // ------------------------------------------------------------------

    /// Spawns `callee(args)` as a task, optionally into a structured group.
    ///
    /// # Errors
    /// Returns [`VmFault`] when the callee is not callable or the arity mismatches.
    pub fn spawn_reg_task(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        group: Option<u64>,
    ) -> Result<u64, VmFault> {
        let (func, cells) = match callee {
            Value::Function {
                entry_ip, arity, ..
            } => {
                if args.len() != arity as usize {
                    return Err(VmFault::TypeMismatch {
                        expected: format!("{arity} arguments"),
                        actual: format!("{} arguments", args.len()),
                    });
                }
                (entry_ip as usize, Vec::new())
            }
            Value::Closure(closure) => {
                if args.len() != closure.arity {
                    return Err(VmFault::TypeMismatch {
                        expected: format!("{} arguments", closure.arity),
                        actual: format!("{} arguments", args.len()),
                    });
                }
                (closure.entry_ip, closure.upvalues.clone())
            }
            other => {
                return Err(VmFault::NotCallable {
                    type_name: other.type_name().to_string(),
                });
            }
        };
        let func = self
            .functions
            .get(func)
            .cloned()
            .ok_or_else(|| VmFault::CorruptedBytecode {
                offset: self.pc.saturating_sub(1),
                reason: format!("unknown function index {func}"),
            })?;
        let id = self.sched.next_task;
        self.sched.next_task += 1;
        self.sched.tasks.insert(
            id,
            RegTaskState {
                status: RegTaskStatus::Pending,
                slot: None,
                entry: Some(RegTaskEntry {
                    func,
                    args,
                    upvalues: cells,
                }),
                result: None,
                sleeping_until: None,
            },
        );
        self.sched.run_queue.push_back(id);
        if let Some(group_id) = group {
            if let Some(state) = self.sched.groups.get_mut(&group_id) {
                state.members.push(id);
            }
            // Late spawns join open group waits.
            let open: Vec<JoinId> = self
                .sched
                .joins
                .iter()
                .filter(|(_, join)| {
                    !join.done
                        && join.kind == RegJoinKind::GroupWait
                        && join.group == Some(group_id)
                })
                .map(|(join_id, _)| *join_id)
                .collect();
            for join_id in open {
                if let Some(join) = self.sched.joins.get_mut(&join_id) {
                    if !join.members.contains(&id) {
                        join.members.push(id);
                        join.order.push(id);
                    }
                }
            }
        }
        Ok(id)
    }

    /// Dispatches the `task.*` natives intercepted in the `Call` handler.
    ///
    /// # Errors
    /// Returns [`VmFault`] on argument violations, or the cooperative suspend
    /// signal ([`RegCall::Suspended`]) when the call parks the task.
    pub fn task_call(
        &mut self,
        name: &str,
        args: &[Value],
        code: &[RegInstruction],
    ) -> Result<RegCall, VmFault> {
        // Model B at the boundary: failures pass through untouched.
        for arg in args {
            if arg.is_failure() {
                return Ok(RegCall::Value(arg.clone()));
            }
        }
        match name {
            "task.spawn" => {
                let items =
                    task_list_arg(args.get(1).unwrap_or(&Value::None), "task.spawn(f, args)")?;
                let id = self.spawn_reg_task(args[0].clone(), items, None)?;
                Ok(RegCall::Value(Value::Task(id)))
            }
            "task.sleep" => self.do_sleep(&args[0], code),
            "task.all" => match task_members(&args[0], "task.all")? {
                Members::Tasks(members) => {
                    self.do_join(RegJoinKind::All, members, None, "task.all", code)
                }
                Members::Failure(failure) => Ok(RegCall::Value(failure)),
            },
            "task.race" => match task_members(&args[0], "task.race")? {
                Members::Tasks(members) => {
                    self.do_join(RegJoinKind::Race, members, None, "task.race", code)
                }
                Members::Failure(failure) => Ok(RegCall::Value(failure)),
            },
            "task.timeout" => {
                let member = match &args[0] {
                    Value::Task(id) => *id,
                    other => {
                        return Err(VmFault::TypeMismatch {
                            expected: "Task for task.timeout".to_string(),
                            actual: other.type_name().to_string(),
                        });
                    }
                };
                match sleep_ticks(&args[1], "task.timeout")? {
                    Ticks::Failure(failure) => Ok(RegCall::Value(failure)),
                    Ticks::Ticks(ticks) => {
                        let deadline = self.sched.tick.saturating_add(ticks);
                        self.do_join(
                            RegJoinKind::Timeout,
                            vec![member],
                            Some(deadline),
                            "task.timeout",
                            code,
                        )
                    }
                }
            }
            "task.cancel" => {
                let id = match &args[0] {
                    Value::Task(id) => *id,
                    other => {
                        return Err(VmFault::TypeMismatch {
                            expected: "Task for task.cancel".to_string(),
                            actual: other.type_name().to_string(),
                        });
                    }
                };
                match self.sched.tasks.get(&id).map(|state| state.status) {
                    None => {
                        return Err(VmFault::TypeMismatch {
                            expected: "live task for task.cancel".to_string(),
                            actual: format!("unknown task {id}"),
                        });
                    }
                    Some(
                        RegTaskStatus::Ready | RegTaskStatus::Failed | RegTaskStatus::Cancelled,
                    ) => {}
                    Some(_) => self.finish_task(id, RegTaskOutcome::Cancelled),
                }
                Ok(RegCall::Value(Value::None))
            }
            "task.group" => {
                let id = self.sched.next_group;
                self.sched.next_group += 1;
                self.sched.groups.insert(
                    id,
                    RegGroupState {
                        members: Vec::new(),
                    },
                );
                Ok(RegCall::Value(Value::Group(id)))
            }
            other => Err(VmFault::CorruptedBytecode {
                offset: self.pc,
                reason: format!("unknown task function {other}"),
            }),
        }
    }

    /// `task.sleep(ticks | duration)`: yields until the virtual clock passes
    /// the deadline. Resuming re-executes the call and takes the fast path.
    fn do_sleep(&mut self, arg: &Value, code: &[RegInstruction]) -> Result<RegCall, VmFault> {
        let me = self.sched.current.unwrap_or(MAIN_TASK);
        if let Some(until) = self
            .sched
            .tasks
            .get_mut(&me)
            .and_then(|s| s.sleeping_until.take())
        {
            if until <= self.sched.tick {
                return Ok(RegCall::Value(Value::None));
            }
            if let Some(state) = self.sched.tasks.get_mut(&me) {
                state.sleeping_until = Some(until);
            }
            return self.suspend_at_instruction(code, RegTaskStatus::Sleeping { until });
        }
        match sleep_ticks(arg, "task.sleep")? {
            Ticks::Failure(failure) => Ok(RegCall::Value(failure)),
            Ticks::Ticks(ticks) => {
                let until = self.sched.tick.saturating_add(ticks);
                self.suspend_at_instruction(code, RegTaskStatus::Sleeping { until })
            }
        }
    }

    /// Shared `all`/`race`/`timeout` implementation: terminal members resolve
    /// inline, otherwise the waiter's pending join is reused or created.
    fn do_join(
        &mut self,
        kind: RegJoinKind,
        members: Vec<u64>,
        deadline: Option<u64>,
        _operation: &str,
        code: &[RegInstruction],
    ) -> Result<RegCall, VmFault> {
        if members.is_empty() {
            let empty = match kind {
                RegJoinKind::Race => {
                    Value::Failure(Rc::new(FailureValue::new("race of no tasks".to_string())))
                }
                RegJoinKind::All | RegJoinKind::GroupWait => {
                    Value::List(Rc::new(RefCell::new(Vec::new().into())))
                }
                RegJoinKind::Timeout => {
                    Value::Failure(Rc::new(FailureValue::new("timeout of no task".to_string())))
                }
            };
            return Ok(RegCall::Value(empty));
        }
        let me = self.sched.current.unwrap_or(MAIN_TASK);
        if matches!(kind, RegJoinKind::All | RegJoinKind::GroupWait) {
            for &member in &members {
                if member == me || self.blocks_on(member, me) {
                    return Err(VmFault::AwaitCycle {
                        chain: self.await_chain(me, member),
                    });
                }
            }
        }
        // Reuse the waiter's pending join when it matches this call;
        // otherwise create one. A task blocks on at most one join at a time,
        // so the pending join is unambiguous.
        let join_id = match self
            .join_for(kind, &members)
            .or_else(|| self.pending_join(kind))
        {
            Some(id) => id,
            None => {
                let id = self.sched.next_join;
                self.sched.next_join += 1;
                self.sched.joins.insert(
                    id,
                    RegJoinState {
                        kind,
                        order: members.clone(),
                        members,
                        completed: Vec::new(),
                        waiters: vec![me],
                        deadline,
                        group: None,
                        done: false,
                        outcome: None,
                        fault: None,
                    },
                );
                id
            }
        };
        self.poll_join(join_id);
        let join = &self.sched.joins[&join_id];
        if join.done {
            let (fault, outcome) = (join.fault.clone(), join.outcome.clone());
            self.sched.joins.remove(&join_id);
            if let Some(fault) = fault {
                return Err(fault);
            }
            let outcome = outcome.unwrap_or(Value::None);
            return Ok(RegCall::Value(outcome));
        }
        self.suspend_at_instruction(code, RegTaskStatus::Blocked(RegWaitTarget::Join(join_id)))
    }

    /// The waiter's pending join of this kind, if any.
    fn pending_join(&self, kind: RegJoinKind) -> Option<JoinId> {
        let me = self.sched.current.unwrap_or(MAIN_TASK);
        for (&id, join) in &self.sched.joins {
            if join.kind == kind && join.waiters.contains(&me) {
                return Some(id);
            }
        }
        None
    }

    /// Reuses the waiter's pending join when it matches this call.
    fn join_for(&self, kind: RegJoinKind, members: &[u64]) -> Option<JoinId> {
        let me = self.sched.current.unwrap_or(MAIN_TASK);
        for (&id, join) in &self.sched.joins {
            if join.kind == kind && join.members == members && join.waiters.contains(&me) {
                return Some(id);
            }
        }
        None
    }

    /// Dispatches `Group` methods (`spawn`, `wait`) intercepted in `Call`.
    ///
    /// # Errors
    /// Returns [`VmFault`] on receiver/argument violations or the suspend signal.
    pub fn group_method(
        &mut self,
        name: &str,
        receiver: &Value,
        args: &[Value],
        code: &[RegInstruction],
    ) -> Result<RegCall, VmFault> {
        let group_id = match receiver {
            Value::Group(id) => *id,
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "Group receiver".to_string(),
                    actual: other.type_name().to_string(),
                });
            }
        };
        if !self.sched.groups.contains_key(&group_id) {
            return Err(VmFault::TypeMismatch {
                expected: "live group".to_string(),
                actual: format!("unknown group {group_id}"),
            });
        }
        for arg in args {
            if arg.is_failure() {
                return Ok(RegCall::Value(arg.clone()));
            }
        }
        match name {
            "spawn" => {
                let items =
                    task_list_arg(args.get(1).unwrap_or(&Value::None), "group.spawn(f, args)")?;
                let id = self.spawn_reg_task(args[0].clone(), items, Some(group_id))?;
                Ok(RegCall::Value(Value::Task(id)))
            }
            "wait" => self.do_group_wait(group_id, code),
            other => Err(VmFault::TypeMismatch {
                expected: "spawn or wait".to_string(),
                actual: format!("unknown group method {other}"),
            }),
        }
    }

    /// `group.wait()`: collects every member outcome in completion order.
    /// Cancelled members fault the waiter, since cancellation has no value form.
    fn do_group_wait(
        &mut self,
        group_id: u64,
        code: &[RegInstruction],
    ) -> Result<RegCall, VmFault> {
        let me = self.sched.current.unwrap_or(MAIN_TASK);
        // Reuse the waiter's pending group join for this group.
        let pending = if let Some(RegTaskStatus::Blocked(RegWaitTarget::Join(id))) =
            self.sched.tasks.get(&me).map(|state| state.status)
        {
            self.sched
                .joins
                .get(&id)
                .filter(|join| join.kind == RegJoinKind::GroupWait && join.group == Some(group_id))
                .map(|_| id)
        } else {
            None
        };
        let join_id = match pending {
            Some(id) => id,
            None => {
                let members = self.sched.groups[&group_id].members.clone();
                if members.iter().all(|member| {
                    matches!(
                        self.task_result(member),
                        Some(RegTaskOutcome::Ready(_) | RegTaskOutcome::Failed(_))
                    )
                }) {
                    if members.iter().any(|member| {
                        matches!(self.task_result(member), Some(RegTaskOutcome::Cancelled))
                    }) {
                        return Err(VmFault::Cancelled {
                            details: "a group member was cancelled".to_string(),
                        });
                    }
                    let values = members
                        .iter()
                        .map(|member| {
                            self.task_result(member)
                                .and_then(RegTaskOutcome::into_waiter_value)
                                .unwrap_or(Value::None)
                        })
                        .collect();
                    return Ok(RegCall::Value(Value::List(Rc::new(RefCell::new(values)))));
                }
                let id = self.sched.next_join;
                self.sched.next_join += 1;
                self.sched.joins.insert(
                    id,
                    RegJoinState {
                        kind: RegJoinKind::GroupWait,
                        order: members.clone(),
                        members,
                        completed: Vec::new(),
                        waiters: vec![me],
                        deadline: None,
                        group: Some(group_id),
                        done: false,
                        outcome: None,
                        fault: None,
                    },
                );
                id
            }
        };
        self.poll_join(join_id);
        let join = &self.sched.joins[&join_id];
        if join.done {
            if let Some(fault) = join.fault.clone() {
                return Err(fault);
            }
            let outcome = join.outcome.clone().unwrap_or(Value::None);
            return Ok(RegCall::Value(outcome));
        }
        self.suspend_at_instruction(code, RegTaskStatus::Blocked(RegWaitTarget::Join(join_id)))
    }

    /// `await target`: fast paths resolve terminal tasks inline; pending tasks
    /// suspend the waiter (rewound onto `await`) and drive the target first.
    ///
    /// # Errors
    /// Returns [`VmFault`] for cycles, cancellation and type violations, or the
    /// suspend signal when the await parks the running task.
    pub fn reg_await(&mut self, dest: u8, code: &[RegInstruction]) -> Result<RegCall, VmFault> {
        let target = self.registers[dest as usize].clone();
        if target.is_failure() {
            return Ok(RegCall::Value(target));
        }
        let id = match target {
            Value::Task(id) => id,
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "Task to await".to_string(),
                    actual: other.type_name().to_string(),
                });
            }
        };
        match self.task_result(&id) {
            Some(RegTaskOutcome::Ready(value) | RegTaskOutcome::Failed(value)) => {
                Ok(RegCall::Value(value))
            }
            Some(RegTaskOutcome::Cancelled) => Err(VmFault::Cancelled {
                details: format!("task {id} was cancelled"),
            }),
            None => {
                if !self.sched.tasks.contains_key(&id) {
                    return Err(VmFault::TypeMismatch {
                        expected: "live task to await".to_string(),
                        actual: format!("unknown task {id}"),
                    });
                }
                let me = self.sched.current.unwrap_or(MAIN_TASK);
                if me == id || self.blocks_on(id, me) {
                    return Err(VmFault::AwaitCycle {
                        chain: self.await_chain(me, id),
                    });
                }
                let waiters = self.sched.waiters.entry(id).or_default();
                if !waiters.contains(&me) {
                    waiters.push(me);
                }
                // Handover: drive the awaited task before the rest of the queue.
                if !self.sched.run_queue.contains(&id) {
                    self.sched.run_queue.push_front(id);
                }
                self.suspend_at_instruction(code, RegTaskStatus::Blocked(RegWaitTarget::Task(id)))
            }
        }
    }
}

/// Whether a native's name is one of the `task.*` combinators the register
/// engine intercepts in `Call` (mirrors the stack VM's `begin_call` dispatch).
#[must_use]
pub fn is_task_native(name: &str) -> bool {
    matches!(
        name,
        "task.spawn"
            | "task.sleep"
            | "task.all"
            | "task.race"
            | "task.timeout"
            | "task.cancel"
            | "task.group"
    )
}

/// Reads an `Int` tick count or `Duration` for `sleep`/`timeout`.
///
/// `Duration` seconds convert to virtual ticks at 1000 ticks per second;
/// negative or non-finite amounts resolve to a recoverable `Failure`.
fn sleep_ticks(arg: &Value, operation: &str) -> Result<Ticks, VmFault> {
    match arg {
        Value::Int(n) if *n >= 0 => Ok(Ticks::Ticks(*n as u64)),
        Value::Int(_) => Ok(Ticks::Failure(Value::Failure(Rc::new(FailureValue::new(
            format!("{operation} amount must be >= 0"),
        ))))),
        Value::Byte(b) => Ok(Ticks::Ticks(u64::from(*b))),
        Value::Duration(seconds) if seconds.is_finite() && *seconds >= 0.0 =>
        {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Ok(Ticks::Ticks((seconds * 1000.0) as u64))
        }
        Value::Duration(_) => Ok(Ticks::Failure(Value::Failure(Rc::new(FailureValue::new(
            format!("{operation} duration must be finite and >= 0"),
        ))))),
        other => Err(VmFault::TypeMismatch {
            expected: "Int ticks or Duration".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Reads the `[args]` list of a `spawn` call.
fn task_list_arg(arg: &Value, operation: &str) -> Result<Vec<Value>, VmFault> {
    match arg {
        Value::List(items) => Ok(items.borrow().to_vec()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("argument list for {operation}"),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Reads and validates the `[tasks]` list of an `all`/`race` call.
///
/// A `Failure` element resolves the join with that failure (Model B at the
/// boundary); non-task elements are a type mismatch.
fn task_members(arg: &Value, operation: &str) -> Result<Members, VmFault> {
    let items = match arg {
        Value::List(items) => items.borrow().clone(),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: format!("task list for {operation}"),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut members = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Task(id) => members.push(id),
            failure if failure.is_failure() => return Ok(Members::Failure(failure)),
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: format!("Task members for {operation}"),
                    actual: other.type_name().to_string(),
                });
            }
        }
    }
    Ok(Members::Tasks(members))
}
