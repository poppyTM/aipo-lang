# Continuidade dos estudos: verificação, chamadas e embedding

**Status:** leitura dirigida aplicada · **Rodada:** P07-G02 · **Data:** 2026-10-09.
**Corpus:** os nove commits de `studies/refs.json`; não houve repin implícito para HEAD upstream.

Esta rodada continuou o [guia master](llm-study-guide.md) com leitura das regiões abaixo. Não houve build/benchmark dos runtimes externos nem auditoria integral de seus arquivos. Inferências para Aipo estão separadas das propriedades observadas no source.

## Tier A: custo de chamada e fronteira do host

| Referência pinada | Região lida | Observação do código | Aplicação na Aipo |
|---|---|---|---|
| [Wren](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_vm.c#L1392) | `wrenMakeCallHandle`, `wrenCall`, 1392–1469; declarações em `wren.h` | Call handle guarda stub/closure; a API prepara slots e distingue execução da stack de host | Fronteiras explícitas, recusa de reentrada C durante callback e ownership de valores; não copiamos seu GC |
| [QuickJS-ng](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.c#L2436) | `JS_SetMemoryLimit`, `JS_SetInterruptHandler`, `__js_poll_interrupts`, 2434–2446, 2468–2478, 8655–8675 | Limite de allocator e callback de interrupção são mecanismos separados; polling usa contador | Budget guest não é limite de memória/wall-clock; limites Wasm separados e callback externo sob controle do host |
| [Lua](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/ldo.c#L717) | `luaD_precall`, `luaD_poscall`, 717–742 e 607–629; localizações de execução protegida | Ativação usa `maxstacksize`/CallInfo e retorno repõe caller; entradas C são distintas | Janela viva Reg e restauração do caller, sem copiar 256 slots indiscriminadamente |
| [LuaJIT](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_frame.h#L21) | Macros/layouts de frame, 21–75; declarações de unwind em `lj_vm.h` | Layouts condicionais `LJ_FR2`; base/top e metadata dependem da configuração | Medir layout do alvo e preservar frames; não transportar tamanhos/NaN boxing para Rust sem experimento |

**Hipótese medida em Aipo:** remover cópia total e clonagem profunda por chamada deve reduzir custo de chamadas curtas. O benchmark usa a mesma fonte no baseline e candidata, retorna Int ou String efetivamente usados e mede o módulo completo. Resultado e limites estão em [P07-G02](../evidence/P07-G02/README.md). Não é benchmark Lua/Wren/QuickJS/LuaJIT.

## Tier B: provas e execução retomável

| Referência pinada | Região lida | Observação do código | Aplicação na Aipo |
|---|---|---|---|
| [Luau](https://github.com/luau-lang/luau/blob/74f768309c380d40601b28c4c4cdda27da415cef/Bytecode/src/BytecodeBuilder.cpp#L1620) | `validate`, `validateInstructions`, 1620–1692 | Valida registrador/janela, tipo de constante e destination boundary; passes de captures/variadic separados | RegVerifier valida cada papel do operando, inclusive unreachable; análise adicional de inicialização definida e escopos pelo CFG |
| [Janet](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/core/vm.c#L1603) | `janet_continue`, `janet_continue_signal`, `janet_pcall`, 1603–1648; sinais no header | Retomar fiber verifica estado; chamada protegida entrega sinal e resultado explicitamente | API C begin/pump/abort, runtime ocupado e conclusão/falha explícitas; scheduler canônico compartilhado |

O validator de Luau observado usa asserts internos; Aipo recebe buffers raw C e retorna erro antes de executar. Definite assignment e modelagem de handlers são implementação própria baseada nos invariantes da Aipo, não cópias daquela região de Luau.

Pump da Aipo mede quanta, não execução preemptiva de nativos. Abort mantém efeitos realizados. O event loop deve devolver controle entre pumps; não há deadline para callback que bloqueia.

## Tier C: recursos Wasm por configuração

| Referência pinada | Região lida | Observação do código | Aplicação na Aipo |
|---|---|---|---|
| [wasm3](https://github.com/wasm3/wasm3/blob/28ecb9af6d2040e474a70f7cb7f43666740141fb/source/m3_env.c#L252) | `m3_NewRuntime`/`m3_SetValidation`, 215–240; `m3_SetResourceLimit`, 252–304 | Stack configurável; limites de memória/tabela/continuações/gas dependem do build | Comparar capacidades por configuração; não anunciar budget/suspensão iguais em toda build |
| [WAMR](https://github.com/wasm-micro-runtime/wasm-micro-runtime/blob/38b044a676893e678ceb19babaa431ca5e460114/core/iwasm/include/wasm_export.h#L219) | `RuntimeInitArgs`, 219–260; `wasm_runtime_create_exec_env`, 960–973; `wasm_runtime_terminate`, 1360–1375 | Init tem opções de allocator/modo; exec_env recebe stack; terminate é fronteira separada | Futuras escolhas devem medir allocator/stack/WASI e lifetime; não houve integração nem escolha de vencedor |
| [wasmi](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasmi/src/engine/config.rs#L349) | `Config::consume_fuel`, 347–369 | Fuel opt-in instrumenta execução; Store recebe fuel antes de executar | Documentar instrumentação/consumo e separar memória/output; implementação atual permanece Wasmtime |

## Decisões e limites

- Verificar operandos/CFG antes de otimizar permite reutilizar prova somente na imagem correspondente.
- Metadados de ativação determinam janelas; semântica permanece em `Value` e serviços canônicos.
- Snapshot guest preserva aliases por restauração em lugar; não desfaz recursos externos.
- `is` Wasm precisa de prova semântica; tipo físico de registrador não é essa prova.
- Perfis removem dependências por features, sem crates duplicados ou linguagens diferentes.

Continuam pendentes auditoria integral de GC/ciclos, cold start/RSS, limites de allocator nativo, Miri/fuzz/ASan e conformance executada. Os testes desta rodada foram explicitamente não executados. Compilação e microbenchmark não substituem esses gates.
