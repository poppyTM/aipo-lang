const STATES = ['TODO', 'IN PROGRESS', 'DONE']
const GATES = ['not-run', 'pass', 'fail', 'blocked']
const EVIDENCE_TYPES = ['source-inspection', 'static-and-historical', 'test-run', 'manual-review']
export const STATES_ORDER = Object.freeze([...STATES])

export function normalizeText(value) {
  return String(value ?? '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLocaleLowerCase('pt-BR')
}
export function percentage(task) {
  if (!Array.isArray(task?.checkpoints) || task.checkpoints.length === 0) return 0
  return Math.floor(100 * task.checkpoints.filter((c) => c.completed === true).length / task.checkpoints.length)
}
export function statusLabel(task) {
  return task.status === 'IN PROGRESS'
    ? `IN PROGRESS (${String(percentage(task)).padStart(3, '0')}%)`
    : task.status
}
export function totals(tasks) {
  return Object.fromEntries(STATES.map((name) => [name, tasks.filter((t) => t.status === name).length]))
}
export function filterTasks(tasks, { query = '', phase = '', status = '' } = {}) {
  const needle = normalizeText(query).trim()
  return tasks.filter((task) =>
    (!phase || task.phase === phase) &&
    (!status || task.status === status) &&
    (!needle || normalizeText([task.id, task.title, task.description, task.baseline, task.next].join(' ')).includes(needle))
  )
}
const validDate = (value) =>
  typeof value === 'string' &&
  /^\d{4}-\d{2}-\d{2}$/.test(value) &&
  !Number.isNaN(Date.parse(value)) &&
  new Date(value).toISOString().slice(0, 10) === value

export function validateProgress(data, { routeExists = () => true, sourceExists = () => true } = {}) {
  const error = (message) => { throw new Error(`progress: ${message}`) }
  if (!data || data.schemaVersion !== 1 || data.repository !== 'poppyTM/aipo-lang' || data.branch !== 'main') error('schema, repository or branch')
  if (typeof data.baselineSha !== 'string' || !/^[0-9a-f]{40}$/.test(data.baselineSha) || !validDate(data.updated)) error('baselineSha or updated')
  if (!Array.isArray(data.phases) || !data.phases.length || !Array.isArray(data.tasks) || !data.tasks.length) error('phases or tasks missing')
  if (!data.evidence || typeof data.evidence !== 'object' || Array.isArray(data.evidence)) error('evidence must be an object')
  const phases = new Set()
  for (const phase of data.phases) {
    if (!/^[a-z][a-z0-9-]*$/.test(phase.id || '') || phases.has(phase.id) || !phase.title || !phase.description) error('duplicate or invalid phase')
    phases.add(phase.id)
  }
  const allEvidence = new Set(Object.keys(data.evidence))
  for (const [key, proof] of Object.entries(data.evidence)) {
    if (!/^[a-z0-9-]+$/.test(key) || !proof || !EVIDENCE_TYPES.includes(proof.kind) || !/^[0-9a-f]{40}$/.test(proof.revision || '') || !proof.summary || !routeExists(proof.document)) error(`invalid evidence: ${key}`)
    if (!Array.isArray(proof.sourcePaths) || !proof.sourcePaths.length) error(`sources missing: ${key}`)
    for (const source of proof.sourcePaths) {
      if (typeof source !== 'string' || source.startsWith('/') || source.includes('..') || !sourceExists(source)) error(`missing or unsafe source: ${key} / ${source}`)
    }
  }
  const ids = new Set()
  for (const task of data.tasks) {
    if (!/^[A-Z]{1,3}\d{2}$/.test(task.id || '') || ids.has(task.id)) error(`invalid task id: ${task.id}`)
    ids.add(task.id)
    if (!phases.has(task.phase) || !task.title || !task.description || !task.baseline || !task.next) error(`missing task metadata: ${task.id}`)
    if (!validDate(task.updated) || task.updated > data.updated) error(`task date exceeds snapshot: ${task.id}`)
    if (!STATES.includes(task.status)) error(`invalid state: ${task.id}`)
    if (!Array.isArray(task.documents) || !task.documents.length || task.document !== task.documents[0] || new Set(task.documents).size !== task.documents.length || task.documents.some((r) => !routeExists(r))) error(`invalid task documents: ${task.id}`)
    if (!Array.isArray(task.checkpoints) || task.checkpoints.length === 0) error(`missing checkpoints: ${task.id}`)
    for (const point of task.checkpoints) {
      if (!point.title || typeof point.completed !== 'boolean' || (point.completed && !allEvidence.has(point.evidence))) error(`checkpoint invalid or unproven: ${task.id}`)
      if (!point.completed && point.evidence !== null) error(`open checkpoint must have null evidence: ${task.id}`)
      if (point.completed && !task.documents.includes(data.evidence[point.evidence].document)) error(`evidence outside task documents: ${task.id}`)
    }
    if (!Array.isArray(task.gates) || !task.gates.length) error(`missing gates: ${task.id}`)
    for (const gate of task.gates) {
      if (!gate.name || !GATES.includes(gate.status)) error(`invalid gate: ${task.id}`)
      if (gate.status === 'pass' && (!allEvidence.has(gate.evidence) || !task.documents.includes(data.evidence[gate.evidence].document))) error(`gate pass without task-specific proof: ${task.id}`)
      if (gate.status !== 'pass' && gate.evidence !== null) error(`pending/failing gate cannot link passing proof: ${task.id}`)
    }
    const completed = task.checkpoints.filter((c) => c.completed).length
    if (task.status === 'TODO' && completed !== 0) error(`TODO has completed checkpoints: ${task.id}`)
    if (task.status === 'IN PROGRESS' && completed === task.checkpoints.length) error(`IN PROGRESS inconsistent: ${task.id}`)
    if (task.status === 'DONE' && (completed !== task.checkpoints.length || task.gates.some((g) => g.status !== 'pass'))) error(`DONE lacks proven acceptance: ${task.id}`)
  }
  return data
}
