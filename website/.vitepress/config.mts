import { defineConfig } from 'vitepress'
import chapters from '../content/_meta/chapters.json'

const bookParts = [...new Set(chapters.map(c => c.part))]
const book = [
  { text: 'Sobre o livro', link: '/learn/' },
  ...bookParts.map(part => ({
    text: part, collapsed: true,
    items: chapters.filter(c => c.part === part).map(c => ({ text: c.title, link: c.route }))
  }))
]
const manual = [
  { text: 'Manual · por assunto', link: '/manual/' },
  { text: 'Fundamentos', collapsed: false, items: [
    { text: 'Variáveis e tipos', link: '/manual/variables' },
    { text: 'Texto e Unicode', link: '/manual/text' },
    { text: 'Números e Bytes', link: '/manual/numbers' },
    { text: 'Coleções', link: '/manual/collections' },
    { text: 'Condições e laços', link: '/manual/control-flow' },
    { text: 'Funções e closures', link: '/manual/functions' }
  ] },
  { text: 'Modelagem', collapsed: true, items: [
    { text: 'Structs e métodos', link: '/manual/structs' },
    { text: 'Interfaces e contratos', link: '/manual/interfaces' },
    { text: 'Falhas e rollback', link: '/manual/failures' },
    { text: 'Módulos e pacotes', link: '/manual/modules-packages' },
    { text: 'Enums e padrões', link: '/manual/enums-patterns' },
    { text: 'Atualizações com with', link: '/manual/updates' }
  ] },
  { text: 'Recursos', collapsed: true, items: [
    { text: 'Operadores', link: '/manual/operators' },
    { text: 'Async e tarefas', link: '/manual/async' },
    { text: 'Diretivas e testes', link: '/manual/directives' },
    { text: 'Biblioteca padrão', link: '/manual/standard-library' }
  ] },
  { text: 'Projetos e integração', collapsed: true, items: [
    { text: 'CLI', link: '/manual/cli' },
    { text: 'Backends', link: '/manual/backends' },
    { text: 'Testes', link: '/manual/testing' },
    { text: 'Hosts, jogos e GUI', link: '/manual/hosts' }
  ] }
]
const api = ['collections','string','math','json','random','binary','encoding','path','url','regex','time','task','testing','log','sh','io','env','fs']
const reference = [
  { text: 'Referência', link: '/reference/' },
  { text: 'Sintaxe e operadores', link: '/reference/syntax' },
  { text: 'Comandos CLI', link: '/reference/cli' },
  { text: 'Biblioteca padrão', link: '/reference/stdlib' },
  { text: 'Compatibilidade por backend', link: '/reference/status' },
  { text: 'Diagnósticos', link: '/reference/diagnostics' },
  { text: 'APIs por módulo', collapsed: true, items: api.map(m => ({ text: m, link: '/reference/api/'+m })) }
]
const engineering = [
  { text: 'Visão da engenharia', link: '/engineering/' },
  { text: 'Arquitetura', link: '/engineering/architecture' },
  { text: 'Frontend', link: '/engineering/frontend' },
  { text: 'Runtime e bytecode', link: '/engineering/runtime' },
  { text: 'Backends', link: '/engineering/backends' },
  { text: 'Embedding e C ABI', link: '/engineering/embedding' },
  { text: 'Qualidade e testes', link: '/engineering/quality' },
  { text: 'Progresso', link: '/progress/' },
  { text: 'Protocolo de progresso', link: '/engineering/progress-protocol' },
  { text: 'Agentes', link: '/engineering/agents/' },
  { text: 'Arquivo técnico', link: '/archive/' }
]
export default defineConfig({
  title: 'Aipo',
  description: 'Aprenda a linguagem Aipo com explicações claras, exemplos reais e uma referência atualizada.',
  srcDir: 'content', outDir: 'dist', publicDir: 'public', cleanUrls: true,
  head: [
    ['meta', { name:'theme-color', content:'#0f766e' }],
    ['link', { rel:'icon', href:'/logo.svg', type:'image/svg+xml' }]
  ],
  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'Aipo',
    search: { provider: 'local', options: { locales: { root: { translations: { button: { buttonText: 'Buscar', buttonAriaLabel: 'Buscar na documentação' }, modal: { noResultsText: 'Nenhum resultado encontrado', resetButtonTitle: 'Limpar busca', footer: { selectText: 'Selecionar', navigateText: 'Navegar', closeText: 'Fechar' } } } } } } },
    nav: [
      { text:'Começar', link:'/start/' },
      { text:'Manual', link:'/manual/' },
      { text:'Exemplos', link:'/examples/' },
      { text:'Referência', link:'/reference/' },
      { text:'Projeto', items: [
        { text:'Progresso da implementação', link:'/progress/' },
        { text:'Arquitetura e engenharia', link:'/engineering/' },
        { text:'Guias especializados', link:'/guides/' },
        { text:'Documentação histórica', link:'/archive/' }
      ]}
    ],
    sidebar: {
      '/start/': [{ text:'Primeiros passos', link:'/start/' }, { text:'Manual', link:'/manual/' }, { text:'Exemplos reais', link:'/examples/' }],
      '/manual/': manual,
      '/examples/': [{ text:'Exemplos completos', link:'/examples/' }, { text:'Manual por assunto', link:'/manual/' }],
      '/learn/': book,
      '/reference/': reference,
      '/guides/': [
        { text:'Guias', link:'/guides/' },
        { text:'Instalação', link:'/guides/installation' },
        { text:'Módulos', link:'/guides/modules' },
        { text:'Testes', link:'/guides/testing' },
        { text:'Compilar para JS/Wasm', link:'/guides/build-targets' },
        { text:'Embedding', link:'/guides/embedding' },
        { text:'Solucionar erros', link:'/guides/troubleshooting' }
      ],
      '/engineering/': engineering,
      '/progress/': [ {text:'Progresso',link:'/progress/'},{text:'Como atualizar',link:'/engineering/progress-protocol'} ],
      '/archive/': engineering
    },
    outline: false,
    docFooter: { prev:'Anterior', next:'Próximo' },
    returnToTopLabel: 'Voltar ao topo',
    sidebarMenuLabel: 'Navegação',
    darkModeSwitchLabel: 'Tema',
    footer: { copyright:'Aipo Language · Poppy' }
  },
  locales: {
    root: { label:'Português', lang:'pt-BR' },
    en: {
      label:'English (partial)',lang:'en-US',link:'/en/',title:'Aipo',
      themeConfig: {
        nav: [ {text:'Home',link:'/en/'},{text:'Getting started',link:'/en/getting-started'},{text:'Português',link:'/manual/'} ],
        sidebar: {'/en/':[{text:'Overview',link:'/en/'},{text:'Getting started',link:'/en/getting-started'}]}
      }
    }
  }
})
