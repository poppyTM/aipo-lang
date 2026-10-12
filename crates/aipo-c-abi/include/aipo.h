/**
 * \file aipo.h
 * \brief C ABI and embedding interface for the Aipo programming language (ADP-008, ADP-009).
 *
 * This header defines the stable, versioned, synchronous, single-threaded C ABI for
 * embedding Aipo into host environments (C, C++, Python ctypes, Go cgo, etc.).
 *
 * Memory Ownership & Thread Safety:
 * - The runtime is single-threaded and not thread-safe. Calls to a runtime instance
 *   must be externally synchronized if invoked across threads.
 * - String and bytes pointers returned in `aipo_value_t` from module execution, function
 *   calls, or handle resolution are runtime-owned snapshots. They remain valid until
 *   explicitly freed with `aipo_value_release` or until `aipo_runtime_destroy` is called.
 * - Generational handles (`aipo_handle_t`) are opaque references validated against
 *   slot index and generation number. Once released, any subsequent access returns
 *   `AIPO_ERR_STALE_HANDLE`.
 */

#ifndef AIPO_H
#define AIPO_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

#define AIPO_VERSION_MAJOR 0
#define AIPO_VERSION_MINOR 2
#define AIPO_VERSION_PATCH 0

/**
 * \brief Status and error codes returned by Aipo C ABI functions.
 */
typedef enum aipo_status_t {
    /** Operation completed successfully. */
    AIPO_OK = 0,
    /** Usage error: invalid arguments or unexpected null pointer. */
    AIPO_ERR_USAGE = 1,
    /** Compilation or semantic diagnostic error in provided source code. */
    AIPO_ERR_DIAGNOSTIC = 2,
    /** Uncaught failure (Model B) returned by Aipo code. */
    AIPO_ERR_UNCAUGHT_FAILURE = 3,
    /** Runtime fault (e.g., division by zero, type mismatch). */
    AIPO_ERR_FAULT = 4,
    /** Operation refused because required host capability was not granted. */
    AIPO_ERR_CAPABILITY_DENIED = 5,
    /** Attempted to address an invalid, released, or stale generational handle. */
    AIPO_ERR_STALE_HANDLE = 6,
    /** Null pointer passed to an argument where a valid pointer was required. */
    AIPO_ERR_NULL_POINTER = 7
} aipo_status_t;

/**
 * \brief Tag discriminator identifying the active variant in an `aipo_value_t`.
 */
typedef enum aipo_val_tag_t {
    /** Nil/None value. */
    AIPO_VAL_NONE = 0,
    /** Boolean value. */
    AIPO_VAL_BOOL = 1,
    /** 64-bit integer within canonical safe range +/- (2^53 - 1). */
    AIPO_VAL_INT = 2,
    /** Finite 64-bit IEEE-754 float. */
    AIPO_VAL_FLOAT = 3,
    /** UTF-8 string snapshot. */
    AIPO_VAL_STRING = 4,
    /** Contiguous byte sequence. */
    AIPO_VAL_BYTES = 5,
    /** Generational host handle. */
    AIPO_VAL_HANDLE = 6,
    /** Recoverable failure value with message. */
    AIPO_VAL_FAILURE = 7
} aipo_val_tag_t;

/**
 * \brief Generational handle referencing a host-managed object.
 */
typedef struct aipo_handle_t {
    /** Slot index in the host handle table. */
    uint64_t index;
    /** Generation count when the handle was minted. */
    uint32_t generation;
} aipo_handle_t;

/**
 * \brief Plain C representation of an Aipo value.
 */
typedef struct aipo_value_t {
    /** Tag discriminator. */
    aipo_val_tag_t tag;
    /** Boolean value when tag == AIPO_VAL_BOOL. */
    bool bool_val;
    /** 64-bit integer when tag == AIPO_VAL_INT. */
    int64_t int_val;
    /** 64-bit float when tag == AIPO_VAL_FLOAT. */
    double float_val;
    /** Pointer to null-terminated UTF-8 string when tag == AIPO_VAL_STRING or AIPO_VAL_FAILURE. */
    const char *str_ptr;
    /** Byte length of string data, excluding null terminator. */
    size_t str_len;
    /** Pointer to byte buffer when tag == AIPO_VAL_BYTES. */
    const uint8_t *bytes_ptr;
    /** Length of byte buffer in bytes when tag == AIPO_VAL_BYTES. */
    size_t bytes_len;
    /** Generational handle when tag == AIPO_VAL_HANDLE. */
    aipo_handle_t handle_val;
} aipo_value_t;

