//! Counting-allocator leak canaries over the Aipo compiler, VM, and runtime lifecycle (AIPO-OMNI-001).
//!
//! Verifies that repetitive instantiation and destruction of VM engines, tasks,
//! closures, bytecode modules, and compilation pipelines operate in steady-state
//! with net-zero unbounded memory leakage.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

struct CountingAllocator;

static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);

// SAFETY: delegates every allocation/deallocation operation to `System` unchanged.
// The only addition is atomic accounting of live bytes, which allocates nothing.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size() as isize, Ordering::Relaxed);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(
                new_size as isize - layout.size() as isize,
                Ordering::Relaxed,
            );
        }
        ptr
    }
}

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

fn live_bytes() -> isize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

/// Number of iterations per scenario.
const ITERS: usize = 32;

/// Maximum allowable live-byte growth across all `ITERS` iterations after warmup.
/// Absorbs platform allocator fragmentation and amortized static/TLS tables.
const TOLERANCE: isize = 4096;

fn assert_steady_state(name: &str, mut scenario: impl FnMut()) {
    // Warm up caches, thread locals, and static tables
    scenario();
    scenario();

    let baseline = live_bytes();
    for _ in 0..ITERS {
        scenario();
    }
    let growth = live_bytes() - baseline;
    eprintln!(
        "{name}: baseline={baseline}, final={}, growth={growth}",
        live_bytes()
    );

    assert!(
        growth <= TOLERANCE,
        "{name}: live bytes grew by {growth} over {ITERS} iterations (~{}/iter) after warmup — memory leaked or retained per iteration",
        growth / (ITERS as isize)
    );
}

#[test]
fn test_embedding_lifecycle_is_steady_state() {
    // 1. Stack VM lifecycle churn: creation, execution of collections/strings, drop
    assert_steady_state("vm_churn: create VM, run collections, drop", || {
        let mut vm = aipo_vm::Vm::new();
        let mut registry = aipo_runtime::NativeRegistry::new();
        aipo_stdlib::register_stdlib(&mut vm, &mut registry);

        let script = r#"
var l = [1, 2, 3]
var d = { "a": 10, "b": 20 }
var s = "hello" + " world"
"#;
        let (_, module) =
            aipo_testkit::pipeline::compile_text("vm_churn.aipo", script).expect("compile script");
        vm.run(&module).expect("run script");
        drop(vm);
    });

    // 2. Closure and upvalue capture churn: closures capturing locals
    let script = r#"
fn make_counter() {
    var count = 0
    fn bump() {
        count += 1
        return count
    }
    return bump
}
let c = make_counter()
let val = c()
"#;
    let (_, module) =
        aipo_testkit::pipeline::compile_text("closure_churn.aipo", script).expect("compile script");
    assert_steady_state(
        "closure_churn: create closures, capture upvalues, drop",
        || {
            let mut vm = aipo_vm::Vm::new();
            vm.run(&module).expect("run script");
            drop(vm);
        },
    );

    // 3. Register VM lifecycle churn
    assert_steady_state("reg_vm_churn: compile to reg module, execute, drop", || {
        let source = aipo_source::Source::new(
            aipo_source::SourceId::next(),
            "reg_churn.aipo",
            r#"
struct Point {
    var x: Int,
    var y: Int,
}
var p = Point{ x: 10, y: 20 }
p.x += 5
"#,
        );
        let (program, _) = aipo_syntax::parse(&source);
        let hir = aipo_hir::lower(program);
        let ir = aipo_ir::lower_to_ir(&hir);
        let reg_emitter = aipo_bytecode::RegEmitter::new();
        let reg_module = reg_emitter.compile_module(&ir).expect("compile reg module");

        let mut reg_vm = aipo_vm::RegVm::new();
        reg_vm.run_module(&reg_module).expect("run reg module");
        drop(reg_vm);
        drop(reg_module);
    });

    // 4. Frontend analysis churn: parsing, lowering to HIR, semantic checking
    assert_steady_state("frontend_churn: parse, HIR lower, sema check, drop", || {
        let script = r#"
interface Printable {
    fn print(self) -> String
}

#!satisfies Printable
struct Item {
    name
    price
}

Item:print() -> String {
    return self.name
}
"#;
        let res = aipo_testkit::pipeline::check_text("frontend_churn.aipo", script);
        let _ = res.expect("check text");
    });

    // 5. Full compiler churn: source to verified bytecode module
    assert_steady_state("compiler_churn: source to verified bytecode", || {
        let script = r#"
fn fib(n: Int) -> Int {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}
let r = fib(5)
"#;
        let (_, module) = aipo_testkit::pipeline::compile_text("compiler_churn.aipo", script)
            .expect("compile bytecode");
        drop(module);
    });

    // 6. WebAssembly compilation churn
    assert_steady_state(
        "wasm_compile_churn: HIR to Wasm binary, disassemble",
        || {
            let source = aipo_source::Source::new(
                aipo_source::SourceId::next(),
                "wasm_churn.aipo",
                r#"
fn calc(a: Int, b: Int) -> Int {
    return a * b + 1
}
"#,
            );
            let (program, _) = aipo_syntax::parse(&source);
            let hir = aipo_hir::lower(program);
            let wasm_bytes = aipo_wasm::compile_hir(&hir).expect("compile hir to wasm");
            let wat = aipo_wasm::disassemble_wasm(&wasm_bytes).expect("disassemble wasm");
            drop(wat);
            drop(wasm_bytes);
        },
    );

    // 7. Generational HandleTable churn
    assert_steady_state(
        "handle_table_churn: generational handles allocate and clear",
        || {
            let mut table = aipo_host::HandleTable::<u64>::new();
            for i in 0..100 {
                let _handle = table.insert(i as u64);
            }
            table.clear();
            drop(table);
        },
    );

    // 8. Scoped Engine lifecycle churn (AIPO-OMNI-003)
    let source = aipo_source::Source::new(
        aipo_source::SourceId::next(),
        "scope_churn.aipo",
        "let res = tick(world)\n",
    );
    let (program, _) = aipo_syntax::parse(&source);
    let hir = aipo_hir::lower(program);
    let ir = aipo_ir::lower_to_ir(&hir);
    let module = aipo_bytecode::compile(&ir).expect("compile bytecode");

    struct ScopedState {
        val: i64,
    }

    fn tick_fn(
        vm: &mut aipo_vm::Vm,
        args: &[aipo_vm::Value],
    ) -> Result<aipo_vm::Value, aipo_vm::VmError> {
        let handle = match args.first() {
            Some(aipo_vm::Value::HostHandle(h)) => *h,
            _ => {
                return Err(aipo_vm::VmFault::TypeMismatch {
                    expected: "HostHandle".to_string(),
                    actual: "none".to_string(),
                }
                .into());
            }
        };
        let state = vm
            .host_context()
            .resolve_userdata_mut::<ScopedState>(handle)?;
        state.val += 1;
        Ok(aipo_vm::Value::Int(state.val))
    }

    assert_steady_state(
        "engine_scope_churn: engine, scope userdata, native call, drop",
        || {
            let mut engine = aipo_vm::AipoEngine::new();
            engine
                .scope(|scope| {
                    let handle = scope.insert_userdata(ScopedState { val: 42 });
                    scope.expose_global("world", &handle);
                    scope.register_host_fn("tick", 1, tick_fn);
                    scope.run(&module)?;
                    Ok(())
                })
                .expect("run scope");
            drop(engine);
        },
    );
}
