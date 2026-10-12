# Protocolo de Evidências & Performance

O desenvolvimento do Aipo é regido pelo princípio de **Engenharia Baseada em Evidências**: nenhuma alegação de desempenho, segurança ou conformidade é aceita sem dados mensuráveis, fixtures reproduzíveis e scripts automatizados.

---

## Seções de Evidências

- **[P07-G01: reconstrução do runtime](P07-G01-runtime-hardening.md)**: correções, estudos pinados e guia de reimplementação; testes não executados por pedido do usuário, com limites de validação explícitos.

- **[Benchmarks Cross-Language](/evidence/cross-language)**: Comparativo de execução entre Aipo CLI/VM, Aipo in-process, Aipo transpilado para JavaScript (Node.js), Lua, LuaJIT, Wren, Luau, CPython, PyPy, Ruby e Rust nativo.
- **[A Saga de Otimização & Lições](/evidence/performance-lessons)**: A análise honesta e detalhada dos 7 experimentos de micro-otimização revertidos por ausência de ganho comprovado, os limites do laço de despacho e a metodologia de medição sob contenção de CPU.
- **[Matriz de Conformance](/evidence/conformance)**: Catálogo com mais de 40 suítes de conformance, abrangendo programas canônicos, diagnósticos semânticos e paridade diferencial VM ↔ JS.
- **[Histórico de Slices](/evidence/slices)**: Registro dos relatórios de certificação de cada fatia vertical do projeto (de P00-G01 a P04-G11).
