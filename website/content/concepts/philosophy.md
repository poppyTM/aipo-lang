---
title: Filosofia e ergonomia
---
# Uma linguagem pensada para ser lida

Aipo procura simplificar leitura e manutenção de código: pouca cerimônia para o caso comum, nomes claros, comportamento previsível e contratos explícitos onde eles ajudam.

O [ADR-001](https://github.com/poppy-team/aipo-lang/blob/main/docs/decisions/adr-001-canonical-syntax.md) fixa quatro princípios: uma forma para cada coisa, palavras claras em vez de símbolos densos, sintaxe concisa onde repetida e erros orientados à correção.

## Acessibilidade não é só aparência

Para pessoas com TDAH, dislexia ou sobrecarga cognitiva, a dificuldade frequentemente está na quantidade de escolhas e nas abstrações não visíveis. Regras previsíveis e mensagens de diagnóstico acionáveis ajudam a reduzir esse custo.

O benefício deve ser verificado com tarefas reais e leitores diversos: nenhuma preferência de sintaxe funciona igualmente bem para todas as pessoas. A documentação oferece progressão, modo foco, bom contraste, controle de espaçamento e exemplos curtos, mas isso exige avaliação contínua.

## Não é uma linguagem baseada em classes

Aipo favorece estruturas de dados, funções livres, métodos associados a tipos e interfaces estruturais, em vez de hierarquias de classes como mecanismo central. Essa escolha reduz parte da cerimônia, mas torna importante documentar responsabilidades e efeitos de mutação.
