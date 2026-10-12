//! Cooperative task scheduler, structured joins, lazy sequences, and groups.
//!
//! Scheduling is single-threaded and deterministic: tasks run depth-first (an
//! `await` drives its target immediately, handover-style) over a FIFO run
//! queue, and virtual time advances only while tasks sleep or wait on
//! deadlines. Pure computation never moves the clock, so a fixed program input
//! always produces the same interleaving. There is no preemption: a task that
//! never suspends starves every other task.
//!
//! Suspension protocol: every blocking primitive (`await`, `task.sleep`, the
//! `all`/`race`/`timeout`/group joins) suspends with the triggering
//! instruction and the operand stack intact, so resuming re-executes the
//! instruction once its target is terminal. Fast paths resolve terminal
//! targets inline without ever suspending. Joins are idempotent per waiter: a
//! re-executed join call reuses the waiter's pending join instead of
//! allocating a new one.
//!
//! Cancellation is a fault (`AIPO_RT_CANCELLED`), never a value: awaiting a
//! cancelled task, or driving one, faults. Failures inside a driven task fail
//! that task with a `Failure` value, which waiters observe through Model B
//! propagation.

use super::{HostNativeCallback, MutationEntry, UpvalueFrame, Vm};
use crate::fault::{VmError, VmFault};
use crate::frame::{CallFrame, HandlerFrame};
use crate::value::{
    DictMap, FailureValue, GroupId, SeqOp, SequencePipeline, SequenceSource, TaskId, Value,
    check_safe_int,
};
use aipo_bytecode::BytecodeModule;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

/// Identifier of the module entry script inside [`Vm::tasks`].
pub(super) const MAIN_TASK: TaskId = 0;

/// Identifier of a structured join.
pub(super) type JoinId = u64;

/// Lifecycle of a scheduler-managed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskStatus {
    /// Created, queued, never driven.
    Pending,
    /// Suspended until the virtual clock reaches `until` (inclusive wake).
    Sleeping { until: u64 },
    /// Suspended on a task or join completing.
    Blocked(WaitTarget),
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
pub(super) enum WaitTarget {
    /// Completion of one task.
    Task(TaskId),
    /// Completion of one structured join.
    Join(JoinId),
}

/// Terminal outcome of a task.
#[derive(Debug, Clone)]
pub(super) enum TaskOutcome {
    /// Completed with a plain value.
    Ready(Value),
    /// Completed with a `Failure` value.
    Failed(Value),
    /// Cancelled before or during execution.
    Cancelled,
}

