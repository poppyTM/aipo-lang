//! C ABI exports for Aipo (ADP-009).

use crate::runtime::AipoRuntime;
use crate::types::{
    aipo_handle_t, aipo_host_fn_t, aipo_reg_vm_t, aipo_runtime_t, aipo_status_t, aipo_val_tag_t,
    aipo_value_t,
};
use aipo_vm::host_value_to_value;
use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Major version component (0 for v0.2.0).
pub const AIPO_VERSION_MAJOR: u32 = 0;
/// Minor version component (1 before 0.2.0, 2 from 0.2.0).
///
/// 0.2.0 replaces the pooled-string lifetime with runtime-owned snapshots released
/// by [`aipo_value_release`], and refuses re-entrant calls from host callbacks.
pub const AIPO_VERSION_MINOR: u32 = 2;
/// Patch version component (0 for v0.2.0).
pub const AIPO_VERSION_PATCH: u32 = 0;

/// Returns the runtime version.
///
/// # Safety
///
/// If non-null, `major`, `minor`, and `patch` must point to valid, aligned writable `u32` locations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_version(major: *mut u32, minor: *mut u32, patch: *mut u32) {
    if !major.is_null() {
        unsafe {
            *major = AIPO_VERSION_MAJOR;
        }
    }
    if !minor.is_null() {
        unsafe {
            *minor = AIPO_VERSION_MINOR;
        }
    }
    if !patch.is_null() {
        unsafe {
            *patch = AIPO_VERSION_PATCH;
        }
    }
}

/// Creates a new opaque Aipo runtime instance.
///
/// Returns null if memory allocation fails.
#[unsafe(no_mangle)]
pub extern "C" fn aipo_runtime_create() -> *mut aipo_runtime_t {
    let result = catch_unwind(AssertUnwindSafe(|| {
        Box::into_raw(Box::new(AipoRuntime::new()))
    }));
    result.unwrap_or(std::ptr::null_mut())
}

/// Destroys an Aipo runtime instance and releases all associated memory.
///
/// # Safety
///
/// If non-null, `rt` must be a valid pointer returned by [`aipo_runtime_create`] that has
/// not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_destroy(rt: *mut aipo_runtime_t) {
    if rt.is_null() {
        return;
    }
    // Destroying the runtime from inside a host callback would free the VM while
    // the dispatcher is still holding it borrowed.
    if crate::runtime::in_host_callback() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(rt));
    }));
}

