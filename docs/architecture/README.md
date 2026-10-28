# Arquitetura de Sistemas (`docs/architecture/`)

## O que é este diretório?
Contém as definições de topologia, limites de domínio (boundaries), regras de composição modular e registros de decisões arquiteturais (ADRs).

## Para que serve?
Garante que o sistema mantenha alta coesão e baixo acoplamento durante todo o seu ciclo de evolução, prevenindo dependências circulares e degradação arquitetural.

## Inventário
- `overview.md`: Visão histórica das Waves 1–2.
- `crates.md`: Inventário atual de 24 crates e dependências normais (fonte: `cargo metadata`).
- `backend-support-matrix.md`: Cobertura medida por backend em 2026-10-27.
- `bytecode-and-vm.md`: Formato `.aibc` e modelo de execução.
- `embedded-and-shell-profile.md`: Perfis de build, limites e embedding.
- `clean-code-contract.md`: Contrato estrito de engenharia de software e Clean Code.
- `adr/`: Registros formais de decisões arquiteturais (Architectural Decision Records).
