---
title: Host API, embedding e ABI C
---
# Embedding: Aipo dentro de outro processo

A linguagem precisa servir como engine de scripting em jogos, interfaces, automação e ferramentas, sem conceder privilégios implícitos aos scripts.

## Fronteiras de integração

| Fronteira | Contrato |
| --- | --- |
| `aipo-host` | Capabilities e descrição das operações autorizadas |
| AHS (Host Schema) | Superfície declarativa para análise estática por invocação |
| `aipo-c-abi` | API binária síncrona, tipos opacos e handles |
| Rust embedding | Integração in-process com ownership e erro explícitos |
| Adaptadores de domínio | Ex.: game host e interfaces, fora da semântica base |

## Princípio do menor privilégio

Descrever um método `host.foo` no esquema não instala automaticamente sua implementação. O host decide quais operações existem e quais capacidades são concedidas. Handles expirados devem falhar de forma definida; não podem permitir uso após liberação.

## Embedded e Shell Profile

Há uma proposta arquitetural de engine mais enxuta e perfil shell. Ela define objetivos e trade-offs, mas **não equivale a implementação final validada**. Não prometa targets microcontroladores, cold start garantido ou execução de scripts shell até existirem artefatos e medições.

## Checklist de integração

1. Observar header `crates/aipo-c-abi/include/aipo.h` ou API Rust correspondente.
2. Validar versão de ABI e contrato de memória.
3. Definir capabilities e budgets.
4. Exercitar chamada, retorno, erro, cancelamento e handle obsoleto.
5. Testar host real, não apenas mock.
6. Documentar formatos públicos e semântica de versão.

Mais detalhes: [Guia de embedding](/guides/embedding), [arquitetura histórica](https://github.com/poppy-team/aipo-lang/blob/main/docs/architecture/embedded-and-shell-profile.md) e [ADP-009](https://github.com/poppy-team/aipo-lang/blob/main/docs/adp/ADP-009-synchronous-c-abi.md).