typedef struct aipo_runtime_t aipo_runtime_t;

/**
 * \brief Opaque pointer representing an Aipo register machine instance.
 */
typedef struct aipo_reg_vm_t aipo_reg_vm_t;

/**
 * \brief Callback signature for host native functions callable from Aipo.
 *
 * \param rt Pointer to the active runtime instance.
 * \param args Array of argument values passed from Aipo.
 * \param argc Number of arguments in `args`.
 * \param out_result Pointer to location where the callback stores its return value.
 * \return AIPO_OK on success, or an error status code.
 */
typedef aipo_status_t (*aipo_host_fn_t)(
    aipo_runtime_t *rt,
    const aipo_value_t *args,
    size_t argc,
    aipo_value_t *out_result
);

/**
 * \brief Queries the runtime ABI version.
 *
 * \param major Out-pointer for major version component (may be NULL).
 * \param minor Out-pointer for minor version component (may be NULL).
 * \param patch Out-pointer for patch version component (may be NULL).
 */
void aipo_version(uint32_t *major, uint32_t *minor, uint32_t *patch);

/**
 * \brief Creates a new, isolated Aipo runtime instance with deny-by-default capabilities.
 *
 * \return Newly allocated runtime pointer, or NULL on allocation failure.
 */
aipo_runtime_t *aipo_runtime_create(void);

/**
 * \brief Destroys an Aipo runtime instance and releases all associated memory.
 *
 * \param rt Runtime pointer to destroy (safe no-op if NULL).
 */
void aipo_runtime_destroy(aipo_runtime_t *rt);

/**
 * \brief Grants a capability to the runtime (e.g., "clock", "io", "poppy").
 *
 * \param rt Runtime pointer.
 * \param capability Null-terminated capability path name.
 * \return AIPO_OK on success, AIPO_ERR_NULL_POINTER if rt or capability is NULL,
 *         or AIPO_ERR_USAGE if capability name is invalid.
 */
aipo_status_t aipo_runtime_grant_capability(aipo_runtime_t *rt, const char *capability);

/**
 * \brief Revokes a capability previously granted to the runtime.
 *
 * \param rt Runtime pointer.
 * \param capability Null-terminated capability path name.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_revoke_capability(aipo_runtime_t *rt, const char *capability);

/**
 * \brief Compiles, analyzes, and executes top-level code for a module from UTF-8 source.
 *
 * \param rt Runtime pointer.
 * \param name Canonical module name identifier.
 * \param source UTF-8 source code for the module.
 * \return AIPO_OK on success, AIPO_ERR_DIAGNOSTIC on syntax/semantic errors,
 *         or runtime error code if top-level initialization faults.
 */
aipo_status_t aipo_runtime_load_module(aipo_runtime_t *rt, const char *name, const char *source);

/**
 * \brief Calls an Aipo function in a loaded module.
 *
 * \param rt Runtime pointer.
 * \param module_name Name of the module containing the function.
 * \param func_name Name of the function to invoke.
 * \param args Array of argument values to pass.
 * \param argc Number of arguments in `args`.
 * \param out_result Out-pointer to store the function's return value (may be NULL).
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_call(
    aipo_runtime_t *rt,
    const char *module_name,
    const char *func_name,
    const aipo_value_t *args,
    size_t argc,
    aipo_value_t *out_result
);

/**
 * \brief Registers a host native function in the runtime.
 *
 * \param rt Runtime pointer.
 * \param name Function identifier exposed to Aipo code.
 * \param arity Expected parameter count.
 * \param required_capability Capability required to call this function (or NULL if unrestricted).
 * \param callback Host C function pointer.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_register_host_fn(
    aipo_runtime_t *rt,
    const char *name,
    size_t arity,
    const char *required_capability,
    aipo_host_fn_t callback
);

/**
 * \brief Mints a new generational handle for a host value, reporting validation errors.
 *
 * This is the checked form of `aipo_handle_create`. If the provided value violates
 * canonical host contracts (e.g. integer out of safe range, non-finite float, or invalid
 * UTF-8), this returns `AIPO_ERR_USAGE` and no handle is allocated.
 *
 * \param rt Runtime pointer.
 * \param val Value representation associated with the handle.
 * \param out_handle Out-pointer to store the minted generational handle.
 * \return AIPO_OK on success, AIPO_ERR_NULL_POINTER if rt is NULL, or AIPO_ERR_USAGE on validation failure.
 */
