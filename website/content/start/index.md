---
title: Começar a usar Aipo
description: Instale a CLI, execute um exemplo e escolha o próximo passo.
---

# Seu primeiro programa em Aipo

O Aipo é uma linguagem de programação com uma VM escrita em Rust. Você pode começar **sem configurar um projeto grande**: basta compilar a CLI, criar um arquivo e executar.

## 1. Preparar

Você precisa de Rust/Cargo e Git. Node.js só é necessário para executar JavaScript gerado ou desenvolver o site da documentação.

```bash
git clone https://github.com/poppyTM/aipo-lang.git
cd aipo-lang
cargo build -p aipo-cli
```

## 2. Executar um exemplo que já existe

```bash
cargo run -q -p aipo-cli -- run examples/06_variables_and_values.aipo
```

O programa imprime valores simples e mostra a diferença entre `let` e `var`.

## 3. Criar seu arquivo

Crie `hello.aipo` na raiz do repositório:

```aipo
let name = "mundo"
io.println(f"Olá, {name}!")
```

Execute:

```bash
cargo run -q -p aipo-cli -- run hello.aipo
```

## 4. Se algo não funcionar

Use `cargo run -q -p aipo-cli -- check hello.aipo` para verificar o programa sem executá-lo. Se o erro citar uma feature do backend, retorne à VM de referência e consulte [compatibilidade](/reference/status).

## Qual é o próximo passo?

- **Aprender os conceitos:** [Manual por assunto](/manual/)
- **Ver programas reais:** [26 exemplos completos](/examples/)
- **Encontrar um comando ou assinatura:** [Referência](/reference/)

Você não precisa seguir todos os capítulos. **Escolha um problema simples que queira resolver** e consulte somente os recursos necessários.
