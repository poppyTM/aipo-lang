# Aipo — Visão Geral da Arquitetura (Waves 1–2)

**Status:** histórico — este documento descreve o recorte das Waves 1–2.
**Arquitetura atual:** consulte [`docs/architecture/crates.md`](crates.md) e o portal em `website/content/engineering/`.
**Autoridade:** subordinada aos documentos canônicos em `docs/canon/`
**Escopo:** arquitetura de implementação das Waves 1–2: pipeline, propriedade dos dados, fronteiras.
Wasm, host ABI, pacotes, C ABI e adaptadores de GUI/game foram adicionados depois e não aparecem neste recorte.
**Questões bloqueantes:**
- Quem é o dono do estado canônico?
- Quais componentes podem mutá-lo?

## Fronteiras do sistema e principais componentes

O sistema Aipo é particionado em componentes de workspace estritamente delimitados e acíclicos:

1. **Source & Diagnostics**: `aipo-source` (gerencia arquivos, byte spans, indexação de linhas) e `aipo-diagnostics` (catálogo de códigos de erro estáveis e serialização JSONL).
2. **Frontend do Compilador**: `aipo-lexer` (tokenização), `aipo-syntax` (CST lossless e parser com recovery), `aipo-ast` (AST tipada), `aipo-hir` (desugaring e lowering) e `aipo-sema` (resolução de escopo, contratos, mutabilidade).
3. **Middle-end Neutro em Relação ao Alvo**: `aipo-ir` (Core IR).
4. **Bytecode & Runtime de Execução**: `aipo-bytecode` (codificação de instruções e verifier) e `aipo-vm` (máquina virtual baseada em stack sobre valores compartilhados `Rc<RefCell<…>>` — single-threaded por decisão, sem crate de GC).
5. **Serviços de runtime & Stdlib**: `aipo-runtime` (registro de módulos, registro de nativos) e `aipo-stdlib` (Prelude, `math`, `string`, `io`, métodos de coleções).
6. **Backend JavaScript (Wave 2)**: `aipo-js` (Core IR → bundle ESM + runtime shim versionado + source maps; depende de `aipo-ir` e `aipo-diagnostics`, nunca de bytecode).
7. **Tooling & Orquestração**: `aipo-formatter` e `aipo-cli` (`aipo run`, `aipo check`, `aipo build`, `aipo fmt`).
8. **Harnesses exclusivos de teste**: `aipo-testkit` (RNG determinístico, gerador de programas, runners de pipeline/JS — nunca distribui semântica da linguagem) e `aipo-bench` (binário runner de benchmark).

## Direção das dependências

A direção das dependências é estritamente unidirecional e acíclica:
`CLI → Runtime / VM → Bytecode → Core IR → Sema → HIR → AST → Syntax → Lexer → Source → Diagnostics`,
com `aipo-js` ramificando-se a partir do Core IR (`JS → Core IR` apenas).
Nenhuma camada de backend ou de execução pode ser importada para o frontend ou para a árvore de sintaxe.

## Integrações externas

A Wave 1 tem integrações externas mínimas:
- Sistema de arquivos do sistema operacional (via leituras de arquivo da biblioteca padrão para arquivos-fonte `.aipo`).
- Streams padrão (`stdin`, `stdout`, `stderr`) para execução de programas e diagnósticos.

## Propriedade e mutação do estado canônico

### Quem é o dono do estado canônico?
- **Tempo de compilação**: o `SourceMap` em `aipo-source` é dono do texto-fonte canônico. Cada fase de compilação produz artefatos de dados imutáveis (`Source` → `Tokens` → `SyntaxTree` → `Ast` → `Hir` → `CoreIr` → `BytecodeModule`, ou `CoreIr` → bundle JS).
- **Runtime**: a máquina virtual `aipo-vm` é dona do estado de execução (call stack, frames e valores compartilhados `Rc<RefCell<…>>`); o backend JS espelha o mesmo modelo de valores em seu shim versionado.

### Quais componentes podem mutá-lo?
- As estruturas de dados de tempo de compilação são append-only ou transformações imutáveis através das fronteiras de fase.
- Em runtime, apenas o frame de avaliação ativo da VM e o loop de execução podem mutar variáveis locais e estruturas de dados mutáveis (bindings `var`, elementos mutáveis de list/dict), de acordo com a semântica de mutabilidade do Aipo.

## Pipeline (Waves 1–2)

```
.aipo file
  → aipo-source     (load, UTF-8, BOM/CRLF normalize, SourceMap)
  → aipo-lexer      (TokenKind + SourceSpan; keywords; f/r/fr strings)
  → aipo-syntax     (lossless tree + recovery; recursion bounds per ADP-005)
  → aipo-ast        (typed AST, module structural split)
  → aipo-hir        (early lowering: trailing blocks → fn literals, |>, ellipsis)
  → aipo-sema       (scopes, mutability paths, contracts, modules, interfaces)
  → aipo-ir         (target-neutral Core IR)
  → ┬→ aipo-bytecode (instruction selection + encoding + verifier)
  │   → aipo-vm     (stack interpreter; aipo-runtime + aipo-stdlib linked in)
  └──→ aipo-js       (ESM bundle + versioned shim + source maps)
  → aipo-cli        (aipo run / aipo check / aipo build / aipo fmt)
```

Os diagnósticos fluem de todos os estágios para `aipo-diagnostics`; a saída ao usuário é
renderizada para humanos ou em `--message-format=jsonl` (códigos estáveis, legível por máquina).

## Failure vs fault (modelo de runtime)

- **Failure** = recuperável, criada por `fail(...)` ou por operações de runtime documentadas como
  falíveis; propaga-se automaticamente (Model B); capturada por `or_else` / `attempt...failed`.
- **Fault** = erro de programação (overflow, divisão por zero, erros de índice/chave, mutação durante
  iteração, violação de contrato descoberta em runtime, condição não Bool); não capturável;
  encerra a execução com um diagnóstico estruturado de fault de runtime. Nunca um panic do Rust.
