import { defineConfig } from 'vitepress'

const aipoLanguage = {
  name: 'aipo',
  scopeName: 'source.aipo',
  displayName: 'Aipo',
  patterns: [
    {
      name: 'comment.line.number-sign.directive.aipo',
      match: '#!.*$'
    },
    {
      name: 'comment.line.number-sign.aipo',
      match: '#.*$'
    },
    {
      name: 'string.quoted.double.aipo',
      match: '"(?:[^"\\\\]|\\\\.)*"'
    },
    {
      name: 'keyword.control.aipo',
      match: '\\b(if|then|elif|else|while|loop|repeat|each|in|do|break|continue|return|fail|attempt|failed|async|await|and|or|not|or_else|import|export|as|is|when|match)\\b'
    },
    {
      name: 'keyword.declaration.aipo',
      match: '\\b(fn|let|var|struct|enum|interface|invariant|init|self)\\b'
    },
    {
      name: 'constant.language.aipo',
      match: '\\b(true|false|none)\\b'
    },
    {
      name: 'support.type.aipo',
      match: '\\b(Int|Float|Bool|String|None|Bytes|List|Dict|Set|Sequence|Result)\\b'
    },
    {
      name: 'support.class.aipo',
      match: '\\b(math|random|json|encoding|path|url|regex|binary|time|testing|log|env|fs|task|expect)\\b'
    },
    {
      name: 'support.function.aipo',
      match: '\\b(print)\\b'
    },
    {
      name: 'constant.numeric.aipo',
      match: '\\b(0x[0-9a-fA-F_]+|0b[01_]+|0o[0-7_]+|[0-9][0-9_]*(\\.[0-9_]+)?([eE][+-]?[0-9_]+)?)\\b'
    }
  ]
}

