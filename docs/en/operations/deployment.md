# Deployment guide

Aipo is a CLI toolchain (the `aipo` binary), not a service: there is no staging, database
migration or rollout — there are gates, a version and a checksum.

## Release requirements

1. `fast` gates 100% green (`docs/testing/ci-tiers.md`): fmt, check, Clippy `-D warnings`,
   full suite, doc, `prumo validate/doctor`.
2. `cargo audit` and `cargo deny check` clean.
3. Version updated (`Cargo.toml` workspace + affected crates) and notes in `CHANGELOG.md`.

## Procedure

- `cargo build --release -p aipo-cli`; publish the SHA-256 checksum of the binary.
- Post-build smoke test: `aipo run examples/01_fizzbuzz.aipo` and an `aipo build` + `node`
  conference must reproduce the committed outputs.
- Rollback = republish the binary from the previous tag (Git tags per release).
