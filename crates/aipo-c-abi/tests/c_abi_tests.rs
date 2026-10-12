//! Comprehensive automated integration tests for Aipo C ABI (ADP-009, ADP-010).

use aipo_c_abi::*;
use std::ffi::{CStr, CString};

#[test]
fn test_version_query() {
    let mut major = 0;
    let mut minor = 0;
    let mut patch = 0;
    unsafe {
        aipo_version(&mut major, &mut minor, &mut patch);
    }
    // Pinned on purpose rather than compared against the constants: the point of
    // this test is that a version bump is a deliberate, visible change. 0.2.0 is
    // the pooled-string lifetime replaced by runtime-owned snapshots. A consumer
    // that compiled against 0.1.0 must be able to detect the break, so the value
    // may not drift silently.
    assert_eq!((major, minor, patch), (0, 2, 0), "documented ABI version");

    // Null pointer resilience
    unsafe {
        aipo_version(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}

#[test]
fn test_runtime_lifecycle() {
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());
    unsafe {
        aipo_runtime_destroy(rt);
        // Double-destroy null resilience
        aipo_runtime_destroy(std::ptr::null_mut());
    }
}

#[test]
fn test_module_load_and_call_primitive_types() {
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());

    let mod_name = CString::new("math_mod").unwrap();
    let source = CString::new(
        r#"
fn add(a, b) {
    return a + b
}

fn concat(prefix, suffix) {
    return prefix + suffix
}

fn is_positive(n) {
    return n > 0
}
"#,
    )
    .unwrap();

    let status = unsafe { aipo_runtime_load_module(rt, mod_name.as_ptr(), source.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    // Call add(10, 25) -> 35
    let fn_add = CString::new("add").unwrap();
    let args = [aipo_value_t::int_val(10), aipo_value_t::int_val(25)];
    let mut result = aipo_value_t::none();

    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_add.as_ptr(),
            args.as_ptr(),
            args.len(),
            &mut result,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(result.tag, aipo_val_tag_t::AIPO_VAL_INT);
    assert_eq!(result.int_val, 35);

    // Call concat("hello, ", "world!")
    let fn_concat = CString::new("concat").unwrap();
    let str_a = CString::new("hello, ").unwrap();
    let str_b = CString::new("world!").unwrap();
    let mut val_a = aipo_value_t::none();
    val_a.tag = aipo_val_tag_t::AIPO_VAL_STRING;
    val_a.str_ptr = str_a.as_ptr();
    val_a.str_len = 7;

    let mut val_b = aipo_value_t::none();
    val_b.tag = aipo_val_tag_t::AIPO_VAL_STRING;
    val_b.str_ptr = str_b.as_ptr();
    val_b.str_len = 6;

    let args_str = [val_a, val_b];
    let mut res_str = aipo_value_t::none();

    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_concat.as_ptr(),
            args_str.as_ptr(),
            args_str.len(),
            &mut res_str,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(res_str.tag, aipo_val_tag_t::AIPO_VAL_STRING);
    assert!(!res_str.str_ptr.is_null());
    let res_text = unsafe { CStr::from_ptr(res_str.str_ptr) }.to_str().unwrap();
    assert_eq!(res_text, "hello, world!");

    // Call is_positive(-5) -> false
    let fn_pos = CString::new("is_positive").unwrap();
    let args_pos = [aipo_value_t::int_val(-5)];
    let mut res_bool = aipo_value_t::none();

    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_pos.as_ptr(),
            args_pos.as_ptr(),
            args_pos.len(),
            &mut res_bool,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(res_bool.tag, aipo_val_tag_t::AIPO_VAL_BOOL);
    assert!(!res_bool.bool_val);

    unsafe { aipo_runtime_destroy(rt) };
}

extern "C" fn host_multiply(
    _rt: *mut aipo_runtime_t,
    args: *const aipo_value_t,
    argc: usize,
    out_result: *mut aipo_value_t,
) -> aipo_status_t {
    if argc != 2 || args.is_null() || out_result.is_null() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let slice = unsafe { std::slice::from_raw_parts(args, argc) };
    let a = slice[0].int_val;
    let b = slice[1].int_val;
    unsafe {
        *out_result = aipo_value_t::int_val(a * b);
    }
    aipo_status_t::AIPO_OK
}

#[test]
fn test_host_function_registration_and_callback() {
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());

    let fn_name = CString::new("host_mul").unwrap();
    let status = unsafe {
        aipo_runtime_register_host_fn(rt, fn_name.as_ptr(), 2, std::ptr::null(), host_multiply)
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    let mod_name = CString::new("client_mod").unwrap();
    let source = CString::new(
        r#"
fn compute_area(width, height) {
    return host_mul(width, height)
}
"#,
    )
    .unwrap();

    let status = unsafe { aipo_runtime_load_module(rt, mod_name.as_ptr(), source.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    let fn_compute = CString::new("compute_area").unwrap();
    let args = [aipo_value_t::int_val(7), aipo_value_t::int_val(8)];
    let mut result = aipo_value_t::none();

    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_compute.as_ptr(),
            args.as_ptr(),
            args.len(),
            &mut result,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(result.tag, aipo_val_tag_t::AIPO_VAL_INT);
    assert_eq!(result.int_val, 56);

    unsafe { aipo_runtime_destroy(rt) };
}

#[test]
fn test_generational_handle_lifecycle_and_stale_detection() {
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());

    // Mint handle for integer payload 42
    let val = aipo_value_t::int_val(42);
    let handle = unsafe { aipo_handle_create(rt, val) };
    assert_ne!(handle.generation, 0);

    // Resolve handle while live
    let mut resolved = aipo_value_t::none();
    let status = unsafe { aipo_handle_resolve(rt, handle, &mut resolved) };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(resolved.tag, aipo_val_tag_t::AIPO_VAL_INT);
    assert_eq!(resolved.int_val, 42);

    // Release handle
    let status = unsafe { aipo_handle_release(rt, handle) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    // Attempting to resolve stale handle must fail with AIPO_ERR_STALE_HANDLE
    let status = unsafe { aipo_handle_resolve(rt, handle, &mut resolved) };
    assert_eq!(status, aipo_status_t::AIPO_ERR_STALE_HANDLE);

    // Attempting to release already released handle must fail with AIPO_ERR_STALE_HANDLE
    let status = unsafe { aipo_handle_release(rt, handle) };
    assert_eq!(status, aipo_status_t::AIPO_ERR_STALE_HANDLE);

    unsafe { aipo_runtime_destroy(rt) };
}

extern "C" fn secure_action(
    _rt: *mut aipo_runtime_t,
    _args: *const aipo_value_t,
    _argc: usize,
    out_result: *mut aipo_value_t,
) -> aipo_status_t {
    unsafe {
        *out_result = aipo_value_t::int_val(100);
    }
    aipo_status_t::AIPO_OK
}

#[test]
fn test_capability_guarding_and_denial() {
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());

    let fn_name = CString::new("secure_vault").unwrap();
    let cap_name = CString::new("system.vault").unwrap();

    let status = unsafe {
        aipo_runtime_register_host_fn(rt, fn_name.as_ptr(), 0, cap_name.as_ptr(), secure_action)
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    let mod_name = CString::new("vault_test").unwrap();
    let source = CString::new(
        r#"
fn access_vault() {
    return secure_vault()
}
"#,
    )
    .unwrap();

    let status = unsafe { aipo_runtime_load_module(rt, mod_name.as_ptr(), source.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    let fn_access = CString::new("access_vault").unwrap();
    let mut result = aipo_value_t::none();

    // 1. Without capability granted: must fault with CAPABILITY_DENIED
    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_access.as_ptr(),
            std::ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_ERR_FAULT);

    // 2. Grant capability and retry: must succeed with AIPO_OK
    let status = unsafe { aipo_runtime_grant_capability(rt, cap_name.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_access.as_ptr(),
            std::ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(result.tag, aipo_val_tag_t::AIPO_VAL_INT);
    assert_eq!(result.int_val, 100);

    // 3. Revoke capability and retry: must fault again
    let status = unsafe { aipo_runtime_revoke_capability(rt, cap_name.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_access.as_ptr(),
            std::ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_ERR_FAULT);

    unsafe { aipo_runtime_destroy(rt) };
}

#[test]
fn test_error_and_diagnostic_reporting() {
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());

    // 1. Diagnostic syntax error in module source
    let mod_name = CString::new("broken").unwrap();
    let bad_source = CString::new("fn syntax_error(return 42").unwrap();
    let status = unsafe { aipo_runtime_load_module(rt, mod_name.as_ptr(), bad_source.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_ERR_DIAGNOSTIC);

    let mut err_buf = [0u8; 128];
    let written = unsafe {
        aipo_last_error(
            rt,
            err_buf.as_mut_ptr().cast::<std::ffi::c_char>(),
            err_buf.len(),
        )
    };
    assert!(written > 0);
    let err_str = unsafe { CStr::from_ptr(err_buf.as_ptr().cast::<std::ffi::c_char>()) }
        .to_str()
        .unwrap();
    assert!(err_str.contains("syntax error") || err_str.contains("parse error"));

    // 2. Runtime fault: division by zero
    let ok_mod = CString::new("calc").unwrap();
    let ok_source = CString::new("fn div_zero() { return 10 / 0 }").unwrap();
    let status = unsafe { aipo_runtime_load_module(rt, ok_mod.as_ptr(), ok_source.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    let fn_div = CString::new("div_zero").unwrap();
    let mut res = aipo_value_t::none();
    let status = unsafe {
        aipo_runtime_call(
            rt,
            ok_mod.as_ptr(),
            fn_div.as_ptr(),
            std::ptr::null(),
            0,
            &mut res,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_ERR_FAULT);

    unsafe { aipo_runtime_destroy(rt) };
}

#[test]
fn test_null_pointer_safety() {
    // Calling C ABI functions with null pointers must return AIPO_ERR_NULL_POINTER without panic
    let status = unsafe { aipo_runtime_grant_capability(std::ptr::null_mut(), std::ptr::null()) };
    assert_eq!(status, aipo_status_t::AIPO_ERR_NULL_POINTER);

    let status = unsafe { aipo_runtime_revoke_capability(std::ptr::null_mut(), std::ptr::null()) };
    assert_eq!(status, aipo_status_t::AIPO_ERR_NULL_POINTER);

    let status = unsafe {
        aipo_runtime_load_module(std::ptr::null_mut(), std::ptr::null(), std::ptr::null())
    };
    assert_eq!(status, aipo_status_t::AIPO_ERR_NULL_POINTER);

    let status = unsafe {
        aipo_runtime_call(
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_ERR_NULL_POINTER);

    let status = unsafe {
        aipo_runtime_register_host_fn(
            std::ptr::null_mut(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            host_multiply,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_ERR_NULL_POINTER);

    let status = unsafe {
        aipo_handle_resolve(
            std::ptr::null_mut(),
            aipo_handle_t::default(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_ERR_NULL_POINTER);

    let status = unsafe { aipo_handle_release(std::ptr::null_mut(), aipo_handle_t::default()) };
    assert_eq!(status, aipo_status_t::AIPO_ERR_NULL_POINTER);

    let written = unsafe { aipo_last_error(std::ptr::null(), std::ptr::null_mut(), 0) };
    assert_eq!(written, 0);
}

#[test]
fn test_reg_vm_c_abi() {
    use aipo_bytecode::{RegInstruction, RegOpCode};

    let vm = aipo_reg_vm_create();
    assert!(!vm.is_null());

    unsafe {
        assert_eq!(aipo_reg_vm_set_reg_int(vm, 1, 100), aipo_status_t::AIPO_OK);
        assert_eq!(aipo_reg_vm_set_reg_int(vm, 2, 250), aipo_status_t::AIPO_OK);

        let mut read_back = 0i64;
        assert_eq!(
            aipo_reg_vm_get_reg_int(vm, 1, &mut read_back),
            aipo_status_t::AIPO_OK
        );
        assert_eq!(read_back, 100);

        let code = [
            RegInstruction::encode_abc(RegOpCode::Add, 0, 1, 2).0,
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0).0,
        ];

        let mut out_res = 0i64;
        let status = aipo_reg_vm_run(vm, code.as_ptr(), code.len(), &mut out_res);
        assert_eq!(status, aipo_status_t::AIPO_OK);
        assert_eq!(out_res, 350);

        aipo_reg_vm_destroy(vm);
    }
}

#[test]
fn test_bytecode_compilation_and_loading() {
    let source = CString::new(
        r#"
fn multiply(a, b) {
    return a * b
}
"#,
    )
    .unwrap();

    let mut bc_ptr: *mut u8 = std::ptr::null_mut();
    let mut bc_len: usize = 0;

    let status = unsafe {
        aipo_compile_to_bytecode(
            source.as_ptr(),
            0, // null-terminated
            &mut bc_ptr,
            &mut bc_len,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert!(!bc_ptr.is_null());
    assert!(bc_len > 4);

    // Verify AIBC magic
    let magic = unsafe { std::slice::from_raw_parts(bc_ptr, 4) };
    assert_eq!(magic, b"AIBC");

    // Load bytecode into runtime
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());

    let mod_name = CString::new("calc_mod").unwrap();
    let status = unsafe { aipo_runtime_load_bytecode(rt, mod_name.as_ptr(), bc_ptr, bc_len) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    // Call multiply(6, 7) -> 42
    let fn_name = CString::new("multiply").unwrap();
    let args = [aipo_value_t::int_val(6), aipo_value_t::int_val(7)];
    let mut result = aipo_value_t::none();

    let status = unsafe {
        aipo_runtime_call(
            rt,
            mod_name.as_ptr(),
            fn_name.as_ptr(),
            args.as_ptr(),
            args.len(),
            &mut result,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(result.tag, aipo_val_tag_t::AIPO_VAL_INT);
    assert_eq!(result.int_val, 42);

    unsafe {
        aipo_bytecode_free(bc_ptr, bc_len);
        aipo_runtime_destroy(rt);
    }
}

#[test]
fn test_stepping_and_run_until_idle() {
    let rt = aipo_runtime_create();
    assert!(!rt.is_null());

    let mod_name = CString::new("step_mod").unwrap();
    let source = CString::new(
        r#"
var x = 0
x += 10
x += 20
x += 30
"#,
    )
    .unwrap();

    let status = unsafe { aipo_runtime_load_module(rt, mod_name.as_ptr(), source.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    // Prepare module to step from instruction 0
    let status = unsafe { aipo_runtime_prepare_module(rt, mod_name.as_ptr()) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    // Single step
    let mut done = false;
    let status = unsafe { aipo_runtime_step(rt, mod_name.as_ptr(), &mut done) };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert!(!done, "first step should not complete the module");

    // Run until idle with small step limit
    let status = unsafe { aipo_runtime_run_until_idle(rt, mod_name.as_ptr(), 2, &mut done) };
    assert_eq!(status, aipo_status_t::AIPO_OK);

    // Run until idle with unlimited steps (0)
    let status = unsafe { aipo_runtime_run_until_idle(rt, mod_name.as_ptr(), 0, &mut done) };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert!(done, "module execution should complete");

    // Stepping when already done returns done == true
    let status = unsafe { aipo_runtime_step(rt, mod_name.as_ptr(), &mut done) };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert!(done);

    unsafe {
        aipo_runtime_destroy(rt);
    }
}

#[test]
fn test_wasm_compilation_and_execution() {
    let source = CString::new(
        r#"
fn sum_loop() -> Int {
    var acc = 0
    var i = 1
    while i <= 10 {
        acc += i
        i += 1
    }
    return acc
}
sum_loop()
"#,
    )
    .unwrap();

    let mut wasm_ptr: *mut u8 = std::ptr::null_mut();
    let mut wasm_len: usize = 0;

    let status = unsafe {
        aipo_wasm_compile(
            source.as_ptr(),
            0, // null-terminated
            &mut wasm_ptr,
            &mut wasm_len,
        )
    };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert!(!wasm_ptr.is_null());
    assert!(wasm_len > 4);

    // Verify Wasm binary magic \0asm
    let magic = unsafe { std::slice::from_raw_parts(wasm_ptr, 4) };
    assert_eq!(magic, b"\0asm");

    // Run without fuel limit
    let mut out_res: i64 = 0;
    let status = unsafe { aipo_wasm_run(wasm_ptr, wasm_len, 0, &mut out_res) };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(out_res, 55);

    // Run with sufficient fuel budget
    let status = unsafe { aipo_wasm_run(wasm_ptr, wasm_len, 100_000, &mut out_res) };
    assert_eq!(status, aipo_status_t::AIPO_OK);
    assert_eq!(out_res, 55);

    // Run with insufficient fuel budget (1 fuel unit traps / faults)
    let status = unsafe { aipo_wasm_run(wasm_ptr, wasm_len, 1, &mut out_res) };
    assert_eq!(status, aipo_status_t::AIPO_ERR_FAULT);

    unsafe {
        aipo_wasm_free(wasm_ptr, wasm_len);
    }
}
