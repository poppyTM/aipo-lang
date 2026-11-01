# Catálogo de Anti-Patterns e Correções Automáticas para LLMs em Aipo V1

Este documento cataloga os erros mais frequentes induzidos por viés de treinamento em LLMs ao gerar código Aipo, juntamente com o motivo exato da falha e a correção obrigatória.

---

## 1. Comentários e Pontuação

### Anti-Pattern 1.1: Usar `//` para comentários
- **Errado:**
  ```text
  // Isso é uma tentativa de comentário
  let x = 10
  ```
- **Por que falha:** O lexer de Aipo reconhece `//` como operador binário aritmético de divisão inteira (`SlashSlash`). O código falhará com erro de sintaxe ou causará divisão de operandos adjacentes.
- **Correto:**
  ```text
  # Isso é um comentário válido
  let x = 10
  ```

### Anti-Pattern 1.2: Ponto e vírgula `;` no final de linhas
- **Errado:**
  ```text
  let a = 1;
  let b = 2;
  ```
- **Por que falha:** `;` não é tokenizado em Aipo V1. Dispara `unexpected character ';'`.
- **Correto:**
  ```text
  let a = 1
  let b = 2
  ```

---

## 2. Controle de Fluxo e Loops

### Anti-Pattern 2.1: Usar `for`
- **Errado:**
  ```text
  for item in lista {
      io.println(item)
  }
  ```
- **Por que falha:** A palavra `for` **não existe** em Aipo V1.
- **Correto:**
  ```text
  each item in lista {
      io.println(item)
  }
  ```

### Anti-Pattern 2.2: Usar `end` para fechar blocos
- **Errado:**
  ```text
  if cond {
      io.println("ok")
  end
  ```
- **Por que falha:** `end` foi removido formalmente pelo ADR-001. Apenas `{}` é admitido.
- **Correto:**
  ```text
  if cond {
      io.println("ok")
  }
  ```

### Anti-Pattern 2.3: `then` em blocos `{}`
- **Errado:**
  ```text
  if cond then {
      io.println("ok")
  }
  ```
- **Por que falha:** A regra de ouro é: "Tem `{}`? NUNCA usa `then`. Não tem `{}` (inline de valor)? SEMPRE usa `then`."
- **Correto:**
  ```text
  if cond {
      io.println("ok")
  }
  # Inline com valor usa then:
  let valor = if cond then 10 else 20
  ```

---

## 3. Tipos e Métodos

### Anti-Pattern 3.1: Usar `fn` dentro ou na declaração de métodos
- **Errado:**
  ```text
  Tipo:fn metodo() { }
  fn Tipo:metodo() { }
  ```
- **Por que falha:** `fn` é reservado exclusivamente para funções livres. O prefixo `Tipo:` já declara a função como método.
- **Correto:**
  ```text
  Tipo:metodo() { }
  ```

### Anti-Pattern 3.2: Omitir `var self` em métodos que mutam campos
- **Errado:**
  ```text
  Contador:incrementar() {
      self.valor += 1
  }
  ```
- **Por que falha:** O compilador trata `self` como somente leitura por padrão. Atribuição dispara `AIPO_SEM_RECEPTOR_IMUTAVEL`.
- **Correto:**
  ```text
  Contador:incrementar(var self) {
      self.valor += 1
  }
  ```

### Anti-Pattern 3.3: Colocar métodos dentro do bloco `struct`
- **Errado:**
  ```text
  struct Ponto {
      x: Int
      y: Int
      dist() { ... }
  }
  ```
- **Por que falha:** `struct` contém exclusivamente campos de dados.
- **Correto:**
  ```text
  struct Ponto {
      x: Int
      y: Int
  }

  Ponto:dist() -> Float {
      return math.sqrt(self.x * self.x + self.y * self.y)
  }
  ```

### Anti-Pattern 3.4: Tentar usar `impl` ou `satisfy`
- **Errado:**
  ```text
  impl Ponto { ... }
  satisfy Ponto: Desenhavel
  ```
- **Por que falha:** Removidos pelo ADR-001. A declaração de métodos é avulsa (`Ponto:metodo`) e interfaces são estruturais (`#!satisfies Desenhavel`).

### Anti-Pattern 3.5: Promoção em lote (`::`) de função sem `self` como primeiro parâmetro
- **Errado:**
  ```text
  fn somar(a, b) { return a + b }
  Ponto::[somar]
  ```
- **Por que falha:** O compilador exige que a função a ser promovida a método possua `self` ou `var self` como primeiro parâmetro. Batch promotion serve para comportamento do tipo, não para namespacing de utilitários soltos.
- **Correto:**
  ```text
  fn somar(self, b) { return self.x + b }
  Ponto::[somar]
  ```

### Anti-Pattern 3.6: `match` em `enum` sem exaustividade e sem `else`
- **Errado:**
  ```text
  enum Cor { Vermelho, Verde, Azul, }
  match c {
      when Cor.Vermelho { io.println("vermelho") }
  }
  ```
- **Por que falha:** O compilador exige cobertura total (exhaustiveness). Se não houver `else`, todas as variantes devem estar presentes nos braços `when`.
- **Correto:**
  ```text
  match c {
      when Cor.Vermelho { io.println("vermelho") }
      when Cor.Verde { io.println("verde") }
      when Cor.Azul { io.println("azul") }
  }
  ```

---

## 4. Tratamento de Exceções

### Anti-Pattern 4.1: `try / catch / throw`
- **Errado:**
  ```text
  try {
      if erro { throw "falha" }
  } catch (e) {
      io.println(e)
  }
  ```
- **Por que falha:** Nenhuma dessas keywords existe em Aipo.
- **Correto:**
  ```text
  attempt {
      if erro { fail "falha" }
  } failed e {
      io.println(e.message)
  }
  ```
