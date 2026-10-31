//! Regressions for canonical capture and construction semantics.
//!
//! Both behaviours are observable only by running a program, so these tests drive the
//! canonical VM end to end. They run as one test because the testkit pipeline installs
//! process-global state (the standard environment), so parallel cases would interleave.

use aipo_testkit::pipeline::run_text;

/// Runs a program on the canonical VM and returns its stdout.
fn run_program(text: &str) -> String {
    run_text("test.aipo", text).expect("program runs")
}

#[test]
fn capture_and_construction_regressions() {
    // Canon captures a mutable binding **by reference**: a closure that assigns to an
    // enclosing `var` and that same `var` read in the enclosing frame must observe one
    // storage location. The lowering promotes a captured local to a shared cell; without
    // it the closure wrote into a snapshot and the enclosing frame kept reading the stale
    // slot.
    assert_eq!(
        run_program(
            "fn make() {\n  var n = 0\n  fn bump() { n = n + 1 }\n  bump()\n  bump()\n  return n\n}\nio.println(String(make()))\n"
        ),
        "2\n",
        "a `fn` local that assigns to an enclosing `var` must share its storage"
    );

    // A returned closure keeps counting across calls.
    assert_eq!(
        run_program(
            "fn make() {\n  var n = 0\n  return fn() { n = n + 1\n    return n }\n}\nlet tick = make()\ntick()\nio.println(String(tick()))\n"
        ),
        "2\n",
        "a returned closure must keep its captured counter"
    );

    // An intermediate closure forwards the *cell*, not a copy, so a two-level capture
    // still reaches the frame that declared the `var`.
    assert_eq!(
        run_program(
            "fn outer() {\n  var x = 0\n  fn mid() {\n    fn inner() { x = x + 1 }\n    inner()\n  }\n  mid()\n  mid()\n  return x\n}\nio.println(String(outer()))\n"
        ),
        "2\n",
        "a two-level capture must forward the shared cell"
    );

    // A positional construction maps its entries onto the declaration order. Matching by
    // name alone made every positional entry fall through to `none`.
    assert_eq!(
        run_program(
            "struct P {\n  x\n  y\n}\nlet p = P{1, 2}\nio.println(String(p.x))\nio.println(String(p.y))\n"
        ),
        "1\n2\n",
        "positional construction fills fields in declaration order"
    );

    // A partial positional literal keeps every declared default in place.
    assert_eq!(
        run_program("struct P {\n  x\n  y = 9\n}\nio.println(String(P{1}.y))\n"),
        "9\n",
        "positional construction respects declared defaults"
    );

    // Named fields stay authoritative even when the literal reorders them.
    assert_eq!(
        run_program(
            "struct P {\n  x\n  y\n}\nlet p = P{y: 2, x: 1}\nio.println(String(p.x))\nio.println(String(p.y))\n"
        ),
        "1\n2\n",
        "named construction still matches by name"
    );
}
