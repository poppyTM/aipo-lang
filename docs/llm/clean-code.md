# Aipo — Clean Code, Ergonomia Cognitiva e Padrões de Projeto

Aipo é desenhada sob o princípio de **baixa sobrecarga cognitiva** e acessibilidade neurodivergente (W3C COGA). Em Aipo, "Clean Code" não significa criar camadas desnecessárias de abstração (YAGNI estrito), mas sim código linear, previsível, com contratos explícitos e zero ambiguidade.

---

## 1. Princípios Fundamentais de Design

### 1.1 Linearidade e Leitura Sem Ruído
- Evite aninhamento excessivo de blocos (máximo 3 níveis). Prefira cláusulas de guarda (*early return* / *early fail*).
- Elimine "código inteligente" ou truques sintáticos. A clareza imediata supera a brevidade críptica.
- Uma responsabilidade clara por módulo (`1 arquivo .aipo = 1 unidade coesa de domínio`).

### 1.2 Imutabilidade por Padrão
Em Aipo, todas as declarações `let` e campos de struct não anotados com `var` são estritamente imutáveis.
- **Prefira transformações funcionais:** Use o operador `with` para produzir novos estados a partir de structs existentes.
- **Mutabilidade deve ser cirúrgica e visível:** Declare `var` em variáveis locais e campos de struct apenas quando mutação in-place for estritamente necessária por desempenho ou semântica.
- **Receptores de método:** A maioria dos métodos deve ser de leitura (`self` implícito). Só utilize `Tipo:metodo(var self)` quando a mutação in-place for a intenção clara do design.

### 1.3 Contratos e Invariantes Explícitos
- Regras de negócio essenciais pertencem a `Tipo:invariant { ... }`.
- Se uma conta bancária nunca pode ter saldo negativo, não confie apenas em verificações manuais de cada método: declare a regra no hook `invariant`.
- O runtime de Aipo valida o hook após a inicialização (`init`) e antes/depois de métodos mutáveis. Em caso de violação dentro de um bloco `attempt`, as mutações são revertidas automaticamente.

---

## 2. Convenções de Nomenclatura e Tipagem

| Elemento | Convenção | Exemplos | Justificativa |
|---|---|---|---|
| Structs | `PascalCase` | `Usuario`, `ItemPedido`, `ResultadoHttp` | Diferenciação imediata de valores comuns. |
| Interfaces | `PascalCase` | `Serializavel`, `Autenticador`, `Repositorio` | Indica contratos estruturais. |
| Funções livres | `snake_case` | `calcular_total`, `buscar_por_id` | Leitura fluida sem atrito visual. |
| Métodos | `snake_case` | `adicionar_item`, `esta_ativo`, `serializar` | Consistência com funções livres. |
| Variáveis e campos | `snake_case` | `limite_maximo`, `taxa_juros`, `var contador` | Clareza e legibilidade. |
| Diretivas | `#!nome` | `#!satisfies`, `#!test`, `#!deprecated`, `#!todo` | Metadados lidos pelo compilador e analisador. |

### 2.1 Anotação de Tipos e Contratos de Dados
- **Campos de Struct:** A anotação de tipos em structs (`campo: Tipo`) é formalmente **opcional**, mas **EXTREMAMENTE RECOMENDADA**. Declarar os tipos das structs blinda o domínio contra corrupção de tipos em tempo de execução, melhora os diagnósticos do compilador e permite que a LLM compreenda o modelo de dados com precisão cirúrgica.
- **Funções públicas e interfaces:** Tipagem explícita é fortemente recomendada em parâmetros e retornos de:
  - Funções públicas exportadas (`export`).
  - Assinaturas de interfaces (`interface`).
  - Assinaturas de métodos de domínio.

### 2.2 Evolução de Código com Diretivas
Em Clean Code Aipo, a evolução de código e débitos técnicos não devem ser deixados como comentários soltos (`# TODO: arrumar`). Use diretivas rastreáveis pelo compilador:
- **`#!deprecated("use novo_metodo()")`**: Sinaliza com precisão para os consumidores que a API será descontinuada, emitindo warnings controlados em vez de breaking changes silenciosas.
- **`#!todo("implementar cache LRU")`**: Anota o item sem poluir a lógica de execução, permitindo que a CLI e agentes auditem pendências estruturadas.

```text
# Modelo de alta qualidade contratual: campos tipados e imutáveis por padrão
struct Pedido {
    id: Int
    cliente_id: Int
    total: Float
    var status: String = "pendente"
}
```

```text
# Recomendado em fronteiras públicas:
export calcular_frete

fn calcular_frete(peso_kg: Float, distancia_km: Float) -> Float {
    if peso_kg <= 0.0 or distancia_km <= 0.0 {
        fail "peso e distância devem ser positivos"
    }
    return (peso_kg * 1.5) + (distancia_km * 0.08)
}
```

---

## 3. Padrões de Modelagem e Arquitetura

### 3.1 Modelagem de Entidades: Dados vs. Comportamento
Em Aipo, a struct define **apenas o formato dos dados**. Comportamentos, validações e construtores são declarados via hooks e métodos associados logo abaixo da struct.

