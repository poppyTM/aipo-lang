# Systems architecture (`docs/architecture/`)

## What is this directory?
Contains topology definitions, domain boundaries, modular composition rules and architectural
decision records (ADRs).

## What is it for?
Ensures the system keeps high cohesion and low coupling throughout its evolution, preventing
circular dependencies and architectural degradation.

## Inventory
- `overview.md`: Historical view of Waves 1–2.
- `crates.md`: Current inventory of 24 crates and normal dependencies (source: `cargo metadata`).
- `backend-support-matrix.md`: Measured backend coverage on 2026-10-27.
- `bytecode-and-vm.md`: `.aibc` format and execution model.
- `embedded-and-shell-profile.md`: Build profiles, limits and embedding.
- `clean-code-contract.md`: Strict software-engineering and clean-code contract.
- `adr/`: Formal architectural decision records (ADRs).
