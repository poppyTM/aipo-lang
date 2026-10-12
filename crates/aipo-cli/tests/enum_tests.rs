//! End-to-end tests for `enum` (ADTs / closed sum types) in Aipo.
//!
//! Exercised through the CLI so the entire toolchain runs together: lexer, parser,
//! AST, HIR, sema (including exhaustiveness), IR lowering, bytecode emission, and VM.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

fn run_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone)]
struct CaptureSink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn run_program(path: &Path) -> (u8, String, String) {
    let _serialized = run_lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(CaptureSink(Arc::clone(&captured)))));
    let args: Vec<String> = vec!["run".into(), path.to_string_lossy().into()];
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = aipo_cli::run_with(&args, &mut out, &mut err);
    aipo_cli::set_output_sink(None);

    let program_output = {
        let buffer = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&buffer).into_owned()
    };
    (
        code,
        program_output,
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn check_program(path: &Path) -> (u8, String, String) {
    let _serialized = run_lock();
    let args: Vec<String> = vec!["check".into(), path.to_string_lossy().into()];
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = aipo_cli::run_with(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

#[test]
fn test_enum_unit_variant_construction_and_match() {
    let temp = std::env::temp_dir().join("aipo_enum_unit.aipo");
    let source = r#"
enum Semáforo {
    Vermelho,
    Amarelo,
    Verde,
}

let s = Semáforo.Verde

match s {
    when Semáforo.Vermelho {
        io.println("pare")
    }
    when Semáforo.Amarelo {
        io.println("atencao")
    }
    when Semáforo.Verde {
        io.println("siga")
    }
}
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "siga");
}

#[test]
fn test_enum_struct_variant_payload_destructuring() {
    let temp = std::env::temp_dir().join("aipo_enum_struct.aipo");
    let source = r#"
enum Mensagem {
    Texto { corpo: String, autor: String },
    Quit,
}

let m = Mensagem.Texto{ corpo: "ola", autor: "ana" }

match m {
    when Mensagem.Texto { corpo, autor } {
        io.println(autor + ": " + corpo)
    }
    when Mensagem.Quit {
        io.println("fim")
    }
}
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "ana: ola");
}

#[test]
fn test_enum_tuple_variant_payload_destructuring() {
    let temp = std::env::temp_dir().join("aipo_enum_tuple.aipo");
    let source = r#"
enum Resultado {
    Sucesso(valor: Int),
    Erro(mensagem: String),
}

let r1 = Resultado.Sucesso(42)
let r2 = Resultado.Erro("falhou")

match r1 {
    when Resultado.Sucesso(valor) {
        io.println(String(valor))
    }
    when Resultado.Erro(mensagem) {
        io.println("erro: " + mensagem)
    }
}

match r2 {
    when Resultado.Sucesso(valor) {
        io.println(String(valor))
    }
    when Resultado.Erro(mensagem) {
        io.println("erro: " + mensagem)
    }
}
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "42\nerro: falhou");
}

#[test]
fn test_enum_variant_guard() {
    let temp = std::env::temp_dir().join("aipo_enum_guard.aipo");
    let source = r#"
enum Status {
    Ativo { nivel: Int },
    Inativo,
}

let s = Status.Ativo{ nivel: 5 }

match s {
    when Status.Ativo { nivel } if nivel > 10 {
        io.println("alto")
    }
    when Status.Ativo { nivel } if nivel <= 10 {
        io.println("baixo")
    }
    when Status.Inativo {
        io.println("desligado")
    }
}
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "baixo");
}

#[test]
fn test_enum_exhaustiveness_failure_without_else() {
    let temp = std::env::temp_dir().join("aipo_enum_non_exhaustive.aipo");
    let source = r#"
enum Opção {
    Sim,
    Não,
    Talvez,
}

let o = Opção.Sim

match o {
    when Opção.Sim {
        io.println("ok")
    }
}
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, _stdout, stderr) = check_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_ne!(code, 0, "non-exhaustive enum match must be rejected");
    assert!(
        stderr.contains("AIPO_SEM_NON_EXHAUSTIVE_MATCH"),
        "must report AIPO_SEM_NON_EXHAUSTIVE_MATCH, got: {stderr}"
    );
    assert!(
        stderr.contains("Não") && stderr.contains("Talvez"),
        "must name missing variants, got: {stderr}"
    );
}

#[test]
fn test_enum_exhaustiveness_satisfied_by_else() {
    let temp = std::env::temp_dir().join("aipo_enum_else.aipo");
    let source = r#"
enum Opção {
    Sim,
    Não,
    Talvez,
}

let o = Opção.Talvez

match o {
    when Opção.Sim {
        io.println("sim")
    }
    else {
        io.println("outro")
    }
}
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "outro");
}

#[test]
fn test_enum_conformance_fixture_32() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture =
        manifest.join("../../docs/conformance/programs/32_enum_variants_and_matching.aipo");
    let (code, stdout, stderr) = run_program(&fixture);
    assert_eq!(code, 0, "stderr: {stderr}");
    let expected = std::fs::read_to_string(
        manifest.join("../../docs/conformance/programs/32_enum_variants_and_matching.stdout"),
    )
    .unwrap();
    assert_eq!(stdout.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
}

#[test]
fn test_enum_is_expression() {
    let temp = std::env::temp_dir().join("aipo_enum_is.aipo");
    let source = r#"
enum Status {
    Ativo,
    Inativo,
}
let s = Status.Ativo
io.println(String(s is Status))
io.println(String(s is Status.Ativo))
io.println(String(s is Status.Inativo))
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "true\ntrue\nfalse");
}

/// An enum method is declared on the enum (`Enum:metodo`), but a payload variant is
/// stored as a struct named `Enum.Variante`. A variant receiver must therefore
/// resolve the method on the parent enum, in the frontend *and* in the VM, instead of
/// reporting `unknown member` / `no field`.
#[test]
fn test_enum_method_on_payload_variant_receiver() {
    let temp = std::env::temp_dir().join("aipo_enum_variant_method.aipo");
    let source = r#"
enum EstadoConexao {
    Desconectado,
    Conectado { ip: String, ping_ms: Int },
}

EstadoConexao:esta_operacional() -> Bool {
    var operacional = false
    match self {
        when EstadoConexao.Conectado { ip, ping_ms } if ping_ms < 500 {
            operacional = true
        }
        else {
            operacional = false
        }
    }
    return operacional
}

let rapido = EstadoConexao.Conectado{ ip: "10.0.0.1", ping_ms: 42 }
let lento = EstadoConexao.Conectado{ ip: "10.0.0.2", ping_ms: 900 }
let off = EstadoConexao.Desconectado

io.println(String(rapido.esta_operacional()))
io.println(String(lento.esta_operacional()))
io.println(String(off.esta_operacional()))
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "true\nfalse\nfalse");
}

#[test]
fn test_enum_invariant_enforcement() {
    let temp = std::env::temp_dir().join("aipo_enum_inv.aipo");
    let source = r#"
enum Forma {
    Circulo { raio: Int },
}
Forma:invariant {
    self.raio > 0
}
let c = Forma.Circulo{ raio: 10 }
io.println("valido")
let c_bad = Forma.Circulo{ raio: -1 }
"#;
    std::fs::write(&temp, source).unwrap();
    let (code, stdout, stderr) = run_program(&temp);
    let _ = std::fs::remove_file(&temp);
    assert_ne!(code, 0);
    assert_eq!(stdout.trim(), "valido");
    assert!(stderr.contains("AIPO_RT_TYPE_MISMATCH"));
}