impl TaskOutcome {
    /// Classifies a returned value: `Failure` values become task failures.
    pub(super) fn from_value(value: Value) -> Self {
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

/// Callback payload for a task owned by the host-native scheduler.
#[derive(Debug, Clone)]
pub(super) struct HostTask {
    /// Callback to execute when the task is loaded.
    pub(super) callback: HostNativeCallback,
    /// Owned arguments captured at call time.
    pub(super) args: Vec<Value>,
}

/// Saved machine state of a suspended task.
#[derive(Debug, Clone)]
pub(super) struct TaskState {
    /// Lifecycle state.
    pub(super) status: TaskStatus,
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    handlers: Vec<HandlerFrame>,
    upvalues: Vec<UpvalueFrame>,
    journal: Vec<MutationEntry>,
    iterations: Vec<usize>,
    iteration_revisions: Vec<(Value, Option<u64>, usize)>,
    ip: usize,
    /// Terminal payload for `Ready`/`Failed` tasks, observed by late awaiters.
    result: Option<Value>,
    /// Deadline for tasks that suspended on `task.sleep`.
    pub(super) sleeping_until: Option<u64>,
    host_task: Option<HostTask>,
}

impl TaskState {
    /// Empty placeholder for a freshly spawned or freshly loaded task.
    fn empty() -> Self {
        Self {
            status: TaskStatus::Pending,
            stack: Vec::new(),
            frames: Vec::new(),
            handlers: Vec::new(),
            upvalues: Vec::new(),
            journal: Vec::new(),
            iterations: Vec::new(),
            iteration_revisions: Vec::new(),
            ip: 0,
            result: None,
            sleeping_until: None,
            host_task: None,
        }
    }
}

/// Kind of a structured join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JoinKind {
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
#[derive(Debug, Clone)]
pub(super) struct JoinState {
    kind: JoinKind,
    /// Member tasks (grows for group waits as the group spawns).
    members: Vec<TaskId>,
    /// Original member order (for `all` results).
    order: Vec<TaskId>,
    /// Members in completion order.
    completed: Vec<TaskId>,
    /// Waiting tasks (usually one; groups admit several).
    waiters: Vec<TaskId>,
    /// Deadline in ticks (`Timeout` only).
    deadline: Option<u64>,
    /// Group being waited on (`GroupWait` only).
    group: Option<GroupId>,
    /// Set once resolved; further polls replay the stored outcome.
    done: bool,
    /// Resolved result value.
    outcome: Option<Value>,
    /// Resolution faults the waiter (cancelled member) instead of resolving.
    fault: Option<VmFault>,
}

/// Live structured-concurrency group.
#[derive(Debug, Clone)]
pub(super) struct GroupState {
    /// Every task spawned into the group, in spawn order.
    members: Vec<TaskId>,
}

/// Retained scheduler state for linked sessions and definition rollback.
#[derive(Debug, Clone)]
pub(super) struct SchedulerSnapshot {
    tasks: HashMap<TaskId, TaskState>,
    run_queue: VecDeque<TaskId>,
    waiters: HashMap<TaskId, Vec<TaskId>>,
    joins: HashMap<JoinId, JoinState>,
    groups: HashMap<GroupId, GroupState>,
    next_task: TaskId,
    next_join: JoinId,
    next_group: GroupId,
    tick: u64,
}
impl SchedulerSnapshot {
    pub(super) fn roots(&self) -> Vec<Value> {
        let mut values = Vec::new();
        for task in self.tasks.values() {
            values.extend(task.stack.iter().cloned());
            values.extend(task.result.iter().cloned());
            values.extend(
                task.iteration_revisions
                    .iter()
                    .map(|(value, _, _)| value.clone()),
            );
            for change in &task.journal {
                values.extend([
                    Value::Struct(change.instance.clone()),
                    change.previous.clone(),
                ]);
            }
            if let Some(host) = &task.host_task {
                values.extend(host.args.iter().cloned());
            }
        }
        values.extend(
            self.joins
                .values()
                .flat_map(|join| join.outcome.iter().cloned()),
        );
        values
    }
    pub(super) fn cells(&self) -> Vec<Rc<RefCell<Value>>> {
        self.tasks
            .values()
            .flat_map(|task| {
                task.upvalues
                    .iter()
                    .flatten()
                    .flat_map(|cells| cells.iter().cloned())
            })
            .collect()
    }
}

impl Vm {
    pub(super) fn snapshot_scheduler(&self) -> SchedulerSnapshot {
        SchedulerSnapshot {
            tasks: self.tasks.clone(),
            run_queue: self.run_queue.clone(),
            waiters: self.waiters.clone(),
            joins: self.joins.clone(),
            groups: self.groups.clone(),
            next_task: self.next_task,
            next_join: self.next_join,
            next_group: self.next_group,
            tick: self.tick,
        }
    }
    pub(super) fn restore_scheduler(&mut self, snapshot: SchedulerSnapshot) {
        self.tasks = snapshot.tasks;
        self.run_queue = snapshot.run_queue;
        self.waiters = snapshot.waiters;
        self.joins = snapshot.joins;
        self.groups = snapshot.groups;
        self.next_task = snapshot.next_task;
        self.next_join = snapshot.next_join;
        self.next_group = snapshot.next_group;
        self.tick = snapshot.tick;
        self.tasks.remove(&MAIN_TASK);
        self.run_queue.retain(|id| *id != MAIN_TASK);
    }
    /// Saves the loaded machine into its task slot and unwinds with
    /// [`VmError::Suspended`]. A cancellation that landed mid-run faults
    /// instead of saving, so the run loop completes the task as cancelled.
    pub(super) fn suspend_current(&mut self, status: TaskStatus) -> Result<(), VmError> {
        let id = self.current.unwrap_or(MAIN_TASK);
        if matches!(
            self.tasks.get(&id),
            Some(state) if state.status == TaskStatus::Cancelled
        ) {
            return Err(VmFault::Cancelled {
                details: format!("task {id} was cancelled"),
            }
            .into());
        }
        let sleeping_until = match status {
            TaskStatus::Sleeping { until } => Some(until),
            _ => None,
        };
        let state = TaskState {
            status,
            stack: std::mem::take(&mut self.stack),
            frames: std::mem::take(&mut self.frames),
            handlers: std::mem::take(&mut self.handlers),
            upvalues: std::mem::take(&mut self.upvalue_frames),
            journal: std::mem::take(&mut self.mutation_journal),
            iterations: std::mem::take(&mut self.active_iterations),
            iteration_revisions: std::mem::take(&mut self.iteration_revisions),
            ip: self.ip,
            result: None,
            sleeping_until,
            host_task: self.host_task.take(),
        };
        self.tasks.insert(id, state);
        self.frame_base = 0;
        if matches!(status, TaskStatus::Sleeping { .. }) && !self.run_queue.contains(&id) {
            self.run_queue.push_back(id);
        }
        self.current = None;
        Err(VmError::Suspended)
    }

    /// Suspends with the machine rewound onto the triggering call instruction
    /// (1 byte for `Call0..4`, 2 bytes for `Call`), so resuming re-executes the call.
    fn suspend_at_call(&mut self, status: TaskStatus) -> Result<(), VmError> {
        self.ip =
            self.ip
                .checked_sub(self.call_inst_len as usize)
                .ok_or(VmFault::CorruptedBytecode {
                    offset: self.ip,
                    reason: "call suspension outside a call instruction".to_string(),
                })?;
        self.suspend_current(status)
    }

    /// Loads a queued task onto the machine, marking it running.
    fn load_task(&mut self, id: TaskId) {
        let mut state = self.tasks.remove(&id).unwrap_or_else(TaskState::empty);
        std::mem::swap(&mut self.stack, &mut state.stack);
        std::mem::swap(&mut self.frames, &mut state.frames);
        std::mem::swap(&mut self.handlers, &mut state.handlers);
        std::mem::swap(&mut self.upvalue_frames, &mut state.upvalues);
        std::mem::swap(&mut self.mutation_journal, &mut state.journal);
        std::mem::swap(&mut self.active_iterations, &mut state.iterations);
        std::mem::swap(
            &mut self.iteration_revisions,
            &mut state.iteration_revisions,
        );
        self.refresh_frame_base();
        self.host_task = state.host_task.take();
        self.ip = state.ip;
        state.stack.clear();
        state.frames.clear();
        state.handlers.clear();
        state.upvalues.clear();
        state.journal.clear();
        state.iterations.clear();
        state.status = TaskStatus::Running;
        self.tasks.insert(id, state);
        self.current = Some(id);
    }

    /// Completes the loaded task: settles its state, wakes waiters, and polls
    /// affected joins. Returns `true` when the entry script completed.
    pub(super) fn complete_current(&mut self, outcome: TaskOutcome) -> bool {
        let id = self.current.take().unwrap_or(MAIN_TASK);
        self.host_task = None;
        let outcome = match self.tasks.get(&id) {
            Some(state) if state.status == TaskStatus::Cancelled => TaskOutcome::Cancelled,
            _ => outcome,
        };
        if id == MAIN_TASK {
            self.main_outcome = Some(outcome);
            self.tasks.remove(&MAIN_TASK);
            return true;
        }
        self.finish_task(id, outcome);
        false
    }

    /// Completes a top-level return or halt of the loaded execution.
    pub(super) fn finish_current(&mut self) -> bool {
        if let Some(failure) = self.halted_with.take() {
            return self.complete_current(TaskOutcome::Failed(failure));
        }
        let retval = self.stack.pop().unwrap_or(Value::None);
        self.complete_current(TaskOutcome::from_value(retval))
    }

    /// Settles a task, wakes its direct waiters, and polls affected joins.
    fn finish_task(&mut self, id: TaskId, outcome: TaskOutcome) {
        let waiter_value = outcome.clone().into_waiter_value();
        {
            let state = self.tasks.get_mut(&id).expect("finished task is tracked");
            match &outcome {
                TaskOutcome::Cancelled => {
                    state.status = TaskStatus::Cancelled;
                    state.host_task = None;
                }
                TaskOutcome::Ready(value) => {
                    state.status = TaskStatus::Ready;
                    state.result = Some(value.clone());
                }
                TaskOutcome::Failed(failure) => {
                    state.status = TaskStatus::Failed;
                    state.result = Some(failure.clone());
                }
            }
            state.host_task = None;
            let _ = waiter_value;
        }
        if let Some(waiters) = self.waiters.remove(&id) {
            for waiter in waiters {
                self.wake_waiter(waiter);
            }
        }
        let affected: Vec<JoinId> = self
            .joins
            .iter()
            .filter(|(_, join)| join.members.contains(&id))
            .map(|(join_id, _)| *join_id)
            .collect();
        for join_id in affected {
            if let Some(join) = self.joins.get_mut(&join_id)
                && !join.completed.contains(&id)
            {
                join.completed.push(id);
            }
            self.poll_join(join_id);
        }
    }

    /// Wakes a waiter whose target completed: it becomes runnable and resumes
    /// by re-executing its blocking instruction.
    fn wake_waiter(&mut self, waiter: TaskId) {
        if let Some(state) = self.tasks.get_mut(&waiter) {
            if matches!(state.status, TaskStatus::Blocked(_)) {
                state.status = TaskStatus::Pending;
            } else {
                return;
            }
        } else {
            return;
        }
        if !self.run_queue.contains(&waiter) {
            self.run_queue.push_back(waiter);
        }
    }

    /// Picks the next runnable task, advancing virtual time past sleepers when
    /// nothing is runnable. Returns after loading a task or when the bare main
    /// machine should keep stepping.
    pub(super) fn select_next(&mut self, _module: &BytecodeModule) -> Result<(), VmError> {
        // Fast path: the never-suspended main script with nothing queued.
        if self.run_queue.is_empty() && !self.tasks.contains_key(&MAIN_TASK) {
            return Ok(());
        }
        loop {
            self.poll_timeouts();
            let mut pick: Option<TaskId> = None;
            self.run_queue.retain(|&id| {
                matches!(
                    self.tasks.get(&id).map(|state| state.status),
                    Some(
                        TaskStatus::Pending
                            | TaskStatus::Blocked(_)
                            | TaskStatus::Running
                            | TaskStatus::Sleeping { .. }
                    )
                )
            });
            for &id in &self.run_queue {
                match self.tasks.get(&id).map(|state| state.status) {
                    Some(TaskStatus::Pending)
                    | Some(TaskStatus::Blocked(_))
                    | Some(TaskStatus::Running) => {
                        pick = Some(id);
                        break;
                    }
                    Some(TaskStatus::Sleeping { until }) if until <= self.tick => {
                        pick = Some(id);
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(id) = pick {
                if let Some(pos) = self.run_queue.iter().position(|&x| x == id) {
                    self.run_queue.remove(pos);
                }
                if matches!(
                    self.tasks.get(&id),
                    Some(state) if state.status == TaskStatus::Cancelled
                ) {
                    continue;
                }
                self.load_task(id);
                return Ok(());
            }
            let next_wake = self
                .tasks
                .values()
                .filter_map(|state| match state.status {
                    TaskStatus::Sleeping { until } if until > self.tick => Some(until),
                    _ => None,
                })
                .min();
            let next_timeout = self
                .joins
                .values()
                .filter_map(|join| match join.deadline {
                    Some(deadline) if !join.done && deadline > self.tick => Some(deadline),
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
                Some(tick) => self.tick = tick,
                None => {
                    let live = self.tasks.values().any(|state| {
                        matches!(
                            state.status,
                            TaskStatus::Pending
                                | TaskStatus::Sleeping { .. }
                                | TaskStatus::Blocked(_)
                                | TaskStatus::Running
                        )
                    });
                    if live {
                        return Err(VmFault::CorruptedBytecode {
                            offset: 0,
                            reason: "scheduler deadlock: live tasks with nothing runnable"
                                .to_string(),
                        }
                        .into());
                    }
                    return Ok(());
                }
            }
        }
    }

    /// Resolves deadline joins whose time has passed.
    fn poll_timeouts(&mut self) {
        let expired: Vec<JoinId> = self
            .joins
            .iter()
            .filter(|(_, join)| {
                !join.done
                    && join.kind == JoinKind::Timeout
                    && join.deadline.is_some_and(|deadline| self.tick > deadline)
            })
            .map(|(join_id, _)| *join_id)
            .collect();
        for join_id in expired {
            if let Some(join) = self.joins.get_mut(&join_id) {
                join.done = true;
                join.outcome = Some(Value::Failure(Rc::new(FailureValue::new(
                    "timeout".to_string(),
                ))));
            }
            let member = self
                .joins
                .get(&join_id)
                .and_then(|join| join.members.first().copied());
            if let Some(member) = member {
                // The late member is cancelled; its other awaiters observe the
                // cancellation fault while this join reports a timeout.
                self.finish_task(member, TaskOutcome::Cancelled);
            }
            self.wake_join(join_id);
        }
    }

    /// Advances a join after a member completes; resolves and wakes waiters
    /// once the join's condition holds. Idempotent: completed joins replay.
    fn poll_join(&mut self, join_id: JoinId) {
        let done = self.joins.get(&join_id).is_some_and(|join| join.done);
        if done {
            return;
        }
        enum Resolution {
            Pending,
            Value(Value),
            Fault(VmFault),
        }
        let resolution =
            match self.joins.get(&join_id).map(|join| join.kind) {
                None => return,
                Some(JoinKind::All) => {
                    let terminal = |id: &TaskId| {
                        matches!(
                            self.tasks.get(id).map(|state| state.status),
                            Some(TaskStatus::Ready | TaskStatus::Failed | TaskStatus::Cancelled)
                        )
                    };
                    let join = &self.joins[&join_id];
                    if join.members.iter().any(|member| {
                        self.task_result(member)
                            .is_some_and(|o| matches!(o, TaskOutcome::Cancelled))
                    }) {
                        Resolution::Fault(VmFault::Cancelled {
                            details: "a joined task was cancelled".to_string(),
                        })
                    } else if join.members.iter().all(terminal) {
                        // First failure by completion order fails the join;
                        // otherwise results follow the original member order.
                        let first_failure = self.completion_order(join_id).into_iter().find_map(
                            |member| match self.task_result(&member) {
                                Some(TaskOutcome::Failed(failure)) => Some(failure),
                                _ => None,
                            },
                        );
                        if let Some(failure) = first_failure {
                            Resolution::Value(failure)
                        } else {
                            let values: Vec<_> = join
                                .order
                                .iter()
                                .map(|member| {
                                    self.task_result(member)
                                        .and_then(|outcome| outcome.into_waiter_value())
                                        .unwrap_or(Value::None)
                                })
                                .collect();
                            Resolution::Value(Value::List(Rc::new(RefCell::new((values).into()))))
                        }
                    } else {
                        Resolution::Pending
                    }
                }
                Some(JoinKind::Race) => {
                    let winner = self
                        .completion_order(join_id)
                        .into_iter()
                        .find_map(|member| self.task_result(&member));
                    match winner {
                        None => Resolution::Pending,
                        Some(TaskOutcome::Cancelled) => Resolution::Fault(VmFault::Cancelled {
                            details: "the winning task was cancelled".to_string(),
                        }),
                        Some(TaskOutcome::Ready(value) | TaskOutcome::Failed(value)) => {
                            Resolution::Value(value)
                        }
                    }
                }
                Some(JoinKind::Timeout) => {
                    let member = self.joins[&join_id].members.first().copied();
                    match member.and_then(|id| self.task_result(&id)) {
                        None => Resolution::Pending,
                        Some(TaskOutcome::Cancelled) => Resolution::Fault(VmFault::Cancelled {
                            details: "the timed task was cancelled".to_string(),
                        }),
                        Some(TaskOutcome::Ready(value) | TaskOutcome::Failed(value)) => {
                            Resolution::Value(value)
                        }
                    }
                }
                Some(JoinKind::GroupWait) => {
                    // Late spawns extend `members`, so completion requires every
                    // currently known member to be terminal.
                    let terminal = |id: &TaskId| {
                        matches!(
                            self.tasks.get(id).map(|state| state.status),
                            Some(TaskStatus::Ready | TaskStatus::Failed | TaskStatus::Cancelled)
                        )
                    };
                    let all_terminal = self.joins[&join_id].members.iter().all(terminal);
                    let any_cancelled = self.joins[&join_id].members.iter().any(|member| {
                        matches!(self.task_result(member), Some(TaskOutcome::Cancelled))
                    });
                    if any_cancelled {
                        Resolution::Fault(VmFault::Cancelled {
                            details: "a group member was cancelled".to_string(),
                        })
                    } else if all_terminal {
                        let values: Vec<_> = self
                            .completion_order(join_id)
                            .into_iter()
                            .map(|member| {
                                self.task_result(&member)
                                    .and_then(|outcome| outcome.into_waiter_value())
                                    .unwrap_or(Value::None)
                            })
                            .collect();
                        Resolution::Value(Value::List(Rc::new(RefCell::new((values).into()))))
                    } else {
                        Resolution::Pending
                    }
                }
            };
        match resolution {
            Resolution::Pending => {}
            Resolution::Value(value) => {
                if let Some(join) = self.joins.get_mut(&join_id) {
                    join.done = true;
                    join.outcome = Some(value);
                }
                self.wake_join(join_id);
            }
            Resolution::Fault(fault) => {
                if let Some(join) = self.joins.get_mut(&join_id) {
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
    fn completion_order(&self, join_id: JoinId) -> Vec<TaskId> {
        let join = &self.joins[&join_id];
        let mut ordered: Vec<TaskId> = join
            .completed
            .iter()
            .filter(|member| join.members.contains(member))
            .copied()
            .collect();
        let mut missing: Vec<TaskId> = join
            .members
            .iter()
            .filter(|member| !ordered.contains(member))
            .copied()
            .collect();
        missing.append(&mut ordered);
        missing
    }

    /// Terminal outcome of a tracked task, if it already completed.
    fn task_result(&self, id: &TaskId) -> Option<TaskOutcome> {
        let state = self.tasks.get(id)?;
        match state.status {
            TaskStatus::Ready => state.result.clone().map(TaskOutcome::Ready),
            TaskStatus::Failed => state.result.clone().map(TaskOutcome::Failed),
            TaskStatus::Cancelled => Some(TaskOutcome::Cancelled),
            _ => None,
        }
    }

    /// `await target`: fast paths resolve terminal tasks inline; pending tasks
    /// suspend the waiter (rewound onto `await`) and drive the target first.
    pub(super) fn op_await(&mut self, await_ip: usize) -> Result<(), VmError> {
        let target = self.stack.last().cloned().ok_or(VmFault::StackUnderflow)?;
        if target.is_failure() {
            return Ok(());
        }
        let id = match target {
            Value::Task(id) => id,
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "Task to await".to_string(),
                    actual: other.type_name().to_string(),
                }
                .into());
            }
        };
        if self.invoke_depth > 0 {
            return Err(VmFault::AwaitInCallback {
                operation: "await".to_string(),
            }
            .into());
        }
        match self.task_result(&id) {
            Some(TaskOutcome::Ready(value) | TaskOutcome::Failed(value)) => {
                self.pop().map_err(VmError::Fault)?;
                self.push(value).map_err(VmError::Fault)?;
                Ok(())
            }
            Some(TaskOutcome::Cancelled) => Err(VmFault::Cancelled {
                details: format!("task {id} was cancelled"),
            }
            .into()),
            None => {
                if !self.tasks.contains_key(&id) {
                    return Err(VmFault::TypeMismatch {
                        expected: "live task to await".to_string(),
                        actual: format!("unknown task {id}"),
                    }
                    .into());
                }
                let me = self.current.unwrap_or(MAIN_TASK);
                if me == id || self.blocks_on(id, me) {
                    return Err(VmFault::AwaitCycle {
                        chain: self.await_chain(me, id),
                    }
                    .into());
                }
                let waiters = self.waiters.entry(id).or_default();
                if !waiters.contains(&me) {
                    waiters.push(me);
                }
                // Handover: drive the awaited task before the rest of the queue.
                if !self.run_queue.contains(&id) {
                    self.run_queue.push_front(id);
                }
                self.ip = await_ip;
                self.suspend_current(TaskStatus::Blocked(WaitTarget::Task(id)))
            }
        }
    }

    /// Whether `from` transitively awaits `target` through task waits or structured joins.
    fn blocks_on(&self, from: TaskId, target: TaskId) -> bool {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(from);
        visited.insert(from);

        while let Some(cursor) = queue.pop_front() {
            match self.tasks.get(&cursor).map(|state| state.status) {
                Some(TaskStatus::Blocked(WaitTarget::Task(next))) => {
                    if next == target {
                        return true;
                    }
                    if visited.insert(next) {
                        queue.push_back(next);
                    }
                }
                Some(TaskStatus::Blocked(WaitTarget::Join(join_id))) => {
                    if let Some(join) = self.joins.get(&join_id) {
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
    fn await_chain(&self, me: TaskId, id: TaskId) -> Vec<u64> {
        let mut chain = vec![me];
        let mut path = Vec::new();
        let mut visited = HashSet::new();
        if self.find_await_path(id, me, &mut visited, &mut path) {
            chain.extend(path);
        } else {
            chain.push(id);
        }
        chain
    }

    fn find_await_path(
        &self,
        current: TaskId,
        target: TaskId,
        visited: &mut HashSet<TaskId>,
        path: &mut Vec<u64>,
    ) -> bool {
        path.push(current);
        if current == target {
            return true;
        }
        if !visited.insert(current) {
            path.pop();
            return false;
        }

        match self.tasks.get(&current).map(|state| state.status) {
            Some(TaskStatus::Blocked(WaitTarget::Task(next))) => {
                if self.find_await_path(next, target, visited, path) {
                    return true;
                }
            }
            Some(TaskStatus::Blocked(WaitTarget::Join(join_id))) => {
                if let Some(join) = self.joins.get(&join_id) {
                    for &member in &join.members {
                        if self.find_await_path(member, target, visited, path) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }

        path.pop();
        false
    }

    /// Shared frame setup for plain function/closure calls, on the live
    /// machine or on a freshly spawned task's vectors.
    pub(super) fn push_plain_frame(
        frames: &mut Vec<CallFrame>,
        upvalues: &mut Vec<UpvalueFrame>,
        journal_start: usize,
        return_ip: usize,
        callee_idx: usize,
        arg_count: usize,
        cells: UpvalueFrame,
    ) {
        frames.push(CallFrame::new(
            return_ip,
            callee_idx + 1,
            arg_count,
            journal_start,
        ));
        upvalues.push(cells);
    }

    /// Spawns a task running `callee(args)`, optionally registered in a group.
    /// Async-ness is irrelevant: any function or closure can run as a task.
    pub(super) fn spawn_task(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        group: Option<GroupId>,
    ) -> Result<TaskId, VmFault> {
        let (entry_ip, arity, cells) = match callee.clone() {
            Value::Function {
                entry_ip, arity, ..
            } => (entry_ip as usize, arity as usize, None),
            Value::Closure(closure) => (
                closure.entry_ip,
                closure.arity,
                Some(Rc::new(closure.upvalues.clone())),
            ),
            other => {
                return Err(VmFault::NotCallable {
                    type_name: other.type_name().to_string(),
                });
            }
        };
        self.check_arity(args.len(), arity)?;
        let id = self.next_task;
        self.next_task += 1;
        let mut stack = Vec::with_capacity(args.len() + 1);
        stack.push(callee);
        stack.extend(args);
        let arg_count = stack.len() - 1;
        let mut frames = Vec::new();
        let mut upvalues = Vec::new();
        Self::push_plain_frame(&mut frames, &mut upvalues, 0, 0, 0, arg_count, cells);
        self.tasks.insert(
            id,
            TaskState {
                status: TaskStatus::Pending,
                stack,
                frames,
                handlers: Vec::new(),
                upvalues,
                journal: Vec::new(),
                iterations: Vec::new(),
                iteration_revisions: Vec::new(),
                ip: entry_ip,
                result: None,
                sleeping_until: None,
                host_task: None,
            },
        );
        self.run_queue.push_back(id);
        if let Some(group_id) = group {
            if let Some(state) = self.groups.get_mut(&group_id) {
                state.members.push(id);
            }
            // Late spawns join open group waits.
            let open: Vec<JoinId> = self
                .joins
                .iter()
                .filter(|(_, join)| {
                    !join.done && join.kind == JoinKind::GroupWait && join.group == Some(group_id)
                })
                .map(|(join_id, _)| *join_id)
                .collect();
            for join_id in open {
                if let Some(join) = self.joins.get_mut(&join_id)
                    && !join.members.contains(&id)
                {
                    join.members.push(id);
                    join.order.push(id);
                }
            }
        }
        Ok(id)
    }

    pub(super) fn spawn_host_task_state(
        &mut self,
        callback: HostNativeCallback,
        args: Vec<Value>,
    ) -> Result<TaskId, VmError> {
        let id = self.next_task;
        self.next_task += 1;
        self.tasks.insert(
            id,
            TaskState {
                status: TaskStatus::Pending,
                stack: Vec::new(),
                frames: Vec::new(),
                handlers: Vec::new(),
                upvalues: Vec::new(),
                journal: Vec::new(),
                iterations: Vec::new(),
                iteration_revisions: Vec::new(),
                ip: 0,
                result: None,
                sleeping_until: None,
                host_task: Some(HostTask { callback, args }),
            },
        );
        self.run_queue.push_back(id);
        Ok(id)
    }

    pub(super) fn drive_host_task(&mut self) -> Result<bool, VmError> {
        let Some(host_task) = self.host_task.take() else {
            return Ok(false);
        };
        let result = (host_task.callback)(self, &host_task.args);
        match result {
            Ok(value) => Ok(self.complete_current(TaskOutcome::from_value(value))),
            Err(VmError::Suspended) => Err(VmFault::AwaitInCallback {
                operation: "host async native".to_string(),
            }
            .into()),
            Err(error) => {
                let cancelled = self.current.is_some()
                    && self.current != Some(MAIN_TASK)
                    && matches!(
                        self.tasks
                            .get(&self.current.unwrap_or(MAIN_TASK))
                            .map(|state| state.status),
                        Some(TaskStatus::Cancelled)
                    );
                if cancelled {
                    Ok(self.complete_current(TaskOutcome::Cancelled))
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Resolves a `task.*` call inline: truncates the callee/args and pushes
    /// the result. Blocking resolutions suspend instead, leaving the stack
    /// intact for resume re-execution.
    fn resolve_call(&mut self, callee_idx: usize, value: Value) -> Result<(), VmError> {
        self.stack.truncate(callee_idx);
        self.push(value)?;
        Ok(())
    }

    /// Dispatches the `task` module functions intercepted in `begin_call`.
    /// Arity is already checked by the call protocol.
    pub(super) fn task_call(
        &mut self,
        name: &str,
        callee_idx: usize,
        args: &[Value],
    ) -> Result<(), VmError> {
        // Model B at the boundary: failures pass through untouched.
        for arg in args {
            if arg.is_failure() {
                let failure = arg.clone();
                return self.resolve_call(callee_idx, failure);
            }
        }
        match name {
            "task.spawn" => {
                let items = task_list_arg(&args[1], "task.spawn(f, args)")?;
                let id = self.spawn_task(args[0].clone(), items, None)?;
                self.resolve_call(callee_idx, Value::Task(id))
            }
            "task.sleep" => self.do_sleep(callee_idx, &args[0]),
            "task.all" => match task_members(&args[0], "task.all")? {
                Members::Tasks(members) => {
                    self.do_join(JoinKind::All, members, None, callee_idx, "task.all")
                }
                Members::Failure(failure) => self.resolve_call(callee_idx, failure),
            },
            "task.race" => match task_members(&args[0], "task.race")? {
                Members::Tasks(members) => {
                    self.do_join(JoinKind::Race, members, None, callee_idx, "task.race")
                }
                Members::Failure(failure) => self.resolve_call(callee_idx, failure),
            },
            "task.timeout" => {
                let member = match &args[0] {
                    Value::Task(id) => *id,
                    other => {
                        return Err(VmFault::TypeMismatch {
                            expected: "Task for task.timeout".to_string(),
                            actual: other.type_name().to_string(),
                        }
                        .into());
                    }
                };
                let ticks = sleep_ticks(&args[1], "task.timeout")?;
                match ticks {
                    Ticks::Failure(failure) => self.resolve_call(callee_idx, failure),
                    Ticks::Ticks(ticks) => {
                        let deadline = self.tick.saturating_add(ticks);
                        self.do_join(
                            JoinKind::Timeout,
                            vec![member],
                            Some(deadline),
                            callee_idx,
                            "task.timeout",
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
                        }
                        .into());
                    }
                };
                match self.tasks.get(&id).map(|state| state.status) {
                    None => {
                        return Err(VmFault::TypeMismatch {
                            expected: "live task for task.cancel".to_string(),
                            actual: format!("unknown task {id}"),
                        }
                        .into());
                    }
                    Some(TaskStatus::Ready | TaskStatus::Failed | TaskStatus::Cancelled) => {}
                    Some(_) => self.finish_task(id, TaskOutcome::Cancelled),
                }
                self.resolve_call(callee_idx, Value::None)
            }
            "task.group" => {
                let id = self.next_group;
                self.next_group += 1;
                self.groups.insert(
                    id,
                    GroupState {
                        members: Vec::new(),
                    },
                );
                self.resolve_call(callee_idx, Value::Group(id))
            }
            other => Err(VmFault::CorruptedBytecode {
                offset: self.ip,
                reason: format!("unknown task function {other}"),
            }
            .into()),
        }
    }

    /// `task.sleep(ticks | duration)`: yields until the virtual clock passes
    /// the deadline. Resuming re-executes the call and takes the fast path.
    fn do_sleep(&mut self, callee_idx: usize, arg: &Value) -> Result<(), VmError> {
        if self.invoke_depth > 0 {
            return Err(VmFault::AwaitInCallback {
                operation: "sleep".to_string(),
            }
            .into());
        }
        let me = self.current.unwrap_or(MAIN_TASK);
        if let Some(until) = self
            .tasks
            .get_mut(&me)
            .and_then(|s| s.sleeping_until.take())
        {
            if until <= self.tick {
                return self.resolve_call(callee_idx, Value::None);
            }
            if let Some(s) = self.tasks.get_mut(&me) {
                s.sleeping_until = Some(until);
            }
            return self.suspend_at_call(TaskStatus::Sleeping { until });
        }
        match sleep_ticks(arg, "task.sleep")? {
            Ticks::Failure(failure) => self.resolve_call(callee_idx, failure),
            Ticks::Ticks(ticks) => {
                let until = self.tick.saturating_add(ticks);
                self.suspend_at_call(TaskStatus::Sleeping { until })
            }
        }
    }

    /// Shared `all`/`race`/`timeout` implementation: terminal members resolve
    /// inline, otherwise the waiter's pending join is reused or created.
    fn do_join(
        &mut self,
        kind: JoinKind,
        members: Vec<TaskId>,
        deadline: Option<u64>,
        callee_idx: usize,
        operation: &str,
    ) -> Result<(), VmError> {
        if self.invoke_depth > 0 {
            return Err(VmFault::AwaitInCallback {
                operation: operation.to_string(),
            }
            .into());
        }
        if members.is_empty() {
            let empty = match kind {
                JoinKind::Race => {
                    Value::Failure(Rc::new(FailureValue::new("race of no tasks".to_string())))
                }
                JoinKind::All | JoinKind::GroupWait => {
                    Value::List(Rc::new(RefCell::new((Vec::new()).into())))
                }
                JoinKind::Timeout => {
                    Value::Failure(Rc::new(FailureValue::new("timeout of no task".to_string())))
                }
            };
            return self.resolve_call(callee_idx, empty);
        }
        let me = self.current.unwrap_or(MAIN_TASK);
        if matches!(kind, JoinKind::All | JoinKind::GroupWait) {
            for &member in &members {
                if member == me || self.blocks_on(member, me) {
                    return Err(VmFault::AwaitCycle {
                        chain: self.await_chain(me, member),
                    }
                    .into());
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
                let id = self.next_join;
                self.next_join += 1;
                self.joins.insert(
                    id,
                    JoinState {
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
        let join = &self.joins[&join_id];
        if join.done {
            let (fault, outcome) = (join.fault.clone(), join.outcome.clone());
            self.joins.remove(&join_id);
            if let Some(fault) = fault {
                return Err(fault.into());
            }
            let outcome = outcome.unwrap_or(Value::None);
            return self.resolve_call(callee_idx, outcome);
        }
        self.suspend_at_call(TaskStatus::Blocked(WaitTarget::Join(join_id)))
    }

    /// The waiter's pending join of this kind, if any.
    fn pending_join(&self, kind: JoinKind) -> Option<JoinId> {
        let me = self.current.unwrap_or(MAIN_TASK);
        for (&id, join) in &self.joins {
            if join.kind == kind && join.waiters.contains(&me) {
                return Some(id);
            }
        }
        None
    }

    /// Reuses the waiter's pending join when it matches this call.
    fn join_for(&self, kind: JoinKind, members: &[TaskId]) -> Option<JoinId> {
        let me = self.current.unwrap_or(MAIN_TASK);
        for (&id, join) in &self.joins {
            if join.kind == kind && join.members == members && join.waiters.contains(&me) {
                return Some(id);
            }
        }
        None
    }

    /// Dispatches `Group` methods (`spawn`, `wait`) intercepted in `begin_call`.
    pub(super) fn group_method(
        &mut self,
        name: &str,
        receiver: &Value,
        args: &[Value],
        callee_idx: usize,
    ) -> Result<(), VmError> {
        let group_id = match receiver {
            Value::Group(id) => *id,
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "Group receiver".to_string(),
                    actual: other.type_name().to_string(),
                }
                .into());
            }
        };
        if !self.groups.contains_key(&group_id) {
            return Err(VmFault::TypeMismatch {
                expected: "live group".to_string(),
                actual: format!("unknown group {group_id}"),
            }
            .into());
        }
        for arg in args {
            if arg.is_failure() {
                let failure = arg.clone();
                return self.resolve_call(callee_idx, failure);
            }
        }
        match name {
            "spawn" => {
                let items = task_list_arg(&args[1], "group.spawn(f, args)")?;
                let id = self.spawn_task(args[0].clone(), items, Some(group_id))?;
                self.resolve_call(callee_idx, Value::Task(id))
            }
            "wait" => self.do_group_wait(group_id, callee_idx),
            other => Err(VmFault::TypeMismatch {
                expected: "spawn or wait".to_string(),
                actual: format!("unknown group method {other}"),
            }
            .into()),
        }
    }

    /// `group.wait()`: collects every member outcome in completion order.
    /// Cancelled members fault the waiter, since cancellation has no value form.
    fn do_group_wait(&mut self, group_id: GroupId, callee_idx: usize) -> Result<(), VmError> {
        if self.invoke_depth > 0 {
            return Err(VmFault::AwaitInCallback {
                operation: "group.wait".to_string(),
            }
            .into());
        }
        let me = self.current.unwrap_or(MAIN_TASK);
        // Reuse the waiter's pending group join for this group.
        let pending = if let Some(TaskStatus::Blocked(WaitTarget::Join(id))) =
            self.tasks.get(&me).map(|state| state.status)
        {
            self.joins
                .get(&id)
                .filter(|join| join.kind == JoinKind::GroupWait && join.group == Some(group_id))
                .map(|_| id)
        } else {
            None
        };
        let join_id = match pending {
            Some(id) => id,
            None => {
                let members = self.groups[&group_id].members.clone();
                if members.iter().all(|member| {
                    matches!(
                        self.task_result(member),
                        Some(TaskOutcome::Ready(_) | TaskOutcome::Failed(_))
                    )
                }) {
                    if members.iter().any(|member| {
                        matches!(self.task_result(member), Some(TaskOutcome::Cancelled))
                    }) {
                        return Err(VmFault::Cancelled {
                            details: "a group member was cancelled".to_string(),
                        }
                        .into());
                    }
                    let values: Vec<_> = members
                        .iter()
                        .map(|member| {
                            self.task_result(member)
                                .and_then(|outcome| outcome.into_waiter_value())
                                .unwrap_or(Value::None)
                        })
                        .collect();
                    return self.resolve_call(
                        callee_idx,
                        Value::List(Rc::new(RefCell::new((values).into()))),
                    );
                }
                let id = self.next_join;
                self.next_join += 1;
                self.joins.insert(
                    id,
                    JoinState {
                        kind: JoinKind::GroupWait,
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
        let join = &self.joins[&join_id];
        if join.done {
            if let Some(fault) = join.fault.clone() {
                return Err(fault.into());
            }
            let outcome = join.outcome.clone().unwrap_or(Value::None);
            return self.resolve_call(callee_idx, outcome);
        }
        self.suspend_at_call(TaskStatus::Blocked(WaitTarget::Join(join_id)))
    }
}

/// Validated sleep amount: ticks, or a `Failure` value resolving inline.
enum Ticks {
    Ticks(u64),
    Failure(Value),
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

/// Validated join members: tasks, or a `Failure` list element resolving inline.
enum Members {
    Tasks(Vec<TaskId>),
    Failure(Value),
}

/// Reads and validates the `[tasks]` list of an `all`/`race` call.
///
/// A `Failure` element resolves the join with that failure (Model B at the
/// boundary); non-task elements are a type mismatch.
fn task_members(arg: &Value, operation: &str) -> Result<Members, VmError> {
    let items = match arg {
        Value::List(items) => items.borrow().to_vec(),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: format!("task list for {operation}"),
                actual: other.type_name().to_string(),
            }
            .into());
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
                }
                .into());
            }
        }
    }
    Ok(Members::Tasks(members))
}

/// Sequence methods bound by [`Vm::bind_method`]: name and caller-visible arity.
pub(super) const SEQUENCE_METHODS: [(&str, usize); 18] = [
    ("map", 1),
    ("filter", 1),
    ("flat_map", 1),
    ("take", 1),
    ("skip", 1),
    ("distinct", 0),
    ("zip", 1),
    ("chain", 1),
    ("chunk", 1),
    ("window", 1),
    ("enumerate", 0),
    ("find", 1),
    ("any", 1),
    ("all", 1),
    ("count", 0),
    ("reduce", 2),
    ("group_by", 1),
    ("collect", 0),
];

impl Vm {
    /// Dispatches every `Sequence` method. Pure stages (`map`, `take`, …)
    /// append to the pipeline and return a new `Sequence`; driving methods
    /// (`find`, `collect`, …) evaluate and return plain values.
    pub(super) fn sequence_method(
        &mut self,
        module: &BytecodeModule,
        name: &str,
        receiver: &Value,
        args: &[Value],
        callee_idx: usize,
    ) -> Result<(), VmError> {
        let pipeline = match receiver {
            Value::Sequence(pipeline) => Rc::clone(pipeline),
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "Sequence receiver".to_string(),
                    actual: other.type_name().to_string(),
                }
                .into());
            }
        };
        for arg in args {
            if arg.is_failure() {
                let failure = arg.clone();
                return self.resolve_call(callee_idx, failure);
            }
        }
        // Pure stages append and return a new pipeline.
        let op = match name {
            "map" => SeqOp::Map(Self::seq_callable(&args[0], "map")?),
            "filter" => SeqOp::Filter(Self::seq_callable(&args[0], "filter")?),
            "flat_map" => SeqOp::FlatMap(Self::seq_callable(&args[0], "flat_map")?),
            "take" => match Self::seq_count(&args[0], "take")? {
                SeqCount::Count(n) => SeqOp::Take(n),
                SeqCount::Failure(failure) => {
                    return self.resolve_call(callee_idx, failure);
                }
            },
            "skip" => match Self::seq_count(&args[0], "skip")? {
                SeqCount::Count(n) => SeqOp::Skip(n),
                SeqCount::Failure(failure) => {
                    return self.resolve_call(callee_idx, failure);
                }
            },
            "distinct" => SeqOp::Distinct,
            "zip" => SeqOp::Zip(self.snapshot_iterable(module, &args[0], "zip")?),
            "chain" => SeqOp::Chain(self.snapshot_iterable(module, &args[0], "chain")?),
            "chunk" => match Self::seq_window(&args[0], "chunk")? {
                SeqCount::Count(n) => SeqOp::Chunk(n),
                SeqCount::Failure(failure) => {
                    return self.resolve_call(callee_idx, failure);
                }
            },
            "window" => match Self::seq_window(&args[0], "window")? {
                SeqCount::Count(n) => SeqOp::Window(n),
                SeqCount::Failure(failure) => {
                    return self.resolve_call(callee_idx, failure);
                }
            },
            "enumerate" => SeqOp::Enumerate,
            _ => return self.sequence_terminal(module, &pipeline, name, args, callee_idx),
        };
        let mut ops = pipeline.ops.clone();
        ops.push(op);
        let next = Rc::new(SequencePipeline::new(pipeline.source.clone(), ops));
        self.resolve_call(callee_idx, Value::Sequence(next))
    }

    /// Driving sequence methods: evaluate the (memoized) pure pipeline and
    /// apply the terminal operation.
    fn sequence_terminal(
        &mut self,
        module: &BytecodeModule,
        pipeline: &Rc<SequencePipeline>,
        name: &str,
        args: &[Value],
        callee_idx: usize,
    ) -> Result<(), VmError> {
        enum Terminal {
            Find(Value),
            Any(Value),
            All(Value),
            Count(Option<Value>),
            Reduce { initial: Value, func: Value },
            GroupBy(Value),
            Collect,
        }
        let terminal = match name {
            "find" => Terminal::Find(Self::seq_callable(&args[0], "find")?),
            "any" => Terminal::Any(Self::seq_callable(&args[0], "any")?),
            "all" => Terminal::All(Self::seq_callable(&args[0], "all")?),
            "count" => Terminal::Count(None),
            "reduce" => Terminal::Reduce {
                initial: args[0].clone(),
                func: Self::seq_callable(&args[1], "reduce")?,
            },
            "group_by" => Terminal::GroupBy(Self::seq_callable(&args[0], "group_by")?),
            "collect" => Terminal::Collect,
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "sequence method".to_string(),
                    actual: format!("unknown sequence method {other}"),
                }
                .into());
            }
        };
        if let Terminal::Reduce { initial, .. } = &terminal
            && initial.is_failure()
        {
            let failure = initial.clone();
            return self.resolve_call(callee_idx, failure);
        }
        let items = self.sequence_items(module, pipeline)?;
        let value = match terminal {
            Terminal::Collect => Value::List(Rc::new(RefCell::new((items).into()))),
            Terminal::Find(predicate) => {
                let mut found = Value::None;
                for item in items {
                    let verdict =
                        self.invoke(module, predicate.clone(), std::slice::from_ref(&item))?;
                    if Self::seq_truthy(&verdict)? {
                        found = item;
                        break;
                    }
                }
                found
            }
            Terminal::Any(predicate) => {
                let mut any = false;
                for item in items {
                    let verdict = self.invoke(module, predicate.clone(), &[item])?;
                    if Self::seq_truthy(&verdict)? {
                        any = true;
                        break;
                    }
                }
                Value::Bool(any)
            }
            Terminal::All(predicate) => {
                let mut all = true;
                for item in items {
                    let verdict = self.invoke(module, predicate.clone(), &[item])?;
                    if !Self::seq_truthy(&verdict)? {
                        all = false;
                        break;
                    }
                }
                Value::Bool(all)
            }
            Terminal::Count(None) =>
            {
                #[allow(clippy::cast_possible_wrap)]
                Value::Int(check_safe_int(items.len() as i64)?)
            }
            Terminal::Count(Some(predicate)) => {
                let mut count = 0i64;
                for item in items {
                    let verdict = self.invoke(module, predicate.clone(), &[item])?;
                    if Self::seq_truthy(&verdict)? {
                        count += 1;
                    }
                }
                Value::Int(check_safe_int(count)?)
            }
            Terminal::Reduce { initial, func } => {
                let mut acc = initial;
                for item in items {
                    acc = self.invoke(module, func.clone(), &[acc, item])?;
                    if acc.is_failure() {
                        break;
                    }
                }
                acc
            }
            Terminal::GroupBy(key_fn) => {
                let mut buckets: Vec<(Value, Vec<Value>)> = Vec::new();
                for item in items {
                    let key = self.invoke(module, key_fn.clone(), std::slice::from_ref(&item))?;
                    if key.is_failure() {
                        return self.resolve_call(callee_idx, key);
                    }
                    match buckets.iter_mut().find(|(existing, _)| existing == &key) {
                        Some((_, bucket)) => bucket.push(item),
                        None => buckets.push((key, vec![item])),
                    }
                }
                let entries = buckets
                    .into_iter()
                    .map(|(key, bucket)| (key, Value::List(Rc::new(RefCell::new((bucket).into())))))
                    .collect();
                Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
            }
        };
        self.resolve_call(callee_idx, value)
    }

    /// Callable validation for sequence stages: anything `invoke` can drive.
    fn seq_callable(arg: &Value, operation: &str) -> Result<Value, VmFault> {
        match arg {
            Value::Function { .. }
            | Value::Closure(_)
            | Value::Native(_)
            | Value::BoundMethod(_)
            | Value::StructMethod(_)
            | Value::Type(_) => Ok(arg.clone()),
            other => Err(VmFault::NotCallable {
                type_name: format!("{} as {operation} callable", other.type_name()),
            }),
        }
    }

    /// Non-negative stage counts (`take`, `skip`); negatives resolve to a
    /// recoverable `Failure` at the call.
    fn seq_count(arg: &Value, operation: &str) -> Result<SeqCount, VmError> {
        match arg {
            Value::Int(n) if *n >= 0 => Ok(SeqCount::Count(*n)),
            Value::Int(_) => Ok(SeqCount::Failure(Value::Failure(Rc::new(
                FailureValue::new(format!("{operation} count must be >= 0")),
            )))),
            Value::Byte(b) => Ok(SeqCount::Count(i64::from(*b))),
            other => Err(VmFault::TypeMismatch {
                expected: format!("Int count for {operation}"),
                actual: other.type_name().to_string(),
            }
            .into()),
        }
    }

    /// Positive window sizes (`chunk`, `window`); non-positive resolve to a
    /// recoverable `Failure` at the call.
    fn seq_window(arg: &Value, operation: &str) -> Result<SeqCount, VmError> {
        match arg {
            Value::Int(n) if *n > 0 => Ok(SeqCount::Count(*n)),
            Value::Int(_) => Ok(SeqCount::Failure(Value::Failure(Rc::new(
                FailureValue::new(format!("{operation} size must be > 0")),
            )))),
            Value::Byte(b) if *b > 0 => Ok(SeqCount::Count(i64::from(*b))),
            Value::Byte(_) => Ok(SeqCount::Failure(Value::Failure(Rc::new(
                FailureValue::new(format!("{operation} size must be > 0")),
            )))),
            other => Err(VmFault::TypeMismatch {
                expected: format!("Int size for {operation}"),
                actual: other.type_name().to_string(),
            }
            .into()),
        }
    }

    /// Pure items of a pipeline's stages: snapshot, strict per-stage
    /// evaluation, first-consumption memo. Stages are strict in v1 (no
    /// cross-stage short-circuit); the memo keeps `each` (Len + GetIndex per
    /// iteration) and repeated `collect` calls to a single evaluation.
    pub(super) fn sequence_items(
        &mut self,
        module: &BytecodeModule,
        pipeline: &SequencePipeline,
    ) -> Result<Vec<Value>, VmError> {
        if let Some(hit) = pipeline.cached.borrow().clone() {
            return Ok(hit);
        }
        let mut items = snapshot_source(&pipeline.source);
        for op in &pipeline.ops {
            match op {
                SeqOp::Map(_)
                | SeqOp::Filter(_)
                | SeqOp::FlatMap(_)
                | SeqOp::Take(_)
                | SeqOp::Skip(_)
                | SeqOp::Distinct
                | SeqOp::Zip(_)
                | SeqOp::Chain(_)
                | SeqOp::Chunk(_)
                | SeqOp::Window(_)
                | SeqOp::Enumerate => {
                    items = self.apply_seq_stage(module, op, items)?;
                }
                // Terminal stages never sit inside a stored pipeline (driving
                // methods append them to a throwaway copy); reaching one here
                // stops the pure prefix.
                _ => break,
            }
        }
        pipeline.cached.borrow_mut().replace(items.clone());
        Ok(items)
    }

    /// Applies one pure pipeline stage (strict: the whole stage materializes).
    fn apply_seq_stage(
        &mut self,
        module: &BytecodeModule,
        op: &SeqOp,
        items: Vec<Value>,
    ) -> Result<Vec<Value>, VmError> {
        match op {
            SeqOp::Map(func) => {
                // Mirrors `transform`: results (including `Failure` values)
                // become elements; only faults abort the stage.
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.invoke(module, func.clone(), &[item])?);
                }
                Ok(out)
            }
            SeqOp::Filter(predicate) => {
                // Mirrors `filter`: the predicate must return `Bool`;
                // anything else (including a `Failure`) is a type mismatch.
                let mut kept = Vec::new();
                for item in items {
                    let verdict =
                        self.invoke(module, predicate.clone(), std::slice::from_ref(&item))?;
                    if Self::seq_truthy(&verdict)? {
                        kept.push(item);
                    }
                }
                Ok(kept)
            }
            SeqOp::FlatMap(func) => {
                let mut out = Vec::new();
                for item in items {
                    let mapped = self.invoke(module, func.clone(), &[item])?;
                    match mapped {
                        Value::List(list) => out.extend(list.borrow().to_vec()),
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "List flat_map result".to_string(),
                                actual: other.type_name().to_string(),
                            }
                            .into());
                        }
                    }
                }
                Ok(out)
            }
            SeqOp::Take(n) => {
                let count = (*n).max(0) as usize;
                Ok(items.into_iter().take(count).collect())
            }
            SeqOp::Skip(n) => {
                let count = (*n).max(0) as usize;
                Ok(items.into_iter().skip(count).collect())
            }
            SeqOp::Distinct => {
                let mut unique = Vec::new();
                for item in items {
                    if !unique.iter().any(|seen| seen == &item) {
                        unique.push(item);
                    }
                }
                Ok(unique)
            }
            SeqOp::Zip(other) => {
                let paired = items
                    .into_iter()
                    .zip(other.iter().cloned())
                    .map(|(first, second)| {
                        Value::List(Rc::new(RefCell::new((vec![first, second]).into())))
                    })
                    .collect();
                Ok(paired)
            }
            SeqOp::Chain(other) => {
                let mut chained = items;
                chained.extend(other.iter().cloned());
                Ok(chained)
            }
            SeqOp::Chunk(n) => {
                let size = (*n).max(1) as usize;
                Ok(items
                    .chunks(size)
                    .map(|chunk| Value::List(Rc::new(RefCell::new((chunk.to_vec()).into()))))
                    .collect())
            }
            SeqOp::Window(n) => {
                let size = (*n).max(1) as usize;
                if size > items.len() {
                    return Ok(Vec::new());
                }
                Ok(items
                    .windows(size)
                    .map(|window| Value::List(Rc::new(RefCell::new((window.to_vec()).into()))))
                    .collect())
            }
            SeqOp::Enumerate => {
                let mut indexed = Vec::with_capacity(items.len());
                for (index, item) in items.into_iter().enumerate() {
                    #[allow(clippy::cast_possible_wrap)]
                    let position = Value::Int(check_safe_int(index as i64)?);
                    indexed.push(Value::List(Rc::new(RefCell::new(
                        (vec![position, item]).into(),
                    ))));
                }
                Ok(indexed)
            }
            // Terminal stages never reach the strict applier.
            SeqOp::Find(_)
            | SeqOp::Any(_)
            | SeqOp::All(_)
            | SeqOp::Count(_)
            | SeqOp::Reduce { .. }
            | SeqOp::GroupBy(_) => Err(VmFault::CorruptedBytecode {
                offset: 0,
                reason: "terminal sequence stage in pure position".to_string(),
            }
            .into()),
        }
    }

    /// Strict predicate truth: sequence predicates return `Bool`, like the
    /// higher-order collection methods; anything else is a type mismatch.
    fn seq_truthy(value: &Value) -> Result<bool, VmFault> {
        match value {
            Value::Bool(verdict) => Ok(*verdict),
            other => Err(VmFault::TypeMismatch {
                expected: "Bool predicate result".to_string(),
                actual: other.type_name().to_string(),
            }),
        }
    }

    /// Snapshots any iterable value into items: lists, dict values, set
    /// members, string chars, range ints, byte values, or a nested sequence
    /// (drained now, so `zip`/`chain` freeze their argument at call time).
    fn snapshot_iterable(
        &mut self,
        module: &BytecodeModule,
        value: &Value,
        operation: &str,
    ) -> Result<Vec<Value>, VmError> {
        match value {
            Value::List(items) => Ok(items.borrow().to_vec()),
            Value::Dict(dict) => Ok(dict.borrow().values()),
            Value::Set(items) => Ok(items.borrow().to_vec()),
            Value::String(text) => Ok(text
                .chars()
                .map(|ch| Value::String(Rc::new(ch.to_string())))
                .collect()),
            Value::Range(r) => Ok((r.start..r.end).map(Value::Int).collect()),
            Value::Bytes(bytes) => Ok(bytes
                .borrow()
                .iter()
                .map(|byte| Value::Byte(*byte))
                .collect()),
            Value::Sequence(pipeline) => self.sequence_items(module, pipeline),
            other => Err(VmFault::TypeMismatch {
                expected: format!("iterable argument for sequence {operation}"),
                actual: other.type_name().to_string(),
            }
            .into()),
        }
    }
}

/// Validated stage count: a count, or a `Failure` value resolving inline.
enum SeqCount {
    Count(i64),
    Failure(Value),
}

/// Snapshots a pipeline source into items (pure: no callbacks run).
fn snapshot_source(source: &SequenceSource) -> Vec<Value> {
    match source {
        SequenceSource::List(items) => items.clone(),
        SequenceSource::Dict(items) => items.clone(),
        SequenceSource::Set(items) => items.clone(),
        SequenceSource::Range { start, end } => (*start..*end).map(Value::Int).collect(),
    }
}
