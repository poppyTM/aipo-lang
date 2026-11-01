# Aipo — product vision

**Status:** normative
**Scope:** problem, users, product outcome, success boundaries
**Blocking question:** What problem does the product solve?

## Problem and users

Modern dynamic languages often suffer from ambiguous contracts, runtime surprises and lack of
strict modularity, while static languages impose high boilerplate for rapid prototyping.
Furthermore, collaborative development between human programmers and autonomous code agents
requires crystal-clear architectural boundaries, deterministic tooling and verifiable
documentation.

Aipo is designed for developers, systems engineers and autonomous code agents who need a small,
robust, dynamically and strongly typed language with optional signature contracts, fast start-up
and an embeddable runtime.

## What problem does the product solve?

Aipo solves the friction between dynamic ergonomics and strong structural correctness. It provides:
1. Strong dynamic typing with optional signature contracts and runtime boundary verification.
2. A small, cohesive language surface that avoids clever or ambiguous edge cases.
3. Clean, modular architecture separating frontend, bytecode compiler and virtual machine.
4. Seamless integration with agentic harnesses and deterministic quality gates.

## Product outcome

An expressive, general-purpose language implemented in safe Rust that executes reliably across
Linux, macOS and Windows, featuring an end-to-end stack:
`.aipo` source → lexer → parser → HIR → sema → Core IR → bytecode → VM → CLI.

The first delivery boundary is the `aipo v0.1.0` language release defined in
`docs/product/scope.md` and `docs/adp/ADP-008-v0.1.0-language-release.md`.

## Success boundaries

- Small, predictable runtime without uncontrolled memory leaks or panics.
- 100% of language specifications verifiable through executable test suites and diagnostic
  fixtures.
