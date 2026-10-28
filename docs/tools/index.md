# Ferramentas do Desenvolvedor (CLI `aipo`)

A experiência de desenvolvimento com Aipo é desenhada para ser completa e imediata. Diferente de ecossistemas fragmentados que exigem múltiplos pacotes externos para formatação, testes e empacotamento, o **Aipo entrega uma suíte completa de ferramentas nativas em um único binário (`aipo`)**.

<div class="tool-grid">

<div class="tool-card">
  <div>
    <h3>▶️ aipo run</h3>
    <p>Executa arquivos de código-fonte diretamente, bytecodes pré-compilados <code>.aibc</code> ou módulos WebAssembly com scheduler assíncrono cooperativo.</p>
  </div>
  <div class="tool-cmd">aipo run src/main.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>🧪 aipo test</h3>
    <p>Descobre e executa testes unitários automaticamente com isolamento estrito entre testes, semente PRNG zerada e congelamento temporal determinístico.</p>
  </div>
  <div class="tool-cmd">aipo test --filter math</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔍 aipo check</h3>
    <p>Análise estática instantânea: valida sintaxe, regras de mutabilidade, escopo e contratos de interface antes da execução.</p>
  </div>
  <div class="tool-cmd">aipo check src/main.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>✨ aipo fmt</h3>
    <p>Formatador canônico opinativo com preservação de comentários, sem necessidade de configuração e suporte a modo de auditoria em CI.</p>
  </div>
  <div class="tool-cmd">aipo fmt --check src/</div>
</div>

<div class="tool-card">
  <div>
    <h3>📦 aipo build</h3>
    <p>Emite bundles JavaScript ES2022 otimizados com mapas de fontes ou binários WebAssembly para rodar no navegador ou Node.js.</p>
  </div>
  <div class="tool-cmd">aipo build src/main.aipo --out dist/</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔒 aipo package</h3>
    <p>Gestor de pacotes hermético: lockfiles determinísticos, dependências GitHub pinadas por commit SHA e cache local imutável.</p>
  </div>
  <div class="tool-cmd">aipo package audit .</div>
</div>

</div>

---

## 1. Execução: `aipo run`

O subcomando `aipo run` compila e executa código Aipo com o runtime nativo de alta performance.

```bash
# Execução direta de um script
aipo run src/main.aipo

# Execução com diretório de cache de pacotes explícito
aipo run src/main.aipo --package-cache .aipo/cache

# Saída de diagnósticos em formato JSONL (ideal para editores e IDEs)
aipo run src/main.aipo --message-format=jsonl

# Execução de bytecode pré-compilado (início ultrarrápido sem compilação)
aipo run bin/app.aibc
```

### Códigos de Saída (Exit Codes)

O Aipo adota códigos de saída determinísticos e padronizados:

| Código | Significado | Descrição |
| :---: | :--- | :--- |
| `0` | **Sucesso** | O programa foi executado até o final sem falhas não capturadas. |
| `1` | **Falha de Linguagem** | Erro semântico, erro de sintaxe, falha em runtime (`fail(...)`) ou desvio de formatação. |
| `2` | **Erro de Uso** | Argumentos de linha de comando inválidos, flags desconhecidas ou arquivos inexistentes. |

---

## 2. Testes Automatizados: `aipo test`

O comando `aipo test` é o executor de testes nativo da linguagem. Sem argumento, ele varre `tests/` e os arquivos do diretório atual; para um pacote, passe o diretório. Ele reconhece arquivos `*_test.aipo`, `test_*.aipo` e arquivos com diretiva `#!test`. Suítes que chamam `host_*` precisam do bridge de host.

```bash
# Executar os testes de um pacote
aipo test packages/aipo-ui

# Filtrar testes por nome ou caminho
aipo test packages/aipo-ui --filter layout

# Saída legível por máquinas para integrações de CI/CD
aipo test packages/aipo-ui --message-format=jsonl
```

### Isolamento e Determinismo em Testes

Cada teste é executado em um ambiente de isolamento absoluto:
- **VM Limpa**: Cada teste recebe uma instância completamente virgem da Máquina Virtual.
- **Relógio Congelado**: O tempo virtual é inicializado em `0` e o relógio real não avança espontaneamente, eliminando testes intermitentes (*flaky tests*).
- **PRNG Previsível**: A semente aleatória é redefinida para `0` antes de cada caso de teste.

