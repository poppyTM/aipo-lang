# Concorrência & Async

O modelo de concorrência do Aipo é **cooperativo, determinístico e baseado em tempo virtual**. Ele elimina condições de corrida de baixo nível (*data races*) e garante testes 100% reprodutíveis sem atrasos reais na suíte de testes.

---

## Funções Assíncronas (`async fn`)

Funções que realizam temporização, I/O cooperativo ou orquestração assíncrona são declaradas com `async fn` e delimitadas por chaves `{ ... }`. Invocar uma função assíncrona agenda a tarefa imediatamente no scheduler cooperativo e retorna seu handle `Task`:

```aipo
async fn buscar_dados(recurso) {
    # task.sleep é uma primitiva de suspensão cooperativa em tempo virtual
    task.sleep(50)
    return f"Dados para {recurso}"
}

# Invocação dispara a tarefa e retorna o handle Task
let tarefa = buscar_dados("usuarios")

# Aguarda explicitamente a conclusão e obtém o resultado
let dados = await tarefa
io.println(dados) # "Dados para usuarios"
```

---

## Bloco de Espera Sequencial (`await do { ... }`)

Diferente de ecossistemas onde `await` pode ser inserido aleatoriamente no meio de subexpressões complexas, o Aipo introduz o bloco `await do { ... }` para encadeamento sequencial claro e seguro:

```aipo
async fn obter_etapa_1() {
    return 10
}

async fn obter_etapa_2() {
    return 20
}

async fn executar_fluxo() {
    await do {
        let a = await obter_etapa_1()
        let b = await obter_etapa_2()
        return a + b
    }
}

let total = await executar_fluxo()
io.println(f"Total acumulado: {total}") # 30
```

Essa disciplina de engenharia evita promessas pendentes descontroladas e tarefas esquecidas no ar (`AIPO_SEM_FORGOTTEN_TASK`).

---

## Combinadores Assíncronos (`task.*`)

A biblioteca padrão fornece combinadores primitivos de alto nível:

- **`task.spawn(callable, args_list)`**: Dispara uma nova tarefa concorrente no scheduler com os argumentos fornecidos.
- **`task.sleep(ms)`**: Suspende a tarefa corrente pelo número de ticks de tempo virtual especificados.
- **`task.all(lista_tasks)`**: Aguarda até que todas as tarefas da lista tenham sido concluídas, retornando a lista dos resultados.
- **`task.race(lista_tasks)`**: Retorna assim que a primeira tarefa concluir, cancelando cooperativamente as demais concorrentes.
- **`task.timeout(tarefa, ms)`**: Cancela a tarefa alvo caso ela exceda o tempo virtual estipulado.
- **`task.cancel(tarefa)`**: Cancela a execução cooperativa de uma tarefa ativa.
- **`task.group()`**: Cria um grupo estruturado de tarefas para ciclo de vida coordenado e cancelamento em cascata.

```aipo
async fn obter_etapa_1() { task.sleep(10) return 10 }
async fn obter_etapa_2() { task.sleep(10) return 20 }

let t1 = task.spawn(obter_etapa_1, [])
let t2 = task.spawn(obter_etapa_2, [])

# Aguarda todas as tarefas em paralelo determinístico
let resultados = task.all([t1, t2])
io.println(resultados) # [10, 20]
```

---

## Detecção Transitiva de Ciclos (`AIPO_RT_AWAIT_CYCLE`)

A máquina virtual do Aipo mantém um grafo de dependências de espera ativo. Se duas ou mais tarefas entrarem em espera mútua transitiva (deadlock assíncrono), o runtime detecta o ciclo imediatamente e dispara a falha determinística `AIPO_RT_AWAIT_CYCLE` com a cadeia completa dos identificadores envolvidos.
