# C Embedding Example

Este diretório contém uma prova real e idiomática de incorporação (*embedding*) da linguagem Aipo em aplicações C nativas utilizando a interface `libaipo_c_abi` e o cabeçalho oficial `aipo.h`.

## Visão Geral

O exemplo demonstra:
1. **Inicialização do Runtime:** Criação de runtime isolado com capabilities controladas (`aipo_runtime_create`, `aipo_runtime_grant_capability`).
2. **Registro de Funções C Nativas:** Registro de callback C acessível pelo código Aipo (`aipo_runtime_register_host_fn`).
3. **Carga e Compilação de Módulos:** Compilação de código fonte em tempo de execução (`aipo_runtime_load_module`).
4. **Chamada de Funções com Argumentos:** Execução e extração de valores tipados (inteiros, strings dinâmicas, buffers `Bytes`).
5. **Stepping Instrução a Instrução:** Controle granular de execução com `aipo_runtime_prepare_module` e `aipo_runtime_step`.
6. **Desalocação Determinística:** Liberação de valores e destruição segura da máquina virtual (`aipo_value_release`, `aipo_runtime_destroy`).

## Como Compilar e Executar

1. Compile a biblioteca compartilhada do C ABI na raiz do projeto:
   ```bash
   cargo build -p aipo-c-abi
   ```

2. Compile o executável C utilizando o `Makefile`:
   ```bash
   cd examples/c_embedding
   make
   ```

3. Execute o binário:
   ```bash
   make run
   ```