export default defineConfig({
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: [
    '**/canon/**',
    '**/contracts/**',
    '**/conformance/**',
    '**/journal/**',
    '**/operations/**',
    '**/installation/**',
    '**/product/**',
    '**/reference/**',
    '**/security/**',
    '**/stdlib/**',
    '**/testing/**',
    '**/waves/**',
    '**/crates/**',
    '**/development/**',
    '**/diagnostics/**',
    '**/language/**',
    '**/README.md',
    '**/PRUMO.md'
  ],

  markdown: {
    languages: [aipoLanguage as any]
  },

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/assets/logo.svg' }],
    ['meta', { name: 'theme-color', content: '#059669' }],
    ['meta', { name: 'og:type', content: 'website' }],
    ['meta', { name: 'og:site_name', content: 'Aipo Programming Language' }]
  ],

  locales: {
    root: {
      label: 'Português',
      lang: 'pt-BR',
      title: 'Aipo',
      description: 'Documentação Oficial e Trajetória de Engenharia — Linguagem Dinâmica, Forte e Concorrente com Contratos Opcionais',
      themeConfig: {
        nav: [
          { text: 'Início', link: '/' },
          { text: 'Começando', link: '/getting-started/' },
          {
            text: 'Manual',
            items: [
              { text: 'Visão Geral do Manual', link: '/manual/' },
              { text: 'Sintaxe & Tipos', link: '/manual/syntax-and-types' },
              { text: 'Controle de Fluxo & Falhas', link: '/manual/control-flow' },
              { text: 'Funções, Closures & Lambdas', link: '/manual/functions-and-closures' },
              { text: 'Interfaces & Contratos', link: '/manual/interfaces-and-contracts' },
              { text: 'Concorrência & Async', link: '/manual/async-and-concurrency' },
              { text: 'Biblioteca Padrão (Stdlib)', link: '/manual/stdlib' },
              { text: 'Pacotes & Módulos', link: '/manual/packages-and-modules' }
            ]
          },
          { text: 'Ferramentas', link: '/tools/' },
          { text: 'Exemplos', link: '/examples/' },
          { text: 'Guia LLMs', link: '/llm/' },
          { text: 'Zoe UI', link: '/zoe/' },
          {
            text: 'Pacotes',
            items: [
              { text: 'Catálogo Oficial', link: '/packages/' },
              { text: 'aipo.html (Web & DOM)', link: '/packages/aipo-html' },
              { text: 'aipo.zoe (GUI & Leona)', link: '/packages/aipo-zoe' },
              { text: 'aipo.ui (Multiplataforma)', link: '/packages/aipo-ui' },
              { text: 'aipo.game (Game Engine 2D)', link: '/packages/aipo-game' },
              { text: 'aipo.http (Web & APIs)', link: '/packages/aipo-http' }
            ]
          },
          {
            text: 'Engenharia',
            items: [
              { text: 'Trajetória & Waves', link: '/trajectory/' },
              { text: 'Arquitetura do Compilador & VM', link: '/architecture/' },
              { text: 'Decisões Arquiteturais (ADPs)', link: '/decisions/' },
              { text: 'Evidências & Benchmarks', link: '/evidence/cross-language' },
              { text: 'Governança & Processo', link: '/governance/prumo-and-lpc' }
            ]
          }
        ],

        sidebar: {
          '/llm/': [
            {
              text: 'Guia Canônico para LLMs (V1)',
              items: [
                { text: 'Visão Geral & Índice', link: '/llm/' },
                { text: 'Regras Positivas & Negativas', link: '/llm/rules' },
                { text: 'Clean Code & Ergonomia', link: '/llm/clean-code' },
                { text: 'Estrutura de Projetos', link: '/llm/project-structure' },
                { text: 'Cheat Sheet Ultra-Compacto', link: '/llm/cheatsheet' }
              ]
            }
          ],
          '/zoe/': [
            {
              text: 'Zoe UI Framework',
              items: [
                { text: 'Visão Geral', link: '/zoe/' },
                { text: 'Playground Interativo', link: '/zoe/playground' }
              ]
            },
            {
              text: 'Guias de Desenvolvimento',
              items: [
                { text: 'Primeiros Passos', link: '/zoe/guide/getting-started' },
                { text: 'Motor Leona 2.0', link: '/zoe/guide/layout-leona' },
                { text: 'Reatividade & Sinais', link: '/zoe/guide/reactivity' },
                { text: 'Componentes Customizados', link: '/zoe/guide/custom-components' }
              ]
            },
            {
              text: 'Catálogo de Componentes',
              items: [
                { text: 'Botões & Seleção', link: '/zoe/components/buttons' },
                { text: 'Campos de Entrada (Inputs)', link: '/zoe/components/inputs' },
                { text: 'Contêineres de Layout', link: '/zoe/components/layout' },
                { text: 'Navegação & Hierarquia', link: '/zoe/components/navigation' },
                { text: 'Widgets Avançados', link: '/zoe/components/advanced' }
              ]
            },
            {
              text: 'Design & Roadmap',
              items: [
                { text: 'Dossiê de Modernização Visual', link: '/design/zoe-visual-modernization-dossier' }
              ]
            }
          ],
          '/packages/': [
            {
              text: 'Pacotes & Frameworks Oficiais',
              items: [
                { text: 'Catálogo Oficial', link: '/packages/' },
                { text: 'aipo.html (Web & DOM)', link: '/packages/aipo-html' },
                { text: 'aipo.zoe (GUI & Leona)', link: '/packages/aipo-zoe' },
                { text: 'aipo.ui (Multiplataforma)', link: '/packages/aipo-ui' },
                { text: 'aipo.game (Game Engine 2D)', link: '/packages/aipo-game' },
                { text: 'aipo.http (Web & APIs)', link: '/packages/aipo-http' }
              ]
            }
          ],
          '/getting-started/': [
            {
              text: 'Começando com Aipo',
              items: [
                { text: 'Visão Geral', link: '/getting-started/' },
                { text: 'O que é Aipo?', link: '/getting-started/what-is-aipo' },
                { text: 'Instalação & Setup', link: '/getting-started/installation' },
                { text: 'Primeiro Programa (5 min)', link: '/getting-started/first-program' },
                { text: 'Guia do CLI (aipo)', link: '/getting-started/cli-overview' }
              ]
            }
          ],
          '/manual/': [
            {
              text: 'Manual da Linguagem',
              items: [
                { text: 'Introdução ao Manual', link: '/manual/' },
                { text: 'Sintaxe & Tipos', link: '/manual/syntax-and-types' },
                { text: 'Controle de Fluxo & Falhas', link: '/manual/control-flow' },
                { text: 'Funções, Closures & Lambdas', link: '/manual/functions-and-closures' },
                { text: 'Interfaces & Contratos', link: '/manual/interfaces-and-contracts' },
                { text: 'Concorrência & Async', link: '/manual/async-and-concurrency' },
                { text: 'Biblioteca Padrão (Stdlib)', link: '/manual/stdlib' },
                { text: 'Pacotes & Módulos', link: '/manual/packages-and-modules' }
              ]
            }
          ],
          '/tools/': [
            {
              text: 'Ferramentas do Desenvolvedor',
              items: [
                { text: 'Visão Geral do CLI', link: '/tools/' },
                { text: 'Instalação & Setup', link: '/getting-started/installation' },
                { text: 'Primeiro Programa (5 min)', link: '/getting-started/first-program' },
                { text: 'Guia do CLI (aipo)', link: '/getting-started/cli-overview' },
                { text: 'Catálogo de Exemplos', link: '/examples/' }
              ]
            }
          ],
          '/examples/': [
            {
              text: 'Exemplos Práticos & Receitas',
              items: [
                { text: 'Receitas & Casos Reais', link: '/examples/' },
                { text: 'Primeiro Programa', link: '/getting-started/first-program' },
                { text: 'Ferramentas do Desenvolvedor', link: '/tools/' },
                { text: 'Manual da Linguagem', link: '/manual/' }
              ]
            }
          ],
          '/trajectory/': [
            {
              text: 'Trajetória de Desenvolvimento',
              items: [
                { text: 'Visão Geral da Jornada', link: '/trajectory/' },
                { text: 'Wave 0: MVP em 11 Slices', link: '/trajectory/wave-0-mvp' },
                { text: 'Wave 1: Contratos & Conformance', link: '/trajectory/wave-1-contracts' },
                { text: 'Wave 2: Paridade JS & Fuzzing', link: '/trajectory/wave-2-js-parity' },
                { text: 'Wave 3: Tipos Ricos & Async', link: '/trajectory/wave-3-async' },
                { text: 'Wave 4: Host ABI & Poppy Engine', link: '/trajectory/wave-4-host-poppy' },
                { text: 'Wave 5: Pacotes Herméticos', link: '/trajectory/wave-5-packages' },
                { text: 'Wave 6: Rumo à Release v0.1.0', link: '/trajectory/wave-6-release-v010' }
              ]
            }
          ],
          '/architecture/': [
            {
              text: 'Arquitetura do Compilador & VM',
              items: [
                { text: 'Macroarquitetura', link: '/architecture/' },
                { text: 'Frontend do Compilador', link: '/architecture/compiler-frontend' },
                { text: 'Bytecode & Máquina Virtual', link: '/architecture/bytecode-and-vm' },
                { text: 'Perfil Embedded & Shell', link: '/architecture/embedded-and-shell-profile' },
                { text: 'Backend JavaScript', link: '/architecture/js-emitter' },
                { text: 'Host ABI & Sandboxing', link: '/architecture/host-abi' },
                { text: 'Contratos das Crates', link: '/architecture/crates' }
              ]
            }
          ],
          '/studies/': [
            {
              text: 'Estudos de Referência',
              items: [
                { text: 'Visão Geral & Índice', link: '/studies/' },
                { text: 'VMs de Referência (Tier A)', link: '/studies/reference-vms' },
                { text: 'Runtimes de Referência (Tier B + C)', link: '/studies/reference-runtimes' },
                { text: 'Lições para a Aipo', link: '/studies/lessons-for-aipo' },
                { text: 'Guia Master p/ LLMs', link: '/studies/llm-study-guide' },
                { text: 'Uso e Reimplementação do Runtime', link: '/studies/runtime-hardening-guide' }
              ]
            }
          ],
          '/decisions/': [
            {
              text: 'Decisões Arquiteturais (ADPs)',
              items: [
                { text: 'Índice de Decisões', link: '/decisions/' },
                { text: 'ADP-001: Tipos Core & Bytes', link: '/decisions/adp-001' },
                { text: 'ADP-002: Hooks & Contratos', link: '/decisions/adp-002' },
                { text: 'ADP-003: Orçamentos de Execução', link: '/decisions/adp-003' },
                { text: 'ADP-004: Identificadores Unicode', link: '/decisions/adp-004' },
                { text: 'ADP-005: Limites do Parser', link: '/decisions/adp-005' },
                { text: 'ADP-006: Decisões Waves 3 & 4', link: '/decisions/adp-006' },
                { text: 'ADP-007: Identidade de Pacotes', link: '/decisions/adp-007' },
                { text: 'ADP-008: Release v0.1.0 Boundary', link: '/decisions/adp-008' },
                { text: 'ADP-009: C ABI Síncrona', link: '/decisions/adp-009' },
                { text: 'ADP-010: Thin Proofs (Rust, C, JS)', link: '/decisions/adp-010' },
                { text: 'ADP-011: Roteiro Performance & Ergonomia', link: '/decisions/adp-011' },
                { text: 'ADP-012: Modernização Ergonômica', link: '/decisions/adp-012' },
                { text: 'ADP-013: Substrato WebAssembly & Self-Hosting', link: '/decisions/adp-013' },
                { text: 'ADP-014: Perfil Embedded & Dual-Track', link: '/decisions/adp-014' }
              ]
            }
          ],
          '/evidence/': [
            {
              text: 'Evidências & Benchmarks',
              items: [
                { text: 'Benchmarks Cross-Language', link: '/evidence/cross-language' },
                { text: 'A Saga de Otimização & Lições', link: '/evidence/performance-lessons' },
                { text: 'Matriz de Conformance', link: '/evidence/conformance' },
                { text: 'Registro de Slices (P00-P04)', link: '/evidence/slices' }
              ]
            }
          ],
          '/governance/': [
            {
              text: 'Governança & Processo',
              items: [
                { text: 'Prumo & Metodologia LPC', link: '/governance/prumo-and-lpc' },
                { text: 'Padrões de Código & Testes', link: '/governance/standards-and-testing' },
                { text: 'Segurança & Sandboxing', link: '/governance/security-and-threat-model' },
                { text: 'Changelog Oficial', link: '/governance/changelog' }
              ]
            }
          ]
        },

        footer: {
          message: 'Distribuído sob licença MIT e Apache 2.0. Governança com Prumo v0.6.',
          copyright: 'Copyright © 2026 Poppy Team & Contribuidores da Linguagem Aipo'
        },

        docFooter: {
          prev: 'Página anterior',
          next: 'Próxima página'
        },

        outline: {
          label: 'Nesta página',
          level: [2, 3]
        }
      }
    },

    en: {
      label: 'English',
      lang: 'en-US',
      link: '/en/',
      title: 'Aipo',
      description: 'Official Documentation and Engineering Trajectory — Dynamic, Strongly-Typed, and Concurrent Language with Optional Contracts',
      themeConfig: {
        nav: [
          { text: 'Home', link: '/en/' },
          { text: 'Getting Started', link: '/en/getting-started/' },
          {
            text: 'Manual',
            items: [
              { text: 'Manual Overview', link: '/en/manual/' },
              { text: 'Syntax & Types', link: '/en/manual/syntax-and-types' },
              { text: 'Control Flow & Faults', link: '/en/manual/control-flow' },
              { text: 'Functions, Closures & Lambdas', link: '/en/manual/functions-and-closures' },
              { text: 'Interfaces & Contracts', link: '/en/manual/interfaces-and-contracts' },
              { text: 'Concurrency & Async', link: '/en/manual/async-and-concurrency' },
              { text: 'Standard Library (Stdlib)', link: '/en/manual/stdlib' },
              { text: 'Packages & Modules', link: '/en/manual/packages-and-modules' }
            ]
          },
          { text: 'Tools', link: '/en/tools/' },
          { text: 'Examples', link: '/en/examples/' },
          { text: 'Zoe UI', link: '/en/zoe/' },
          {
            text: 'Packages',
            items: [
              { text: 'Official Catalog', link: '/en/packages/' },
              { text: 'aipo.html (Web & DOM)', link: '/en/packages/aipo-html' },
              { text: 'aipo.zoe (GUI & Leona)', link: '/en/packages/aipo-zoe' },
              { text: 'aipo.ui (Multiplatform)', link: '/en/packages/aipo-ui' },
              { text: 'aipo.game (2D Game Engine)', link: '/en/packages/aipo-game' },
              { text: 'aipo.http (Web & APIs)', link: '/en/packages/aipo-http' }
            ]
          },
          {
            text: 'Engineering',
            items: [
              { text: 'Trajectory & Waves', link: '/en/trajectory/' },
              { text: 'Compiler & VM Architecture', link: '/en/architecture/' },
              { text: 'Architectural Decisions (ADPs)', link: '/en/decisions/' },
              { text: 'Evidence & Benchmarks', link: '/en/evidence/cross-language' },
              { text: 'Governance & Process', link: '/en/governance/prumo-and-lpc' }
            ]
          }
        ],

        sidebar: {
          '/en/zoe/': [
            {
              text: 'Zoe UI Framework',
              items: [
                { text: 'Overview', link: '/en/zoe/' },
                { text: 'Interactive Playground', link: '/en/zoe/playground' }
              ]
            },
            {
              text: 'Development Guides',
              items: [
                { text: 'Getting Started', link: '/en/zoe/guide/getting-started' },
                { text: 'Leona 2.0 Engine', link: '/en/zoe/guide/layout-leona' },
                { text: 'Reactivity & Signals', link: '/en/zoe/guide/reactivity' },
                { text: 'Custom Components', link: '/en/zoe/guide/custom-components' }
              ]
            },
            {
              text: 'Component Catalog',
              items: [
                { text: 'Buttons & Selection', link: '/en/zoe/components/buttons' },
                { text: 'Inputs & Scrubbers', link: '/en/zoe/components/inputs' },
                { text: 'Layout Containers', link: '/en/zoe/components/layout' },
                { text: 'Navigation & Hierarchy', link: '/en/zoe/components/navigation' },
                { text: 'Advanced Widgets', link: '/en/zoe/components/advanced' }
              ]
            },
            {
              text: 'Design & Roadmap',
              items: [
                { text: 'Visual Modernization Dossier', link: '/en/design/zoe-visual-modernization-dossier' }
              ]
            }
          ],
          '/en/packages/': [
            {
              text: 'Official Packages & Frameworks',
              items: [
                { text: 'Official Catalog', link: '/en/packages/' },
                { text: 'aipo.html (Web & DOM)', link: '/en/packages/aipo-html' },
                { text: 'aipo.zoe (GUI & Leona)', link: '/en/packages/aipo-zoe' },
                { text: 'aipo.ui (Multiplatform)', link: '/en/packages/aipo-ui' },
                { text: 'aipo.game (2D Game Engine)', link: '/en/packages/aipo-game' },
                { text: 'aipo.http (Web & APIs)', link: '/en/packages/aipo-http' }
              ]
            }
          ],
          '/en/getting-started/': [
            {
              text: 'Getting Started with Aipo',
              items: [
                { text: 'Overview', link: '/en/getting-started/' },
                { text: 'What is Aipo?', link: '/en/getting-started/what-is-aipo' },
                { text: 'Installation & Setup', link: '/en/getting-started/installation' },
                { text: 'First Program (5 min)', link: '/en/getting-started/first-program' },
                { text: 'CLI Overview (aipo)', link: '/en/getting-started/cli-overview' }
              ]
            }
          ],
          '/en/manual/': [
            {
              text: 'Language Manual',
              items: [
                { text: 'Manual Overview', link: '/en/manual/' },
                { text: 'Syntax & Types', link: '/en/manual/syntax-and-types' },
                { text: 'Control Flow & Faults', link: '/en/manual/control-flow' },
                { text: 'Functions, Closures & Lambdas', link: '/en/manual/functions-and-closures' },
                { text: 'Interfaces & Contracts', link: '/en/manual/interfaces-and-contracts' },
                { text: 'Concurrency & Async', link: '/en/manual/async-and-concurrency' },
                { text: 'Standard Library (Stdlib)', link: '/en/manual/stdlib' },
                { text: 'Packages & Modules', link: '/en/manual/packages-and-modules' }
              ]
            }
          ],
          '/en/tools/': [
            {
              text: 'Developer Tooling',
              items: [
                { text: 'CLI Suite Overview', link: '/en/tools/' },
                { text: 'Installation & Setup', link: '/en/getting-started/installation' },
                { text: 'First Program (5 min)', link: '/en/getting-started/first-program' },
                { text: 'Quick CLI Guide', link: '/en/getting-started/cli-overview' },
                { text: 'Examples Catalog', link: '/en/examples/' }
              ]
            }
          ],
          '/en/examples/': [
            {
              text: 'Practical Examples & Recipes',
              items: [
                { text: 'Recipes & Real Cases', link: '/en/examples/' },
                { text: 'First Program', link: '/en/getting-started/first-program' },
                { text: 'Developer Tooling', link: '/en/tools/' },
                { text: 'Language Manual', link: '/en/manual/' }
              ]
            }
          ],
          '/en/trajectory/': [
            {
              text: 'Development Trajectory',
              items: [
                { text: 'Journey Overview', link: '/en/trajectory/' },
                { text: 'Wave 0: MVP in 11 Slices', link: '/en/trajectory/wave-0-mvp' },
                { text: 'Wave 1: Contracts & Conformance', link: '/en/trajectory/wave-1-contracts' },
                { text: 'Wave 2: JS Parity & Fuzzing', link: '/en/trajectory/wave-2-js-parity' },
                { text: 'Wave 3: Rich Types & Async', link: '/en/trajectory/wave-3-async' },
                { text: 'Wave 4: Host ABI & Poppy Engine', link: '/en/trajectory/wave-4-host-poppy' },
                { text: 'Wave 5: Hermetic Packages', link: '/en/trajectory/wave-5-packages' },
                { text: 'Wave 6: Towards Release v0.1.0', link: '/en/trajectory/wave-6-release-v010' }
              ]
            }
          ],
          '/en/architecture/': [
            {
              text: 'Compiler & VM Architecture',
              items: [
                { text: 'System Macroarchitecture', link: '/en/architecture/' },
                { text: 'Compiler Frontend', link: '/en/architecture/compiler-frontend' },
                { text: 'Bytecode & Virtual Machine', link: '/en/architecture/bytecode-and-vm' },
                { text: 'JavaScript Backend', link: '/en/architecture/js-emitter' },
                { text: 'Host ABI & Sandboxing', link: '/en/architecture/host-abi' },
                { text: 'Workspace Crate Contracts', link: '/en/architecture/crates' }
              ]
            }
          ],
          '/en/decisions/': [
            {
              text: 'Architectural Decisions (ADPs)',
              items: [
                { text: 'Decisions Index', link: '/en/decisions/' },
                { text: 'ADP-001: Core Types & Bytes', link: '/en/decisions/adp-001' },
                { text: 'ADP-002: Hooks & Contracts', link: '/en/decisions/adp-002' },
                { text: 'ADP-003: Execution Budgets', link: '/en/decisions/adp-003' },
                { text: 'ADP-004: Unicode Identifiers', link: '/en/decisions/adp-004' },
                { text: 'ADP-005: Parser Recursion Limits', link: '/en/decisions/adp-005' },
                { text: 'ADP-006: Decisions Waves 3 & 4', link: '/en/decisions/adp-006' },
                { text: 'ADP-007: Package Identity', link: '/en/decisions/adp-007' },
                { text: 'ADP-008: Release v0.1.0 Boundary', link: '/en/decisions/adp-008' },
                { text: 'ADP-009: Synchronous C ABI', link: '/en/decisions/adp-009' },
                { text: 'ADP-010: Thin Proofs (Rust, C, JS)', link: '/en/decisions/adp-010' },
                { text: 'ADP-011: Performance & Ergonomics Roadmap', link: '/en/decisions/adp-011' },
                { text: 'ADP-012: Modern Syntax Ergonomics', link: '/en/decisions/adp-012' },
                { text: 'ADP-013: WebAssembly Substrate & Self-Hosting', link: '/en/decisions/adp-013' }
              ]
            }
          ],
          '/en/evidence/': [
            {
              text: 'Evidence & Benchmarks',
              items: [
                { text: 'Cross-Language Benchmarks', link: '/en/evidence/cross-language' },
                { text: 'Optimization Saga & Lessons', link: '/en/evidence/performance-lessons' },
                { text: 'Conformance Matrix', link: '/en/evidence/conformance' },
                { text: 'Slice Register (P00-P04)', link: '/en/evidence/slices' }
              ]
            }
          ],
          '/en/governance/': [
            {
              text: 'Governance & Process',
              items: [
                { text: 'Prumo & LPC Methodology', link: '/en/governance/prumo-and-lpc' },
                { text: 'Code Standards & Testing', link: '/en/governance/standards-and-testing' },
                { text: 'Security & Sandboxing', link: '/en/governance/security-and-threat-model' },
                { text: 'Official Changelog', link: '/en/governance/changelog' }
              ]
            }
          ]
        },

        footer: {
          message: 'Distributed under MIT and Apache 2.0 licenses. Continuous governance with Prumo v0.6.',
          copyright: 'Copyright © 2026 Poppy Team & Aipo Language Contributors'
        },

        docFooter: {
          prev: 'Previous page',
          next: 'Next page'
        },

        outline: {
          label: 'On this page',
          level: [2, 3]
        }
      }
    }
  },

  themeConfig: {
    logo: '/assets/logo.svg',
    siteTitle: 'Aipo',

    search: {
      provider: 'local',
      options: {
        locales: {
          root: {
            translations: {
              button: {
                buttonText: 'Pesquisar documentação...',
                buttonAriaLabel: 'Pesquisar documentação'
              },
              modal: {
                noResultsText: 'Nenhum resultado encontrado para',
                resetButtonTitle: 'Limpar pesquisa',
                footer: {
                  selectText: 'para selecionar',
                  navigateText: 'para navegar',
                  closeText: 'para fechar'
                }
              }
            }
          },
          en: {
            translations: {
              button: {
                buttonText: 'Search documentation...',
                buttonAriaLabel: 'Search documentation'
              },
              modal: {
                noResultsText: 'No results found for',
                resetButtonTitle: 'Reset search',
                footer: {
                  selectText: 'to select',
                  navigateText: 'to navigate',
                  closeText: 'to close'
                }
              }
            }
          }
        }
      }
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/poppy-team/aipo-lang' }
    ]
  }
})
