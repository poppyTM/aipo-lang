---
title: Criar um projeto pequeno
---
# Criar um projeto pequeno

## Objetivo

Criar um script de controle de tarefas sem introduzir frameworks, serviços ou estrutura de diretórios complexa.

## Arquivos

Comece com `main.aipo`:

```aipo
struct Tarefa {
    titulo
    var concluida = false
}
var tarefas = []
tarefas.add(Tarefa{titulo: "Estudar funções"})
each tarefa in tarefas {
    io.println(tarefa.titulo)
}
```

Execute `cargo run -q -p aipo-cli -- run main.aipo` na raiz do repositório.

## Evoluir sem perder clareza

1. Extraia operações que se repetem para pequenas funções livres.
2. Mantenha nomes ligados a intenções, não a detalhes de implementação.
3. Passe para módulos quando houver responsabilidade independente.
4. Acrescente testes com exemplos de entrada e saída reproduzíveis.
5. Execute `check`, `fmt --check` e `test` quando a estrutura estiver estável.

## Definição de pronto

O projeto deve ter uma instrução de execução, um exemplo de saída, e nenhum requisito de dependência implícito. Consulte [Módulos](/guides/modules) para dividir arquivos e [Testes](/guides/testing) para proteger as mudanças.
