# Syntax & Data Types

Aipo was engineered to offer lean, deterministic syntax free from dangerous implicit behaviors. This section details the type system, canonical best practices for writing idiomatic code, and modern language features.

---

## The Aipo Way: Principles of Idiomatic Writing

To guarantee clean, high-performance, and accessible code (especially welcoming for developers with ADHD and dyslexia), Aipo establishes clear canonical rules:

| Canonical Practice (The Aipo Way) | Anti-Pattern to Avoid | Why this matters |
| :--- | :--- | :--- |
| `let name = "Dev"` (Immutable by default) | `var name = "Dev"` without need for mutation | Prevents accidental mutation and simplifies deterministic reasoning about state. |
| `struct Point { x, var y = 0 }` (Immutable fields by default) | Global mutations or unstructured state | Guarantees struct integrity; mutation requires explicit intent via `var`. |
| `User{ name: "Ana", age: 28 }` (Key-value with `:`) | Divergent mapping syntax | Colon unifies key-value association across dictionaries and structs without motor confusion. |
| `if condition { ... }` (Brace blocks) | Redundant parentheses around conditions | Lean syntax without parenthesis clutter, fully compatible with native rainbow brackets and code folding. |
| `Type:deposit(var self, amount)` (Explicit mutator receiver) | Hidden side effects in methods | `var` is the universal keyword for mutability, making side effects visually clear. |
| `a // b` and `a //= b` (Integer division) | Truncated manual casts | Expressive and symmetric canonical arithmetic operator alongside `+`, `-`, `*`, `/`. |
| `f"User {id}: {email}"` (Direct interpolation) | String concatenation `"User " + String(id) + ": " + email` | Direct interpolation eliminates visual clutter and intermediate heap allocations. |
| `r"C:\data\report.csv"` (Raw strings) | Escape sequences `"C:\\data\\report.csv"` | Raw strings eliminate backslash pollution in file paths and regex patterns. |
| `let city = user?.profile?.city` (Safe navigation) | Defensive cascading `if user != none and ...` | Safe navigation avoids redundant nested null-checking boilerplate. |
| `let port = load_port() or_else 8080` (Fallback `or_else`) | Heavy error handling `attempt { port = ... } failed ...` | `or_else` provides immediate default values in failure-prone expressions. |
| `data |> filter() |> calculate()` (Pipeline `\|>`) | Deep nesting `calculate(filter(data))` | The pipeline operator expresses data transformations in natural left-to-right order. |
| `list.add(item)` (Universal insertion) | Heterogeneous collection operations | `.add()` is the universal canonical insertion method for lists and sets. |
| `data.lazy().filter(...).collect()` (Lazy evaluation) | Intermediate heap allocations | `.lazy()` consumes constant memory without allocating temporary intermediate lists. |

---

## 1. Modern Strings in Aipo

Text handling in Aipo is robust, expressive, and mathematically predictable. Every string is guaranteed to be **valid UTF-8 with automatic Unicode NFC canonical normalization**.

### String Interpolation (`f"..."`)
Interpolation with the `f` prefix is the canonical way to format messages and compose text:

```aipo
let user = "Alice"
let score = 98.5
let level = 4

# Interpolation with variables and numeric expressions
let report = f"Player: {user} | Level: {level} | Points: {score}"
io.println(report)
# Prints: "Player: Alice | Level: 4 | Points: 98.5"

# Executing operations directly inside interpolated braces
let delta = 1.5
io.println(f"Next milestone: {score + delta}") # 100.0
```

