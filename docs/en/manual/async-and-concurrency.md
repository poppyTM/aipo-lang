# Concurrency & Async

Aipo's concurrency model is **cooperative, deterministic, and based on virtual time**. It completely eliminates low-level data races and ensures 100% reproducible test suites with zero artificial wall-clock delays.

---

## Asynchronous Functions (`async fn`)

Functions performing timing operations, cooperative I/O, or asynchronous orchestration are declared with `async fn` and delimited by curly braces `{ ... }`. Calling an async function schedules the task immediately on the cooperative scheduler and returns its `Task` handle:

```aipo
async fn fetch_data(resource) {
    # task.sleep suspends execution cooperatively using virtual time ticks
    task.sleep(50)
    return f"Data for {resource}"
}

# Invoking launches the task and returns a Task handle
let my_task = fetch_data("users")

# Explicitly await completion to obtain the result
let data = await my_task
io.println(data) # "Data for users"
```

---

## Sequential Await Block (`await do { ... }`)

Unlike ecosystems where `await` can be dropped into arbitrary nested subexpressions, Aipo provides the `await do { ... }` block for clear, sequential asynchronous execution:

```aipo
async fn step_one() {
    return 10
}

async fn step_two() {
    return 20
}

async fn run_pipeline() {
    await do {
        let a = await step_one()
        let b = await step_two()
        return a + b
    }
}

let total = await run_pipeline()
io.println(f"Accumulated total: {total}") # 30
```

This discipline prevents unhandled promises and forgotten background tasks (`AIPO_SEM_FORGOTTEN_TASK`).

---

## Async Combinators (`task.*`)

The standard library provides powerful high-level combinators:

- **`task.spawn(callable, args_list)`**: Spawns a new concurrent task in the scheduler with the provided arguments.
- **`task.sleep(ms)`**: Suspends current task execution for a specified number of virtual time ticks.
- **`task.all(task_list)`**: Awaits until all tasks in the list complete, returning an ordered list of results.
- **`task.race(task_list)`**: Resolves as soon as the first task completes, cooperatively cancelling the others.
- **`task.timeout(target_task, ms)`**: Cancels target task if it exceeds virtual time duration.
- **`task.cancel(target_task)`**: Cooperatively aborts an active task.
- **`task.group()`**: Creates a structured task group for coordinated lifecycle and cascading cancellation.

```aipo
async fn step_one() { task.sleep(10) return 10 }
async fn step_two() { task.sleep(10) return 20 }

let t1 = task.spawn(step_one, [])
let t2 = task.spawn(step_two, [])

# Await all tasks concurrently with deterministic ordering
let results = task.all([t1, t2])
io.println(results) # [10, 20]
```

---

## Transitive Cycle Detection (`AIPO_RT_AWAIT_CYCLE`)

The Aipo runtime maintains an active await-dependency graph. If two or more tasks enter a mutual waiting cycle (async deadlock), the runtime immediately detects it and triggers the deterministic fault `AIPO_RT_AWAIT_CYCLE` with the complete chain of cycle members.
