# Practical Examples & Code Recipes

This section brings together idiomatic code patterns, real-world recipes, and the complete catalog of the 25 canonical examples that accompany the official Aipo repository.

---

## 1. Data Pipeline with `|>` and Higher-Order Functions

The pipeline operator `|>` passes the result of the preceding expression as the first argument of the following function, creating readable, left-to-right processing streams:

```aipo
struct Product {
    name: String
    price: Int
    category: String
}

let catalog = [
    Product{ name: "Keyboard", price: 250, category: "Hardware" },
    Product{ name: "Rust Guide", price: 120, category: "Education" },
    Product{ name: "Optical Mouse", price: 80, category: "Hardware" },
    Product{ name: "4K Monitor", price: 2200, category: "Hardware" }
]

# Filter affordable hardware (< $500) and extract product names
fn filter_affordable_hardware(items, ceiling: Int) {
    let filtered = items.filter(fn (p) { return p.category == "Hardware" and p.price <= ceiling })
    return filtered.transform(fn (p) { return p.name })
}

let affordable = catalog |> filter_affordable_hardware(300)
each name in affordable {
    io.println(f"Available: {name}")
}
```

---

## 2. Domain Modeling with Invariants and Atomic Rollback

In Aipo, structs equipped with `invariant()` hooks safeguard their internal invariants at every stable mutable boundary. If a method breaks an invariant or issues a `fail(...)`, **all changes undergo an immediate atomic rollback**:

```aipo
struct Wallet {
    user: String
    var balance: Int
}

Wallet:invariant {
    self.balance >= 0
}

Wallet:credit(var self, amount: Int) {
    if amount <= 0 {
        fail("credit amount must be positive")
    }
    self.balance += amount
}

Wallet:debit(var self, amount: Int) {
    self.balance -= amount
}

let w = Wallet{ user: "Lucas", balance: 150 }

attempt {
    w.credit(50)     # balance increases to 200
    w.debit(500)     # violation: balance would become -300!
} failed err {
    io.println(f"Operation rejected: {err.message}")
}

# The balance remains 150 (the previous valid state was completely restored)
io.println(f"Safe balance: {w.balance}")
```

---

## 3. Concurrency with `async`, `task`, and `await do`

Aipo's concurrency model is cooperative, deterministic, and task-driven:

```aipo
async fn fetch_user(id: Int) {
    task.sleep(50)
    return {"id": id, "name": "Aipo Dev"}
}

async fn fetch_permissions(id: Int) {
    task.sleep(30)
    return ["read", "write", "deploy"]
}

async fn load_profile(id: Int) {
    let t_user = task.spawn(fetch_user, [id])
    let t_perms = task.spawn(fetch_permissions, [id])

    let user = await t_user
    let perms = await t_perms

    return {"user": user, "permissions": perms}
}
```

---

## 4. Safe JSON Serialization and Parsing

The standard library `json` module provides strict, unambiguous handling:

```aipo
let payload_text = r"""{"service": "auth", "port": 8080, "active": true}"""

# Safe parsing with fallback using or_else
let data = json.parse(payload_text) or_else fail("Malformed JSON")

let service = data["service"]
let port = data["port"]
io.println(f"Service: {service}")
io.println(f"Port: {port}")

# Serializing structured values
let response = {
    "status": "ok",
    "timestamp": 1727376000
}
let generated_json = json.stringify(response)
io.println(generated_json)
```

---

## 5. Repository Example Catalog (25 Programs)

All examples below are tested and directly runnable under the `examples/` directory in the repository:

| Example | Key Topics |
| :--- | :--- |
| `01_fizzbuzz.aipo` | Pure functions, `repeat ... as`, modulo `%`, and `if/elif` control. |
| `02_local_functions.aipo` | Nested local functions, closures, and direct self-recursion. |
| `03_contracts_and_interfaces.aipo` | Contract signatures, structural interfaces, and rollback invariants. |
| `04_data_pipeline.aipo` | Full `|>` data pipeline, collection methods, and trailing blocks `do { ... }`. |
| `05_modules/` | Modularity with `export`, selective imports, and private scoping. |
| `06_variables_and_values.aipo` | Immutability with `let`, mutability with `var`, and primitive types. |
| `07_functions_defaults_named_args.aipo` | Default arguments (`greeting = "hi"`) and named parameter passing. |
| `08_lists_dicts_and_slices.aipo` | Lists, dictionaries, negative indices, and tolerant slicing (`..`). |
| `09_strings_unicode_and_formatting.aipo` | String interpolation `f"..."`, raw strings `r"..."`, and UTF-8 Unicode. |
| `10_bytes.aipo` | Binary byte buffer `Bytes` manipulation and conversions. |
| `11_failures_or_else_attempt.aipo` | Idiomatic failure handling with `fail`, `or_else`, and `attempt/failed`. |
| `12_struct_init_fixed_invariant.aipo` | Constructors `init`, immutable-by-default fields, and invariant checks. |
| `13_mutation_and_rollback.aipo` | Atomic state restoration on struct invariant violations. |
| `14_interfaces_and_satisfy.aipo` | Automatic structural subtyping and explicit `satisfy` assertions. |
| `15_pipelines_and_trailing_blocks.aipo` | Function piping with `|>` and block closures. |
| `16_ranges_repeat_each.aipo` | Iterations with ranges `0..10`, `each` loops, `break`, and `continue`. |
| `17_word_frequency.aipo` | Word frequency counter with string splitting and dictionaries. |
| `18_small_statistics.aipo` | Mathematical operations with `math`, mean, and standard deviation. |
| `19_unicode_normalization.aipo` | NFC canonical normalization at Unicode text boundaries. |
| `20_safe_numeric_boundaries.aipo` | Safe integer arithmetic within the ±(2^53 - 1) boundary. |
| `21_closure_state.aipo` | Shared mutable state capture across closure invocations. |
| `22_small_budget_application.aipo` | Small budget application with transactional struct instances. |
| `23_multi_module_application/` | Multi-file application with wallet and pricing submodules. |
| `24_idiomatic_aipo_showcase.aipo` | Comprehensive showcase of idiomatic Aipo features and best practices. |
| `25_snake_game.aipo` | Complete terminal-rendered Snake game simulation featuring structs, lists, collision math, and score tracking. |
