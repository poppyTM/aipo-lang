//! Aliasing and collection regression cases; not executed in P07-G02.
use aipo_vm::{Collection, HeapSnapshot, Value};
use std::rc::Rc;

#[test]
fn structural_revision_detects_same_length_mutations_without_changing_equality() {
    let mut changed: Collection<_> = vec![Value::Int(1), Value::Int(2)].into();
    let original = changed.clone();
    changed.pop();
    changed.push(Value::Int(2));
    assert_ne!(changed.revision(), original.revision());
    assert_eq!(changed, original);
    let revision = changed.revision();
    changed[0] = Value::Int(3);
    assert_eq!(changed.revision(), revision);
}

#[test]
fn rollback_preserves_host_aliases_and_nested_guest_cycles() {
    let inner = Value::list(vec![Value::Int(1)]);
    let root = Value::list(vec![inner.clone()]);
    let Value::List(alias) = &inner else {
        panic!("list");
    };
    let allocation = Rc::as_ptr(alias);
    let snapshot = HeapSnapshot::capture([root]);
    alias.borrow_mut().push(inner.clone());
    alias.borrow_mut()[0] = Value::Int(99);
    snapshot.restore();
    assert_eq!(allocation, Rc::as_ptr(alias));
    assert_eq!(&alias.borrow()[..], &[Value::Int(1)]);
    assert_eq!(alias.borrow().revision(), 0);
}
