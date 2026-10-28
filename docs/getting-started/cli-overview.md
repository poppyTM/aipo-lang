# Guia do Utilitário de Linha de Comando (`aipo`)

O executável `aipo` é a ferramenta unificada para execução, análise estática, emissão de JavaScript, desmontagem de bytecode, formatação de código e auditoria hermética de pacotes.

---

## Subcomandos Principais

### `aipo run`

Compila e executa um arquivo de código-fonte (`.aipo`) ou um bytecode compilado (`.aibc`):

```bash
# Executar código-fonte diretamente
aipo run src/main.aipo

# Executar com cache customizado de pacotes
aipo run src/main.aipo --package-cache .aipo/cache

# Saída de diagnósticos em formato JSONL estruturado (ideal para editores e CI)
aipo run src/main.aipo --message-format=jsonl
```

### `aipo test`

Descobre e executa testes unitários em arquivos `*_test.aipo`, `test_*.aipo` ou com diretiva `#!test`, garantindo isolamento por VM limpa, semente PRNG zerada e relógio congelado. Sem argumento, varre `tests/` e o diretório atual; para um pacote, passe o diretório:

```bash
# Executar os testes de um pacote
aipo test packages/aipo-ui

# Filtrar testes por padrão de nome ou caminho
aipo test packages/aipo-ui --filter layout

# Saída em streaming JSONL para CI/CD
aipo test packages/aipo-ui --message-format=jsonl
```

### `aipo check`

Executa a análise estática completa (Lexer, Parser, HIR, Semântica e verificação de bytecode) sem rodar a VM:

```bash
aipo check src/main.aipo
```

Reporta com precisão erros de sintaxe, contratos de interface incompatíveis, referências a variáveis indefinidas e tentativas de mutação de campos imutáveis.

### `aipo build`

Emite o bundle JavaScript completo (`app.js`, `aipo-runtime.js` e mapa de fontes `app.js.map`) pronto para execução em navegadores ou Node.js:

```bash
# Compilar e emitir bundle no diretório de saída
aipo build src/main.aipo --out dist/
```

### `aipo disasm`

Desassembla um arquivo `.aipo` ou `.aibc`, exibindo as instruções da VM, offsets em bytes, pool de constantes e coordenadas de código originais:

```bash
aipo disasm src/main.aipo
```

### `aipo fmt`

Formata arquivos de código Aipo de acordo com o padrão canônico da linguagem:

```bash
# Formatar arquivos no local
aipo fmt src/main.aipo

# Verificar se há desvios de formatação sem alterar os arquivos (modo CI)
aipo fmt --check src/
```

### `aipo package`

Conjunto de comandos herméticos para gerenciamento, bloqueio e auditoria de pacotes:

```bash
# Criar ou atualizar o lockfile determinístico (aipo.lock)
aipo package lock .

# Criar lockfile buscando snapshots remotos do GitHub para o cache
aipo package lock . --fetch-github --cache .aipo/cache

# Auditar a integridade e conformidade de um pacote local
aipo package audit .

# Verificar a integridade criptográfica SHA-256 de todas as entradas no cache
aipo package cache verify .aipo/cache

# Limpar entradas de cache obsoletas não referenciadas no lockfile
aipo package cache prune .aipo/cache --lock aipo.lock --apply
```

::: tip 📖 Guia Aprofundado de Ferramentas
Para mais detalhes sobre flags de compilação, WebAssembly, integração contínua e exemplos práticos, consulte o [Guia Completo de Ferramentas do Desenvolvedor](/tools/).
:::