#### Escaping Literal Braces
If you need to include literal `{` or `}` characters inside an f-string, duplicate them (<code>&#123;&#123;</code> and <code>&#125;&#125;</code>):

```aipo
let key = "token"
let value = "xyz123"

# Renders a JSON object with literal braces and interpolated values
let json_payload = f"{{\"{key}\": \"{value}\"}}"
io.println(json_payload) # {"token": "xyz123"}
```

---

### Raw Strings (`r"..."`)
In conventional languages, writing host file paths on Windows or regular expressions requires doubling every backslash (`\\\\`), generating visual noise that impairs readability.

With Aipo's **Raw Strings** (`r"..."`), backslashes are preserved literally:

```aipo
# 1. Host system file paths
let windows_path = r"C:\Users\dev\AppData\Local\Aipo\config.toml"
io.println(windows_path)
# Prints literally: C:\Users\dev\AppData\Local\Aipo\config.toml

# 2. Clean regular expression patterns without double backslashes
let email_pattern = r"^[a-zA-Z0-9_.+-]+@[a-zA-Z0-9-]+\.[a-zA-Z0-9-.]+$"
io.println(email_pattern)
```

---

### Multiline Strings (`"""..."""` and `r"""..."""`)
For long blocks of text, inline documentation, SQL queries, or templates, use triple quotes. They preserve formatting, indentation, and line breaks with clarity:

```aipo
let sql_query = """
SELECT u.id, u.name, p.role
FROM users u
JOIN permissions p ON p.user_id = u.id
WHERE u.active = true
ORDER BY u.name ASC
"""

io.println(sql_query)
```

---

### Raw Format Strings (`fr"..."` / `rf"..."`)
When you need to compose a dynamic regex pattern or a system path containing interpolated variables without escaping backslashes, combine both prefixes:

```aipo
let folder = "logs"
let extension = "txt"

# Combines raw string semantics (unescaped backslashes) with variable interpolation:
let dynamic_path = fr"C:\system\{folder}\app.{extension}"
io.println(dynamic_path)
# Prints: C:\system\logs\app.txt
```

---

### Binary String Conversion (`.encode()` and `.decode()`)
For networking and binary file I/O, strings and `Bytes` buffers convert directly without third-party dependencies:

```aipo
let message = "Deterministic Aipo"

# Converts UTF-8 String into a raw Bytes buffer
let raw_bytes = message.encode()
io.println(f"Byte size: {raw_bytes.len()}")

# Decodes Bytes buffer back into a normalized UTF-8 NFC String
let original_text = raw_bytes.decode()
io.println(original_text) # "Deterministic Aipo"
```

---

## 2. Variables and Immutability

Aipo adopts the immutable-by-default principle consistently across all scopes:

```aipo
# Immutable: compiler prevents subsequent reassignments
let service_fee = 0.15

# Mutable: reserved for loop accumulators or local state
var accumulated_total = 100.0
accumulated_total += 25.0
io.println(accumulated_total) # 125.0
```

::: tip Golden Rule
Always start new variable declarations with `let`. Only change to `var` if the variable is explicitly mutated in the local execution flow.
:::

---

## 3. Primitive Types & Safe Arithmetic

### Integers (`Int`)
Signed 64-bit integers (`i64`). Supports base prefixes and visual grouping with underscores (`_`):

```aipo
let decimal = 42
let million = 1_000_000   # Visual separator for effortless readability
let hexadecimal = 0xFF   # 255
let binary = 0b101010    # 42
let octal = 0o777        # 511
```

### Floating-Point (`Float`)
64-bit IEEE 754 double-precision numbers:

```aipo
let pi = 3.14159
let fractional = 0.005
```

### Real Division (`/`) vs Truncated Integer Division (`//`)
In Aipo, the `/` operator always returns a `Float`. To perform truncated integer division, use the symmetric `//` operator:

```aipo
let a = 10
let b = 3

let real_div = a / b     # 3.3333333333333335 (Float)
let integer_div = a // b # 3 (Int)

# Symmetric compound assignment
var value = 20
value //= 3
io.println(value) # 6
```

::: warning No Silent NaN or Infinity
Aipo's value model strictly forbids corrupted values like `NaN` or `Infinity`. Invalid mathematical operations (such as division by zero or square roots of negative numbers) trigger structured runtime failures immediately.
:::

---

## 4. Idiomatic Collections

### 1. Lists (`List`)
Ordered dynamic arrays indexed from `0`.
- Element insertion: use `.add(item)` (Aipo standardizes on `.add` across lists and sets).
- Reverse indexing: negative indices count from the end (`[-1]` accesses the last element).
- Trailing commas are fully supported in multiline lists.

```aipo
let languages = [
    "Rust",
    "Aipo",
    "TypeScript",
]

languages.add("Odin")

io.println(languages[0])   # "Rust"
io.println(languages[-1])  # "Odin"
io.println(languages.len()) # 4
```

### 2. Dictionaries (`Dict`)
Associative key-value maps declared with `{}` that strictly preserve original insertion order:

```aipo
let config = {
    "server": "api.aipo.dev",
    "port": 8443,
    "ssl": true,
}

# Explicit presence verification with .has()
if config.has("port") {
    let port = config["port"]
    io.println(f"Configured port: {port}")
}
```

::: tip `dict[key]` fails; `dict.get(key)` returns `none`
Aipo has two accesses with different contracts. `config["port"]` is the **direct** read: if the key is missing, the program raises `AIPO_RT_KEY_NOT_FOUND` — the error appears where it happened instead of becoming a `none` that breaks later. `config.get("port")` is the **optional** read: it returns the value when the key exists and `none` when it does not.

Choose by what you know about the key: use `[]` when absence is a bug, and `.get()` when absence is a normal case. To test presence without reading, use `.has(key)`.
:::

### 3. Insertion-Ordered Sets (`Set`)
Store unique elements while preserving the order in which they were first added:

```aipo
let tags = Set(["backend", "compiler", "backend"]) # Duplicate discarded

tags.add("cli")
io.println(tags.has("backend")) # true
io.println(tags.len())           # 3
io.println(tags.to_list())       # ["backend", "compiler", "cli"]
```

### 4. Lazy Sequences (`Sequence`)
To process large datasets without allocating intermediate collections, create a lazy sequence with `.lazy()`:

```aipo
let numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

# Chained operations evaluate on-demand in constant memory.
# Write the chain on one line: the parser does not continue an expression
# onto the next line just because it starts with `.`.
let tripled_evens = numbers.lazy().filter(x => x % 2 == 0).map(x => x * 3).take(2).collect()

io.println(tripled_evens) # [6, 12]
```

If the line gets too long, split it with intermediate bindings instead of breaking before the dot:

```aipo
let numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
let evens = numbers.lazy().filter(x => x % 2 == 0)
let tripled = evens.map(x => x * 3)
io.println(tripled.take(2).collect()) # [6, 12]
```

### 5. Enums (`enum`)
`enum` defines a closed sum type with compiler-verified variants, supporting unit variants, positional payloads, and named field payloads:

```aipo
enum ExecutionMode {
    Debug,
    Optimized { level: Int },
    Profiled(output_file: String),
}

let mode = ExecutionMode.Optimized { level: 3 }
```

---

## 5. Modern & Expressive Operators

### Safe Navigation (`?.`)
Avoids nested null checks. If any link in the chain is `none`, the entire expression resolves to `none` without throwing a runtime error:

```aipo
struct Address {
    city
}

struct User {
    address
}

# Symmetric instantiation using consistent colon (:) syntax:
let u1 = User{ address: Address{ city: "Curitiba" } }
let u2 = User{ address: none }

io.println(u1?.address?.city) # "Curitiba"
io.println(u2?.address?.city) # none
```

### Failure Fallback Operator (`or_else`)
Provides an immediate fallback value if an expression produces a `fail`, without needing an explicit `attempt` block:

```aipo
fn load_port(env) {
    if env == "production" {
        return 443
    }
    return fail("unknown environment")
}

# If load_port() produces a fail, or_else evaluates the right-hand fallback:
let port = load_port("test") or_else 8080
io.println(f"Active port: {port}") # 8080
```

### Pipeline Operator (`|>`)
Allows data transformations to be organized in a natural left-to-right sequence:

```aipo
fn clean(txt) {
    return txt.trim()
}

fn highlight(txt, prefix) {
    return prefix + txt
}

# "  alert  " is passed as first argument to clean(), and result to highlight():
let label = "  alert  " |> clean() |> highlight("[URGENTE] ")
io.println(label) # "[URGENTE] alert"
```

### Type Checking (`is`)
Checks whether a value matches a concrete language type:

```aipo
let value = 42

if value is Int {
    io.println("It is a safe 64-bit integer")
}

# To check for absence of value, check equality with none directly:
let data = none
if data == none {
    io.println("Value is null")
}
```
