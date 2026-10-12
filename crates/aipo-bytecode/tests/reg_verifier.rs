//! Regression cases for register proofs; compile-only in the P07-G02 delivery.
use aipo_bytecode::{
    Constant, RegCompiledFunction, RegInstruction as I, RegOpCode as O, RegVerifier,
};

fn function(code: Vec<I>, registers: usize) -> RegCompiledFunction {
    RegCompiledFunction {
        name: "proof".into(),
        arity: 0,
        is_async: false,
        num_registers: registers,
        instructions: code,
        constants: vec![],
        contracts: vec![],
    }
}

#[test]
fn branch_join_requires_initialization_on_every_path() {
    let code = function(
        vec![
            I::encode_abc(O::LoadBool, 0, 1, 0),
            I::encode_asbx(O::JumpIfTrue, 0, 1),
            I::encode_asbx(O::LoadInt, 1, 42),
            I::encode_abc(O::Return, 1, 0, 0),
        ],
        2,
    );
    assert!(RegVerifier::verify_function(&code, 0).is_err());
}

#[test]
fn unreachable_operands_are_still_checked_before_effects() {
    let mut code = function(
        vec![
            I::encode_abc(O::LoadNil, 0, 0, 0),
            I::encode_abc(O::Return, 0, 0, 0),
            I::encode_abx(O::SetGlobal, 0, 0),
        ],
        1,
    );
    code.constants.push(Constant::Int(7));
    assert!(RegVerifier::verify_function(&code, 0).is_err());
}

#[test]
fn scope_and_argument_windows_must_be_valid() {
    for instructions in [
        vec![
            I::encode_abc(O::IterGuardEnd, 0, 0, 0),
            I::encode_abc(O::Return, 0, 0, 0),
        ],
        vec![
            I::encode_abc(O::PopHandler, 0, 0, 0),
            I::encode_abc(O::Return, 0, 0, 0),
        ],
        vec![
            I::encode_abc(O::Call, 0, 2, 1),
            I::encode_abc(O::Return, 0, 0, 0),
        ],
    ] {
        assert!(RegVerifier::verify_function(&function(instructions, 2), 0).is_err());
    }
}

#[test]
fn valid_activation_and_raw_host_registers_have_different_entry_facts() {
    let code = function(vec![I::encode_abc(O::Return, 0, 0, 0)], 1);
    assert!(RegVerifier::verify_function(&code, 0).is_err());
    assert!(RegVerifier::verify_raw(&code.instructions, &[], 0).is_ok());
}
