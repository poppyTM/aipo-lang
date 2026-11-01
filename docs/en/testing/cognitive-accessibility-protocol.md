# Cognitive accessibility validation protocol (manual)

**Status:** research-oriented protocol (process level, not automated proof)
**Scope:** how to usability-test ten beginner mistakes against the rubric
**Related:** `docs/testing/diagnostic-accessibility-rubric.md`,
`crates/aipo-cli/tests/diagnostic_ui.rs`

## Method

For each mistake below: give a tester (ideally an Aipo beginner) the failing program and its
diagnostic output only. Record answers, do not coach.

### Mistake set

1. Missing `end`
2. Unknown identifier
3. Wrong argument count
4. Wrong contract (`Int` where `String` passed)
5. Invalid mutation (`fixed` field reassignment)
6. Index out of range
7. Incorrect import (private name / missing module)
8. Invalid interface implementation (missing operation)
9. Uncaught `Failure`
10. Invalid conversion (`Int("nope")` unhandled)

### Per-mistake record

- Can the user locate the problem? (file/line/column or described region)
- Can the user state what went wrong, in their own words?
- Can the user identify a likely correction?
- Is the primary explanation understandable without implementation details?
- How many independent concepts must be held in working memory? (count them)
- Does the diagnostic introduce unexplained Aipo terminology? (list terms)

### Pass guidance (qualitative, not a gate)

- Location found in under a minute; correction guessed correctly.
- Zero unexplained terms, or terms defined in the message itself.
- Working-memory load of three or fewer concepts.

### Reporting

File results under `docs/testing/cognitive-sessions/` (one file per session, dated). Recurring
failures become diagnostic-wording issues against the rubric, then golden updates — never
silent message edits.
