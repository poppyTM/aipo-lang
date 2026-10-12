---
title: Acessibilidade de leitura e documentação
description: Requisitos de usabilidade para pessoas neurodivergentes, auditáveis em cada mudança.
---

# Acessibilidade para aprender e consultar

O Aipo deve ser acessível não apenas na sintaxe e nos erros, mas **também no modo como a documentação é apresentada**. Esta página estabelece padrões editoriais e de interface para reduzir distração, esforço de navegação e sobrecarga de informação.

## Critérios de interface

- O caminho principal tem quatro opções visíveis: **Começar, Manual, Exemplos e Referência**. Arquitetura e manutenção ficam em **Projeto**.
- O corpo do texto possui largura de leitura limitada e linhas espaçadas. Evitar parágrafos densos e tabelas largas quando uma explicação breve basta.
- Botões de preferências de leitura ficam agrupados atrás de um único controle **Leitura**, com estados acessíveis por teclado; em telas pequenas, o controle permanece disponível e recolhido.
- **Foco** esconde a navegação lateral, não o conteúdo. **Texto maior** aumenta tipografia e entrelinha sem exigir alterações no navegador. Há opção **Restaurar**.
- A seleção por teclado precisa ser visível; o contraste deve ser revisado em ambos os temas. Preferências de redução de movimento do sistema são respeitadas.
- Não depender apenas de cor para indicar estados. Rótulos são escritos por extenso (ex.: `DONE`, `IN PROGRESS`).
- Não usar autoplay, popups de onboarding, elementos piscantes ou animações decorativas.
- Links devem identificar o destino. Evitar `clique aqui` sem contexto.

## Critérios de escrita

- Começar pelo **objetivo prático**; depois mostrar o exemplo, as regras e links opcionais para aprofundar.
- Uma página responde primeiro a **uma pergunta**. Recursos diferentes devem ter páginas próprias.
- Explicar siglas na primeira ocorrência e evitar misturar a sintaxe real com propostas futuras.
- Destacar dependências de host e diferenças de backend somente quando forem relevantes para a tarefa.
- Para cada funcionalidade ensinada, apontar para **código mantido** no repositório. Não copiar exemplos imaginados quando já há uma fixture real.
- Organizar páginas por **assunto que a pessoa quer resolver**, não por nome de crate, meta histórica ou etapa interna.
- Erros, limitações e estado de testes devem ser objetivos e próximos da funcionalidade, sem avisos longos em toda página.

## Checkpoints em cada alteração

1. Navegação com `Tab`, `Shift+Tab` e `Enter`: nenhum caminho principal deve exigir mouse.
2. Legibilidade nas larguras de 320px, 390px, 768px e desktop; tabelas podem ter rolagem própria, mas nunca cortar botões essenciais.
3. Revisão dos temas claro e escuro, zoom 200% e reflow; não confiar somente em uma paleta.
4. A opção `prefers-reduced-motion` não pode provocar efeitos indesejados.
5. Abrir e fechar **Leitura**; conferir foco, texto maior, restaurar e acesso por teclado.
6. Inspecionar títulos, sequência de headings, links de exemplo e linguagem de cada tutorial.
7. Testar leitores de tela com pessoas e ambientes reais quando houver mudanças relevantes. **Build estático não equivale a certificação WCAG**.

A CI contém `website/scripts/check-usage-docs.mjs`, que verifica integridade básica dos conteúdos, fontes de exemplos e hooks estáticos. Esse check não simula NVDA, VoiceOver, Orca nem testes cognitivos reais.

[Começar](/start/) · [Manual](/manual/) · [Exemplos](/examples/) · [Melhorar a documentação no GitHub](https://github.com/poppyTM/aipo-lang).
