# Control Flow & Failures

Aipo offers clean, deterministic, and highly readable control flow constructs: blocks enclosed in curly braces `{ ... }` without redundant parentheses around conditions, combined with a transactional error handling model with automatic atomic rollback.

---

## Conditional Structures

### `if ... elif ... else` Blocks

Conditions require no parentheses, and blocks open and close with braces `{ ... }`. The syntax provides immediate visual clarity, with native support for rainbow brackets and automatic code folding in any IDE:

```aipo
var score = 85
var status = ""

if score >= 90 {
    status = "Excellent"
} elif score >= 70 {
    status = "Passing"
} else {
    status = "Remedial"
}

io.println(status) # "Passing"
```

### Inline Expression `if condition then a else b`

Aipo also supports single-line ternary conditional value expressions using `then`:

```aipo
let active = true
let message = if active then "Online" else "Offline"
io.println(message) # "Online"
```

---

## Pattern Matching (`match ... when`)

The `match` construct branches execution by comparing an expression against literals, struct destructuring patterns, and enum variants:

### Simple Patterns & Conditional Guards
Each branch begins with `when` (never `case`). It supports optional boolean guards using `if <condition>`:

```aipo
struct User {
    name
    age
}

let user = User{ name: "Ana", age: 70 }

match user {
    when { name, age } if age >= 65 {
        io.println(f"Senior: {name}")
    }
    when { name, age } if age >= 18 {
        io.println(f"Adult: {name}")
    }
    else {
        io.println("Minor")
    }
}
```

### Enum Variant Matching & Exhaustiveness
When matching over an `enum`, the compiler strictly enforces exhaustive checking. If no `else` branch is supplied, all variants must be explicitly covered:

```aipo
enum ConnectionState {
    Disconnected,
    Connecting(tries: Int),
    Connected { ip: String, ping_ms: Int },
    Error(reason: String),
}

let state = ConnectionState.Connecting(2)

match state {
    when ConnectionState.Disconnected {
        io.println("Disconnected")
    }
    when ConnectionState.Connecting(a) {
        io.println(f"Connecting attempt #{a}")
    }
    when ConnectionState.Connected { ip, ping_ms } {
        io.println(f"Connected to {ip}")
    }
    when ConnectionState.Error(r) {
        io.println(f"Error: {r}")
    }
}
```

---

## Loop Constructs

All loop constructs in Aipo use delimited `{ ... }` blocks and require **no** parentheses or connecting words like `do`.

### `while`

Executes the loop body as long as the boolean condition is true:

```aipo
var i = 0
while i < 3 {
    io.println(f"Step: {i}")
    i += 1
}
```

### `loop`

Canonical infinite loop, designed for repetitions that terminate via explicit `break`:

```aipo
var attempts = 0
loop {
    attempts += 1
    if attempts >= 3 {
        break
    }
}
io.println(f"Total attempts: {attempts}")
```

### `repeat`

Repeats the block a fixed number of times with an optional iteration counter (`repeat count as index`):

```aipo
# Executes 3 times (with indices 0, 1, and 2)
repeat 3 as idx {
    io.println(f"Iteration number: {idx}")
}
```

### `each`

Canonical iteration over collections (`List`, `Dict`, `Set`, `Sequence`):

```aipo
# Simple iteration over a list
let fruits = ["Apple", "Banana", "Orange"]
each fruit in fruits {
    io.println(fruit)
}

# Iteration with index and element
each idx, fruit in fruits {
    io.println(f"{idx}: {fruit}")
}

# Iteration over dictionary (key and value in insertion order)
let config = {
    "host": "127.0.0.1",
    "port": 5432,
}
each key, val in config {
    io.println(f"{key} => {val}")
}
```

---

## Failure Model & Transactions (`attempt ... failed`)

In Aipo, errors are neither uncontrolled stack-unwinding exceptions nor easily ignored status codes. Failures are raised explicitly with `fail` (or returned with `return fail(...)`) and handled in transactional blocks with **journaling and automatic atomic rollback**:

```aipo
struct Vault {
    var balance = 0.0
}

Vault:init(initial_balance = 0.0) {
    self.balance = initial_balance
}

Vault:invariant {
    self.balance >= 0.0
}

let v = Vault{ balance: 100.0 }

attempt {
    # This mutation temporarily drops balance to -100.0
    v.balance -= 200.0
} failed err {
    # Violating the invariant triggers automatic rollback restoring self.balance to 100.0!
    io.println(f"Caught failure: {err.message}")
}

# The balance remains untouched at its pre-attempt state!
io.println(f"Preserved balance: {v.balance}") # 100.0
```

If any operation inside an `attempt` block executes `fail` or violates a structural invariant, all mutations to journaled objects are atomically reverted to their original state.

### Immediate Fallback with `or_else`

For expressions where you simply want to provide a fallback recovery value without the boilerplate of an `attempt` block, use the canonical `or_else` operator:

```aipo
fn read_file(path) {
    # If the file does not exist or fails, return fail
    return fail("file not found")
}

# If read_file raises fail, or_else evaluates and yields the fallback alternative:
let content = read_file("config.toml") or_else "host = 127.0.0.1"
io.println(content) # "host = 127.0.0.1"
```
