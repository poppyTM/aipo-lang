# Your First Program in 5 Minutes

In this quick walkthrough, you will write, verify, compile, and execute your first program in Aipo.

---

## 1. Hello, World!

Create a file named `hello.aipo`:

```aipo
# hello.aipo
io.println("Hello from Aipo!")
```

Run it directly with the CLI:

```bash
aipo run hello.aipo
```

**Expected output:**
```
Hello from Aipo!
```

---

## 2. Structs, Invariants, and Methods

Let's model a bank account with an invariant guaranteeing a non-negative balance. Create `account.aipo`:

```aipo
# account.aipo
struct Account {
    holder: String
    account_number: Int
    var balance: Float = 0.0
}

Account:init(holder: String, account_number: Int = 0, balance: Float = 0.0) {
    self.holder = holder
    self.account_number = account_number
    self.balance = balance
}

# Invariant: executed upon construction and every field mutation
Account:invariant {
    self.balance >= 0.0
}

Account:deposit(var self, amount: Float) {
    if amount <= 0.0 {
        fail("Deposit amount must be positive")
    }
    self.balance += amount
}

Account:withdraw(var self, amount: Float) {
    if amount <= 0.0 {
        fail("Withdrawal amount must be positive")
    }
    # The mutation is provisional until the call ends: if it violates
    # `self.balance >= 0.0`, the operation fails and the previous value is restored.
    self.balance -= amount
}

# Instantiating the account using consistent colon (:) syntax
var acc = Account{ holder: "Alice Johnson", account_number: 1042, balance: 150.0 }

io.println(f"Account created for: {acc.holder}")
io.println(f"Initial balance: {acc.balance}")

acc.deposit(50.0)
io.println(f"Balance after deposit: {acc.balance}")

acc.withdraw(75.0)
io.println(f"Balance after withdrawal: {acc.balance}")

# A withdrawal above the balance fails and the balance is preserved.
attempt {
    acc.withdraw(500.0)
} failed err {
    io.println(f"Withdrawal rejected: {err.message}")
}
io.println(f"Preserved balance: {acc.balance}")
```

Run the program:

```bash
aipo run account.aipo
```

**Expected output:**
```
Account created for: Alice Johnson
Initial balance: 150.0
Balance after deposit: 200.0
Balance after withdrawal: 125.0
Withdrawal rejected: invariant() of Account did not hold after mutation
Preserved balance: 125.0
```

---

## 3. Inspecting Bytecode

Aipo provides full transparency into compiler output. You can disassemble the bytecode for any file:

```bash
aipo disasm account.aipo
```

The output shows bytecode mnemonics annotated with corresponding source line and column numbers.

---

## 4. Compiling to JavaScript

You can transpile the exact same program to JavaScript for Node.js or browser execution:

```bash
aipo build account.aipo -o dist/account.js
node dist/account.js
```

The emitted JavaScript provides bit-for-bit behavioral parity with the native Rust VM.
