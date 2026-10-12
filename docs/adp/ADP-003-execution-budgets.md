# ADP-003 — Orçamentos de Execução para Programas Não Confiáveis (fuel, memória, interrupção)

**Status:** rascunho; política unificada de memória, interrupção, sinais e paridade ainda aberta. APIs parciais de orçamento já existem.
**Relacionado:** `docs/evidence/P01-G02-*.md` (suíte de exaustão de recursos), contratos das crates
(linha Owns de `aipo-vm` corrigida pelo mesmo goal), Fechamento Arquitetural §10–11
**Autoridade:** subordinada a `docs/canon/Aipo V1 — Language Reference…` e
`docs/language/authority-map.md` (política de não invenção)

## Fatos de implementação (não encerram o ADP)

- Stack VM e C ABI já tinham mecanismos de orçamento na base desta revisão, além do limite da operand stack. A afirmação histórica de inexistência de instruction budget ficou obsoleta.
- RegVM expõe `set_instruction_budget(Some(n))`, contador cumulativo e reset explícito. Exaustão usa `VmFault::Overflow`; callbacks nativos não são contabilizados pelo limite.
- O runner Wasm expõe fuel opt-in em `WasmExecutionOptions`, configurado antes de instanciar o módulo. Sem opções, a API legada continua sem limite.
- Não há, nesta revisão, orçamento global de memória, limite de output, interrupção de trabalho nativo ou regra equivalente no shim JavaScript.
- Os testes desta reconstrução não foram executados a pedido do usuário. Source e APIs publicados não equivalem à certificação dos gates.
- Veja o [guia de runtime](../development/runtime-hardening-guide.md) para uso, escopo e riscos.

## Questões em aberto (todas indecididas)

1. **Sinal de exaustão:** quando um orçamento estoura, o resultado é uma `Failure`
   recuperável, um fault de runtime com um novo código de diagnóstico, ou um abort do
   processo? Cada escolha altera o contrato de Failure/fault e exige respaldo na Language
   Reference que ainda não existe.
2. **Escopo do orçamento:** por chamada, por execução de módulo, por sessão do host? Quem o
   define — sintaxe do código-fonte, flags da CLI ou apenas a API do host?
3. **Contabilização de memória:** o que conta (coletas, closures, strings, bytecode), e a
   contabilização é exata ou amostrada?
4. **Interrupção:** cooperativa (verificada entre instruções) ou preemptiva? Que estado é
   observável depois de uma interrupção?
5. **Backend JS:** qualquer orçamento precisa de uma regra de paridade VM↔JS definida; o shim
   atualmente também não tem orçamento.

## Não-objetivos deste ADP

- Inventar semântica de fuel dentro de um goal de teste/qualidade. Orçamentos alteram o
  comportamento observável e precisam de um goal de design próprio, com patrocínio do canon.
- Afirmar sandboxing: a saída do Aipo "não é, por si só, um sandbox de segurança"
  (Fechamento §10). Scripts não confiáveis ainda exigem sandboxing de VM/host ou isolamento
  externo.

## Critérios de saída

Este ADP se encerra quando um goal de design especificar o sinal, o escopo, a contabilização
e a regra de paridade acima, com fixtures que provem cada um — ou quando tirar explicitamente
os orçamentos da V1, com uma justificativa documentada.
