//! VM call frames and failure handler stack representations.

/// Call frame tracking activation of functions and closures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallFrame {
    /// Return instruction pointer in the caller's code.
    pub return_ip: usize,
    /// Offset in the VM operand stack where this frame's arguments and locals start.
    pub stack_base: usize,
    /// Number of arguments passed to this invocation.
    pub arg_count: usize,
    /// `true` when slot 0 of the frame is the receiver of an `impl` method.
    ///
    /// A bound method is called with the receiver occupying the callee slot, so unlike a
    /// free function call there is no separate callee value to drop on return.
    pub receiver_in_place: bool,
    /// Length of the mutation journal when this frame started.
    ///
    /// Invariant-protected assignments are provisional until a stable mutable boundary
    /// verifies them, so the frame owns every journal entry from this index onward and
    /// releases them when it returns.
    pub journal_start: usize,
}

impl CallFrame {
    /// Creates a new call frame for a free function or closure call.
    #[must_use]
    pub const fn new(
        return_ip: usize,
        stack_base: usize,
        arg_count: usize,
        journal_start: usize,
    ) -> Self {
        Self {
            return_ip,
            stack_base,
            arg_count,
            receiver_in_place: false,
            journal_start,
        }
    }

    /// Creates a new call frame for an `impl` method call.
    #[must_use]
    pub const fn method(
        return_ip: usize,
        stack_base: usize,
        arg_count: usize,
        journal_start: usize,
    ) -> Self {
        Self {
            return_ip,
            stack_base,
            arg_count,
            receiver_in_place: true,
            journal_start,
        }
    }

    /// Stack length to unwind to before pushing the call's result value.
    ///
    /// A free call leaves the callee value below the arguments, so it is dropped together
    /// with them; a method call reuses that slot for the receiver, so only the arguments
    /// and the receiver are dropped.
    #[must_use]
    pub const fn result_base(&self) -> usize {
        if self.receiver_in_place {
            self.stack_base
        } else {
            self.stack_base - 1
        }
    }
}

/// Recovery handler frame registered by an `attempt ... failed` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandlerFrame {
    /// Iteration scopes to retain when this handler recovers.
    pub iteration_depth: usize,
    /// Collection identities to retain when this handler recovers.
    pub active_iteration_depth: usize,
    /// Bytecode offset of the `failed` handler block.
    pub handler_ip: usize,
    /// Stack depth to unwind to before executing the handler.
    pub stack_depth: usize,
    /// Call frame depth to unwind to before executing the handler.
    pub frame_depth: usize,
}

impl HandlerFrame {
    /// Creates a new handler frame.
    #[must_use]
    pub const fn new(handler_ip: usize, stack_depth: usize, frame_depth: usize) -> Self {
        Self {
            iteration_depth: 0,
            active_iteration_depth: 0,
            handler_ip,
            stack_depth,
            frame_depth,
        }
    }
}
