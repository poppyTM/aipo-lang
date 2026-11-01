# Seu Primeiro Programa em 5 Minutos

Neste tutorial rápido, vamos criar, verificar, compilar e executar o seu primeiro programa em Aipo.

---

## 1. Olá, Mundo!

Crie um arquivo chamado `hello.aipo`:

```aipo
# hello.aipo
io.println("Olá do Aipo!")
```

Execute diretamente pelo CLI:

```bash
aipo run hello.aipo
```

**Saída esperada:**
```
Olá do Aipo!
```

---

## 2. Estruturas, Invariantes e Métodos

Vamos criar um programa que modela uma conta bancária com invariante de saldo positivo. Crie o arquivo `conta.aipo`:

```aipo
# conta.aipo
struct Conta {
    titular: String
    numero: Int
    var saldo: Float = 0.0
}

Conta:init(titular: String, numero: Int = 0, saldo: Float = 0.0) {
    self.titular = titular
    self.numero = numero
    self.saldo = saldo
}

# Invariante: executada em todas as criações e mutações de campos
Conta:invariant {
    self.saldo >= 0.0
}

Conta:depositar(var self, valor: Float) {
    if valor <= 0.0 {
        fail("Valor de depósito deve ser positivo")
    }
    self.saldo += valor
}

Conta:sacar(var self, valor: Float) {
    if valor <= 0.0 {
        fail("Valor de saque deve ser positivo")
    }
    # A mutação é provisória até o fim da chamada: se violar
    # `self.saldo >= 0.0`, a operação falha e o valor anterior é restaurado.
    self.saldo -= valor
}

# Instanciando a conta usando dois-pontos (:) consistente
var c = Conta{ titular: "Maria Silva", numero: 1042, saldo: 150.0 }

io.println(f"Conta criada para: {c.titular}")
io.println(f"Saldo inicial: {c.saldo}")

c.depositar(50.0)
io.println(f"Saldo após depósito: {c.saldo}")

c.sacar(75.0)
io.println(f"Saldo após saque: {c.saldo}")

# Um saque acima do saldo falha e o saldo é preservado.
attempt {
    c.sacar(500.0)
} failed erro {
    io.println(f"Saque recusado: {erro.message}")
}
io.println(f"Saldo preservado: {c.saldo}")
```

Execute o programa:

```bash
aipo run conta.aipo
```

**Saída esperada:**
```
Conta criada para: Maria Silva
Saldo inicial: 150.0
Saldo após depósito: 200.0
Saldo após saque: 125.0
Saque recusado: invariant() of Conta did not hold after mutation
Saldo preservado: 125.0
```

---

## 3. Inspecionando o Bytecode

Uma das grandes forças do Aipo é a transparência do compilador. Você pode visualizar as instruções de bytecode geradas para qualquer arquivo:

```bash
aipo disasm conta.aipo
```

O comando exibirá o desassembly com linhas e colunas mapeadas, demonstrando a alocação de registradores, frames de chamada e tabelas de constantes.

---

## 4. Compilando para JavaScript

Você pode transpilar o mesmo código para JavaScript executável via Node.js:

```bash
aipo build conta.aipo -o dist/conta.js
node dist/conta.js
```

O código gerado possui paridade comportamental completa com a VM em Rust.