/// Grants a host capability to the runtime.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`] and `capability` must point to a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_grant_capability(
    rt: *mut aipo_runtime_t,
    capability: *const c_char,
) -> aipo_status_t {
    if rt.is_null() || capability.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let c_str = unsafe { std::ffi::CStr::from_ptr(capability) };
        let cap_name = match c_str.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in capability name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        match runtime.grant(cap_name) {
            Ok(()) => aipo_status_t::AIPO_OK,
            Err(e) => {
                runtime.last_error = Some(e);
                aipo_status_t::AIPO_ERR_USAGE
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Revokes a host capability from the runtime.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`] and `capability` must point to a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_revoke_capability(
    rt: *mut aipo_runtime_t,
    capability: *const c_char,
) -> aipo_status_t {
    if rt.is_null() || capability.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let c_str = unsafe { std::ffi::CStr::from_ptr(capability) };
        let cap_name = match c_str.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in capability name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        match runtime.revoke(cap_name) {
            Ok(()) => aipo_status_t::AIPO_OK,
            Err(e) => {
                runtime.last_error = Some(e);
                aipo_status_t::AIPO_ERR_USAGE
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Compiles and initializes an Aipo module from UTF-8 source code.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`]. `name` and `source` must point to valid null-terminated UTF-8 C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_load_module(
    rt: *mut aipo_runtime_t,
    name: *const c_char,
    source: *const c_char,
) -> aipo_status_t {
    if rt.is_null() || name.is_null() || source.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let name_str = match unsafe { std::ffi::CStr::from_ptr(name) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in module name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        let source_str = match unsafe { std::ffi::CStr::from_ptr(source) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in source text: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        match runtime.load_module(name_str, source_str) {
            Ok(()) => aipo_status_t::AIPO_OK,
            Err((status, msg)) => {
                runtime.last_error = Some(msg);
                status
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Calls an Aipo function in a loaded module.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`]. `module_name` and `func_name` must point to valid null-terminated C strings.
/// If `argc > 0`, `args` must point to an array of at least `argc` initialized [`aipo_value_t`] elements.
/// If non-null, `out_result` must point to a valid writable [`aipo_value_t`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_call(
    rt: *mut aipo_runtime_t,
    module_name: *const c_char,
    func_name: *const c_char,
    args: *const aipo_value_t,
    argc: usize,
    out_result: *mut aipo_value_t,
) -> aipo_status_t {
    if rt.is_null() || module_name.is_null() || func_name.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let mod_str = match unsafe { std::ffi::CStr::from_ptr(module_name) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in module name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        let fn_str = match unsafe { std::ffi::CStr::from_ptr(func_name) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in function name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };

        if argc > 0 && args.is_null() {
            runtime.last_error = Some(format!(
                "argc is {argc} but args is NULL; the argument array is required"
            ));
            return aipo_status_t::AIPO_ERR_NULL_POINTER;
        }

        let arg_slice = if argc == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(args, argc) }
        };

        match runtime.call(mod_str, fn_str, arg_slice) {
            Ok(res) => {
                if !out_result.is_null() {
                    unsafe {
                        *out_result = res;
                    }
                }
                aipo_status_t::AIPO_OK
            }
            Err((status, msg)) => {
                runtime.last_error = Some(msg);
                status
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Registers a host native function in the runtime.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`]. `name` must point to a valid null-terminated C string.
/// `required_capability` (if non-null) must point to a valid null-terminated C string.
/// `callback` must be a valid function pointer with the expected C signature.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_register_host_fn(
    rt: *mut aipo_runtime_t,
    name: *const c_char,
    arity: usize,
    required_capability: *const c_char,
    callback: aipo_host_fn_t,
) -> aipo_status_t {
    if rt.is_null() || name.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let fn_name = match unsafe { std::ffi::CStr::from_ptr(name) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in host fn name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        let req_cap = if required_capability.is_null() {
            None
        } else {
            match unsafe { std::ffi::CStr::from_ptr(required_capability) }.to_str() {
                Ok(s) => Some(s),
                Err(e) => {
                    runtime.last_error = Some(format!("invalid UTF-8 in required capability: {e}"));
                    return aipo_status_t::AIPO_ERR_USAGE;
                }
            }
        };
        runtime.register_host_fn(fn_name, arity, req_cap, callback);
        aipo_status_t::AIPO_OK
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Creates a generational handle for a host value, reporting conversion failures.
///
/// This is the checked form of [`aipo_handle_create`]. A value that violates a
/// host contract yields an error and no handle; the legacy entrypoint below is
/// retained for source compatibility and is defined in terms of this one.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`]. If `val` contains pointers
/// (`str_ptr` or `bytes_ptr`), they must be valid for the duration of the call.
/// If non-null, `out_handle` must point to a writable [`aipo_handle_t`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_handle_create_checked(
    rt: *mut aipo_runtime_t,
    val: aipo_value_t,
    out_handle: *mut aipo_handle_t,
) -> aipo_status_t {
    if rt.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        match val.to_host_value() {
            Ok(host_val) => {
                let handle = runtime.handle_create(host_val);
                if !out_handle.is_null() {
                    unsafe {
                        *out_handle = handle;
                    }
                }
                aipo_status_t::AIPO_OK
            }
            Err(e) => {
                // No handle is created: a rejected value must not become a
                // valid handle to `none`.
                runtime.last_error = Some(format!("cannot handle this value: {e}"));
                aipo_status_t::AIPO_ERR_USAGE
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Creates a generational handle, returning a default handle on failure.
///
/// Prefer [`aipo_handle_create_checked`], which reports why the value was
/// rejected. This wrapper cannot distinguish success from failure, so it is kept
/// only so existing 0.1.x hosts keep compiling.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`]. If `val` contains pointers,
/// they must be valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_handle_create(
    rt: *mut aipo_runtime_t,
    val: aipo_value_t,
) -> aipo_handle_t {
    let mut handle = aipo_handle_t::default();
    let _ = unsafe { aipo_handle_create_checked(rt, val, &mut handle) };
    handle
}

/// Releases the runtime-owned snapshot backing a returned value.
///
/// Strings and bytes crossing the boundary are copied into a runtime-owned
/// snapshot. A value with `AIPO_VAL_STRING`, `AIPO_VAL_BYTES` or
/// `AIPO_VAL_FAILURE` must be released once the host is done reading it. Calling
/// this on a value with no heap payload is a no-op, and calling it twice is
/// harmless: the snapshot is gone and the pointer is already invalid.
///
/// # Safety
///
/// `val` must be a value that was returned by this runtime and not yet released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_value_release(rt: *mut aipo_runtime_t, val: aipo_value_t) {
    if rt.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        match val.tag {
            aipo_val_tag_t::AIPO_VAL_STRING | aipo_val_tag_t::AIPO_VAL_FAILURE => {
                runtime.release_snapshot_at(val.str_ptr.cast::<u8>());
            }
            aipo_val_tag_t::AIPO_VAL_BYTES => {
                runtime.release_snapshot_at(val.bytes_ptr);
            }
            _ => {}
        }
    }));
}

/// Resolves a generational handle to its host value.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`] and `out_value` must point to a valid writable [`aipo_value_t`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_handle_resolve(
    rt: *mut aipo_runtime_t,
    handle: aipo_handle_t,
    out_value: *mut aipo_value_t,
) -> aipo_status_t {
    if rt.is_null() || out_value.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        match runtime.handle_resolve(handle) {
            Ok(val) => {
                let vm_val = match host_value_to_value(&val) {
                    Ok(v) => v,
                    Err(_) => return aipo_status_t::AIPO_ERR_FAULT,
                };
                let c_val = aipo_value_t::from_vm_value(&vm_val, runtime.snapshotter());
                unsafe {
                    *out_value = c_val;
                }
                aipo_status_t::AIPO_OK
            }
            Err(_) => aipo_status_t::AIPO_ERR_STALE_HANDLE,
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Releases a generational handle, invalidating it forever.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_handle_release(
    rt: *mut aipo_runtime_t,
    handle: aipo_handle_t,
) -> aipo_status_t {
    if rt.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        match runtime.handle_release(handle) {
            Ok(()) => aipo_status_t::AIPO_OK,
            Err(_) => aipo_status_t::AIPO_ERR_STALE_HANDLE,
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Copies the last error message into the provided buffer.
///
/// Returns the number of bytes written, including null terminator.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`] and `buffer` must point to a valid writable buffer of at least `buffer_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_last_error(
    rt: *const aipo_runtime_t,
    buffer: *mut c_char,
    buffer_len: usize,
) -> usize {
    if rt.is_null() || buffer.is_null() || buffer_len == 0 {
        return 0;
    }
    let runtime = unsafe { &*rt };
    let msg = match &runtime.last_error {
        Some(s) => s.as_bytes(),
        None => b"no error",
    };
    let copy_len = (buffer_len - 1).min(msg.len());
    unsafe {
        std::ptr::copy_nonoverlapping(msg.as_ptr().cast::<c_char>(), buffer, copy_len);
        *buffer.add(copy_len) = 0;
    }
    copy_len + 1
}

/// Configures the maximum instruction execution budget for the runtime (0 = unlimited).
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_set_instruction_budget(
    rt: *mut aipo_runtime_t,
    max_instructions: u64,
) -> aipo_status_t {
    if rt.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let runtime = unsafe { &mut *rt };
    let limit = if max_instructions == 0 {
        None
    } else {
        Some(max_instructions)
    };
    runtime.set_instruction_budget(limit);
    aipo_status_t::AIPO_OK
}

/// Returns the total number of instructions executed on the runtime.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_instruction_count(rt: *const aipo_runtime_t) -> u64 {
    if rt.is_null() {
        return 0;
    }
    let runtime = unsafe { &*rt };
    runtime.instruction_count()
}

/// Resets the instruction execution counter to zero.
///
/// # Safety
///
/// `rt` must point to a live [`aipo_runtime_t`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_reset_instruction_count(
    rt: *mut aipo_runtime_t,
) -> aipo_status_t {
    if rt.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let runtime = unsafe { &mut *rt };
    runtime.reset_instruction_count();
    aipo_status_t::AIPO_OK
}

struct RegHandle {
    vm: aipo_vm::RegVm,
    last_error: String,
}

/// Creates a new register execution handle. The handle is thread-confined.
#[unsafe(no_mangle)]
pub extern "C" fn aipo_reg_vm_create() -> *mut aipo_reg_vm_t {
    catch_unwind(AssertUnwindSafe(|| {
        Box::into_raw(Box::new(RegHandle {
            vm: aipo_vm::RegVm::new(),
            last_error: String::new(),
        }))
        .cast()
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// Destroys a register handle.
/// # Safety
/// `vm` must be null or a live handle created by `aipo_reg_vm_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_destroy(vm: *mut aipo_reg_vm_t) {
    if !vm.is_null() && !crate::runtime::in_host_callback() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            drop(unsafe { Box::from_raw(vm.cast::<RegHandle>()) })
        }));
    }
}

fn reg_operation(
    vm: *mut aipo_reg_vm_t,
    operation: impl FnOnce(&mut aipo_vm::RegVm) -> Result<(), String>,
) -> aipo_status_t {
    if vm.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let handle = unsafe { &mut *vm.cast::<RegHandle>() };
    match catch_unwind(AssertUnwindSafe(|| operation(&mut handle.vm))) {
        Ok(Ok(())) => {
            handle.last_error.clear();
            aipo_status_t::AIPO_OK
        }
        Ok(Err(error)) => {
            handle.last_error = error;
            aipo_status_t::AIPO_ERR_FAULT
        }
        Err(_) => {
            handle.last_error = "panic contained at register C boundary".into();
            aipo_status_t::AIPO_ERR_FAULT
        }
    }
}

/// Sets an Aipo-safe integer in a register.
/// # Safety
/// `vm` must be a live, exclusively owned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_set_reg_int(
    vm: *mut aipo_reg_vm_t,
    reg: u8,
    val: i64,
) -> aipo_status_t {
    reg_operation(vm, |runtime| {
        aipo_vm::value::check_safe_int(val).map_err(|error| error.to_string())?;
        runtime.registers[reg as usize] = aipo_vm::Value::Int(val);
        Ok(())
    })
}

/// Reads an integer register.
/// # Safety
/// `vm` must be live and `out_val` must point to writable aligned `i64` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_get_reg_int(
    vm: *const aipo_reg_vm_t,
    reg: u8,
    out_val: *mut i64,
) -> aipo_status_t {
    if vm.is_null() || out_val.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let handle = unsafe { &*vm.cast::<RegHandle>() };
        if let aipo_vm::Value::Int(value) = handle.vm.registers[reg as usize] {
            unsafe {
                *out_val = value;
            }
            aipo_status_t::AIPO_OK
        } else {
            aipo_status_t::AIPO_ERR_FAULT
        }
    }))
    .unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Sets a cumulative instruction limit; zero disables it and resets consumption.
