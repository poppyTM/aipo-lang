---
title: "Módulos, imports e pacotes"
description: "Referência de uso organizada por tarefas e comportamento do Aipo."
---

# Módulos, imports e pacotes

**Um arquivo `.aipo` é uma unidade de código; um pacote organiza arquivos, dependências e uma identidade versionada.** Você não precisa criar um pacote para testar seu primeiro programa.

## Separar responsabilidades

O Aipo permite exportar símbolos explicitamente e importar outro arquivo:

```aipo
# utils.aipo
fn double(value) {
    return value * 2
}
export double
```

```aipo
# main.aipo
import utils
io.println(utils.double(21))
```

Para executar um projeto organizado, confira o [exemplo multi-módulo do repositório](https://github.com/poppyTM/aipo-lang/tree/main/examples/23_multi_module_application).

## Imports disponíveis

- `import module`: acesso qualificado pelo nome do módulo.
- `import module as alias`: apelido local.
- `import module: symbol`: importar símbolo selecionado.

Os símbolos não exportados permanecem privados ao módulo. O módulo é inicializado conforme as regras de resolução e ciclos do compilador.

## Pacotes e lockfile

Um projeto pode declarar `aipo.toml`, `aipo.lock`, dependências locais e referências GitHub **fixadas por SHA**. O CLI fornece `package lock`, `package audit`, verificação e limpeza de cache. Baixar conteúdo remoto é uma operação explícita; não confunda comandos offline com fetch de rede.

```bash
cargo run -q -p aipo-cli -- package lock ./my-package
cargo run -q -p aipo-cli -- package audit ./my-package
```

Para detalhes de manifesto, opções de cache e segurança, consulte o [contrato completo de pacotes](https://github.com/poppyTM/aipo-lang/blob/main/docs/manual/packages-and-modules.md).

**Pratique:** transforme um pequeno cálculo em `utils.aipo` e importe-o a partir de `main.aipo`.

[Próximo: CLI](/manual/cli) · [Exemplos](/examples/).