aipo_status_t aipo_handle_create_checked(
    aipo_runtime_t *rt,
    aipo_value_t val,
    aipo_handle_t *out_handle
);

/**
 * \brief Mints a new generational handle for a host value (legacy unchecked form).
 *
 * Prefer `aipo_handle_create_checked`. On validation failure, this returns a default
 * invalid handle without reporting the error code.
 *
 * \param rt Runtime pointer.
 * \param val Value representation associated with the handle.
 * \return Minted generational handle, or a zeroed handle on failure.
 */
aipo_handle_t aipo_handle_create(aipo_runtime_t *rt, aipo_value_t val);

/**
 * \brief Releases the runtime-owned snapshot backing a returned string, bytes, or failure value.
 *
 * Returned values carrying heap payloads (`AIPO_VAL_STRING`, `AIPO_VAL_BYTES`,
 * `AIPO_VAL_FAILURE`) point to buffers owned by the runtime snapshot table. When the host
 * is finished reading these buffers, calling `aipo_value_release` frees the backing allocation.
 * Calling this on non-heap values is a safe no-op.
 *
 * \param rt Runtime pointer.
 * \param val Value whose snapshot allocation should be released.
 */
void aipo_value_release(aipo_runtime_t *rt, aipo_value_t val);

/**
 * \brief Resolves a live generational handle to its host value.
 *
 * \param rt Runtime pointer.
 * \param handle Generational handle to resolve.
 * \param out_value Out-pointer to receive the resolved value.
 * \return AIPO_OK on success, or AIPO_ERR_STALE_HANDLE if the handle is invalid/stale.
 */
aipo_status_t aipo_handle_resolve(
    aipo_runtime_t *rt,
    aipo_handle_t handle,
    aipo_value_t *out_value
);

/**
 * \brief Releases a generational handle, invalidating all handles minted for its slot.
 *
 * \param rt Runtime pointer.
 * \param handle Generational handle to release.
 * \return AIPO_OK on success, or AIPO_ERR_STALE_HANDLE if already stale.
 */
aipo_status_t aipo_handle_release(aipo_runtime_t *rt, aipo_handle_t handle);

/**
 * \brief Retrieves the most recent error or diagnostic message.
 *
 * \param rt Runtime pointer.
 * \param buffer Destination buffer to write null-terminated UTF-8 error string.
 * \param buffer_len Maximum bytes available in `buffer`.
 * \return Number of bytes written, including null terminator.
 */
size_t aipo_last_error(const aipo_runtime_t *rt, char *buffer, size_t buffer_len);

/**
 * \brief Configures the maximum instruction execution budget for the runtime.
 *
 * If `max_instructions > 0`, the runtime increments an instruction counter on every step.
 * If the counter exceeds `max_instructions`, execution immediately halts with `AIPO_ERR_FAULT`
 * (diagnostic code `AIPO_RT_OVERFLOW`). Passing `0` disables the budget (unlimited execution).
 *
 * \param rt Runtime pointer.
 * \param max_instructions Maximum instructions allowed, or 0 for unlimited.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_set_instruction_budget(
    aipo_runtime_t *rt,
    uint64_t max_instructions
);

/**
 * \brief Returns the instruction counter maintained while a budget is enabled.
 *
 * \param rt Runtime pointer.
 * Budgeted calls retain consumption until reset. An unbudgeted execution may reset
 * the counter and does not count instructions unless VM metrics are enabled.
 * \return Current counter value, or 0 for a null runtime.
 */