/// # Safety
/// `vm` must be a live, exclusively owned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_set_instruction_budget(
    vm: *mut aipo_reg_vm_t,
    limit: u64,
) -> aipo_status_t {
    reg_operation(vm, |runtime| {
        runtime.set_instruction_budget((limit != 0).then_some(limit));
        Ok(())
    })
}

/// Reads cumulative bytecode consumption; returns zero for a null handle.
/// # Safety
/// A non-null `vm` must be live and must not be executing a callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_instruction_count(vm: *const aipo_reg_vm_t) -> u64 {
    if vm.is_null() || crate::runtime::in_host_callback() {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| unsafe {
        (&*vm.cast::<RegHandle>()).vm.instruction_count()
    }))
    .unwrap_or(0)
}

/// Resets consumption without changing the limit.
/// # Safety
/// `vm` must be a live, exclusively owned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_reset_instruction_count(
    vm: *mut aipo_reg_vm_t,
) -> aipo_status_t {
    reg_operation(vm, |runtime| {
        runtime.reset_instruction_count();
        Ok(())
    })
}

/// Copies the last diagnostic, including its NUL terminator; returns required bytes.
/// The copied prefix is NUL-terminated when capacity is nonzero. Query with null/zero.
/// # Safety
/// `vm` must be live; `buffer` must have `capacity` writable bytes when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_copy_last_error(
    vm: *const aipo_reg_vm_t,
    buffer: *mut std::ffi::c_char,
    capacity: usize,
) -> usize {
    if vm.is_null() || crate::runtime::in_host_callback() {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let bytes = unsafe { (&*vm.cast::<RegHandle>()).last_error.as_bytes() };
        if !buffer.is_null() && capacity > 0 {
            let count = bytes.len().min(capacity - 1);
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast(), count);
                *buffer.add(count) = 0;
            }
        }
        bytes.len() + 1
    }))
    .unwrap_or(0)
}

