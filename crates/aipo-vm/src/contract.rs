//! The signature-contract predicate both engines evaluate.
//!
//! The stack VM and the register engine must answer "does this value satisfy `T`?"
//! identically — a contract is part of the language, not of an engine. The
//! predicate was written twice, once per engine, character for character; keeping
//! one copy is what guarantees a fix reaches both.

use crate::convert::TypeTag;
use crate::value::Value;
use std::collections::HashMap;

/// Struct layouts as the engines record them: field name and fixed-ness per type.
///
/// Declared parent-first, so a dotted name (`Enum.Variant`) identifies a variant of
/// a declared parent.
pub type StructDefs = HashMap<String, Vec<(String, bool)>>;

/// Returns whether `value` satisfies a written signature contract.
///
/// `T?` also accepts `none`. A core-type name is checked against its [`TypeTag`]; a
/// name the running module declares as a struct is checked against the instance's
/// type. A name the MVP cannot resolve at runtime (an interface) is accepted,
/// because inventing a failure for a contract the runtime cannot evaluate would be
/// worse than the missing check.
#[must_use]
pub fn contract_holds(
    struct_defs: &StructDefs,
    type_name: &str,
    nullable: bool,
    value: &Value,
) -> bool {
    if matches!(value, Value::None) {
        return nullable;
    }
    if type_name == "Function" {
        return matches!(
            value,
            Value::Function { .. }
                | Value::Closure(_)
                | Value::Native(_)
                | Value::BoundMethod(_)
                | Value::StructMethod(_)
        );
    }
    if let Some(tag) = TypeTag::from_name(type_name) {
        return tag.matches(value);
    }
    if struct_defs.contains_key(type_name)
        || struct_defs.keys().any(|k| {
            k.split_once('.')
                .is_some_and(|(parent, _)| parent == type_name)
        })
    {
        return match value {
            Value::Struct(instance) => {
                let actual = &instance.borrow().type_name;
                actual == type_name
                    || actual
                        .split_once('.')
                        .is_some_and(|(parent, _)| parent == type_name)
            }
            _ => false,
        };
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn defs(types: &[(&str, &[&str])]) -> StructDefs {
        types
            .iter()
            .map(|(name, fields)| {
                (
                    (*name).to_string(),
                    fields.iter().map(|f| ((*f).to_string(), false)).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn nullable_accepts_none_and_rejects_it_when_not_nullable() {
        let defs = defs(&[]);
        assert!(contract_holds(&defs, "Int", true, &Value::None));
        assert!(!contract_holds(&defs, "Int", false, &Value::None));
    }

    #[test]
    fn core_tag_matches_its_own_kind_only() {
        let defs = defs(&[]);
        assert!(contract_holds(&defs, "Int", false, &Value::Int(1)));
        assert!(!contract_holds(&defs, "Int", false, &Value::Bool(true)));
        assert!(contract_holds(
            &defs,
            "String",
            false,
            &Value::String(Rc::new("x".to_string()))
        ));
    }

    fn instance(
        type_name: &str,
        fields: Vec<(String, Value)>,
    ) -> Rc<RefCell<crate::value::StructInstance>> {
        Rc::new(RefCell::new(crate::value::StructInstance {
            type_name: type_name.to_string(),
            fields,
            fixed_fields: Default::default(),
            under_construction: false,
        }))
    }

    #[test]
    fn declared_struct_matches_by_name_and_variant_parent() {
        let defs = defs(&[("Point", &["x", "y"]), ("Enemy.Slime", &[])]);
        let point = instance("Point", vec![]);
        assert!(contract_holds(&defs, "Point", false, &Value::Struct(point)));

        let slime = instance("Enemy.Slime", Vec::new());
        // A variant satisfies its parent's contract and its own.
        assert!(contract_holds(
            &defs,
            "Enemy.Slime",
            false,
            &Value::Struct(slime.clone())
        ));
        assert!(contract_holds(&defs, "Enemy", false, &Value::Struct(slime)));
    }

    #[test]
    fn unknown_name_is_accepted_rather_than_invented_as_a_failure() {
        // An interface the runtime cannot resolve must not become a spurious fault.
        let defs = defs(&[]);
        assert!(contract_holds(
            &defs,
            "SomeUnresolvedInterface",
            false,
            &Value::Int(1)
        ));
    }
}
