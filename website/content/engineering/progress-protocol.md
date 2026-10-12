---
title: Protocolo de progresso e evidência
description: Contrato obrigatório de manutenção da tabela de progresso da linguagem Aipo.
---

# Protocolo de acompanhamento da implementação

> **Contrato operacional** · Adotado em 2026-10-09 para `website/`. Inspirado no [protocolo do Petunia3D](https://github.com/WASDst/petunia3d/blob/refactor/architecture-foundation/website/docs/17-reimplementation/index.md), com regras próprias para frontend, IR, stack VM, RegVM, JavaScript, WebAssembly, host/ABI, pacotes e documentação.

[Abrir a tabela de progresso](/progress/) · [Estado dos backends](/reference/status) · [Qualidade e testes](/engineering/quality) · [Auditoria do legado](/engineering/legacy-audit)

## 1. Fonte de verdade única

`website/public/progress/tasks.json` é o inventário **versionado**, legível pelo site, contendo:

- `schemaVersion`, repositório, branch-base, `baselineSha` (40 caracteres) e `updated`;
- etapas `phases`, cada qual com ID, título e descrição;
- processos `tasks`, com ID estável, domínio, diagnóstico inicial (`baseline`), documento canônico, próxima ação, data e checkpoints;
- `evidence`, com identificador, revisão, tipo de evidência, resumo, documento e caminhos de código/documentação;
- `gates`, com nome, estado, evidência opcional.

A UI importa esse JSON como dados locais durante a compilação do VitePress; **não permite editar estados pelo navegador**, não persiste flags locais e não consulta GitHub Actions automaticamente. Não adicione um percentual numérico ao JSON; a visualização calcula o valor.

**Autoridade:** o código comprova existência de implementação; especificações e ADPs determinam intenção normativa; testes e logs comprovam comportamento na revisão executada. Um documento histórico não passa a ser a autoridade atual só por ser citado.

## 2. Os três estados

| Estado | Critério obrigatório |
| --- | --- |
| `TODO` | Zero checkpoints concluídos. Código legado pode existir sem que o novo critério tenha começado. |
| `IN PROGRESS (000%)` a `IN PROGRESS (099%)` | Trabalho iniciado ou em validação; ao menos um critério fechado na fonte atual. O painel inicial não usa 000%, mas a convenção do Petunia3D admite 000% quando o início é documentado antes do primeiro checkpoint. |
| `DONE` | Todos os checkpoints fechados com referência existente e **todos** os gates aplicáveis em `pass`. |

**Fórmula:** `floor(100 × checkpoints_concluídos / checkpoints_totais)`. Ex.: 2/6 = `IN PROGRESS (033%)`. É proibido arredondar 99% para 100%. Critérios têm peso igual e devem ser pequenos, observáveis e independentes; não ajustar o denominador para melhorar artificialmente a porcentagem.

### Por que a auditoria inicial marca muitos processos como parciais?

O snapshot inicial distingue **implementação identificada** de **execução atual dos testes**. Encontrar `aipo-vm`, `aipo-wasm` ou uma suíte antiga não certifica paridade completa ou zero regressões. O registro [P07-G01](https://github.com/poppyTM/aipo-lang/blob/main/docs/evidence/P07-G01-runtime-hardening.md) afirma expressamente que testes foram deixados sem execução na revisão descrita. Por isso a evidência de presença do código pode encerrar um checkpoint de inventário, mas não o gate de teste. Os percentuais **não são um downgrade automático** de funcionalidades concluídas historicamente.

O único processo `DONE` na baseline inicial é o inventário básico de arquitetura: seu **escopo limitado** é o mapa de crates e boundaries, não a qualidade ou performance da linguagem.

## 3. Evidência válida

| Categoria | O que demonstra | O que **não** demonstra |
| --- | --- | --- |
| `source-inspection` | Caminho e estrutura existem na revisão | Que o recurso executa corretamente |
| `static-and-historical` | Código e relatório anterior estão localizados | Que o relatório certifica o HEAD |
| `test-run` | Testes rodados com comando, ambiente, SHA, saída, conclusão | Que outras suites/backends foram cobertos |
| `manual-review` | Verificação humana com cenário e ambiente registrados | Que cenários não inspecionados funcionam |

Um checkpoint `completed: true` exige um ID de `evidence` existente e associado ao `document` daquela tarefa. Checkpoint aberto exige `evidence: null`. O campo `sourcePaths` deve apontar para arquivos/diretórios reais; o validador **não lê arquivos do GitHub remotamente**.

Não inventar logs, benchmarks, percentuais, resultados de CI, aprovação humana, checks de segurança ou transições Prumo. Uma execução antiga pode ser relatada como *histórica* — nunca como gate atual.

## 4. Gates e regressões

Para cada gate, usar **exatamente** um dos quatro resultados:

- `not-run`: não executado para o SHA declarado; não atribuir `pass`;
- `pass`: comando/cenário executado, revisão e evidência documentados;
- `fail`: execução produziu divergência ou regressão reproduzível;
- `blocked`: condição externa ou dependência impede a prova, com justificativa e próximo passo.

`DONE` exige todos os gates do próprio processo em `pass`. Não existe quarto status para bloqueios: manter `IN PROGRESS` e registrar bloqueio no gate, baseline e próximo passo. Em regressão, voltar `DONE → IN PROGRESS`, reabrir checkpoint(s) e gate(s), justificar mudança e registrar data. Nunca esconder uma falha aumentando o percentual.

Os gates são **específicos** ao escopo. Uma auditoria de documentação pode exigir checagem de caminhos, navegação e CI do portal; uma mudança em RegVM exige testes Rust, conformance, paridade e casos negativos; uma API C exige teste do consumidor de C e boundary safety. O build do VitePress não substitui testes da linguagem.

## 5. Atualização obrigatória em cada mudança

1. **Antes de começar:** ler `PROJECT_STATE.md`, decisões/ADPs vigentes, `website/content/reference/status.md`, a tarefa, testes e código afetados. Confirmar conflitos entre implementação e documentação.
2. **Ao abrir implementação:** citar IDs afetados na proposta do PR, marcar início quando houver trabalho concreto, registrar `updated` e baseline. É permitido `IN PROGRESS (000%)` sem checkpoint concluído desde que baseline, impedimento ou próximo passo sejam registrados claramente; não inventar prova apenas para elevar o percentual.
3. **Durante cada slice:** modificar implementação e testes; fechar apenas os checkpoints provados. Criar `evidence` com SHA e origem, e registrar no documento canônico o comando, ambiente, resultado, limites e riscos.
4. **Ao alterar API, sintaxe, backend, ABI ou comportamento:** atualizar também livro, referência, matriz de suporte e exemplos atingidos. Revisar interoperabilidade VM, RegVM, JS, Wasm e C **somente quando aplicável**.
5. **Quando o escopo mudar:** dividir requisitos e criar IDs novos; conservar IDs antigos no histórico. Não remover tarefas ou checkpoints concluídos apenas para subir porcentagens.
6. **No mesmo PR:** atualizar `tasks.json`, `updated`, checkpoints, gates e provas. Um PR que altera comportamento sem registrar o delta de progresso deve falhar na revisão, mesmo que o código compile.
7. **Handoff:** informar IDs tocados, antes/depois, gates `pass/fail/not-run/blocked`, links de provas, SHA e próximos critérios pendentes.

## 6. Procedimento técnico

A partir da raiz:

```bash
cd website
npm install
npm run check
npm run build
```

O script `npm run check` invoca validação de links, metadados, `tasks.json`, fontes e testes unitários do painel. O CI de `website/**` exige esses gates e o build. Eles não certificam compilação Rust, Wasm, C, benchmark ou compatibilidade de VMs.

Conforme o código afetado e com permissão de executar testes:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Também executar o runner específico de conformance e as suítes diferenciais, de host, segurança, WASI e C quando relevantes. Registre a seleção real de comandos; não declarar executados os comandos apenas mostrados como recomendação.

## 7. Evolução do formato sem quebrar links

IDs estáveis seguem `A01`, `F01`, `R01`, `B01`, `I01`, `Q01`, `D01`; o link `/progress/#R04` aponta à linha correspondente. Alterar ID rompe permalinks. Novos campos obrigatórios exigem `schemaVersion` novo, migração do validador, tests e consumidores. Não excluir provas consumidas por checkpoints ou gates.

As datas são ISO-8601 `YYYY-MM-DD`. `baselineSha` identifica a revisão auditada, **não muda automaticamente ao fazer merge**: atualizar esse campo só após executar auditoria correspondente e reconciliar evidências afetadas.

## 8. Escopo e critérios de saída da documentação antiga

A migração do legado é um processo próprio (`D02`). **Não remover `docs/` indiscriminadamente:** o corpus é usado por testes, há decisões antigas com conflitos e páginas inglesas ainda não reescritas. A exclusão por lote requer inventário por caminho, consumidores, paridade semântica, status de autoridade, redirecionamentos e checks positivos no CI. Consulte [a auditoria](/engineering/legacy-audit).

## 9. Relação com o Petunia3D

Foram preservados os princípios de três estados, checkpoints de peso igual, prova obrigatória, busca/filtros, permalink, separação entre status e impedimento, atualização junto do código e retorno de `DONE` em regressão. Foram adaptados o armazenamento para build VitePress, o controle de múltiplos backends e gates Rust/JS/Wasm/C, a evidência histórica e a política de migração documental.

A página é uma **reimplementação própria**, não uma cópia do componente HTML/JS do Petunia.


## 10. Proteção automática de sincronização em pull requests

O workflow [`progress-sync.yml`](https://github.com/poppyTM/aipo-lang/blob/main/.github/workflows/progress-sync.yml) verifica automaticamente PRs que alteram **implementação** em `crates/`, `packages/`, `examples/`, `fuzz/`, fixtures em `docs/conformance/` ou o manifesto raiz `Cargo.toml`.

Ao detectar alterações de código/contrato, exige no mesmo diff a atualização de `website/public/progress/tasks.json`. Sem esse arquivo, o check `Aipo progress sync` falha e apresenta os caminhos relevantes. PRs apenas de texto/documentação editorial não exigem mutação artificial do progresso. Testes de política acompanham o validador:

```bash
python3 -m unittest discover -s website/tests -p 'test_progress_delta.py' -v
```

O gate prova **sincronização de arquivo**, não que cada ID afetado foi tocado ou que a evidência seja verdadeira; revisão de escopo, consistência dos checkpoints e execução real de testes permanecem obrigatórias. Mudanças de dependência no manifesto também passam pela regra, por poderem alterar comportamento ou ABI; discutir uma exceção justificada no PR antes de relaxar a política.
