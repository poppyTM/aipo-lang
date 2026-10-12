# Mapeamento de Riscos, Ameaças e Salvaguardas Arquiteturais

Auditoria de riscos na incorporação de técnicas inspiradas no **omniLua** para a **Aipo-lang** (Fase 0).

---

## 1. Riscos de Integridade Linguística e Semântica

### Risco 1: Contaminação Semântica por Lua
- **Descrição:** Tentar "simplificar" coleções, strings ou tratamento de erros adotando o padrão do omniLua (ex.: permitir byte-strings opacas em vez de UTF-8 rigoroso; tolerar metatables dinâmicas).
- **Impacto:** Destruição da identidade da Aipo, quebra do modelo de tipos e contratos estáticos (ADP-012), e perda da coerência cognitiva (ADR-001/ND-First).
- **Mitigação Obrigatória:** Rejeição explícita (`REJECT`) de qualquer semântica própria de Lua. Todo código candidato deve ser validado contra o corpus canônico de conformidade da Aipo em `docs/conformance/`.

### Risco 2: Quebra da Paridade Multi-backend (VM ↔ JS ↔ Wasm)
- **Descrição:** Adicionar funcionalidades ou mudar o comportamento do despachante na VM Rust de modo que os emissores `aipo-js` e `aipo-wasm` fiquem defasados ou incompatíveis.
- **Impacto:** Fragmentação da linguagem e quebra do contrato de portabilidade universal da Aipo.
- **Mitigação Obrigatória:** Nenhuma alteração semântica entra sem que as suítes diferenciais (`crates/aipo-js/tests/differential.rs` e `crates/aipo-wasm/tests/`) permaneçam 100% verdes com os mesmos resultados bit-a-bit.

---

## 2. Riscos de Segurança e Confiabilidade do Sandbox

### Risco 3: Falsa Sensação de Segurança em Sandbox In-Process
- **Descrição:** Acreditar que limitar instruções (fuel) e memória amostrada impede todas as formas de ataque por scripts não confiáveis.
- **Fato Técnico:** O fuel por opcode só interrompe a execução no despachante da VM; uma chamada a uma função nativa de host (`HostNative`) bloqueante (ex.: computação intensiva de regex, loop nativo de imagem, ou leitura de socket externo) não pode ser preemptada cooperativamente por fuel de opcode sem pontos de suspensão explícitos.
- **Mitigação Obrigatória:**
  1. Manter `CapabilitySet` estrito (deny-by-default): scripts não confiáveis não recebem acesso a recursos bloqueantes.
  2. Documentar que sandbox in-process é uma barreira de contenção de erros acidentais e timeouts de lógica, não um isolamento criptográfico no nível de SO.
  3. Para isolamento estrito contra adversários maliciosos, recomendar processo separado ou isolamento WASI.

### Risco 4: Quebra da Invariante `#![forbid(unsafe_code)]`
- **Descrição:** Tentar copiar otimizações de baixo nível ou ponteiros brutos para acelerar o GC ou a VM.
- **Impacto:** Violação da política institucional de Zero Unsafe estabelecida no `Cargo.toml` raiz do workspace da Aipo.
- **Mitigação Obrigatória:** Rejeição incondicional de blocos `unsafe`. Todas as adaptações para a Aipo devem ser redigidas em 100% Safe Rust idiomático, validadas pelo compilador.

---

## 3. Riscos de Performance e Manutenção

### Risco 5: Degradação do Caminho Quente por Metering Invasivo
- **Descrição:** Inserir contadores de fuel, chamadas atômicas ou checagens complexas a cada instrução executada no laço de despacho (`Vm::step`), elevando o piso de dispatch acima dos ~100ns/opcode.
- **Mitigação Obrigatória:**
  - Adotar a técnica do omniLua de **amostragem periódica** (`interval metering`): tarifar o fuel em blocos (ex.: a cada 64 ou 256 passos) ou acoplar a contagem à máscara de traps já existente, garantindo que o custo em tempo normal seja próximo de zero.
  - Exigir benchmarking comparativo pareado (`scripts/perf/paired.sh`) antes de aceitar a alteração.

### Risco 6: Conformidade de Licenças e Proveniência de Terceiros
- **Descrição:** Copiar código do omniLua sem manter as devidas atribuições de licença.
- **Impacto:** Violação de copyright MIT de terceiros e dos termos do aviso PUC-Rio (Lua 5.4.7).
- **Mitigação Obrigatória:**
  - Todo trecho de código adaptado ou copiado deve manter a atribuição explícita aos contribuidores do `omnilua` e à PUC-Rio no cabeçalho do arquivo e em `THIRD_PARTY_CODE.md`.
