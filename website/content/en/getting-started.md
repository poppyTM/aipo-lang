---
title: Getting started
---
# Getting started with Aipo

Install a compatible Rust toolchain and build the CLI from the repository root:

```bash
cargo build -p aipo-cli
cargo run -q -p aipo-cli -- run examples/06_variables_and_values.aipo
```

A minimal Aipo file looks like this:

```aipo
let name = "world"
io.println(f"Hello, {name}!")
```

Save it as `hello.aipo` and run `cargo run -q -p aipo-cli -- run hello.aipo`.

The [Portuguese book](/learn/) is the main progressively organized resource. English chapter translations have not yet passed content-parity review. Check [backend status](/reference/status) and do not confuse the target specification with the currently implemented surface.
