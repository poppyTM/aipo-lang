# Journal — docs audit 2026-09-20 (Odin residue + stale claims)

**Scope:** full sweep of `docs/` for divergent tech/stack information and stale claims
**Method:** word-boundary grep for `Odin`, counts inventory, claim-vs-code checks per normative
file, 114-link validation, authority-map classification before editing
**Related:** P01-G02 evidence (this audit was ordered alongside the gauntlet)

## Odin verdict

Standalone "Odin" survives **only** where it belongs: `docs/canon/` historical material (book
chapters, 01–06, Especificação Viva history) and the authority-map entry that classifies them as
historical. One exception found and fixed: normative `Sintaxe Canônica` §numeric-model said "A
VM Odin pode…" → "A VM pode…" (zero semantic change).

## Normative fixes applied

| File | Was | Now |
|---|---|---|
| `product/scope.md` | Frozen at Wave 1 (`build` missing, JS "deferred") | Wave 1 + W2-1 delivered; remaining waves still deferred |
| `architecture/overview.md` | Wave-1-only crates, `aipo build`/`aipo-js`/stdlib missing, "GC-managed heap" | Waves 1–2 crates, JS branch, `Rc<RefCell>` (no GC crate) |
| `development/coding-standards.md` | Multi-language boilerplate (`gofmt`, `black`, `docstrings`) | Rust-specific, bound to workspace lints + canon standard |
| `governance/repository-governance.md` | Go gates (`gofmt`, `govet`, `gotest -race`); PR-only rule vs direct-push reality | Rust gates; solo direct-push documented, PRs on collaborators |
| `operations/deployment.md` | Generic service boilerplate (staging, race detector) | CLI release: gates, audit/deny, version+checksum, smoke, tag rollback |
| `operations/observability.md` | Microservice pillars (endpoints, traces) | Diagnostics/exit-codes/baselines/coverage/evidence |
| `installation/lifecycle.md` | External stdlib dir, `~/.cache/aipo/` | Stdlib compiled in; no daemon, no cache dir |
| `security/security-contract.md` | Fixture path `docs/conformance/fixtures/` (nonexistent); unsafe table with 1 row | Correct corpus paths; 9 exception rows from measured inventory |
| `waves/wave-1-mvp.md` | Open-looking plan | Marked closed with evidence pointer |
| `waves/wave-2-js-parity.md` | Undifferentiated plan | W2-1 DONE marked |
| `stdlib/mvp-subset.md` | `aipo-js` listed as non-delivery | Delivered-by-P01-G01 noted, rest still deferred |
| `architecture/adr/001-*.md` | "Accepted" boilerplate contradicting real architecture | Partially-superseded note with precedence pointer |
| `PRUMO.md` | Stale CLI surface, stale ADP-001 line, missing Wave 2/ADP-003-005/testing docs | All corrected/linked |
| `Cargo.toml` | Repository URL pointed at `github.com/aipo-lang/aipo` | `github.com/raillen/aipo-lang` |

## Deliberately untouched

- `docs/canon/` historical material (authority-map class 8): kept verbatim.
- Evidence files: historical counts preserved by policy.
- `wave-1-mvp.md` Wave-1-scoped statements (`run/check/fmt`, exit criteria): accurate for the
  closed wave.
- Scaffold routers (`*/README.md`, `clean-code-contract.md` generics): generic but not false
  about this project; left for a dedicated docs pass.
- Empty dirs (`features/`, `hosts/`, `javascript/`, `poppy/`, `tooling/`, …): no content
  invented to fill them.

## Link validation

114 internal links checked across `docs/` + root markdown: 0 broken (1 false positive: inline
code ``fn first[T](...)`` in the Living Spec).
