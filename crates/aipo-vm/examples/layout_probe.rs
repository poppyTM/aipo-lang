//! Prints target-dependent runtime layouts; this is a measurement tool, not a guarantee.
use aipo_bytecode::RegInstruction;
use aipo_vm::{Value, reg_vm::RegCallFrame};

fn main() {
    println!("pointer_bits={}", usize::BITS);
    println!("value_bytes={}", std::mem::size_of::<Value>());
    println!(
        "instruction_bytes={}",
        std::mem::size_of::<RegInstruction>()
    );
    println!("frame_bytes={}", std::mem::size_of::<RegCallFrame>());
    println!(
        "register_file_bytes={}",
        std::mem::size_of::<[Value; 256]>()
    );
}