/// Verifies then executes a buffer of 32-bit register instructions.
/// # Safety
/// `vm` must be exclusively owned. `instructions` must cover `count` aligned words.
/// A non-null `out_int` must point to writable aligned `i64` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_reg_vm_run(
    vm: *mut aipo_reg_vm_t,
    instructions: *const u32,
    count: usize,
    out_int: *mut i64,
) -> aipo_status_t {
    if instructions.is_null() && count > 0 {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if count > isize::MAX as usize / std::mem::size_of::<u32>() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    reg_operation(vm, |runtime| {
        let code = if count == 0 {
            &[]
        } else {
            unsafe {
                std::slice::from_raw_parts(
                    instructions.cast::<aipo_bytecode::RegInstruction>(),
                    count,
                )
            }
        };
        let result = runtime.run(code).map_err(|error| error.to_string())?;
        if !out_int.is_null() {
            let aipo_vm::Value::Int(value) = result else {
                return Err("register result is not Int".into());
            };
            unsafe {
                *out_int = value;
            }
        }
        Ok(())
    })
}

/// Begins a cooperative function call. Use `aipo_runtime_pump` to drive it.
/// # Safety
/// All pointers must be live; strings are NUL-terminated; `args` covers `argc` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_begin(
    rt: *mut aipo_runtime_t,
    module: *const c_char,
    function: *const c_char,
    args: *const aipo_value_t,
    argc: usize,
) -> aipo_status_t {
    if rt.is_null() || module.is_null() || function.is_null() || (argc > 0 && args.is_null()) {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() || argc > 65535 {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let names = unsafe {
            (
                std::ffi::CStr::from_ptr(module).to_str(),
                std::ffi::CStr::from_ptr(function).to_str(),
            )
        };
        let (Ok(module), Ok(function)) = names else {
            return aipo_status_t::AIPO_ERR_USAGE;
        };
        let values = if argc == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(args, argc) }
        };
        match runtime.begin(module, function, values) {
            Ok(()) => aipo_status_t::AIPO_OK,
            Err((status, message)) => {
                runtime.last_error = Some(message);
                status
            }
        }
    }))
    .unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Drives bounded scheduler quanta; writes a value only when `completed` is true.
