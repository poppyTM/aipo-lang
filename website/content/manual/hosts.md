---
title: "Hospedar o Aipo em jogos, interfaces e aplicações"
description: Manual atualizado com fontes da linguagem Aipo.
---

# Hospedar o Aipo em jogos, interfaces e aplicações

Uma das finalidades do Aipo é funcionar como **linguagem embutida** em outros programas. Um *host* escolhe quais APIs estarão disponíveis, como o código é carregado e quais operações são autorizadas.

## Limites que importam

- O compilador, a VM e o código Aipo são componentes separados de uma interface gráfica.
- O host pode oferecer funções para desenho, câmera, teclado, arquivos, ambiente, relógio ou entidades.
- Recursos de host exigem uma superfície declarada (AHS) e capacidades concedidas explicitamente.
- Uma função visível no esquema não é necessariamente autorizada no runtime.
- Integração com jogo ou GUI é **opcional**, não um requisito do executável `aipo-cli`.

## Exemplos práticos

| O que deseja construir | Leia |
| --- | --- |
| Simulação no terminal | [Snake](/examples/25-snake-game) |
| Jogo com janela e entrada | [Jogo interativo](/examples/26-interactive-game) |
| Câmera, sprites e mundo 2D | [Câmera e sprites](/examples/27-camera-and-sprites) |
| Controles de interface imediata | [egui](/examples/28-egui-immediate-gui) |

Para os exemplos gráficos, use o crate `aipo-game-host`, conforme a instrução no topo de cada arquivo. Eles não rodam apenas com `aipo run` na CLI padrão.

## Segurança e embedding

O crate `aipo-host` implementa schemas e capacidades, `aipo-c-abi` fornece uma fronteira de integração C, e `aipo-poppy` mostra integração com simulação headless. Esses subsistemas têm escopos diferentes.

Comece pelo [guia de embedding](/guides/embedding) e aprofunde em [arquitetura de host](/engineering/embedding).
