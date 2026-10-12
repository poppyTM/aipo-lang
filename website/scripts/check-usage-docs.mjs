import { readFileSync, existsSync, readdirSync } from 'node:fs'
import { resolve, dirname, join, basename } from 'node:path'
import { fileURLToPath } from 'node:url'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const root = resolve(site, '..')
const fails = []
function assert(condition, message) { if (!condition) fails.push(message) }
const get = (path) => readFileSync(resolve(site, path), 'utf8')

const essentials = [
  'content/start/index.md',
  'content/manual/index.md',
  'content/examples/index.md',
  'content/reference/index.md',
  'content/reference/stdlib.md',
  'content/reference/status.md',
  'content/engineering/accessibility.md'
]
for (const path of essentials) assert(existsSync(resolve(site, path)), 'missing core documentation: '+path)

const manual = readdirSync(resolve(site, 'content/manual')).filter(x => x.endsWith('.md'))
const api = readdirSync(resolve(site, 'content/reference/api')).filter(x => x.endsWith('.md'))
assert(manual.length >= 20, 'manual coverage unexpectedly reduced: '+manual.length)
assert(api.length >= 18, 'source-backed API coverage unexpectedly reduced: '+api.length)

let examples = 0
for (const file of readdirSync(resolve(root, 'examples'))) {
  if (!file.endsWith('.aipo')) continue
  examples++
  const route = file.slice(0, -5).replaceAll('_', '-')
  const target = resolve(site, 'content/examples', route+'.md')
  if (!existsSync(target)) { fails.push('example missing page: '+file); continue }
  const source = readFileSync(join(root, 'examples', file), 'utf8').trimEnd()
  const page = readFileSync(target, 'utf8')
  const block = page.split('## Código completo')[1]?.match(/```aipo\s*\n([\s\S]*?)\n```/)
  assert(Boolean(block), 'missing complete source block: '+route)
  if (block) assert(block[1].trimEnd() === source, 'stale example; source differs from '+file)
}
assert(examples >= 26, 'the source example catalogue shrank unexpectedly: '+examples)

const theme = get('.vitepress/theme/custom.css')
const layout = get('.vitepress/theme/Layout.vue')
const home = get('content/index.md')
const config = get('.vitepress/config.mts')
assert(theme.includes(':focus-visible'), 'missing visible keyboard focus treatment')
assert(theme.includes('prefers-reduced-motion'), 'missing reduced-motion fallback')
assert(theme.includes('max-width: 760px') || theme.includes('max-width:760px'), 'reading width not constrained')
assert(layout.includes('aria-pressed') && layout.includes('aria-label'), 'reading settings missing accessible controls')
for (const path of ['/start/', '/manual/', '/examples/', '/reference/']) {
  assert(config.includes(path), 'missing reader-first navigation route '+path)
  assert(home.includes(path), 'home does not point to '+path)
}
if (fails.length) {
  for (const message of fails) console.error('FAIL '+message)
  process.exitCode = 1
} else {
  console.log('OK: '+manual.length+' manual pages, '+api.length+' API modules, '+examples+' exact source examples, reader-first navigation and static accessibility hooks')
  console.log('NOTE: this is a structural audit, not a screen-reader usability study or execution of Aipo programs')
}
