//! Reproducible scalar-index workload. Prints CSV; no benchmark ran during reconstruction.
use aipo_bytecode::{RegInstruction, RegOpCode};
use aipo_vm::{RegVm, Value};
use std::hint::black_box;
use std::rc::Rc;
use std::time::Instant;

fn main() {
    let iterations: u64 = std::env::args()
        .nth(1)
        .map(|arg| arg.parse().expect("iteration count"))
        .unwrap_or(100_000);
    let code = [
        RegInstruction::encode_abc(RegOpCode::GetIndex, 0, 1, 2),
        RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
    ];
    println!("workload,iterations,elapsed_ns,checksum");
    for (name, text, index) in [
        ("ascii_middle", "a".repeat(4096), 2048),
        ("unicode_first", "🦀é".repeat(2048), 0),
        ("unicode_middle", "🦀é".repeat(2048), 2048),
        ("unicode_last", "🦀é".repeat(2048), -1),
    ] {
        let mut vm = RegVm::new();
        vm.registers[1] = Value::String(Rc::new(text));
        vm.registers[2] = Value::Int(index);
        for _ in 0..1000 {
            black_box(vm.run(black_box(&code)).expect("valid index"));
        }
        let start = Instant::now();
        let mut checksum = 0u64;
        for _ in 0..iterations {
            let value = vm.run(black_box(&code)).expect("valid index");
            let Value::String(text) = black_box(value) else {
                panic!("expected scalar string")
            };
            checksum += text.len() as u64;
        }
        println!(
            "{name},{iterations},{},{checksum}",
            start.elapsed().as_nanos()
        );
    }
}
