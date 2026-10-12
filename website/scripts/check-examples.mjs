import { existsSync, readFileSync } from 'node:fs'
import { join, resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const binary = process.env.AIPO_BIN || join(repo, 'target/debug/aipo')
if (!existsSync(binary)) {
  console.error('No compiled Aipo CLI: set AIPO_BIN or run cargo build -p aipo-cli')
  process.exit(2)
}
const sources = [
  'examples/01_fizzbuzz.aipo',
  'examples/06_variables_and_values.aipo',
  'examples/07_functions_defaults_named_args.aipo',
  'examples/09_strings_unicode_and_formatting.aipo',
  'examples/11_failures_or_else_attempt.aipo',
  'examples/16_ranges_repeat_each.aipo',
  'examples/24_idiomatic_aipo_showcase.aipo'
]
let failures = 0
for (const source of sources) {
  const path = join(repo, source)
  const expected = path.slice(0, -'.aipo'.length) + '.stdout'
  if (!existsSync(path) || !existsSync(expected)) {
    console.error('Missing fixture for ' + source); failures++; continue
  }
  const result = spawnSync(binary, ['run', path], {cwd: repo, encoding: 'utf8', timeout: 15000})
  const stdout = result.stdout || ''
  const want = readFileSync(expected, 'utf8')
  if (result.error || result.status !== 0 || stdout !== want) {
    console.error('FAIL ' + source + ': exit=' + result.status)
    if (result.stderr) console.error(result.stderr.slice(0, 1000))
    failures++
  } else {
    console.log('PASS ' + source)
  }
}
console.log('Original examples run:', sources.length, 'failed:', failures)
console.log('New lesson snippets are NOT automatically verified by this smoke suite')
process.exitCode = failures ? 1 : 0