/// Release returned heap snapshots with `aipo_value_release`.
/// # Safety
/// Runtime must be exclusively owned; outputs must be valid writable aligned storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_pump(
    rt: *mut aipo_runtime_t,
    steps: usize,
    completed: *mut bool,
    out_value: *mut aipo_value_t,
) -> aipo_status_t {
    if rt.is_null() || completed.is_null() || out_value.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    unsafe {
        *completed = false;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        match runtime.pump(steps) {
            Ok(Some(value)) => {
                unsafe {
                    *out_value = value;
                    *completed = true;
                }
                aipo_status_t::AIPO_OK
            }
            Ok(None) => aipo_status_t::AIPO_OK,
            Err((status, message)) => {
                runtime.last_error = Some(message);
                status
            }
        }
    }));
    match result {
        Ok(status) => status,
        Err(_) => {
            let runtime = unsafe { &mut *rt };
            runtime.abort();
            runtime.last_error = Some("panic contained during cooperative execution".into());
            aipo_status_t::AIPO_ERR_FAULT
        }
    }
}

/// Stops cooperative execution, retaining current guest state and completed side effects.
/// # Safety
/// Runtime must be live and exclusively owned, outside a host callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_abort(rt: *mut aipo_runtime_t) -> aipo_status_t {
    if rt.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    catch_unwind(AssertUnwindSafe(|| {
        unsafe { &mut *rt }.abort();
        aipo_status_t::AIPO_OK
    }))
    .unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Loads and executes an already compiled Aipo bytecode module (.aibc).