```aipo
# math_test.aipo
fn test_soma_simples() {
    let resultado = 2 + 2
    if resultado != 4 {
        fail("esperava 4 na soma")
    }
}

fn test_divisao_inteira() {
    let valor = 14 // 3
    if valor != 4 {
        fail(f"esperava 4 na divisão truncada, obteve {valor}")
    }
}
```

---

## 3. Análise Estática: `aipo check`

O comando `aipo check` realiza toda a esteira do compilador (Lexer, Parser, HIR, Análise Semântica e Verificação de Bytecode) **sem iniciar a execução**:

```bash
aipo check src/main.aipo
```

Ele valida:
- Sintaxe e fechamento correto de blocos `{ ... }`.
- Resolução e visibilidade de identificadores e variáveis.
- Violações de imutabilidade (tentativas de mutação de campos não declarados com `var`).
- Conformidade estática de contratos de interface e aridade de chamadas.

---

## 4. Formatação de Código: `aipo fmt`

O Aipo inclui um formatador canônico opinativo com regras bem definidas:

```bash
# Formatar um ou múltiplos arquivos no local
aipo fmt src/main.aipo

# Formatar todos os arquivos do projeto recursivamente
aipo fmt src/

# Modo CI: apenas verifica se o código está formatado (exit code 1 se houver desvios)
aipo fmt --check src/
```

### Regras do Formato Canônico:
- Indentação padrão de **4 espaços** (sem tabulações).
- Blocos `{ ... }` alinhados com fechamento no mesmo nível do comando de abertura.
- Espaçamento interno legível em literais de uma linha: `User{ id: 1, name: "Ana" }`.
- Preservação intacta de comentários e quebras de linha intencionais.

---

## 5. Emissão Web e JavaScript: `aipo build`

O compilador Aipo produz código otimizado para o ecossistema Web e Node.js com paridade diferencial comprovada no corpus:

```bash
# Compilar projeto para JavaScript ES2022
aipo build src/main.aipo --out dist/

# Compilar para WebAssembly nativo
aipo build src/main.aipo --target wasm --out dist/
```

O bundle gerado inclui:
- `app.js`: Código da aplicação e inicialização do runtime.
- `aipo-runtime.js`: Shim modular e versionado sem dependências externas.
- `app.js.map`: Mapa de fontes completo para depuração direta de arquivos `.aipo` no DevTools do navegador.
- `package.json`: Declara `"type": "module"` para que `node app.js` funcione no diretório de saída.

---

## 6. Gestão Hermética de Pacotes: `aipo package`

O ecossistema Aipo adota dependências determinísticas e reproduzíveis baseadas em `aipo.toml` e `aipo.lock`.

```bash
# Bloquear dependências locais e gerar o lockfile determinístico
aipo package lock .

# Buscar dependências remotas do GitHub e armazenar no cache
aipo package lock . --fetch-github --cache .aipo/cache

# Autenticação opcional para repositórios privados
aipo package lock . --fetch-github --cache .aipo/cache --github-token-env GITHUB_TOKEN

# Auditar integridade e conformidade de dependências
aipo package audit .

# Verificar hashes SHA-256 de todas as entradas do cache local
aipo package cache verify .aipo/cache

# Remover versões antigas e órfãs do cache local
aipo package cache prune .aipo/cache --lock aipo.lock --apply
```

---

## 7. Desassemblador: `aipo disasm`

Para entusiastas, pesquisadores de compiladores e engenharia de performance, `aipo disasm` expõe os opcodes e a estrutura interna gerada:

```bash
aipo disasm src/main.aipo
```

Exibe os mnemonics da VM, referências a constantes, tamanhos de instruções e o mapeamento exato de linha e coluna no código-fonte original.

---

## Exemplo de Workflow em CI/CD (GitHub Actions)

Adicione este workflow em `.github/workflows/ci.yml` para garantir qualidade contínua em seu repositório Aipo:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Instalar Rust Toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Compilar Aipo CLI
        run: cargo build --release -p aipo-cli

      - name: Adicionar aipo ao PATH
        run: echo "$(pwd)/target/release" >> $GITHUB_PATH

      - name: Verificar Formatação
        run: aipo fmt --check examples/

      - name: Checagem Estática
        run: aipo check examples/01_fizzbuzz.aipo

      - name: Executar Testes Unitários
        run: aipo test packages/aipo-ui
```