uint64_t aipo_runtime_instruction_count(const aipo_runtime_t *rt);

/**
 * \brief Resets the executed instruction counter to zero.
 *
 * \param rt Runtime pointer.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_reset_instruction_count(aipo_runtime_t *rt);

/**
 * \brief Creates a new virtual register machine instance.
 *
 * \return Pointer to a newly created register machine, or NULL on allocation failure.
 */
aipo_reg_vm_t *aipo_reg_vm_create(void);

/**
 * \brief Destroys a virtual register machine instance.
 *
 * \param vm Pointer returned by aipo_reg_vm_create, or NULL.
 */
void aipo_reg_vm_destroy(aipo_reg_vm_t *vm);

/**
 * \brief Sets an integer value into a specific register.
 *
 * \param vm Register machine pointer.
 * \param reg Register index (0..255).
 * \param val Integer value to set.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_reg_vm_set_reg_int(
    aipo_reg_vm_t *vm,
    uint8_t reg,
    int64_t val
);

/**
 * \brief Reads an integer value from a specific register.
 *
 * \param vm Register machine pointer.
 * \param reg Register index (0..255).
 * \param out_val Out-pointer to receive the integer value.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_reg_vm_get_reg_int(
    const aipo_reg_vm_t *vm,
    uint8_t reg,
    int64_t *out_val
);

/**
 * \brief Executes a buffer of 32-bit register instructions.
 *
 * \param vm Register machine pointer.
 * \param instructions Pointer to array of 32-bit register instructions.
 * \param count Number of instruction words in the array.
 * \param out_int Optional out-pointer to receive the integer return value (can be NULL).
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_reg_vm_run(
    aipo_reg_vm_t *vm,
    const uint32_t *instructions,
    size_t count,
    int64_t *out_int
);

/**
 * \brief Configures instruction budget limit on the register machine.
 *
 * \param vm Register machine pointer.
 * \param limit Maximum instructions allowed, or 0 to disable.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_reg_vm_set_instruction_budget(aipo_reg_vm_t *vm, uint64_t limit);

/**
 * \brief Returns the number of instructions executed by the register machine.
 *
 * \param vm Register machine pointer.
 * \return Cumulative executed instruction count.
 */
uint64_t aipo_reg_vm_instruction_count(const aipo_reg_vm_t *vm);

/**
 * \brief Resets the instruction execution counter of the register machine.
 *
 * \param vm Register machine pointer.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_reg_vm_reset_instruction_count(aipo_reg_vm_t *vm);

/**
 * \brief Copies the last error message from the register machine into the provided buffer.
 *
 * \param vm Register machine pointer.
 * \param buffer Buffer to receive null-terminated error string, or NULL to query required length.
 * \param capacity Size of buffer in bytes.
 * \return Required byte count including null terminator.
 */
size_t aipo_reg_vm_copy_last_error(const aipo_reg_vm_t *vm, char *buffer, size_t capacity);

/**
 * \brief Starts a function as the cooperative main computation.
 *
 * \param rt Runtime pointer.
 * \param module Module name containing the function.
 * \param function Function name to invoke.
 * \param args Array of argument values.
 * \param argc Number of argument values in args.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_begin(
    aipo_runtime_t *rt,
    const char *module,
    const char *function,
    const aipo_value_t *args,
    size_t argc
);

/**
 * \brief Pumps up to `steps` cooperative execution quanta.
 *
 * \param rt Runtime pointer.
 * \param steps Maximum quanta steps to run.
 * \param completed Out-pointer set to true if execution finished, false if pending.
 * \param out Out-pointer to receive completion value when completed is true.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_pump(
    aipo_runtime_t *rt,
    size_t steps,
    bool *completed,
    aipo_value_t *out
);

/**
 * \brief Cancels the active cooperative computation.
 *
 * \param rt Runtime pointer.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_abort(aipo_runtime_t *rt);

/**
 * \brief Loads and initializes an Aipo module from precompiled bytecode (.aibc).
 *
 * \param rt Runtime pointer.
 * \param name Null-terminated C string identifying the module name.
 * \param bytecode Pointer to binary bytecode bytes.
 * \param len Length of the bytecode buffer in bytes.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_load_bytecode(
    aipo_runtime_t *rt,
    const char *name,
    const uint8_t *bytecode,
    size_t len
);

/**
 * \brief Compiles Aipo source code to serialized binary bytecode (.aibc).
 *
 * Allocates a buffer containing the compiled bytecode. The caller is responsible
 * for freeing the buffer using \ref aipo_bytecode_free.
 *
 * \param source Pointer to UTF-8 source code bytes.
 * \param source_len Length of source in bytes (or 0 if null-terminated).
 * \param out_bytes Out-pointer to receive the allocated bytecode buffer.
 * \param out_len Out-pointer to receive the size of the allocated bytecode buffer.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_compile_to_bytecode(
    const char *source,
    size_t source_len,
    uint8_t **out_bytes,
    size_t *out_len
);

/**
 * \brief Frees a bytecode buffer previously allocated by \ref aipo_compile_to_bytecode.
 *
 * \param bytes Pointer returned by aipo_compile_to_bytecode (or NULL).
 * \param len Length of the bytecode buffer in bytes.
 */
