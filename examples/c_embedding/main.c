/**
 * \file main.c
 * \brief Idiomatic C Embedding Example for the Aipo Language.
 *
 * Demonstrates:
 * 1. Initializing the isolated Aipo runtime and querying version.
 * 2. Registering custom host native C functions callable from Aipo.
 * 3. Compiling and loading Aipo modules from source text.
 * 4. Invoking functions with typed arguments and handling return values (Int, String, Bytes).
 * 5. Instruction-level stepping (`aipo_runtime_step`).
 * 6. Deterministic memory and resource teardown.
 */

#include "aipo.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Custom host function callable from Aipo: adds two numbers and logs to C stdout */
static aipo_status_t host_multiply(
    aipo_runtime_t *rt,
    const aipo_value_t *args,
    size_t argc,
    aipo_value_t *out_result
) {
    (void)rt;
    if (argc != 2 || args[0].tag != AIPO_VAL_INT || args[1].tag != AIPO_VAL_INT) {
        return AIPO_ERR_USAGE;
    }

    int64_t product = args[0].int_val * args[1].int_val;
    printf("  [Host C] host_multiply(%ld, %ld) = %ld\n", args[0].int_val, args[1].int_val, product);

    out_result->tag = AIPO_VAL_INT;
    out_result->int_val = product;
    return AIPO_OK;
}

int main(void) {
    printf("=== Aipo C Embedding Example ===\n\n");

    /* 1. Version query */
    uint32_t major = 0, minor = 0, patch = 0;
    aipo_version(&major, &minor, &patch);
    printf("1. Initialized Aipo Runtime (ABI v%u.%u.%u)\n", major, minor, patch);

    /* 2. Create isolated runtime */
    aipo_runtime_t *rt = aipo_runtime_create();
    if (!rt) {
        fprintf(stderr, "Failed to create Aipo runtime\n");
        return 1;
    }

    /* 3. Grant capability and register host callback */
    aipo_runtime_grant_capability(rt, "io");
    aipo_status_t status = aipo_runtime_register_host_fn(
        rt,
        "host_multiply",
        2,
        NULL,
        host_multiply
    );
    if (status != AIPO_OK) {
        fprintf(stderr, "Failed to register host function: status %d\n", status);
        aipo_runtime_destroy(rt);
        return 1;
    }
    printf("2. Registered host native C function `host_multiply(a, b)`\n");

    /* 4. Load Aipo module */
    const char *module_source =
        "fn calculate_total(base: Int, factor: Int) -> Int {\n"
        "    let mult = host_multiply(base, factor)\n"
        "    return mult + 10\n"
        "}\n"
        "\n"
        "fn make_greeting(name: String) -> String {\n"
        "    return \"Hello, \" + name + \"! Welcome to embedded Aipo.\"\n"
        "}\n"
        "\n"
        "fn make_packet() -> Bytes {\n"
        "    var b = Bytes(4)\n"
        "    b.write_u8(0, 0xDE)\n"
        "    b.write_u8(1, 0xAD)\n"
        "    b.write_u8(2, 0xBE)\n"
        "    b.write_u8(3, 0xEF)\n"
        "    return b\n"
        "}\n"
        "\n"
        "var step_counter = 0\n"
        "step_counter = step_counter + 1\n"
        "step_counter = step_counter + 2\n";

    status = aipo_runtime_load_module(rt, "app", module_source);
    if (status != AIPO_OK) {
        fprintf(stderr, "Failed to load module: status %d\n", status);
        aipo_runtime_destroy(rt);
        return 1;
    }
    printf("3. Successfully loaded module `app`\n\n");

    /* 5. Invoke `calculate_total(7, 6)` */
    printf("4. Calling `calculate_total(7, 6)`:\n");
    aipo_value_t args[2];
    memset(args, 0, sizeof(args));
    args[0].tag = AIPO_VAL_INT;
    args[0].int_val = 7;
    args[1].tag = AIPO_VAL_INT;
    args[1].int_val = 6;

    aipo_value_t res_int;
    memset(&res_int, 0, sizeof(res_int));
    status = aipo_runtime_call(rt, "app", "calculate_total", args, 2, &res_int);
    if (status == AIPO_OK && res_int.tag == AIPO_VAL_INT) {
        printf("  => Result: %ld (expected 52)\n\n", res_int.int_val);
    } else {
        fprintf(stderr, "Call failed: status %d\n", status);
    }

    /* 6. Invoke `make_greeting(\"World\")` */
    printf("5. Calling `make_greeting(\"C Host\")`:\n");
    aipo_value_t arg_str;
    memset(&arg_str, 0, sizeof(arg_str));
    arg_str.tag = AIPO_VAL_STRING;
    arg_str.str_ptr = "C Host";
    arg_str.str_len = strlen(arg_str.str_ptr);

    aipo_value_t res_str;
    memset(&res_str, 0, sizeof(res_str));
    status = aipo_runtime_call(rt, "app", "make_greeting", &arg_str, 1, &res_str);
    if (status == AIPO_OK && res_str.tag == AIPO_VAL_STRING) {
        printf("  => Result: \"%.*s\"\n\n", (int)res_str.str_len, res_str.str_ptr);
        aipo_value_release(rt, res_str);
    }

    /* 7. Invoke `make_packet()` returning Bytes */
    printf("6. Calling `make_packet()`:\n");
    aipo_value_t res_bytes;
    memset(&res_bytes, 0, sizeof(res_bytes));
    status = aipo_runtime_call(rt, "app", "make_packet", NULL, 0, &res_bytes);
    if (status == AIPO_OK && res_bytes.tag == AIPO_VAL_BYTES) {
        printf("  => Bytes length: %zu\n  => Bytes hex: 0x", res_bytes.bytes_len);
        for (size_t i = 0; i < res_bytes.bytes_len; i++) {
            printf("%02X", res_bytes.bytes_ptr[i]);
        }
        printf(" (expected 0xDEADBEEF)\n\n");
        aipo_value_release(rt, res_bytes);
    }

    /* 8. Fine-grained instruction stepping */
    printf("7. Demonstrating instruction stepping (`aipo_runtime_step`):\n");
    status = aipo_runtime_prepare_module(rt, "app");
    if (status == AIPO_OK) {
        bool done = false;
        int steps = 0;
        while (!done && steps < 20) {
            status = aipo_runtime_step(rt, "app", &done);
            if (status != AIPO_OK) {
                fprintf(stderr, "Stepping encountered status %d\n", status);
                break;
            }
            steps++;
        }
        printf("  => Executed %d instructions single-stepped (done: %s)\n\n", steps, done ? "true" : "false");
    }

    /* 9. Teardown */
    aipo_runtime_destroy(rt);
    printf("8. Runtime cleanly destroyed. All resources released.\n");
    printf("\n=== C Embedding Test Completed Successfully ===\n");
    return 0;
}
