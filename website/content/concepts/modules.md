---
title: Módulos e pacotes
---
# Organização de código: unidades distintas

Um **módulo** corresponde a um arquivo Aipo; um **pacote** agrupa módulos, identidade e dependências. Separar esses conceitos evita que o programa precise de configuração de pacote para cada arquivo.

## Fronteiras de visibilidade

Exportações comunicam a API pública. Referências locais continuam encapsuladas. Importações exigem regras de resolução e inicialização previsíveis. Os aliases atuais não devem ser presumidos idênticos aos namespaces de outras linguagens.

## Reprodutibilidade

A combinação de lockfile e cache verificável permite saber de onde as dependências vieram. Integridade de arquivos não garante automaticamente confiança no código: o usuário continua responsável pela escolha das fontes e pelas capacidades concedidas.

Consulte [Guia de módulos](/guides/modules) e [Referência do CLI](/reference/cli).
