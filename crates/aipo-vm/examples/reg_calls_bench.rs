//! Native register-call benchmark; compilation and assertions are not timed.
use aipo_bytecode::{
    Constant, RegCompiledFunction, RegCompiledModule, RegInstruction as I, RegOpCode as O,
};
use aipo_vm::RegVm;
use std::{collections::HashMap, hint::black_box, time::Instant};
fn main() {
    let calls = 20_000usize;
    let case = std::env::args().nth(1).unwrap_or_else(|| "int".into());
    let (load, constants) = match case.as_str() {
        "int" => (I::encode_asbx(O::LoadInt, 0, 42), vec![]),
        "string" => (
            I::encode_abx(O::LoadConst, 0, 0),
            vec![Constant::String("stable constant".repeat(20))],
        ),
        _ => panic!("usage: reg_calls_bench [int|string]"),
    };
    let function = RegCompiledFunction {
        name: "f".into(),
        arity: 0,
        is_async: false,
        num_registers: 1,
        instructions: vec![load, I::encode_abc(O::Return, 0, 0, 0)],
        constants,
        contracts: vec![],
    };
    let instructions = vec![
        I::encode_asbx(O::LoadInt, 1, 0),
        I::encode_asbx(O::LoadInt, 2, 1),
        I::encode_asbx(O::LoadInt, 3, calls as i32),
        I::encode_abx(O::MakeFunction, 0, 0),
        I::encode_abc(O::Call, 0, 0, 1),
        I::encode_abc(O::Add, 1, 1, 2),
        I::encode_abc(O::Less, 4, 1, 3),
        I::encode_asbx(O::JumpIfTrue, 4, -5),
        I::encode_abc(O::Return, 0, 0, 0),
    ];
    let module = RegCompiledModule {
        top_level: RegCompiledFunction {
            name: "main".into(),
            arity: 0,
            is_async: false,
            num_registers: 5,
            instructions,
            constants: vec![],
            contracts: vec![],
        },
        functions: vec![function],
        constants: vec![],
        struct_defs: HashMap::new(),
    };
    println!("sample,calls,elapsed_ns");
    let mut vm = RegVm::new();
    for sample in 0..9 {
        let start = Instant::now();
        black_box(vm.run_module(black_box(&module)).expect("valid workload"));
        println!("{sample},{calls},{}", start.elapsed().as_nanos());
    }
}
