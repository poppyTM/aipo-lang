# P07-G01 — Reconstrução do runtime, estudos e guia

**Data:** 2026-10-09. **Goal:** `P07-G01`, mantido em `DRAFT`.
**Base estudada:** `488905ffb0b83ebbff458939542350d7280ad79b`.
**Base integrada:** `bcbc4c991dee1d9f853c597aaed2790e8981b8b9`.
**Branch:** `audit/runtime-hardening-and-studies`.

Esta entrega reconstrói correções após a perda dos arquivos temporários da rodada anterior. Não é recuperação byte a byte daquele patch. Os resultados antigos de testes e benchmarks não certificam esta reconstrução. O usuário autorizou commit, push e PR e pediu: “não precisa executar os testes”. Nenhum teste, selftest JS, programa de conformance ou benchmark foi executado nesta rodada.

## Implementação entregue

| Área | Problema na base | Alteração e motivo |
| --- | --- | --- |
| Emissor Reg | IR desconhecido era ignorado; operadores desconhecidos viravam igualdade | APIs retornam `Result`, preflight rejeita recursos e operandos não representáveis |
| Fluxo Reg | Altura de temporários seguia ordem textual dos ramos | CFG calcula alturas, verifica underflow/junções e emite usando a entrada de cada instrução |
| Encoding Reg | Índice de iteração comprimido em C; contagens e destinos podiam truncar | Novos opcodes usam C completo; limites de registradores, pools, offsets e lista de 256 itens são checados |
| Valores Reg | Aritmética duplicada incompleta | Reuso dos helpers `Value` para promoção, finitude, limite seguro e igualdade |
| Execução Reg | Operand B, pools, argumentos e destinos inválidos podiam falhar incorretamente | Checagens por papel de operando e erros explícitos; lookup estrito não vira `none` |
| Ativações Reg | Estado e handlers de chamadas eram vulneráveis a propagação incorreta | Metadados/aridade checados, restauração do caller, handlers locais e retorno imediato de falha sem handler |
| Limites Reg | Nenhum limite de instruções | API cumulativa opcional, com reset explícito; callbacks continuam fora do limite |
| Iteração/contratos Reg | Guards e contratos sem lowering | Guards por ativação e contratos fundamentais/nominais; interfaces com operações são rejeitadas |
| Dict em VM/Reg/JS | Primary retornava valor em vez de chave | Chaves na ordem de inserção, conforme canon; expectativa JS corrigida manualmente |
| Strings VM/Reg | Indexação construía `Vec<char>` temporário | Helper compartilhado ASCII/Unicode, índice negativo e `i64::MIN` sem negação que cause overflow |
| Wasm | Ausência de entrypoint e erros de output podiam parecer sucesso | Erros propagados, entrega de output após traps de start/entrypoint e fuel opcional antes da instanciação |
| Código morto Wasm | Índices privados nunca lidos e helper de span sem uso | Remoção de metadados mortos, preservando helpers exportados |
| Features e regressões | Suítes JIT/formatter não respeitavam builds reduzidos | Guards por feature, casos Reg e duas fixtures novas; código compilado, sem execução |
| Estudos | Premissas e referências sem vínculo reproduzível suficiente | Manifest de nove SHAs, fetch externo/offline verify e notas com leitura dirigida e propostas delimitadas |
| Documentação | Sem guia consolidado para reproduzir as alterações | [Guia completo](../development/runtime-hardening-guide.md), referência CLI, diagnóstico, contratos e roadmap sincronizados |

A atualização do `wasm-encoder` para `0.261.0` veio de um merge na `main` durante o trabalho. Foi incorporada por fast-forward antes das checagens finais; este PR não atribui essa atualização à auditoria.

## Verificação efetivamente feita

Resultados e hashes dos arquivos de implementação estão em [P07-G01-checks.json](P07-G01-checks.json). Compilação de alvos de teste significa somente checagem de tipos: `cargo check --all-targets` não executa seus testes.

| Verificação | Resultado |
| --- | --- |
| `cargo fmt --all -- --check` | Registrado no JSON após a revisão final |
| `cargo check` de CLI/VM/bytecode/Wasm, `--all-targets --locked`, features padrão | Registrado no JSON; inclui os alvos de regressão e exemplos sem executá-los |
| CLI com compilador Wasm e sem Wasmtime, `--all-targets --locked` | Registrado no JSON |
| CLI sem features padrão, `--all-targets --locked` | Registrado no JSON |
| `node --check` no runtime e selftest JS | Sintaxe aceita; não executa o selftest |
| AST dos scripts Python e leitura dos JSONs alterados | Inspeção sintática, sem executar benchmarks |
| Links locais das páginas alteradas | Existência de destinos conferida; não é build VitePress nem verificação de anchors |
| `fetch-refs.sh --verify` | Os nove HEADs, origins, árvores limpas e lock coincidem com o manifest |
| `git diff --check` | Registrado no JSON |

Ambiente: Rust `1.99.0` (`b940084d7`, host `x86_64-unknown-linux-gnu`), Node `24.19.0`, Python `3.12.14`. Compilações usam `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2` e `CARGO_PROFILE_DEV_DEBUG=0` por restrição de recursos. Não são medidas de performance nem builds release. O MSRV declarado `1.85` não foi certificado.

## Gates pendentes

| Gate | Estado e razão |
| --- | --- |
| Testes Rust, JS, CLI e conformance | Não executados por instrução explícita do usuário |
| Benchmarks/probes de layout | Não executados; protocolo e harness entregues para outra rodada |
| Clippy, rustdoc e MSRV | Não executados nesta reconstrução |
| Prumo validate/doctor/docs verify | Não executados; nenhuma aprovação ou transição formal foi fabricada |
| Build/render da documentação VitePress | Não executado; documentação pode ser lida diretamente no GitHub |
| Auditoria integral dos upstreams | Não concluída; leitura limitada às regiões identificadas nas notas |
| Review humano e merge | Pendentes; PR publicado em rascunho |

O registro de inteligência foi atualizado com esta entrega em estado de revisão, sem contagem fictícia de testes passados. O estado histórico de `prumo.json` foi preservado. A autorização do usuário para publicar não equivale à certificação de todos os gates.

## Compatibilidade e continuidade

As APIs experimentais de compilação Reg agora exigem tratamento de `Result`; campos privados novos exigem `RegVm::new`/`Default`. Novos opcodes precisam do runtime desta revisão. Consumidores que usavam o bug de Dict Primary por valores devem migrar para o binding `key, value` canônico.

RegVM continua sem paridade completa de métodos, closures/upvalues, async, hooks, invariantes, journal de mutações e host. Guardas por comprimento não detectam toda remoção/inserção de mesmo tamanho. Fuel Wasm não limita memória, output ou trabalho nativo. O [guia, seções 14–16](../development/runtime-hardening-guide.md#14-o-que-ainda-pode-melhorar) ordena essas pendências e descreve como reimplementar e validar cada camada.

Para reproduzir esta entrega, use o SHA do commit do PR, o lock de dependências e os SHAs de `studies/refs.json`. O diff do PR e os arquivos publicados substituem os links temporários que deixaram de funcionar.
