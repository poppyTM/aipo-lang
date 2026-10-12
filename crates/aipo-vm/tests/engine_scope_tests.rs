//! Integration tests for `AipoEngine` and scoped execution (`ADP-011`, `AIPO-OMNI-003`).

use aipo_bytecode::compile;
use aipo_diagnostics::DiagnosticCode;
use aipo_hir::lower;
use aipo_host::{Capability, CapabilitySet, Handle};
use aipo_ir::lower_to_ir;
use aipo_source::{Source, SourceId};
use aipo_syntax::parse;
use aipo_vm::{AipoEngine, Value, Vm, VmError, VmFault};
use std::cell::RefCell;
use std::rc::Rc;

fn compile_script(code: &str) -> aipo_bytecode::BytecodeModule {
    let src = Source::new(SourceId::next(), "test.aipo", code);
    let (ast, diags) = parse(&src);
    assert!(diags.is_empty(), "parse errors: {:?}", diags);
    let hir = lower(ast);
    let ir = lower_to_ir(&hir);
    compile(&ir).expect("compilation failed")
}

#[derive(Debug, PartialEq, Eq)]
struct GameWorld {
    score: i64,
    ticks: i64,
}

fn tick_native(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle = match args.first() {
        Some(Value::HostHandle(h)) => *h,
        _ => {
            return Err(VmFault::TypeMismatch {
                expected: "HostHandle".to_string(),
                actual: format!("{:?}", args.first()),
            }
            .into());
        }
    };
    let world = vm
        .host_context()
        .resolve_userdata_mut::<GameWorld>(handle)?;
    world.ticks += 1;
    world.score += 10;
    Ok(Value::Int(world.score))
}

#[test]
fn test_engine_builder_and_limits() {
    let mut caps = CapabilitySet::none();
    caps.grant(Capability::parse("poppy").expect("valid capability"));

    let mut engine = AipoEngine::builder()
        .capabilities(caps)
        .max_instructions(50)
        .build()
        .expect("build engine");

    assert!(
        engine
            .vm()
            .host()
            .allows(&Capability::parse("poppy").unwrap())
    );
    assert!(
        !engine
            .vm()
            .host()
            .allows(&Capability::parse("clock").unwrap())
    );

    let infinite_loop = compile_script("while true { }");
    let err = engine
        .run(&infinite_loop)
        .expect_err("budget should halt loop");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_OVERFLOW);
}

#[test]
fn test_scope_userdata_mut_happy_path() {
    let mut engine = AipoEngine::new();
    let script = compile_script(
        r#"
let s1 = tick(world)
let s2 = tick(world)
"#,
    );

    let mut saved_handle: Option<Handle> = None;

    let final_score = engine
        .scope(|scope| {
            let handle = scope.insert_userdata(GameWorld {
                score: 100,
                ticks: 0,
            });
            saved_handle = Some(handle.raw());
            scope.expose_global("world", &handle);
            scope.register_host_fn("tick", 1, tick_native);

            scope.run(&script)?;

            // Resolve inside scope to verify state mutation
            let world = scope.resolve_userdata::<GameWorld>(&handle)?;
            assert_eq!(world.ticks, 2);
            assert_eq!(world.score, 120);

            Ok(world.score)
        })
        .expect("scope execution should succeed");

    assert_eq!(final_score, 120);

    // Post-scope: handle must be completely invalidated and memory dropped
    let raw = saved_handle.expect("saved handle");
    let err = engine
        .vm()
        .host()
        .resolve_userdata::<GameWorld>(raw)
        .expect_err("handle must be dead post-scope");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_SCOPE_ESCAPE);
}

#[test]
fn test_scope_userdata_invalidated_even_when_body_errors() {
    let mut engine = AipoEngine::new();
    let mut saved_handle: Option<Handle> = None;

    let err = engine
        .scope(|scope| -> Result<(), VmError> {
            let handle = scope.insert_userdata(GameWorld {
                score: 50,
                ticks: 0,
            });
            saved_handle = Some(handle.raw());
            Err(VmFault::TypeMismatch {
                expected: "Int".to_string(),
                actual: "String".to_string(),
            }
            .into())
        })
        .expect_err("scope body should propagate error");

    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_TYPE_MISMATCH);

    // Verify RAII cleanup still executed
    let raw = saved_handle.expect("saved handle");
    let resolve_err = engine
        .vm()
        .host()
        .resolve_userdata::<GameWorld>(raw)
        .expect_err("handle must be dead even after error");
    assert_eq!(
        resolve_err.diagnostic_code(),
        DiagnosticCode::AIPO_RT_SCOPE_ESCAPE
    );
}

