//! In-place rollback of guest heap objects reachable from global definitions.
use crate::value::{SeqOp, SequencePipeline, SequenceSource};
use crate::{Collection, DictMap, StructInstance, Value};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Clone, Debug)]
enum Saved {
    Values(Rc<RefCell<Collection<Value>>>, Collection<Value>),
    Bytes(Rc<RefCell<Collection<u8>>>, Collection<u8>),
    Dict(Rc<RefCell<DictMap>>, DictMap),
    Struct(Rc<RefCell<StructInstance>>, StructInstance),
    Cell(Rc<RefCell<Value>>, Value),
    Sequence(Rc<SequencePipeline>, Option<Vec<Value>>),
}
/// A graph snapshot preserving aliases and cycles. Host resources and native captures
/// remain owned by the host and are outside this guest-heap transaction.
#[derive(Clone, Debug, Default)]
pub struct HeapSnapshot {
    saved: Vec<Saved>,
}
impl HeapSnapshot {
    pub(crate) fn capture_cells(&mut self, cells: impl IntoIterator<Item = Rc<RefCell<Value>>>) {
        let mut unique = HashSet::new();
        let saved: Vec<_> = cells
            .into_iter()
            .filter(|cell| unique.insert(Rc::as_ptr(cell) as usize))
            .map(|cell| {
                let old = cell.borrow().clone();
                (cell, old)
            })
            .collect();
        self.saved
            .extend(Self::capture(saved.iter().map(|(_, old)| old.clone())).saved);
        self.saved
            .extend(saved.into_iter().map(|(cell, old)| Saved::Cell(cell, old)));
    }
    /// Captures every guest object reachable from the roots, once per allocation.
    pub fn capture(roots: impl IntoIterator<Item = Value>) -> Self {
        let mut pending: Vec<_> = roots.into_iter().collect();
        let mut seen = HashSet::new();
        let mut saved = Vec::new();
        while let Some(value) = pending.pop() {
            match value {
                Value::List(items) | Value::Set(items) => {
                    if seen.insert((0, Rc::as_ptr(&items) as usize)) {
                        let old = items.borrow().clone();
                        pending.extend(old.iter().cloned());
                        saved.push(Saved::Values(items, old));
                    }
                }
                Value::Bytes(items) => {
                    if seen.insert((1, Rc::as_ptr(&items) as usize)) {
                        let old = items.borrow().clone();
                        saved.push(Saved::Bytes(items, old));
                    }
                }
                Value::Dict(items) => {
                    if seen.insert((2, Rc::as_ptr(&items) as usize)) {
                        let old = items.borrow().clone();
                        pending.extend(
                            old.entries()
                                .iter()
                                .flat_map(|(key, value)| [key.clone(), value.clone()]),
                        );
                        saved.push(Saved::Dict(items, old));
                    }
                }
                Value::Struct(items) => {
                    if seen.insert((3, Rc::as_ptr(&items) as usize)) {
                        let old = items.borrow().clone();
                        pending.extend(old.fields.iter().map(|(_, value)| value.clone()));
                        saved.push(Saved::Struct(items, old));
                    }
                }
                Value::Closure(closure) => {
                    for cell in &closure.upvalues {
                        if seen.insert((4, Rc::as_ptr(cell) as usize)) {
                            let old = cell.borrow().clone();
                            pending.push(old.clone());
                            saved.push(Saved::Cell(cell.clone(), old));
                        }
                    }
                }
                Value::BoundMethod(method) => pending.push(method.receiver.clone()),
                Value::StructMethod(method) => pending.push(Value::Struct(method.receiver.clone())),
                Value::Failure(failure) => pending.push(failure.payload.clone()),
                Value::Sequence(sequence) => {
                    if !seen.insert((5, Rc::as_ptr(&sequence) as usize)) {
                        continue;
                    }
                    let old = sequence.cached.borrow().clone();
                    if let Some(values) = &old {
                        pending.extend(values.iter().cloned());
                    }
                    match &sequence.source {
                        SequenceSource::List(values)
                        | SequenceSource::Set(values)
                        | SequenceSource::Dict(values) => pending.extend(values.iter().cloned()),
                        _ => {}
                    }
                    for op in &sequence.ops {
                        match op {
                            SeqOp::Filter(v)
                            | SeqOp::Map(v)
                            | SeqOp::Find(v)
                            | SeqOp::FlatMap(v)
                            | SeqOp::Any(v)
                            | SeqOp::All(v)
                            | SeqOp::GroupBy(v) => pending.push(v.clone()),
                            SeqOp::Reduce { initial, func } => {
                                pending.extend([initial.clone(), func.clone()])
                            }
                            SeqOp::Count(Some(v)) => pending.push(v.clone()),
                            SeqOp::Zip(v) | SeqOp::Chain(v) => pending.extend(v.iter().cloned()),
                            _ => {}
                        }
                    }
                    saved.push(Saved::Sequence(sequence, old));
                }
                _ => {}
            }
        }
        Self { saved }
    }
    /// Checks that existing instances can use the newly installed field layouts.
    pub fn validate_layouts(
        &self,
        layouts: &std::collections::HashMap<String, Vec<(String, bool)>>,
    ) -> Result<(), String> {
        for saved in &self.saved {
            if let Saved::Struct(_, instance) = saved {
                let Some(fields) = layouts.get(&instance.type_name) else {
                    return Err(format!(
                        "retained instance of removed struct `{}` requires migration",
                        instance.type_name
                    ));
                };
                if fields.len() == instance.fields.len()
                    && fields.iter().zip(&instance.fields).all(
                        |((expected, fixed), (actual, _))| {
                            expected == actual && *fixed == instance.fixed_fields.contains(actual)
                        },
                    )
                {
                    continue;
                }
                return Err(format!(
                    "reload requires a migration for struct `{}`",
                    instance.type_name
                ));
            }
        }
        Ok(())
    }
    /// Restores original allocations, keeping references held by the host valid.
    pub fn restore(self) {
        for saved in self.saved {
            match saved {
                Saved::Values(value, old) => *value.borrow_mut() = old,
                Saved::Bytes(value, old) => *value.borrow_mut() = old,
                Saved::Dict(value, old) => *value.borrow_mut() = old,
                Saved::Struct(value, old) => *value.borrow_mut() = old,
                Saved::Cell(value, old) => *value.borrow_mut() = old,
                Saved::Sequence(value, old) => *value.cached.borrow_mut() = old,
            }
        }
    }
}
