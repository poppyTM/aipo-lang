# Aipo diagnostic accessibility rubric

**Status:** normative for diagnostic wording review (process level)
**Scope:** what every user-facing diagnostic must satisfy, and how it is checked
**Related:** `crates/aipo-cli/tests/diagnostic_ui.rs` (automated snapshots),
`docs/testing/cognitive-accessibility-protocol.md` (manual protocol),
`docs/diagnostics/catalog.md` (codes)

## Automated checks (run in CI)

For every diagnostic fixture and UI golden, the suite asserts:

1. **No color dependence** — no ANSI escape bytes in human or JSONL output. Semantic
   information is never conveyed by color alone (there is no color).
2. **Complete schema** — every JSONL line carries `code`, `severity`, `message`,
   `primary_span` (`file`, `start`, `end`, `line`, `column`), `notes` and `suggestions`, so
   tooling never parses prose.
3. **Location first** — human rendering starts with `file:line:column`, then the code, then
   the message; the source excerpt with carets follows.
4. **Stdout discipline** — failures print nothing on stdout; diagnostics go to stderr. Piped
   tooling never mixes program output with errors.
5. **Cascade control** — pathological input yields a bounded diagnostic count (the nesting
   bound produces exactly one error).

## Human review checklist (per diagnostic wording change)

- [ ] The message states **what happened** in literal terms.
- [ ] The source location is identified (or the message explains why there is none, as with
      module-cycle errors).
- [ ] Necessary Aipo terminology is used consistently with the Language Reference; no new term
      is introduced without definition.
- [ ] When the correction is safely known, it is explained or suggested.
- [ ] No metaphor, humor, blame ("you forgot"), or cleverness.
- [ ] A beginner can locate the problem, state what went wrong, and guess a correction without
      reading implementation docs.
- [ ] Primary error is separated from secondary notes.

## Non-goals

- Diagnostic wording is a stable product surface: changes need golden-diff review, never silent
  LLM rewrites.
- This rubric does not certify visual rendering (fonts, themes); it certifies the text
  interface, which is the accessible baseline.
