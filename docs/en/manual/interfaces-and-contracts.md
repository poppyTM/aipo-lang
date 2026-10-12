# Interfaces & Contracts

Aipo bridges dynamic flexibility with **signature contracts**, **structural invariants**, and **automatic structural interface subtyping**.

---

## Structs (`struct`)

Structures define aggregate types delimited by curly braces `{ ... }`.

By safe and predictable design default, **all fields in a struct are immutable**. When a field must be mutable during the instance lifecycle, declare it explicitly with the `var` keyword:

```aipo
struct Server {
    id
    created_at
    var status = "offline"
    var cpu_load = 0.0
}

# Structural instantiation using symmetric key-value ':'
let s = Server{
    id: "srv-1",
    created_at: 1600000000,
    status: "online",
    cpu_load: 0.42,
}

io.println(s.id)     # "srv-1"
io.println(s.status) # "online"
```

Reassigning an immutable field after construction triggers the static semantic diagnostic `AIPO_SEM_FIXED_REASSIGN`.

---

## Construction Hook (`init`)

The `init` hook is bound to a type using the `Type:` prefix to validate, normalize, and initialize instance fields before publication:

```aipo
struct User {
    email: String
    name: String
}

User:init(email: String, name: String) {
    if not email.contains("@") {
        return fail("Invalid email address format")
    }
    self.email = email
    self.name = name
}

let u = User{ email: "user@example.com", name: "Dev" }
io.println(u.email) # "user@example.com"
```

---

## Structural Invariants (`invariant`)

Invariants declare logical predicates bound to the type that **must remain true throughout the lifetime of the object**:

```aipo
struct Interval {
    var start: Int = 0
    var end_val: Int = 0
}

Interval:init(start: Int, end_val: Int) {
    self.start = start
    self.end_val = end_val
}

Interval:invariant {
    self.start <= self.end_val
}

let inter = Interval{ start: 5, end_val: 10 }
io.println(inter.start)   # 5
io.println(inter.end_val) # 10
```

Whenever a field is mutated, the invariant predicate is automatically re-evaluated. If it fails, the operation is rejected. Inside an `attempt { ... }` block, provisional mutations are automatically rolled back by the transaction journal.

---

## Methods and Universal Mutability (`var self`)

Methods are declared using the `Type:method_name` syntax. The receiver `self` is implicit in read-only methods and explicit as `var self` when mutating.

By default, the `self` receiver is **read-only**. When a method needs to mutate internal instance state, it explicitly declares `var self`, harmonizing method mutability with the universal variable rules of the language:

```aipo
struct Counter {
    var count: Int = 0
}

# Read-only method: self is implicit and immutable (no 'fn' keyword)
Counter:current() -> Int {
    return self.count
}

# Mutator method: var self explicitly signals state modification
Counter:increment(var self) {
    self.count += 1
}
```

---

## Interfaces and Automatic Structural Subtyping (`interface`)

Interfaces define method signatures without bodies. In Aipo, interface conformance requires no ceremony: **subtyping is structural and automatic** (inspired by Go and Luau).

If a struct or enum implements all methods required by an `interface` with compatible signatures and contracts, it **automatically satisfies the interface**:

```aipo
interface Drawable {
    draw() -> String
}

#!satisfies Drawable
struct Button {
    label: String
}

Button:draw() -> String {
    return f"[Button: {self.label}]"
}

# Accepts any type satisfying the Drawable contract
fn render_element(item: Drawable) -> String {
    return item.draw()
}

let btn = Button{ label: "Submit" }
io.println(render_element(btn)) # "[Button: Submit]"
```

---

## Batch Promotion (`::`)

Free functions can be batch-promoted to methods of a type using the `::` token:

```aipo
fn perimeter(self) -> Float {
    return 2.0 * (self.width + self.height)
}

fn scale(var self, factor: Float) {
    self.width *= factor
    self.height *= factor
}

Rectangle::[perimeter, scale]
```

**Rule:** The promoted function MUST declare `self` (or `var self`) as its first parameter.

---

## Enums: Closed Sum Types (`enum`)

`enum` defines closed algebraic sum types with compiler-verified exhaustiveness in `match`:

```aipo
enum ConnectionState {
    Disconnected,
    Connecting(attempt: Int),
    Connected { ip: String, ping_ms: Int },
    Error(reason: String),
}

fn describe(state: ConnectionState) -> String {
    return match state {
        when ConnectionState.Disconnected { "Offline" }
        when ConnectionState.Connecting(a) { f"Attempt #{a}" }
        when ConnectionState.Connected { ip, ping_ms } { f"{ip}:{ping_ms}" }
        when ConnectionState.Error(r) { f"Error: {r}" }
    }
}
```

---

## Compiler Directives (`#!name`)

Directives are compiler annotations attached to the immediately following item:
- `#!satisfies I1, I2`: Enforces interface conformance at compile time.
- `#!test` / `#!test[tag]` / `#!test("name")`: Marks unit tests.
- `#!deprecated("msg")`: Warns on usage of obsolete APIs.
- `#!todo("msg")`: Tracks technical debt.