```text
# 1. Estrutura pura de dados
struct Carrinho {
    id: Int
    usuario_id: Int
    var itens: List = []
    var total: Float = 0.0
}

# 2. Inicializador personalizado
Carrinho:init(id: Int, usuario_id: Int) {
    self.id = id
    self.usuario_id = usuario_id
    self.itens = []
    self.total = 0.0
}

# 3. Invariante de integridade
Carrinho:invariant {
    self.id > 0
    self.usuario_id > 0
    self.total >= 0.0
}

# 4. Método imutável (leitura)
Carrinho:quantidade_itens() -> Int {
    return len(self.itens)
}

# 5. Método mutável (alteração de estado in-place)
Carrinho:adicionar(var self, item_nome: String, preco: Float) {
    if preco <= 0.0 {
        fail "preço do item deve ser maior que zero"
    }
    self.itens.push({ "nome": item_nome, "preco": preco })
    self.total += preco
}
```

### 3.2 Atualização Funcional com `with`
Para evitar mutabilidade desnecessária, utilize a atualização funcional `with`:

```text
struct Configuracao {
    host: String = "localhost"
    porta: Int = 8080
    ssl: Bool = false
}

let padrao = Configuracao{}
# Produz uma nova instância com a porta e SSL modificados; 'padrao' permanece intacto
let prod = padrao with { porta: 443, ssl: true }
```

### 3.3 Reuso Funcional e Promoção com `::`
Um dos princípios de Clean Code em Aipo é a separação entre lógica pura e conveniência ergonômica.
Em vez de encapsular algoritmos complexos dentro de métodos acoplados, escreva funções que declarem `self` como primeiro parâmetro e promova-as a métodos em lote usando `::`:

```text
# Funções puras declarando 'self' como primeiro parâmetro:
fn calcular_perimetro(self) -> Float {
    return 2.0 * (self.largura + self.altura)
}

fn calcular_area(self) -> Float {
    return self.largura * self.altura
}

# Promoção limpa: torna ambas acessíveis como métodos de Retangulo
Retangulo::[calcular_perimetro, calcular_area]
```
Esse padrão elimina duplicação de código e preserva a clareza arquitetural.

### 3.4 Modelagem de Estados e Variantes com `enum`
Evite modelar estados finitos ou alternativas com strings mágicas (`status: "ativo" | "inativo"`). Em Aipo V1, utilize `enum`:
- **Garantia de Exaustividade:** O compilador impede esquecimento de casos em `match`.
- **Payloads Estruturados:** Utilize variantes com payloads nomeados ou posicionais apenas onde cada caso carregar dados específicos.
- **Comportamento Associado:** Enums suportam métodos associados (`Estado:metodo()`) e hooks (`Estado:invariant`).

```text
enum ResultadoOperacao {
    Sucesso { dados: String },
    Pendente,
    Falha(codigo: Int),
}
```

### 3.5 Tratamento de Erros e Falhas Transacionais
Aipo proíbe `try / catch / throw` e `defer`. O tratamento de erros é feito com um vocabulário enxuto e semântica transacional:

1. **Falha direta:** Use `fail "mensagem"` para sinalizar falhas recuperáveis na lógica de negócio.
2. **Fallback imediato:** Use `or_else` para valores alternativos seguros.
   ```text
   let timeout = obter_parametro("timeout") or_else 3000
   ```
3. **Propagação com `?`:** Semelhante a linguagens modernas, propaga `Failure` para a camada superior imediatamente:
   ```text
   let usuario = buscar_usuario(id)?
   let saldo = consultar_saldo(usuario.conta_id)?
   ```
4. **Fronteira Transacional com `attempt / failed`:**
   Envolva blocos com efeitos colaterais atômicos em `attempt`:
   ```text
   attempt {
       estoque.reservar(produto_id, qtd)
       pagamento.processar(cliente_id, valor)
       notificacao.enviar_confirmacao(cliente_id)
   } failed erro {
       # Se pagamento falhar, as reservas em estoque são desfeitas pelo journal
       log.error(f"Falha ao concluir pedido: {erro.message}")
       return fail(f"Não foi possível processar o pedido: {erro.message}")
   }
   ```

---

## 4. O que Evitar (Anti-patterns de Clean Code em Aipo)

1. **Evite simular classes ou herança:** Aipo não tem `class` nem herança. Use composição de structs e interfaces estruturais.
2. **Evite tentar adivinhar métodos utilitários em números primitivos:** `3.somar(4)` é erro. Chame `somar(3, 4)` ou defina explicitamente.
3. **Evite comentários explicativos óbvios:**
   ```text
   # RUIM: comentário que ecoa o código
   # incrementa o contador
   contador += 1

   # BOM: comentário que explica a motivação de negócio
   # Necessário para respeitar a cota horária da API externa
   taxa_requisições += 1
   ```
4. **Evite funções gigantes:** Se uma função tiver mais de 40 linhas, extraia funções locais (`fn local() { ... }`) ou funções livres auxiliares no módulo.
5. **Evite misturar `var` sem necessidade:** Se uma variável não precisa ser reatribuída, sempre utilize `let`.
