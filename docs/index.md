---
layout: home

hero:
  name: Aipo
  text: Simples, Rápida e Concorrente com Contratos Opcionais
  tagline: Uma linguagem moderna com sintaxe limpa inspirada em Gleam e Swift, tipagem forte e dinâmica, contratos transacionais com rollback automático e um conjunto completo de ferramentas nativas para o desenvolvedor.
  image:
    src: /assets/logo.svg
    alt: Aipo Language Logo
  actions:
    - theme: brand
      text: Começar Agora (5 min)
      link: /getting-started/first-program
    - theme: alt
      text: Manual da Linguagem
      link: /manual/
    - theme: alt
      text: Ferramentas (CLI)
      link: /tools/
    - theme: alt
      text: Exemplos & Receitas
      link: /examples/

features:
  - icon: 🎯
    title: Clareza Cognitiva & Sintaxe Limpa
    details: Inspirada na clareza de Gleam e Swift. Blocos delimitados por chaves, condições sem parênteses desnecessários, imutabilidade por padrão e ausência de ruídos visuais para máxima acessibilidade.
    link: /manual/syntax-and-types
  - icon: 🛠️
    title: Ferramental Completo na Caixa
    details: Compilador, test runner automatizado com isolamento determinístico, linter de checagem estática, formatador canônico opinativo e gestor de pacotes hermético num único binário (aipo).
    link: /tools/
  - icon: 🛡️
    title: Contratos & Rollback Atômico
    details: Invariantes estruturais que protegem suas entidades. Se uma mutação quebrar uma regra de negócio, o Aipo reverte o estado anterior automaticamente sem dados corrompidos.
    link: /manual/interfaces-and-contracts
  - icon: ⚡
    title: Concorrência Cooperativa & Determinística
    details: Sintaxe nativa async fn e await do com scheduler cooperativo e tempo virtual. Zero condições de corrida inexplicáveis e garantia total de reprodutibilidade em testes.
    link: /manual/async-and-concurrency
  - icon: 🌐
    title: Paridade Diferencial VM ↔ JavaScript
    details: Emita código WebAssembly de alta performance ou pacotes JavaScript ES2022 modulares com exatamente as mesmas saídas, checksums e diagnósticos da Máquina Virtual nativa.
    link: /architecture/js-emitter
  - icon: 📦
    title: Gestão Hermética de Dependências
    details: Manifesto aipo.toml, lockfiles reproduzíveis, dependências GitHub pinadas por commit SHA com integridade SHA-256 verificada e cache local totalmente offline.
    link: /manual/packages-and-modules
---

<div class="vp-doc">

## Uma Experiência de Desenvolvimento Sem Fricção

O **Aipo** foi projetado para desenvolvedores que valorizam clareza de raciocínio, previsibilidade e velocidade. Em vez de coerções implícitas perigosas (como `"1" + 2 == "12"`), o Aipo combina tipagem dinâmica e forte com **contratos estruturais opcionais**, **recuperação transacional com rollback automático** e um **ecossistema completo sem dependências externas**.

```aipo
# Interface com satisfação estrutural automática
interface Notificavel {
    resumo() -> String
}

# Enum canônico (tipo fechado de soma)
enum StatusConta {
    Ativa,
    Bloqueada(motivo: String),
}

# Struct com campos imutáveis por padrão e campo mutável explícito
#!satisfies Notificavel
struct Conta {
    id: Int
    titular: String
    var saldo: Int
    var status: StatusConta = StatusConta.Ativa
}

# Invariante de integridade transacional
Conta:invariant {
    self.saldo >= 0
}

# Mutação explícita com `var self`
Conta:transferir(var self, var destino: Conta, valor: Int) {
    if valor <= 0 {
        fail "o valor da transferência deve ser positivo"
    }

    # Se qualquer regra for violada, as duas contas sofrem rollback atômico
    attempt {
        self.saldo -= valor
        destino.saldo += valor
    } failed err {
        fail f"transferência abortada com segurança: {err.message}"
    }
}

# Método de leitura (self implícito)
Conta:resumo() -> String {
    return f"Conta #{self.id} ({self.titular}): R$ {self.saldo // 100}"
}

# Iniciação simétrica com dois-pontos
var c1 = Conta{ id: 101, titular: "Alex", saldo: 25000 }
var c2 = Conta{ id: 102, titular: "Beatriz", saldo: 5000 }

c1.transferir(c2, 5000)
io.println(c1.resumo())
io.println(c2.resumo())
```

---

## Todas as Ferramentas em um Único Binário

Não perca tempo configurando formatadores, runners de testes ou linters de terceiros. Com o comando `aipo`, você tem tudo integrado:

<div class="tool-grid">

<div class="tool-card">
  <div>
    <h3>▶️ Executar</h3>
    <p>Rode scripts <code>.aipo</code> ou bytecodes <code>.aibc</code> instantaneamente.</p>
  </div>
  <div class="tool-cmd">aipo run app.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>🧪 Testar</h3>
    <p>Testes isolados com relógio congelado e PRNG determinístico.</p>
  </div>
  <div class="tool-cmd">aipo test</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔍 Verificar</h3>
    <p>Checagem estática de contratos, escopo e mutabilidade em milissegundos.</p>
  </div>
  <div class="tool-cmd">aipo check app.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>✨ Formatar</h3>
    <p>Formatação canônica opinativa sem discussão de estilo em pull requests.</p>
  </div>
  <div class="tool-cmd">aipo fmt src/</div>
</div>

<div class="tool-card">
  <div>
    <h3>📦 Empacotar</h3>
    <p>Compilação para JavaScript ES2022 ou WebAssembly com mapas de fonte.</p>
  </div>
  <div class="tool-cmd">aipo build app.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔒 Gerenciar</h3>
    <p>Lockfiles herméticos e auditoria estrita de dependências.</p>
  </div>
  <div class="tool-cmd">aipo package audit .</div>
</div>

</div>

---

## Primeiros Passos

::: tip 💡 Como começar em menos de 5 minutos
1. **Instale ou compile o CLI**: `cargo build --release -p aipo-cli` (ou baixe o binário para seu SO).
2. **Escreva seu primeiro código**: Siga nosso guia passo a passo em [Seu Primeiro Programa](/getting-started/first-program).
3. **Explore o Manual e as Ferramentas**: Conheça os fundamentos no [Manual da Linguagem](/manual/) e aprofunde-se no [Guia de Ferramentas](/tools/).
:::

---

## Engenharia Aberta e Transparente

O desenvolvimento da linguagem Aipo é 100% público e orientado por evidências rigorosas:
- **[Macroarquitetura do Sistema](/architecture/)**: Detalhes do compilador, representação intermediária e máquina virtual.
- **[Decisões Arquiteturais (ADPs)](/decisions/)**: Registro formal de todas as decisões técnicas fundamentadas.
- **[Trajetória de Desenvolvimento](/trajectory/)**: A evolução contínua da linguagem através de Waves estruturadas.
- **[Benchmarks Comparativos](/evidence/cross-language)**: Testes de desempenho e reprodutibilidade contra runtimes industriais.

</div>
