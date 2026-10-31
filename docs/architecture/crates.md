# Contratos das Crates do Workspace

O repositório do Aipo é composto por **24 crates em Rust**, cada uma com limites de responsabilidade e dependências internas acíclicas.

Fonte: `cargo metadata --no-deps --format-version 1` e os `Cargo.toml` do workspace. Dependências de teste (`dev-dependencies`) não aparecem nesta tabela.

---

## Inventário de Crates

| Crate | Responsabilidade Primária | Dependências Internas Normais |
| :--- | :--- | :--- |
| **`aipo-source`** | Carregamento de fontes e indexação segura UTF-8 | Nenhuma |
| **`aipo-diagnostics`** | Catálogo estável de erros, spans, locale e JSONL | `aipo-source` |
| **`aipo-lexer`** | Tokenização, keywords, literais e spans | `aipo-source`, `aipo-diagnostics` |
| **`aipo-ast`** | Tipos da árvore sintática abstrata | `aipo-source` |
| **`aipo-syntax`** | Parser recursivo com recuperação de erros | `aipo-ast`, `aipo-diagnostics`, `aipo-lexer`, `aipo-source` |
| **`aipo-hir`** | Representação intermediária de alto nível e lowering | `aipo-ast`, `aipo-diagnostics`, `aipo-source` |
| **`aipo-sema`** | Análise semântica, escopos, contratos e interfaces | `aipo-ast`, `aipo-diagnostics`, `aipo-hir`, `aipo-source` |
| **`aipo-ir`** | Core IR neutro em relação ao alvo | `aipo-ast`, `aipo-diagnostics`, `aipo-hir`, `aipo-lexer`, `aipo-sema`, `aipo-source` |
| **`aipo-bytecode`** | Emissão, verificação, serialização `.aibc` e plano de execução | `aipo-diagnostics`, `aipo-ir`, `aipo-source` |
| **`aipo-vm`** | Stack VM, RegVM, scheduler async e host adapter | `aipo-bytecode`, `aipo-diagnostics`, `aipo-host`, `aipo-source` |
| **`aipo-js`** | Emissor JavaScript ESM, runtime shim e source maps | `aipo-diagnostics`, `aipo-ir` |
| **`aipo-host`** | Host ABI, capabilities e handles geracionais | `aipo-diagnostics` |
| **`aipo-runtime`** | Registro de módulos e funções nativas | `aipo-bytecode`, `aipo-diagnostics` |
| **`aipo-stdlib`** | Biblioteca padrão e métodos de coleções | `aipo-diagnostics`, `aipo-host`, `aipo-runtime`, `aipo-vm` |
| **`aipo-poppy`** | Adaptador ECS e simulação determinística | `aipo-bytecode`, `aipo-diagnostics`, `aipo-host`, `aipo-runtime`, `aipo-stdlib`, `aipo-vm` |
| **`aipo-package`** | Manifesto, lockfile, resolver, cache e vendor | `aipo-host` |
| **`aipo-formatter`** | Formatador automático de sintaxe canônica | `aipo-diagnostics`, `aipo-lexer`, `aipo-source` |
| **`aipo-testkit`** | Harness de testes: RNG determinístico, geradores e runners | `aipo-ast`, `aipo-bytecode`, `aipo-diagnostics`, `aipo-hir`, `aipo-host`, `aipo-ir`, `aipo-js`, `aipo-lexer`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm`, `aipo-wasm` |
| **`aipo-bench`** | Runner de benchmarks e comparação cross-language | `aipo-bytecode`, `aipo-formatter`, `aipo-hir`, `aipo-ir`, `aipo-js`, `aipo-lexer`, `aipo-sema`, `aipo-source`, `aipo-syntax`, `aipo-testkit` |
| **`aipo-c-abi`** | ABI C síncrona, bytecode, stepping e Wasm | `aipo-bytecode`, `aipo-diagnostics`, `aipo-hir`, `aipo-host`, `aipo-ir`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm`, `aipo-wasm` |
| **`aipo-wasm`** | Compilador HIR → WebAssembly, runner JIT e limites | `aipo-ast`, `aipo-diagnostics`, `aipo-hir`, `aipo-ir`, `aipo-lexer`, `aipo-source`, `aipo-syntax` |
| **`aipo-egui`** | Adaptador immediate-mode GUI e bindings eGUI | `aipo-bytecode`, `aipo-diagnostics`, `aipo-host`, `aipo-runtime`, `aipo-stdlib`, `aipo-vm` |
| **`aipo-game-host`** | Host desktop/game, Miniquad, áudio e bridge FFI | `aipo-ast`, `aipo-bytecode`, `aipo-cli`, `aipo-diagnostics`, `aipo-egui`, `aipo-hir`, `aipo-ir`, `aipo-lexer`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm` |
| **`aipo-cli`** | Ponto de entrada CLI, formatter, pacotes, tooling e LSP | `aipo-ast`, `aipo-bytecode`, `aipo-diagnostics`, `aipo-formatter`, `aipo-hir`, `aipo-host`, `aipo-ir`, `aipo-js`, `aipo-lexer`, `aipo-package`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm`, `aipo-wasm` |

## Regras de dependência

- O grafo normal é acíclico. Ciclos aparentes em `cargo metadata` vêm de `dev-dependencies` usadas por testes.
- Frontend (`source` até `sema`) não depende de bytecode, VM, JS, Wasm, host ou GUI.
- `aipo-js` depende de Core IR (`aipo-ir`) e diagnósticos; não depende de bytecode nem da VM.
- `aipo-wasm` consome HIR, não Core IR.
- `aipo-cli`, `aipo-game-host` e `aipo-egui` são folhas de produto: nenhuma crate de linguagem depende delas.
- `aipo-c-abi` é a única crate que habilita `unsafe_code`; a exceção é restrita à fronteira FFI e documentada em `docs/crates/crate-contracts.md`.

## Módulos internos dos backends

Um crate com três domínios independentes num só arquivo é o sinal de god-module
que a doutrina de arquitetura pede para tratar. Os dois maiores consumidores já
foram separados por responsabilidade; a divisão é interna ao crate, não muda a
direção de dependência.

| crate | módulos | fronteira |
|---|---|---|
| `aipo-wasm` | `analysis` / `runtime` / `compiler` | análise estática que recupera o que a representação sem tags perde; helpers Wasm emitidos em todo módulo; geração de código |
| `aipo-cli` | `packages` + `lib` | resolução/fetch/lock/cache de pacotes vs. driver de compilação, execução e tooling |
