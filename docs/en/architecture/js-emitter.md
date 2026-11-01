# JavaScript backend (`aipo-js`)

Aipo's JavaScript backend transpiles Aipo source code into **modern JavaScript (ES2022)** with
a guarantee of total behavioral parity.

---

## The parity challenge

Many transpilers targeting JavaScript fall into the trap of using JavaScript's primitive types
loosely, producing subtle semantic inconsistencies (such as `0 == ""` or `"5" + 2 == "52"`).

`aipo-js` adopts a strict strategy:
1. **Versioned runtime shim**: a small pure-JavaScript helper package that exposes classes and
   functions to preserve strong typing, ordered arrays/maps and byte handling.
2. **Clean emission**: transpiles control structures into JS equivalents (functions, classes,
   `try/catch` for `attempt`), preserving V8 or JavaScriptCore performance.
3. **Line mapping with source maps**: automatic V3 source-map generation, enabling debugging
   directly in browser tools or Node.js using the original `.aipo` file.

---

## Differential test verification

To ensure the JavaScript compiler never diverges from the Rust virtual machine, the project
maintains a continuous differential test suite:
- Every program in `docs/conformance/programs/` runs on the Rust VM and on Node.js.
- If there is a discrepancy in a single output character or in a diagnostic code, the project
  build is blocked in CI.
