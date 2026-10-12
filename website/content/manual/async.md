---
title: "Async e tarefas cooperativas"
description: "Referência de uso organizada por tarefas e comportamento do Aipo."
---

# Async e tarefas cooperativas

O Aipo oferece funções `async fn`, tarefas `Task` e `await`. Sua implementação de scheduler cooperativo foi projetada para testes determinísticos com tempo virtual. **Use primeiro a VM de referência**; suporte a outros backends pode ser parcial.

## Uma função assíncrona

```aipo
async fn buscar_dados(recurso) {
    task.sleep(50)
    return f"Dados para {recurso}"
}

let tarefa = buscar_dados("usuarios")
let dados = await tarefa
io.println(dados)
```

A chamada cria uma tarefa; `await` obtém o resultado. `task.sleep` participa do agendador do Aipo e não deve ser confundido com `sleep` do sistema operacional.

## Combinadores de tarefas

| API | Uso |
| --- | --- |
| `task.spawn(callable, args)` | Criar tarefa |
| `task.sleep(ms)` | Suspender cooperativamente |
| `task.all(tasks)` | Agregar resultados |
| `task.race(tasks)` | Resultado da primeira conclusão |
| `task.timeout(task, ms)` | Aplicar limite temporal |
| `task.cancel(task)` | Cancelar uma tarefa |
| `task.group()` | Agrupar ciclo de vida |

Para executar uma sequência de espera, a linguagem tem `await do { ... }`. Veja [manual original detalhado](https://github.com/poppyTM/aipo-lang/blob/main/docs/manual/async-and-concurrency.md) e [evidência de implementação](https://github.com/poppyTM/aipo-lang/blob/main/docs/evidence/P02-G03-wave3-async-syntax-and-diagnostics.md).

**Atenção:** o backend de registradores ainda apresenta lacunas conhecidas para async. [Confira a cobertura](/reference/status).

[Próximo: módulos](/manual/modules-packages) · [Testes](/manual/testing).
