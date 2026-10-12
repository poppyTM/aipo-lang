# P07-G02: implementação, validação e performance

**Data:** 2026-10-09 · **Estado:** DRAFT · **Testes:** não executados por instrução explícita do usuário.
**Fonte validada e medida:** `33d524b59caecd8b0d33f48f385a64010e14a3c1` · **Árvore:** `42bfc0d63f9b23484e342991713814372339042c`.

O ambiente foi recuperado e o código foi publicado em `feat/runtime-and-tooling-completion`. A árvore remota do checkpoint foi comparada com a local, seguida da correção final Wasm e integração da `main` `23bab1d2`. O [guia completo](../../development/runtime-and-tooling-guide.md) descreve o que mudou, por que, como usar, compatibilidade e reimplementação.

## Resultado das checagens

| Checagem | Resultado | Escopo |
| --- | --- | --- |
| `cargo +1.96.0 clippy --workspace --all-targets --locked -- -D warnings` | exit 0 | Todo o workspace; compila alvos de testes sem executá-los |
| `cargo +1.96.0 fmt --all -- --check` | exit 0 | Formatação do código |
| `cargo +1.96.0 doc --workspace --no-deps --locked`, `RUSTDOCFLAGS=-D warnings` | exit 0 | Rustdoc atual, gerado novamente após limpar o caminho de saída retido |
| CLI `check --all-targets --locked --no-default-features`, com shell/web/embedded e sem alias | exit 0 nos quatro casos | Builds reduzidos no MSRV 1.96 |
| Header C11 e C++17 `-fsyntax-only`, warnings tratados como erro | exit 0 | Sintaxe; não linka nem executa consumidor C; 29 exports encontrados/declarados |
| Python `ast.parse` em scripts release/perf | OK | Sintaxe; não executa instalador/empacotador |
| Links/guias, `git diff --check` | OK | Estrutura documental e whitespace |
| Portal: check-docs, check-usage-docs e verify-progress | exit 0 | 141 páginas, 26 exemplos exatos, 33 processos/49 provas; não executa Aipo |
| Portal: `npm run build` | exit 0 | VitePress; linguagem de fence Aipo cai para texto simples |
| Testes Rust/Aipo/diferenciais, fuzz, Miri, ASan e unitários do portal | Não executados | Instrução do usuário; não houve contagens de aprovação inventadas |

Comandos, ambiente e limites estão em [checks.json](checks.json). `npm run check` não foi invocado porque inclui testes unitários; seus três validadores estáticos foram chamados separadamente. Workflows multiplataforma não foram observados; nenhuma release foi publicada.

Clippy 1.96 apontou duas simplificações booleanas em parser e fonte de teste de cache; foram corrigidas, sem enfraquecer lint. Rustdoc inicialmente falhou ao remover um diretório de saída retido: o diretório gerado foi movido, e a repetição em saída nova, com um job, passou. Falhas anteriores não são tratadas como sucesso.

## Benchmark final de chamadas Reg

Baseline `f0a0d70d176be031cd4b600fde1df6179429fb9a` versus fonte `33d524b59caecd8b0d33f48f385a64010e14a3c1`, **Rust 1.96.0/LLVM 22.1.2**, release padrão, `CARGO_INCREMENTAL=0`, dois jobs por build. Mesma fonte `reg_calls_bench.rs` copiada byte a byte para a baseline. Builds foram concluídos antes da coleta; não houve compilação/npm desta sessão durante a medição. Isso não garante ausência de toda carga externa do host.

Cada execução contém 20 mil chamadas, caller de nove instruções, cinco registradores no caller e um no callee. Casos Int e String retornam valores consumidos por `black_box`. A medida inclui carregamento e verificação de `run_module`, não só a instrução Call. CPU 0, três lotes × cinco pares por caso, ordem alternada; nove amostras por processo, primeira removida como warmup. **480 observações retidas**.

| Caso | Mediana baseline | Mediana candidata | Razão baseline/candidata |
| --- | ---: | ---: | ---: |
| int | 17.361 ms | 3.616 ms | 4.80× |
| string | 20.129 ms | 4.426 ms | 4.55× |

As razões por lote variaram de 4.40× a 4.90×. Consulte os dados brutos, em vez de tratar uma única razão como garantia: [reg-calls-paired.csv](reg-calls-paired.csv) e [performance.json](performance.json) incluem compiler, hashes dos binários, workload, inventário de fontes e CSV.

Após a coleta, o CSV foi normalizado de CRLF para LF para armazenamento no Git. As 480 observações permanecem iguais; o relatório preserva o hash anterior e informa o hash do arquivo publicado. O driver passa a escrever LF nas próximas coletas. Essa mudança de serialização não altera o snapshot Rust nem exige repetir a medição.

As primeiras medições anteriores à recuperação foram substituídas por esta coleta. Elas usavam outra revisão/compilador e incluíram build concorrente; não certificavam o snapshot final. Não extrapolar para velocidade global da linguagem, startup, alocações/RSS, structs, async, callbacks, MCU ou JIT.

### Reprodução

```sh
git worktree add --detach /tmp/aipo-base f0a0d70d176be031cd4b600fde1df6179429fb9a
cp crates/aipo-vm/examples/reg_calls_bench.rs /tmp/aipo-base/crates/aipo-vm/examples/
# Na candidata e na baseline, com target-dir separados:
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo +1.96.0 build --locked --release -p aipo-vm --example reg_calls_bench
# Sem builds em andamento, escolha uma CPU permitida:
python3 scripts/perf/reg_calls.py --baseline /CAMINHO/BASE/reg_calls_bench --candidate target/release/examples/reg_calls_bench --rustc /CAMINHO/rustc --cpu 0 --batches 3 --pairs 5 --out /CAMINHO/evidencia
```

O `rustc` informado deve ser o mesmo 1.96 usado nos dois builds. O driver não cria baselines nem atualiza testes.

## Recuperação e continuidade

[recovery.json](recovery.json) preserva **o snapshot histórico do bloqueio**, quando o código ainda não tinha sido enviado. Não descreve o estado atual. O checkpoint inicial publicado foi `4ad98242a8867da0ab62653d7843ffed83e164b9`; a desconexão `409 environment_offline` foi resolvida. Não houve aprovação automática rejeitada nem necessidade de renovar a autorização para commit/push/PR.

[completion.json](completion.json) registra o estado atual, revisão e limites. P07-G02 permanece DRAFT porque testes/gates de certificação não foram executados. Reg nativo captures/async/hooks, GC de ciclos/retention, recursos externos transacionais, LSP/DAP/editor de linha completo, registry/ranges e no_std/HAL/MCU continuam evolução futura, com critérios descritos no guia.
