---
title: Instalação e primeiro programa
---
# Instalação e primeiro programa

## Pré-requisitos

- Rust e Cargo instalados; o workspace exige Rust 1.96; o lockfile completo inclui Wasmtime/Cranelift com esse mínimo.
- Git para obter o código; Node.js apenas quando for executar o JavaScript emitido ou desenvolver o site.

## Compilar

```bash
git clone https://github.com/poppyTM/aipo-lang.git
cd aipo-lang
cargo build -p aipo-cli
```

O executável local normalmente fica em `target/debug/aipo`, dependendo da plataforma. Para não depender do `PATH`, os próximos comandos invocam Cargo.

## Executar uma amostra do repositório

```bash
cargo run -q -p aipo-cli -- run examples/06_variables_and_values.aipo
```

Crie `hello.aipo` com este conteúdo:

```aipo
io.println("Olá do Aipo!")
```

Execute com `cargo run -q -p aipo-cli -- run hello.aipo`.

## Verificar e formatar

```bash
cargo run -q -p aipo-cli -- check hello.aipo
cargo run -q -p aipo-cli -- fmt hello.aipo
```

Se um comando falhar, copie a saída completa, a revisão do compilador e o comando exato. O [guia de diagnóstico](/guides/troubleshooting) mostra como separar falhas de parse, semântica e runtime.

**Próximo:** [Seu primeiro programa](/learn/01-primeiro-programa).
