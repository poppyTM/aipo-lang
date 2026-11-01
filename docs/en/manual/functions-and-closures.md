# Functions, Closures & Lambdas

Functions in Aipo are first-class citizens. They can be passed as arguments, assigned to variables, returned by other functions, and capture values from their enclosing lexical scope.

---

## Basic Function Declarations

Functions are declared using the `fn` keyword and enclosed in curly braces `{ ... }`:

```aipo
fn add(a, b) {
    return a + b
}

let total = add(10, 20)
io.println(total) # 30
```

If `return` is omitted or does not specify an expression, the function returns `none`.

---

## Default Parameters & Named Arguments

Aipo allows default fallback values for optional parameters:

```aipo
fn connect(host, port = 8080, timeout = 5000) {
    io.println(f"Connecting to {host}:{port} with timeout {timeout}ms")
}

connect("localhost")           # uses port 8080 and timeout 5000
connect("db.internal", 5432)   # uses port 5432 and timeout 5000
```

Functions can also be called with named arguments for enhanced readability:

```aipo
fn connect(host, port = 8080, timeout = 5000) {
    io.println(f"Connecting to {host}:{port} with timeout {timeout}ms")
}

connect("api.service", timeout = 1000)
```

---

## Anonymous Functions & Closures

Anonymous functions (`fn(params) { ... }`) can be assigned to variables and retain lexical access to captured variables:

```aipo
fn create_counter(initial = 0) {
    var count = initial
    return fn() {
        count += 1
        return count
    }
}

let c = create_counter(10)
io.println(c()) # 11
io.println(c()) # 12
```

Aipo's VM implements safe shared upvalues, ensuring that mutations to captured variables are reflected across all sharing closures.

---

## Concise Lambdas (`=>`)

For concise single-expression callbacks (particularly in collection operations like `map` and `filter`), Aipo provides arrow syntax:

```aipo
let numbers = [1, 2, 3, 4, 5]

# Double values with single-parameter lambda
let doubled = numbers.map(x => x * 2)
io.println(doubled) # [2, 4, 6, 8, 10]

# Filter evens
let evens = numbers.filter(x => x % 2 == 0)
io.println(evens) # [2, 4]
```

Multi-parameter lambdas use parenthesized parameter lists: `(a, b) => a + b`.

---

## Local Functions with Self-Recursion

Aipo supports local functions defined inside other function bodies, with full self-recursion resolution via `FillSelfCapture`:

```aipo
fn factorial(n) {
    fn loop_rec(current, acc) {
        if current <= 1 {
            return acc
        }
        return loop_rec(current - 1, acc * current)
    }

    return loop_rec(n, 1)
}

io.println(factorial(5)) # 120
```
