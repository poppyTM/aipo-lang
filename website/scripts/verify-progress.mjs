import { existsSync, readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { validateProgress, totals, percentage } from '../progress/core.mjs'

const website = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repository = resolve(website, '..')
const data = JSON.parse(readFileSync(resolve(website, 'public/progress/tasks.json'), 'utf8'))
const content = resolve(website, 'content')
const routeExists = (route) => {
  if (typeof route !== 'string' || !route.startsWith('/') || route.includes('..')) return false
  const suffix = route.slice(1).replace(/\/$/, '')
  const base = resolve(content, suffix)
  if (base !== content && !base.startsWith(content + '/')) return false
  return existsSync(base + '.md') || existsSync(resolve(base, 'index.md'))
}
const sourceExists = (source) => {
  const path = resolve(repository, source)
  return (path === repository || path.startsWith(repository + '/')) && existsSync(path)
}
try {
  validateProgress(data, { routeExists, sourceExists })
  const count = totals(data.tasks)
  const completed = data.tasks.reduce((sum, t) => sum + t.checkpoints.filter(c => c.completed).length, 0)
  const total = data.tasks.reduce((sum, t) => sum + t.checkpoints.length, 0)
  console.log(`OK: ${data.tasks.length} processos em ${data.phases.length} etapas, ${Object.keys(data.evidence).length} provas referenciadas.`)
  console.log(`TODO=${count.TODO} IN PROGRESS=${count['IN PROGRESS']} DONE=${count.DONE}; ${completed}/${total} checkpoints de aceite inventariados.`)
  console.log(`Baseline ${data.baselineSha.slice(0, 12)}; todas as rotas, datas, origens, gates e evidências são estruturalmente válidas.`)
  console.log('NOTE: este validador não executa o compilador Aipo nem certifica a validade factual de uma prova humana.')
} catch (error) {
  console.error('FAIL', error.message)
  process.exitCode = 1
}