///
/// # Safety
/// `rt` must point to a live [`aipo_runtime_t`]. `name` must point to a valid null-terminated C string.
/// `bytecode` must point to a buffer of at least `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_load_bytecode(
    rt: *mut aipo_runtime_t,
    name: *const c_char,
    bytecode: *const u8,
    len: usize,
) -> aipo_status_t {
    if rt.is_null() || name.is_null() || (bytecode.is_null() && len > 0) {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let name_str = match unsafe { std::ffi::CStr::from_ptr(name) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in module name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        let bytes = if len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytecode, len) }
        };
        match runtime.load_bytecode(name_str, bytes) {
            Ok(()) => aipo_status_t::AIPO_OK,
            Err((status, msg)) => {
                runtime.last_error = Some(msg);
                status
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Compiles Aipo source code to precompiled bytecode (.aibc).
///
/// On success, allocates a buffer containing bytecode bytes, sets `*out_bytes` and `*out_len`.
/// The caller must free this buffer using [`aipo_bytecode_free`].
///
/// # Safety
/// `source` must point to valid UTF-8 bytes of length `source_len` (or null-terminated if `source_len == 0`).
/// `out_bytes` and `out_len` must point to valid writable locations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_compile_to_bytecode(
    source: *const c_char,
    source_len: usize,
    out_bytes: *mut *mut u8,
    out_len: *mut usize,
) -> aipo_status_t {
    if source.is_null() || out_bytes.is_null() || out_len.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let src_str = if source_len > 0 {
            let slice = unsafe { std::slice::from_raw_parts(source.cast::<u8>(), source_len) };
            match std::str::from_utf8(slice) {
                Ok(s) => s,
                Err(_) => return aipo_status_t::AIPO_ERR_USAGE,
            }
        } else {
            match unsafe { std::ffi::CStr::from_ptr(source) }.to_str() {
                Ok(s) => s,
                Err(_) => return aipo_status_t::AIPO_ERR_USAGE,
            }
        };

        match AipoRuntime::compile_bytecode(src_str) {
            Ok(bytes) => {
                let len = bytes.len();
                let mut boxed = bytes.into_boxed_slice();
                let ptr = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                unsafe {
                    *out_bytes = ptr;
                    *out_len = len;
                }
                aipo_status_t::AIPO_OK
            }
            Err((status, _msg)) => status,
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Frees a bytecode buffer previously allocated by [`aipo_compile_to_bytecode`].
///
/// # Safety
/// If non-null, `bytes` must be a pointer returned by [`aipo_compile_to_bytecode`] with matching `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_bytecode_free(bytes: *mut u8, len: usize) {
    if bytes.is_null() || len == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(bytes, len);
        drop(unsafe { Box::from_raw(slice_ptr) });
    }));
}