void aipo_bytecode_free(uint8_t *bytes, size_t len);

/**
 * \brief Prepares a loaded module for execution or stepping from instruction 0.
 *
 * \param rt Runtime pointer.
 * \param module_name Null-terminated C string identifying the loaded module name.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_prepare_module(
    aipo_runtime_t *rt,
    const char *module_name
);

/**
 * \brief Advances execution by a single instruction on the specified or last loaded module.
 *
 * \param rt Runtime pointer.
 * \param module_name Null-terminated C string with module name, or NULL for last loaded module.
 * \param out_done Out-pointer to receive boolean: true if execution finished, false if more steps remain.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_step(
    aipo_runtime_t *rt,
    const char *module_name,
    bool *out_done
);

/**
 * \brief Executes instructions on the specified or last loaded module until completion or hitting max_steps.
 *
 * If `max_steps == 0`, execution proceeds without a step limit until completion or suspension.
 *
 * \param rt Runtime pointer.
 * \param module_name Null-terminated C string with module name, or NULL for last loaded module.
 * \param max_steps Maximum instructions to execute, or 0 for unlimited.
 * \param out_done Out-pointer to receive boolean: true if finished, false if hit max_steps or suspended.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_runtime_run_until_idle(
    aipo_runtime_t *rt,
    const char *module_name,
    uint64_t max_steps,
    bool *out_done
);

/**
 * \brief Compiles Aipo source code to standalone WebAssembly binary (.wasm).
 *
 * Allocates a buffer containing the compiled WebAssembly binary. The caller is responsible
 * for freeing the buffer using \ref aipo_wasm_free.
 *
 * \param source Pointer to UTF-8 source code bytes.
 * \param source_len Length of source in bytes (or 0 if null-terminated).
 * \param out_wasm Out-pointer to receive the allocated WebAssembly buffer.
 * \param out_len Out-pointer to receive the size of the allocated WebAssembly buffer.
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_wasm_compile(
    const char *source,
    size_t source_len,
    uint8_t **out_wasm,
    size_t *out_len
);

/**
 * \brief Frees a WebAssembly buffer previously allocated by \ref aipo_wasm_compile.
 *
 * \param wasm_bytes Pointer returned by aipo_wasm_compile (or NULL).
 * \param len Length of the WebAssembly buffer in bytes.
 */
void aipo_wasm_free(uint8_t *wasm_bytes, size_t len);

/**
 * \brief Executes precompiled WebAssembly bytecode using Wasmtime JIT.
 *
 * \param wasm_bytes Pointer to WebAssembly binary bytes.
 * \param len Length of WebAssembly buffer in bytes.
 * \param fuel Maximum fuel budget allowed, or 0 for unlimited execution.
 * \param out_result Optional out-pointer to receive 64-bit integer return value (can be NULL).
 * \return AIPO_OK on success, or error status code.
 */
aipo_status_t aipo_wasm_run(
    const uint8_t *wasm_bytes,
    size_t len,
    uint64_t fuel,
    int64_t *out_result
);

#ifdef __cplusplus
}
#endif

#endif /* AIPO_H */
