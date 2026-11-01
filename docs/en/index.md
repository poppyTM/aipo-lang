---
layout: home

hero:
  name: Aipo
  text: Simple, Fast & Concurrent with Optional Contracts
  tagline: A modern programming language with clean syntax inspired by Gleam and Swift, dynamic strong typing, transactional contracts with automatic rollback, and a complete built-in developer toolchain.
  image:
    src: /assets/logo.svg
    alt: Aipo Language Logo
  actions:
    - theme: brand
      text: Get Started (5 min)
      link: /en/getting-started/first-program
    - theme: alt
      text: Language Manual
      link: /en/manual/
    - theme: alt
      text: Tooling (CLI)
      link: /en/tools/
    - theme: alt
      text: Examples & Recipes
      link: /en/examples/

features:
  - icon: 🎯
    title: Cognitive Clarity & Clean Syntax
    details: Inspired by Gleam and Swift. Symmetrical brace blocks, parenthesis-free conditions, immutable-by-default fields, and zero visual clutter for maximum reading ergonomics.
    link: /en/manual/syntax-and-types
  - icon: 🛠️
    title: Complete Built-in Toolchain
    details: Compiler, automated isolated test runner, static verification linter, opinionated canonical formatter, and hermetic package manager in a single fast binary (aipo).
    link: /en/tools/
  - icon: 🛡️
    title: Contracts & Atomic Rollback
    details: Structural invariants that protect your domain models. If any mutation breaks a business rule, Aipo rolls back the previous state automatically without corrupted data.
    link: /en/manual/interfaces-and-contracts
  - icon: ⚡
    title: Cooperative & Deterministic Concurrency
    details: Native async fn and await do syntax with cooperative scheduler and virtual time. Zero unpredictable race conditions and total test reproducibility.
    link: /en/manual/async-and-concurrency
  - icon: 🌐
    title: VM ↔ JavaScript Differential Parity
    details: Emit high-performance WebAssembly or clean ES2022 JavaScript bundles with identical outputs, checksums, and diagnostics matching the native Virtual Machine.
    link: /en/architecture/js-emitter
  - icon: 📦
    title: Hermetic Dependency Management
    details: aipo.toml manifest, reproducible lockfiles, remote GitHub dependencies pinned by commit SHA with verified SHA-256 digests, and local offline caching.
    link: /en/manual/packages-and-modules
---

<div class="vp-doc">

## A Frictionless Developer Experience

**Aipo** is designed for engineers who prize reasoning clarity, predictability, and speed. Rather than dangerous implicit type coercions (like `"1" + 2 == "12"`), Aipo pairs dynamic, strong typing with **optional structural contracts**, **transactional failure recovery with automatic rollback**, and a **self-contained toolchain without external dependencies**.

```aipo
# Interface with automatic structural subtyping
interface Notifiable {
    summary() -> String
}

# Closed sum type enum
enum AccountStatus {
    Active,
    Suspended(reason: String),
}

# Struct with fields immutable by default and explicit var mutability
#!satisfies Notifiable
struct Account {
    id: Int
    holder: String
    var balance: Int
    var status: AccountStatus = AccountStatus.Active
}

# Transactional integrity invariant
Account:invariant {
    self.balance >= 0
}

# Explicit receiver mutation with `var self`
Account:transfer(var self, var target: Account, amount: Int) {
    if amount <= 0 {
        fail "transfer amount must be positive"
    }

    # If any rule is broken, both accounts undergo an atomic rollback
    attempt {
        self.balance -= amount
        target.balance += amount
    } failed err {
        fail f"transfer safely aborted: {err.message}"
    }
}

# Immutable reader method (implicit self)
Account:summary() -> String {
    return f"Account #{self.id} ({self.holder}): ${self.balance // 100}"
}

# Symmetrical colon instantiation
var a1 = Account{ id: 101, holder: "Alex", balance: 25000 }
var a2 = Account{ id: 102, holder: "Beatrice", balance: 5000 }

a1.transfer(a2, 5000)
io.println(a1.summary())
io.println(a2.summary())
```

---

## All the Tools You Need in a Single Binary

Never waste time setting up third-party formatters, test runners, or linters. With the `aipo` CLI, everything is built-in:

<div class="tool-grid">

<div class="tool-card">
  <div>
    <h3>▶️ Run</h3>
    <p>Execute <code>.aipo</code> source or compiled <code>.aibc</code> bytecode instantly.</p>
  </div>
  <div class="tool-cmd">aipo run app.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>🧪 Test</h3>
    <p>Isolated tests with frozen virtual clock and deterministic PRNG seeds.</p>
  </div>
  <div class="tool-cmd">aipo test</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔍 Check</h3>
    <p>Static verification of contracts, scope, and mutability in milliseconds.</p>
  </div>
  <div class="tool-cmd">aipo check app.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>✨ Format</h3>
    <p>Opinionated canonical formatting that ends pull request style debates.</p>
  </div>
  <div class="tool-cmd">aipo fmt src/</div>
</div>

<div class="tool-card">
  <div>
    <h3>📦 Build</h3>
    <p>Bundle to ES2022 JavaScript or WebAssembly with full source maps.</p>
  </div>
  <div class="tool-cmd">aipo build app.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔒 Package</h3>
    <p>Hermetic lockfiles and strict SHA-256 dependency auditing.</p>
  </div>
  <div class="tool-cmd">aipo package audit .</div>
</div>

</div>

---

## Getting Started

::: tip 💡 Start in under 5 minutes
1. **Install or compile the CLI**: `cargo build --release -p aipo-cli` (or grab the release binary).
2. **Write your first program**: Follow our quickstart guide at [Your First Program](/en/getting-started/first-program).
3. **Explore the Manual and Tooling**: Learn language fundamentals in the [Language Manual](/en/manual/) and dive into the [Tooling Guide](/en/tools/).
:::

---

## Open and Transparent Engineering

Aipo's development is 100% public, rigorous, and evidence-driven:
- **[System Architecture](/en/architecture/)**: Detailed breakdown of the compiler frontend, IR, and virtual machine.
- **[Architectural Decisions (ADPs)](/en/decisions/)**: Formal technical design records and justifications.
- **[Engineering Trajectory](/en/trajectory/)**: The language's structured evolution through disciplined Waves.
- **[Comparative Benchmarks](/en/evidence/cross-language)**: Reproducible performance measurements against industrial language runtimes.

</div>
