//! Benchmark workloads: frontend stages, commands, VM execution, JS backend,
//! and scaling analysis.

use aipo_testkit::timez::{self, Sample};

fn kib(bytes: usize) -> String {
    if bytes >= 1024 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

fn scale_to(source: &str, target_bytes: usize) -> String {
    let repeats = (target_bytes / source.len().max(1)).max(1);
    source.repeat(repeats)
}

fn corpus_program(name: &str) -> String {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("docs/conformance/programs");
    path.push(name);
    std::fs::read_to_string(&path).expect("corpus program is readable")
}

/// Frontend stages over realistically shaped sources at four sizes.
pub(crate) fn frontend(rounds: usize, quick: bool) -> Vec<Sample> {
    let base = corpus_program("10_integrated.aipo");
    let mut sizes = vec![1024, 10 * 1024, 100 * 1024];
    if !quick {
        sizes.push(1024 * 1024);
    }
    let surface = aipo_testkit::pipeline::prelude_surface();
    let mut out = Vec::new();
    for target in sizes {
        let text = scale_to(&base, target);
        let input = kib(text.len());
        let bytes = Some(text.len() as u64);
        out.push(timez::measure("source/new", &input, bytes, rounds, || {
            let _ = aipo_source::Source::new(aipo_source::SourceId::next(), "bench.aipo", &text);
        }));
        out.push(timez::measure(
            "lexer/tokenize",
            &input,
            bytes,
            rounds,
            || {
                let source =
                    aipo_source::Source::new(aipo_source::SourceId::next(), "bench.aipo", &text);
                let lexer = aipo_lexer::Lexer::new(&source);
                let _ = lexer.tokenize();
            },
        ));
        out.push(timez::measure(
            "syntax/parse",
            &input,
            bytes,
            rounds,
            || {
                let source =
                    aipo_source::Source::new(aipo_source::SourceId::next(), "bench.aipo", &text);
                let _ = aipo_syntax::parse(&source);
            },
        ));
        let source = aipo_source::Source::new(aipo_source::SourceId::next(), "bench.aipo", &text);
        let (program, _) = aipo_syntax::parse(&source);
        out.push(timez::measure("hir/lower", &input, bytes, rounds, || {
            let _ = aipo_hir::lower(program.clone());
        }));
        let hir = aipo_hir::lower(program);
        out.push(timez::measure("sema/check", &input, bytes, rounds, || {
            let _ = aipo_sema::check_with_prelude(&source, &hir, &surface);
        }));
        out.push(timez::measure("ir/lower", &input, bytes, rounds, || {
            let _ = aipo_ir::lower_to_ir(&hir);
        }));
        let ir = aipo_ir::lower_to_ir(&hir);
        out.push(timez::measure(
            "bytecode/compile+verify",
            &input,
            bytes,
            rounds,
            || {
                let _ = aipo_bytecode::compile(&ir);
            },
        ));
        out.push(timez::measure(
            "formatter/format",
            &input,
            bytes,
            rounds,
            || {
                let _ = aipo_formatter::format_text("bench.aipo", &text);
            },
        ));
        out.push(timez::measure("js/emit", &input, bytes, rounds, || {
            let _ = aipo_js::emit_js("bench.aipo", &text, &ir);
        }));
    }
    out
}

/// End-to-end command equivalents (check/run/build-emit/fmt-check).
pub(crate) fn commands(rounds: usize, _quick: bool) -> Vec<Sample> {
    let text = corpus_program("10_integrated.aipo");
    let input = kib(text.len());
    let bytes = Some(text.len() as u64);
    vec![
        timez::measure("cmd/check", &input, bytes, rounds, || {
            let _ = aipo_testkit::pipeline::check_text("bench.aipo", &text);
        }),
        timez::measure("cmd/run", &input, bytes, rounds, || {
            let _ = aipo_testkit::pipeline::run_text("bench.aipo", &text);
        }),
        timez::measure("cmd/build-emit", &input, bytes, rounds, || {
            if let Ok((_, ir)) = aipo_testkit::pipeline::lower_to_ir("bench.aipo", &text) {
                let _ = aipo_js::emit_js("bench.aipo", &text, &ir);
            }
        }),
        timez::measure("cmd/fmt-check", &input, bytes, rounds, || {
            let once =
                aipo_formatter::format_text("bench.aipo", &text).expect("bench source formats");
            let _ = aipo_formatter::format_text("bench.aipo", &once);
        }),
    ]
}

fn vm_case(rounds: usize, name: &str, source: &str) -> Sample {
    let (_, bytecode) = aipo_testkit::pipeline::compile_text("bench.aipo", source)
        .expect("bench workload compiles");
    let input = format!("{} B", source.len());
    timez::measure_split(name, &input, None, rounds, || {
        let report = aipo_testkit::pipeline::run_capture_split(&bytecode)
            .expect("bench VM workload executes");
        (report.setup, report.execution)
    })
}

/// VM execution workloads from the gauntlet list (frontend excluded).
pub(crate) fn vm_workloads(rounds: usize, _quick: bool) -> Vec<Sample> {
    let cases = [
        (
            "vm/int-arith",
            "var t = 0\nrepeat 200 {\nt = t + 3 * 7 - 1\n}\n",
        ),
        (
            "vm/float-arith",
            "var t = 0.5\nrepeat 200 {\nt = t * 1.5 + 0.25\n}\n",
        ),
        (
            "vm/call-overhead",
            "fn id(x) {\nreturn x\n}\nvar t = 0\nrepeat 200 {\nt = id(t + 1)\n}\n",
        ),
        (
            "vm/recursion",
            "fn fib(n) {\nif n < 2 {\nreturn n\n}\nreturn fib(n - 1) + fib(n - 2)\n}\nfib(20)\n",
        ),
        (
            "vm/closure-shared-var",
            "fn make() {\nvar c = 0\nreturn fn () {\nc = c + 1\nreturn c\n}\n}\nlet f = make()\nrepeat 200 {\nf()\n}\n",
        ),
        (
            "vm/global-lookup",
            "let g = 7\nvar t = 0\nrepeat 200 {\nt = t + g\n}\n",
        ),
        (
            "vm/list-build-iter",
            "var xs = []\nrepeat 200 {\nxs.add(1)\n}\nvar t = 0\neach x in xs {\nt = t + x\n}\n",
        ),
        (
            "vm/dict-insert-lookup",
            "var d = {}\nrepeat 100 {\n d[\"k\"] = 1\n}\nvar t = 0\nrepeat 100 {\nt = t + d[\"k\"]\n}\n",
        ),
        (
            "vm/string-concat",
            "var s = \"\"\nrepeat 100 {\ns = s + \"ab\"\n}\n",
        ),
        (
            "vm/interpolation",
            "var t = 0\nrepeat 100 {\nt = len(f\"n={t}\")\n}\n",
        ),
        (
            "vm/bytes-alloc",
            "var t = 0\nrepeat 20 {\nt = t + len(Bytes(4096))\n}\n",
        ),
        (
            "vm/pipeline",
            "var t = 0\nrepeat 100 {\nt = [t] |> len\n}\n",
        ),
        (
            "vm/failure-propagate",
            "fn maybe(x) {\nif x > 100 {\nreturn fail(\"big\")\n}\nreturn x\n}\nvar t = 0\nrepeat 100 {\nt = maybe(t + 1) or_else -1\n}\n",
        ),
        (
            "vm/attempt",
            "var t = 0\nrepeat 50 {\nattempt {\nt = Int(\"xx\")\n} failed err {\nt = -1\n}\n}\n",
        ),
        (
            "vm/contracts",
            "fn f(x: Int) -> Int {\nreturn x + 1\n}\nvar t = 0\nrepeat 100 {\nt = f(t)\n}\n",
        ),
        (
            "vm/invariant-commit",
            "struct R {\nlo\nhi\n}\nR:init(lo, hi) {\nself.lo = lo\nself.hi = hi\n}\nR:invariant {\nself.lo < self.hi\n}\nvar t = 0\nrepeat 50 {\nt = R{lo: t, hi: t + 2}.lo\n}\n",
        ),
    ];
    cases
        .iter()
        .map(|(name, source)| vm_case(rounds, name, source))
        .collect()
}

/// JS backend workloads: full frontend plus bundle generation plus Node.
pub(crate) fn js_workloads(rounds: usize, _quick: bool) -> Vec<Sample> {
    let integrated = corpus_program("10_integrated.aipo");
    let programs = [
        ("js/hello", "io.println(\"hi\")\n".to_string()),
        ("js/integrated", integrated),
    ];
    let mut out = Vec::new();
    for (name, source) in &programs {
        let source = source.to_string();
        let input = kib(source.len());
        out.push(timez::measure(name, &input, None, rounds, || {
            let (_, ir) =
                aipo_testkit::pipeline::lower_to_ir("bench.aipo", &source).expect("compiles");
            let bundle = aipo_testkit::js::emit_bundle("bench.aipo", &source, &ir, "bench");
            let (code, _, _) = aipo_testkit::js::run_node(&bundle.dir);
            assert_eq!(code, Some(0));
            let _ = std::fs::remove_dir_all(&bundle.dir);
        }));
    }
    out
}

/// Scaling analysis over list build/iterate and dict insert at N..8N.
pub(crate) fn scaling(rounds: usize, _quick: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for (label, template) in [
        ("list-build", "var xs = []\nrepeat {n} {\nxs.add(1)\n}\n"),
        (
            "list-iterate",
            "var xs = []\nrepeat {n} {\nxs.add(1)\n}\nvar t = 0\neach x in xs {\nt = t + x\n}\n",
        ),
        ("dict-insert", "var d = {}\nrepeat {n} {\nd[\"k\"] = 1\n}\n"),
    ] {
        let mut medians = Vec::new();
        for scale in [1usize, 2, 4, 8] {
            let n = 200 * scale;
            let source = template.replace("{n}", &n.to_string());
            let (_, bytecode) = aipo_testkit::pipeline::compile_text("bench.aipo", &source)
                .expect("scaling workload compiles");
            let sample = timez::measure(
                &format!("scale/{label}"),
                &format!("N={n}"),
                None,
                rounds,
                || {
                    let _ = aipo_testkit::pipeline::run_capture(&bytecode);
                },
            );
            medians.push(sample.median);
        }
        let ratios: Vec<String> = medians
            .windows(2)
            .map(|pair| {
                let ratio = pair[1].as_secs_f64() / pair[0].as_secs_f64().max(f64::MIN_POSITIVE);
                format!("{ratio:.2}x")
            })
            .collect();
        let verdict = if ratios
            .iter()
            .all(|r| r.parse::<f64>().unwrap_or(99.0) < 3.0)
        {
            "linear"
        } else if ratios.iter().any(|r| r.parse::<f64>().unwrap_or(0.0) > 6.0) {
            "SUPERLINEAR — investigate"
        } else {
            "noisy — rerun on dedicated runner"
        };
        lines.push(format!(
            "{label}: N=200..1600 ratios [{}] verdict={verdict}",
            ratios.join(", ")
        ));
    }
    lines
}
