import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve, dirname } from 'node:path'
import { filterTasks, normalizeText, percentage, statusLabel, totals, validateProgress } from '../progress/core.mjs'

const here = dirname(fileURLToPath(import.meta.url))
const original = JSON.parse(readFileSync(resolve(here, '../public/progress/tasks.json'), 'utf8'))
const clone = () => structuredClone(original)

test('snapshot valid: all processes and evidence structurally consistent', () => {
  assert.equal(validateProgress(clone()).tasks.length, 33)
})
test('percentage truncates (not rounds) and uses three digits', () => {
  const task = { status:'IN PROGRESS', checkpoints:[{completed:true},{completed:true},{completed:false},{completed:false},{completed:false},{completed:false}] }
  assert.equal(percentage(task), 33)
  assert.equal(statusLabel(task), 'IN PROGRESS (033%)')
  assert.equal(percentage({checkpoints:[]}), 0)
})
test('TODO and DONE labels are literal', () => {
  assert.equal(statusLabel({status:'TODO'}), 'TODO')
  assert.equal(statusLabel({status:'DONE'}), 'DONE')
})
test('accent- and case-insensitive search and combined filters', () => {
  const data = clone()
  assert.ok(filterTasks(data.tasks, {query:'semÂntica'}).some(t => t.id === 'F05'))
  const subset = filterTasks(data.tasks, {status:'DONE',phase:'architecture'})
  assert.equal(subset.length, 1)
  assert.equal(subset[0].id, 'A01')
  assert.equal(filterTasks(data.tasks, {query:'this-does-not-exist'}).length, 0)
  assert.equal(normalizeText('EXCEÇÃO'), 'excecao')
})
test('counters match the source of truth, no separate percentage state', () => {
  const count = totals(original.tasks)
  assert.equal(count.TODO, 0)
  assert.equal(count['IN PROGRESS'], 32)
  assert.equal(count.DONE, 1)
})
test('rejects completing a checkpoint without evidence', () => {
  const data=clone()
  data.tasks[1].checkpoints[1].completed = true
  assert.throws(() => validateProgress(data), /checkpoint invalid or unproven/)
})
test('rejects evidence outside task document scope', () => {
  const data=clone()
  data.tasks[1].checkpoints[0].evidence = 'lexer'
  assert.throws(() => validateProgress(data), /evidence outside task documents/)
})
test('rejects DONE without all checkpoints or passing gates', () => {
  const data=clone()
  data.tasks[1].status = 'DONE'
  assert.throws(() => validateProgress(data), /DONE lacks proven acceptance/)
})
test('rejects TODO if any criterion is complete', () => {
  const data=clone()
  data.tasks[1].status = 'TODO'
  assert.throws(() => validateProgress(data), /TODO has completed checkpoints/)
})
test('rejects incomplete status if all criteria complete', () => {
  const data=clone()
  data.tasks[1].checkpoints.forEach(c => { c.completed = true; c.evidence = 'syntax-spec' })
  assert.throws(() => validateProgress(data), /IN PROGRESS inconsistent/)
})
test('rejects duplicate IDs, dates, invalid branches and traversal', () => {
  const duplicate=clone()
  duplicate.tasks[1].id=duplicate.tasks[0].id
  assert.throws(() => validateProgress(duplicate), /invalid task id/)
  const future=clone()
  future.tasks[1].updated='2026-10-10'
  assert.throws(() => validateProgress(future), /task date exceeds snapshot/)
  const branch=clone()
  branch.branch='temporary'
  assert.throws(() => validateProgress(branch), /schema, repository or branch/)
  const unsafe=clone()
  unsafe.evidence.lexer.sourcePaths[0]='../../config'
  assert.throws(() => validateProgress(unsafe), /missing or unsafe source/)
})
test('rejects false CI pass without linked proof', () => {
  const data=clone()
  data.tasks[1].gates[0].status='pass'
  assert.throws(() => validateProgress(data), /gate pass without task-specific proof/)
})
test('rejects missing routes and unresolvable code paths through injectable checks', () => {
  const data=clone()
  assert.throws(() => validateProgress(data,{routeExists:route => route!==data.tasks[1].document}), /invalid evidence|invalid task documents/)
  assert.throws(() => validateProgress(data,{sourceExists:path => !path.includes('aipo-lexer')}), /missing or unsafe source/)
})
