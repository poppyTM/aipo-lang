# Frontend do Compilador

O frontend do Aipo é responsável por processar o código-fonte em texto puro até a geração da representação semântica validada.

---

## Estágios do Pipeline

### 1. `aipo-source`
- Representa o arquivo fonte e seu conteúdo em memória.
- Fornece indexação de linhas e colunas segura em relação a fronteiras UTF-8 (`is_char_boundary`), garantindo compatibilidade estrita com a MSRV (Rust 1.85+).

### 2. `aipo-lexer`
- Converte o fluxo de caracteres UTF-8 em uma sequência contígua de tokens.
- Otimização com zero alocações na identificação de palavras-chave através de casamento direto sobre `&str`.
- Rejeição canônica de números decimais malformados (como `1.e5` ou `1._5`), gerando o diagnóstico estático `AIPO_LEX_INVALID_NUMBER`.

### 3. `aipo-syntax` & `aipo-ast`
- Parser descendente recursivo (*recursive descent*) com recuperação resiliente de falhas (`synchronize`).
- Preservação de anotações `async` em métodos de `interface`.
- Desaçucaramento no parser de comparações contínuas (como `val >= 0 and <= 100`).
- Suporte a expressões condicionais em linha (`if c then a else b`).

### 4. `aipo-hir`
- Lowering da AST para High-Level Intermediate Representation.
- Unificação de spans nos hooks de ciclo de vida (`init` e `invariant`).
- Resolução de referências de escopo local e de módulo.

### 5. `aipo-sema`
- Validador semântico com checagem de tipos estrita e verificação de contratos:
  - Verificação de contratos de `interface` via subtipagem estrutural automática (métodos obrigatórios, aridades, mutabilidade do receptor `self` vs `var self`, tipos de parâmetros e retornos).
  - Proibição estática de mutação de campos imutáveis de estruturas (`AIPO_SEM_FIXED_REASSIGN`).
  - Diagnósticos dedicados para concorrência (`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`, `AIPO_SEM_FORGOTTEN_TASK`, `AIPO_SEM_NESTED_AWAIT_DO`).
