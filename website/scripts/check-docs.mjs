import { readdirSync, readFileSync, existsSync } from 'node:fs'
import { resolve, dirname, join, relative, extname } from 'node:path'
import { fileURLToPath } from 'node:url'

const website = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const contentDir = resolve(website, 'content')
const repository = resolve(website, '..')
const configPath = resolve(website, '.vitepress/config.mts')
const chapters = JSON.parse(readFileSync(resolve(contentDir, '_meta/chapters.json'), 'utf8'))
const failures = []
const files = []

function walk(path) {
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const item = join(path, entry.name)
    if (entry.isDirectory()) walk(item)
    else if (entry.isFile() && item.endsWith('.md')) files.push(item)
  }
}
function fail(file, message) { failures.push(relative(repository, file) + ': ' + message) }
function routePath(route) {
  const normalized = decodeURIComponent(route.split('#')[0].split('?')[0])
  const item = normalized.startsWith('/') ? normalized.slice(1) : normalized
  const pathname = resolve(contentDir, item)
  if (pathname !== contentDir && !pathname.startsWith(contentDir + '/')) return null
  const candidates = normalized.endsWith('/') ? [join(pathname, 'index.md')] : [pathname, pathname + '.md', join(pathname, 'index.md')]
  return candidates.find(file => file.endsWith('.md') && existsSync(file)) || null
}
function checkLinks(file, source) {
  // Remove code fences so examples do not look like documentation links.
  const prose = source.replace(/^```[^\n]*\n[\s\S]*?^```\s*$/gm, '')
  for (const match of prose.matchAll(/(?<!!)\[[^\]]*\]\(([^)]+)\)/g)) {
    const raw = match[1].trim().split(/\s+["']/)[0].replace(/^<|>$/g, '')
    if (!raw || raw.startsWith('#') || /^[a-z]+:/i.test(raw) || raw.startsWith('//')) continue
    const target = raw.startsWith('/') ? raw : '/' + relative(contentDir, resolve(dirname(file), raw))
    if (target.includes('..')) { fail(file, 'path traversal or unusual path ' + raw); continue }
    if (!routePath(target)) fail(file, 'missing link ' + raw)
  }
}

walk(contentDir)
const all = new Set(files)
if (!existsSync(configPath)) failures.push('Missing website config')
if (new Set(chapters.map(c => c.id)).size !== chapters.length) failures.push('Duplicate lesson IDs')
if (new Set(chapters.map(c => c.route)).size !== chapters.length) failures.push('Duplicate lesson routes')
for (const [index, entry] of chapters.entries()) {
  if (!routePath(entry.route)) failures.push('Unresolved chapter ' + entry.route)
  if (entry.previous !== (index === 0 ? null : chapters[index - 1].id)) failures.push('Wrong previous for ' + entry.id)
  if (entry.next !== (index === chapters.length - 1 ? null : chapters[index + 1].id)) failures.push('Wrong next for ' + entry.id)
  if (entry.verification !== 'not-run') failures.push('Unverified lesson cannot claim verified: ' + entry.id)
  if (!existsSync(resolve(repository, entry.source))) failures.push('Missing lesson source ' + entry.source)
}
const fileText = new Map()
for (const file of files) {
  const source = readFileSync(file, 'utf8')
  fileText.set(file, source)
  if (!source.startsWith('---\n')) fail(file, 'missing YAML frontmatter')
  if (!/^#\s+\S/m.test(source) && !source.includes('layout: home')) fail(file, 'missing H1')
  if ((source.match(/^```/gm) || []).length % 2 !== 0) fail(file, 'unclosed code fence')
  checkLinks(file, source)
}
const config = readFileSync(configPath, 'utf8')
for (const match of config.matchAll(/link:\s*['"]([^'"]+)['"]/g)) {
  if (match[1].startsWith('/') && !routePath(match[1])) failures.push('Config navigation missing ' + match[1])
}
const required = [
  '/learn/', '/guides/', '/reference/', '/concepts/', '/engineering/', '/archive/',
  '/engineering/decisions/', '/engineering/agents/', '/reference/status', '/en/',
  '/progress/', '/engineering/progress-protocol', '/engineering/legacy-audit'
]
for (const route of required) if (!routePath(route)) failures.push('Required page absent: ' + route)

if (failures.length) {
  for (const error of failures) console.error('FAIL ' + error)
  console.error(failures.length + ' error(s) found')
  process.exitCode = 1
} else {
  console.log('OK: ' + files.length + ' pages, ' + chapters.length + ' chapters, internal links and navigation resolved')
  console.log('NOTE: this check does not compile or execute Aipo examples')
}
