---
title: Trabalhar com módulos
---
# Trabalhar com módulos

Um arquivo `.aipo` é um módulo; um `aipo.toml` descreve um pacote. Símbolos são privados por padrão até serem exportados.

## Exemplo com dois arquivos

**calculadora.aipo**

```aipo
fn dobro(n) { return n * 2 }
export dobro
```

**main.aipo**

```aipo
import calculadora
io.println(dobro(5))
```

Execute o arquivo principal. O resolvedor atual não deve ser confundido com um import namespace qualificado no estilo Python; há limitações documentadas no [audit de sintaxe](https://github.com/poppy-team/aipo-lang/blob/main/docs/journal/2026-10-05-syntax-drift.md).

## Quando introduzir pacotes

Use um pacote ao precisar distribuir ou versionar uma coleção de módulos. O manifesto descreve identidade e dependências; o lockfile registra a resolução. Veja [CLI](/reference/cli) para `package lock`, `package audit` e operações de cache.

## Cuidados

- Faça `export` apenas de interfaces públicas necessárias.
- Evite ciclos de importação.
- Não presuma que aliases criam namespaces isolados.
- Teste importação, carga e runtime; `check` sozinho pode não cobrir todos os casos conhecidos.
