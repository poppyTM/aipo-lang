//! Persistent-session regressions; not executed in P07-G02 at the user's request.
use aipo_cli::Session;
use aipo_vm::Value;
use std::path::Path;

fn eval(session: &mut Session, code: &str) -> u8 {
    session.eval(
        code,
        Path::new("session.aipo"),
        &mut Vec::new(),
        &mut Vec::new(),
    )
}

#[test]
fn old_functions_keep_access_to_mutable_globals_across_units() {
    let mut session = Session::new();
    assert_eq!(
        eval(
            &mut session,
            "var count = 1\nfn next() {\n count += 1\n return count\n}\n"
        ),
        0
    );
    assert_eq!(eval(&mut session, "let result = next()\n"), 0);
    assert_eq!(session.globals()["result"], Value::Int(2));
    assert_eq!(eval(&mut session, "count += 5\nlet later = next()\n"), 0);
    assert_eq!(session.globals()["later"], Value::Int(8));
}

#[test]
fn failed_unit_rolls_back_an_existing_host_visible_list() {
    let mut session = Session::new();
    assert_eq!(eval(&mut session, "let items = [1]\n"), 0);
    let Value::List(alias) = session.globals()["items"].clone() else {
        panic!("list");
    };
    assert_eq!(eval(&mut session, "items.add(2)\nfail(\"rollback\")\n"), 1);
    assert_eq!(&alias.borrow()[..], &[Value::Int(1)]);
    assert_eq!(session.generation(), 1);
}

#[test]
fn failed_migration_restores_code_and_heap() {
    let mut session = Session::new();
    assert_eq!(
        eval(&mut session, "var count = 3\nfn value() { return 1 }\n"),
        0
    );
    let status = session.reload_with(
        "var count = 0\nfn value() { return 2 }\n",
        Path::new("session.aipo"),
        &mut Vec::new(),
        &mut Vec::new(),
        |globals| {
            globals.insert("count".into(), Value::Int(99));
            Err("migration refused".into())
        },
    );
    assert_eq!(status, 1);
    assert_eq!(session.globals()["count"], Value::Int(3));
    assert_eq!(eval(&mut session, "let answer = value()\n"), 0);
    assert_eq!(session.globals()["answer"], Value::Int(1));
}

#[test]
fn successful_reload_removes_obsolete_public_declarations() {
    let mut session = Session::new();
    assert_eq!(eval(&mut session, "fn removed() { return 1 }\n"), 0);
    assert_eq!(
        session.reload_with(
            "fn current() { return 2 }\n",
            Path::new("session.aipo"),
            &mut Vec::new(),
            &mut Vec::new(),
            |_| Ok(())
        ),
        0
    );
    assert!(!session.globals().contains_key("removed"));
    assert!(session.globals().contains_key("current"));
}

#[test]
fn task_handles_survive_new_units_and_failed_units() {
    let mut session = Session::new();
    assert_eq!(
        eval(
            &mut session,
            "async fn job() { return [42] }\nlet pending = job()\n"
        ),
        0
    );
    assert_eq!(eval(&mut session, "let first = await pending\n"), 0);
    assert_eq!(eval(&mut session, "first.add(99)\nfail(\"rollback\")\n"), 1);
    assert_eq!(eval(&mut session, "let again = await pending\n"), 0);
    let Value::List(items) = &session.globals()["again"] else {
        panic!("list");
    };
    assert_eq!(&items.borrow()[..], &[Value::Int(42)]);
}