#[test]
fn test_nested_scopes_isolated() {
    let mut engine = AipoEngine::new();
    let mut outer_raw: Option<Handle> = None;
    let mut inner_raw: Option<Handle> = None;

    engine
        .scope(|outer| {
            let h_outer = outer.insert_userdata(GameWorld { score: 1, ticks: 1 });
            outer_raw = Some(h_outer.raw());

            outer
                .sub_scope(|inner| {
                    let h_inner = inner.insert_userdata(GameWorld { score: 2, ticks: 2 });
                    inner_raw = Some(h_inner.raw());

                    assert!(inner.resolve_userdata::<GameWorld>(&h_inner).is_ok());
                    Ok(())
                })
                .expect("inner scope ok");

            // Inner handle must now be dead
            let in_err = outer
                .vm()
                .host()
                .resolve_userdata::<GameWorld>(inner_raw.unwrap())
                .expect_err("inner handle dead");
            assert_eq!(
                in_err.diagnostic_code(),
                DiagnosticCode::AIPO_RT_SCOPE_ESCAPE
            );

            // Outer handle must still be live
            assert!(outer.resolve_userdata::<GameWorld>(&h_outer).is_ok());

            Ok(())
        })
        .expect("outer scope ok");

    // Both handles dead outside
    assert!(
        engine
            .vm()
            .host()
            .resolve_userdata::<GameWorld>(outer_raw.unwrap())
            .is_err()
    );
    assert!(
        engine
            .vm()
            .host()
            .resolve_userdata::<GameWorld>(inner_raw.unwrap())
            .is_err()
    );
}

#[test]
fn test_scope_type_mismatch_fault() {
    let mut engine = AipoEngine::new();

    engine
        .scope(|scope| {
            let handle = scope.insert_userdata(GameWorld { score: 1, ticks: 1 });

            // Resolving with wrong type must fault safely
            let err = scope
                .resolve_userdata::<String>(&handle)
                .expect_err("type mismatch");
            assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_TYPE_MISMATCH);

            Ok(())
        })
        .expect("scope ok");
}

#[test]
fn test_scope_shared_rc_interior_mutability() {
    let mut engine = AipoEngine::new();

    // Host owns a shared reference-counted world outside the scope
    let host_world = Rc::new(RefCell::new(GameWorld {
        score: 500,
        ticks: 10,
    }));

    fn inc_score(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
        let handle = match args.first() {
            Some(Value::HostHandle(h)) => *h,
            _ => {
                return Err(VmFault::TypeMismatch {
                    expected: "HostHandle".to_string(),
                    actual: "none".to_string(),
                }
                .into());
            }
        };
        let rc = vm
            .host_context()
            .resolve_userdata::<Rc<RefCell<GameWorld>>>(handle)?;
        rc.borrow_mut().score += 50;
        rc.borrow_mut().ticks += 1;
        Ok(Value::Int(rc.borrow().score))
    }

    let script = compile_script(
        r#"
let s = boost(w)
"#,
    );

    engine
        .scope(|scope| {
            let handle = scope.insert_userdata(Rc::clone(&host_world));
            scope.expose_global("w", &handle);
            scope.register_host_fn("boost", 1, inc_score);

            scope.run(&script)?;
            Ok(())
        })
        .expect("scope run ok");

    // After scope: host_world is mutated, but script handle is dead
    assert_eq!(host_world.borrow().score, 550);
    assert_eq!(host_world.borrow().ticks, 11);
    assert_eq!(Rc::strong_count(&host_world), 1); // scope clone dropped!
}

#[test]
fn test_engine_register_module_declarations() {
    let mut engine = AipoEngine::new();
    let module = compile_script(
        r#"
struct Vector {
    x
    y
}

Vector:magnitude_sq() -> Int {
    return self.x * self.x + self.y * self.y
}

let v = Vector{ x = 3, y = 4 }
let m = v.magnitude_sq()
"#,
    );

    engine.register_module_declarations(&module);
    let _ = engine.run(&module).expect("run module with methods");

    assert_eq!(engine.vm().get_global("m"), Some(&Value::Int(25)));
}

#[test]
fn test_scope_escape_caught_by_publication_check() {
    let mut engine = AipoEngine::new();
    let setup = compile_script("var leaked = world\n");
    let after = compile_script("var smuggled = leaked\n");

    engine
        .scope(|scope| {
            let handle = scope.insert_userdata(GameWorld { score: 1, ticks: 1 });
            scope.expose_global("world", &handle);
            scope.run(&setup)?;
            Ok(())
        })
        .expect("scope run ok");

    // Post-scope: reading or publishing the leaked handle must fault with ScopeEscape
    let err = engine.run(&after).expect_err("escaped handle must fault");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_SCOPE_ESCAPE);
}

#[test]
fn test_scope_callback_fault_does_not_panic() {
    let mut engine = AipoEngine::new();

    fn failing_native(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
        Err(VmFault::CapabilityDenied {
            capability: "vault.secret".to_string(),
            operation: "read".to_string(),
        }
        .into())
    }

    let script = compile_script("let x = fail_now()");

    let err = engine
        .scope(|scope| {
            scope.register_host_fn("fail_now", 0, failing_native);
            scope.run(&script)?;
            Ok(())
        })
        .expect_err("callback fault must propagate");

    assert_eq!(
        err.diagnostic_code(),
        DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
    );
}
