//! Integration tests for the `aipo-sh` shell and evaluation interface (Marco 4 / ADP-014).

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone)]
struct TestSink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for TestSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_eval_source_simple_expression() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = "io.println(10 + 20)";
    let exit_code = aipo_cli::eval_source(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0);
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "30\n");
    assert!(err.is_empty());
}

/// Runs `code` on one engine and returns `(exit, stdout, diagnostics)`.
fn run_engine(code: &str, reg: bool) -> (u8, String, String) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let exit = if reg {
        aipo_cli::eval_source_reg(code, &mut out, &mut err)
    } else {
        aipo_cli::eval_source(code, &mut out, &mut err)
    };
    aipo_cli::set_output_sink(None);
    let stdout = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    let mut diag = String::from_utf8(out).unwrap();
    diag.push_str(&String::from_utf8(err).unwrap());
    (exit, stdout, diag)
}

/// Both engines must agree on open-ended (`usize::MAX`) variadic natives and on arity
/// handling: the RegVm's call path mirrors the stack VM's `check_arity`, where `usize::MAX`
/// accepts any argument count and a fixed arity rejects a wrong one.
#[test]
fn test_engine_parity_variadic_and_arity() {
    let _guard = test_lock();
    for code in [
        "io.println(path.join(\"a\", \"b\", \"c\"))",
        "io.println(path.join(\"a\"))",
        "io.println(String(math.sqrt(4.0)))",
        "io.println(String(math.sqrt(1.0, 2.0)))",
    ] {
        let stack = run_engine(code, false);
        let reg = run_engine(code, true);
        assert_eq!(stack, reg, "engine divergence for: {code}");
    }
}

#[test]
fn test_eval_source_pipeline() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
fn double(x: Int) -> Int {
    return x * 2
}
io.println(21 |> double())
"#;
    let exit_code = aipo_cli::eval_source(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0);
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "42\n");
}

#[test]
fn test_eval_source_syntax_error_reports_failure() {
    let _guard = test_lock();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = "fn broken(";
    let exit_code = aipo_cli::eval_source(code, &mut out, &mut err);
    assert_eq!(exit_code, 1);
    assert!(!err.is_empty());
}

#[test]
fn test_eval_source_reg_simple_expression() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = "io.println(10 + 20)";
    let exit_code = aipo_cli::eval_source_reg(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0);
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "30\n");
    assert!(err.is_empty());
}

#[test]
fn test_eval_source_reg_user_function_call() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
fn double(x: Int) -> Int {
    return x * 2
}
io.println(21 |> double())
"#;
    let exit_code = aipo_cli::eval_source_reg(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0);
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "42\n");
}

#[test]
fn test_run_with_engine_reg_flag() {
    let _guard = test_lock();
    let path = std::env::temp_dir().join(format!("aipo-reg-test-{}.aipo", std::process::id()));
    std::fs::write(&path, "io.println(6 * 7)\n").expect("write temp file");
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let args = vec![
        "run".to_string(),
        path.to_string_lossy().into_owned(),
        "--engine=reg".to_string(),
    ];
    let mut out = Vec::new();
    let mut err = Vec::new();
    let exit_code = aipo_cli::run_with(&args, &mut out, &mut err);
    aipo_cli::set_output_sink(None);
    let _ = std::fs::remove_file(&path);

    assert_eq!(exit_code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "42\n");
}

#[test]
fn test_eval_source_reg_attempt_failed() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
var msg = "ok"
attempt {
    fail "falha capturada com sucesso"
    msg = "nao deve rodar"
} failed err {
    msg = err.message
}
io.println(msg)
"#;
    let exit_code = aipo_cli::eval_source_reg(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "falha capturada com sucesso\n");
}

#[test]
fn test_sh_module_automation() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
let res = sh.run("echo", ["automation-test"])
io.print(res["stdout"])
let pwd = sh.pwd()
io.println(pwd.len() > 0)
"#;
    let exit_code = aipo_cli::eval_source(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0);
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "automation-test\ntrue\n");
}

#[test]
fn test_eval_source_reg_collections_and_structs() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
struct Point {
    x
    y
}
let p = Point{ x: 10, y: 20 }
let list = [p.x, p.y, 30]
let dict = { "total": list[0] + list[1] + list[2] }
io.println(dict["total"])
"#;
    let exit_code = aipo_cli::eval_source_reg(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "60\n");
}

#[test]
fn test_eval_source_reg_each_and_defaults_and_is() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
fn add_with_default(a, b = 10) {
    return a + b
}
let sum1 = add_with_default(5)
let sum2 = add_with_default(5, 20)

var total = 0
each item in [sum1, sum2] {
    if item is Int {
        total = total + item
    }
}
io.println(total)
"#;
    let exit_code = aipo_cli::eval_source_reg(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "40\n");
}

#[test]
fn test_eval_source_reg_closure_captures_upvalue() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
fn make_adder(base) {
    let add = fn(x) { return x + base }
    return add
}
let add5 = make_adder(5)
let add10 = make_adder(10)
io.println(add5(7) + add10(7))
"#;
    let exit_code = aipo_cli::eval_source_reg(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "29\n");
}

#[test]
fn test_eval_source_reg_each_loop() {
    let _guard = test_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(TestSink(Arc::clone(&captured)))));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = r#"
var total = 0
each item in [10, 20, 30] {
    total = total + item
}
io.println(total)
"#;
    let exit_code = aipo_cli::eval_source_reg(code, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    assert_eq!(exit_code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "60\n");
}