/// Prepares a loaded module for execution or stepping from instruction 0.
///
/// # Safety
/// `rt` must point to a live [`aipo_runtime_t`]. `module_name` must point to a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_prepare_module(
    rt: *mut aipo_runtime_t,
    module_name: *const c_char,
) -> aipo_status_t {
    if rt.is_null() || module_name.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let name_str = match unsafe { std::ffi::CStr::from_ptr(module_name) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                runtime.last_error = Some(format!("invalid UTF-8 in module name: {e}"));
                return aipo_status_t::AIPO_ERR_USAGE;
            }
        };
        match runtime.prepare_module(name_str) {
            Ok(()) => aipo_status_t::AIPO_OK,
            Err((status, msg)) => {
                runtime.last_error = Some(msg);
                status
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Advances the execution of the specified (or last loaded) module by a single step.
///
/// # Safety
/// `rt` must point to a live [`aipo_runtime_t`]. If `module_name` is non-null, it must point to a valid null-terminated C string.
/// `out_done` must point to a valid writable `bool`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_step(
    rt: *mut aipo_runtime_t,
    module_name: *const c_char,
    out_done: *mut bool,
) -> aipo_status_t {
    if rt.is_null() || out_done.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let mod_opt = if module_name.is_null() {
            None
        } else {
            match unsafe { std::ffi::CStr::from_ptr(module_name) }.to_str() {
                Ok(s) => Some(s),
                Err(e) => {
                    runtime.last_error = Some(format!("invalid UTF-8 in module name: {e}"));
                    return aipo_status_t::AIPO_ERR_USAGE;
                }
            }
        };

        match runtime.step(mod_opt) {
            Ok(done) => {
                unsafe { *out_done = done };
                aipo_status_t::AIPO_OK
            }
            Err((status, msg)) => {
                runtime.last_error = Some(msg);
                status
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Executes instructions on the specified (or last loaded) module until it finishes or hits `max_steps`.
///
/// If `max_steps == 0`, execution proceeds without a step limit until completion or suspension.
///
/// # Safety
/// `rt` must point to a live [`aipo_runtime_t`]. If `module_name` is non-null, it must point to a valid null-terminated C string.
/// `out_done` must point to a valid writable `bool`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_runtime_run_until_idle(
    rt: *mut aipo_runtime_t,
    module_name: *const c_char,
    max_steps: u64,
    out_done: *mut bool,
) -> aipo_status_t {
    if rt.is_null() || out_done.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    if crate::runtime::in_host_callback() {
        return aipo_status_t::AIPO_ERR_USAGE;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { &mut *rt };
        let mod_opt = if module_name.is_null() {
            None
        } else {
            match unsafe { std::ffi::CStr::from_ptr(module_name) }.to_str() {
                Ok(s) => Some(s),
                Err(e) => {
                    runtime.last_error = Some(format!("invalid UTF-8 in module name: {e}"));
                    return aipo_status_t::AIPO_ERR_USAGE;
                }
            }
        };

        match runtime.run_until_idle(mod_opt, max_steps) {
            Ok(done) => {
                unsafe { *out_done = done };
                aipo_status_t::AIPO_OK
            }
            Err((status, msg)) => {
                runtime.last_error = Some(msg);
                status
            }
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Compiles Aipo source code to standalone WebAssembly binary (.wasm).
///
/// On success, allocates a buffer containing Wasm bytes, sets `*out_wasm` and `*out_len`.
/// The caller must free this buffer using [`aipo_wasm_free`].
///
/// # Safety
/// `source` must point to valid UTF-8 bytes of length `source_len` (or null-terminated if `source_len == 0`).
/// `out_wasm` and `out_len` must point to valid writable locations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_wasm_compile(
    source: *const c_char,
    source_len: usize,
    out_wasm: *mut *mut u8,
    out_len: *mut usize,
) -> aipo_status_t {
    if source.is_null() || out_wasm.is_null() || out_len.is_null() {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let src_str = if source_len > 0 {
            let slice = unsafe { std::slice::from_raw_parts(source.cast::<u8>(), source_len) };
            match std::str::from_utf8(slice) {
                Ok(s) => s,
                Err(_) => return aipo_status_t::AIPO_ERR_USAGE,
            }
        } else {
            match unsafe { std::ffi::CStr::from_ptr(source) }.to_str() {
                Ok(s) => s,
                Err(_) => return aipo_status_t::AIPO_ERR_USAGE,
            }
        };

        match AipoRuntime::compile_wasm(src_str) {
            Ok(bytes) => {
                let len = bytes.len();
                let mut boxed = bytes.into_boxed_slice();
                let ptr = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                unsafe {
                    *out_wasm = ptr;
                    *out_len = len;
                }
                aipo_status_t::AIPO_OK
            }
            Err((status, _msg)) => status,
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}

/// Frees a WebAssembly buffer previously allocated by [`aipo_wasm_compile`].
///
/// # Safety
/// If non-null, `wasm_bytes` must be a pointer returned by [`aipo_wasm_compile`] with matching `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_wasm_free(wasm_bytes: *mut u8, len: usize) {
    if wasm_bytes.is_null() || len == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(wasm_bytes, len);
        drop(unsafe { Box::from_raw(slice_ptr) });
    }));
}

/// Executes precompiled WebAssembly bytecode using Wasmtime JIT.
///
/// If `fuel > 0`, execution is bounded by that fuel budget. If `fuel == 0`, execution is unlimited.
/// On completion, sets `*out_result` if non-null.
///
/// # Safety
/// `wasm_bytes` must point to valid WebAssembly binary bytes of length `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aipo_wasm_run(
    wasm_bytes: *const u8,
    len: usize,
    fuel: u64,
    out_result: *mut i64,
) -> aipo_status_t {
    if wasm_bytes.is_null() || len == 0 {
        return aipo_status_t::AIPO_ERR_NULL_POINTER;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let slice = unsafe { std::slice::from_raw_parts(wasm_bytes, len) };
        let mut stdout = std::io::sink();
        let fuel_opt = if fuel > 0 { Some(fuel) } else { None };
        match aipo_wasm::execute_wasm_with_fuel(slice, &mut stdout, fuel_opt) {
            Ok(ret) => {
                if !out_result.is_null() {
                    unsafe { *out_result = ret };
                }
                aipo_status_t::AIPO_OK
            }
            Err(_) => aipo_status_t::AIPO_ERR_FAULT,
        }
    }));
    result.unwrap_or(aipo_status_t::AIPO_ERR_FAULT)
}
