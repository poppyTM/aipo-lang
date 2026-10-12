// aipo-runtime.js — versioned JavaScript semantics shim for Aipo (Wave 2).
// ESM. Mirrors the Rust VM value model and Core IR interpreter semantics:
// divergence between VM and JS backends is a bug.
// RUNTIME_VERSION must match aipo-js RUNTIME_VERSION in src/lib.rs.
export const RUNTIME_VERSION = '1.2.1';

const MAX_SAFE_INT = 9007199254740991;
const MIN_SAFE_INT = -9007199254740991;
const BYTES_MAX_ALLOCATION = 67108864;

let __nextId = 1;
function freshId() { return __nextId++; }

// ---- output capture (io.print/println write here; harness overrides) ----
export let outputBuffer = [];
let useBuffer = false;
export function setOutputSink(arr) { outputBuffer = arr; useBuffer = true; }
export function resetOutputSink() { outputBuffer = []; useBuffer = false; }
function emitText(s) {
  if (useBuffer) { outputBuffer.push(String(s)); return; }
  if (typeof process !== 'undefined' && process.stdout && process.stdout.write) {
    process.stdout.write(String(s));
  } else {
    outputBuffer.push(String(s));
  }
}

// ---- faults (never capturable by attempt) ----
export class AipoFault extends Error {
  constructor(code, message) {
    super(`[${code}] ${message}`);
    this.code = code;
  }
}
function fault(code, message) { throw new AipoFault(code, message); }
function typeMismatch(expected, actual) {
  fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected ${expected}, got ${actual}`);
}

// ---- value constructors (tagged objects) ----
export function vNone() { return { t: 'none' }; }
export function vBool(b) { return { t: 'bool', v: !!b }; }
export function vInt(n) {
  if (!Number.isInteger(n)) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Int, got ${n}`);
  return checkSafeInt(n);
}
export function vFloat(f) { return checkFiniteFloat(f); }
export function vByte(b) {
  if (!Number.isInteger(b) || b < 0 || b > 255) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Byte, got ${b}`);
  return { t: 'byte', v: normInt(b) };
}
export function vStr(s) { return { t: 'str', v: String(s).normalize('NFC') }; }
export function vList(items) { return { t: 'list', items: items || [], id: freshId() }; }
function buildStrIdx(entries) {
  const idx = new Map();
  entries.forEach(([k], i) => {
    if (k.t === 'str' && !idx.has(k.v)) idx.set(k.v, i);
  });
  return idx;
}
function rebuildStrIdx(d) { d.strIdx = buildStrIdx(d.entries); }
export function vDict(entries) {
  const list = entries || [];
  return { t: 'dict', entries: list, strIdx: buildStrIdx(list), id: freshId() };
}
export function dictGet(d, key) {
  if (key.t === 'str') {
    const at = d.strIdx.get(key.v);
    if (at === undefined) return null;
    return d.entries[at][1];
  }
  const found = d.entries.find(([k]) => valuesEqual(k, key));
  return found ? found[1] : null;
}
export function dictUpsert(d, key, value) {
  if (key.t === 'str') {
    const at = d.strIdx.get(key.v);
    if (at !== undefined && d.entries[at]) {
      d.entries[at][1] = value;
      return;
    }
  } else {
    const found = d.entries.find(([k]) => valuesEqual(k, key));
    if (found) {
      found[1] = value;
      return;
    }
  }
  if (key.t === 'str' && !d.strIdx.has(key.v)) d.strIdx.set(key.v, d.entries.length);
  d.entries.push([key, value]);
}
export function dictRemove(d, key) {
  let at = -1;
  if (key.t === 'str') {
    const found = d.strIdx.get(key.v);
    if (found !== undefined) at = found;
  } else {
    at = d.entries.findIndex(([k]) => valuesEqual(k, key));
  }
  if (at < 0) return false;
  d.entries.splice(at, 1);
  rebuildStrIdx(d);
  return true;
}
export function dictClear(d) { d.entries.length = 0; d.strIdx.clear(); }
export function vBytes(data) { return { t: 'bytes', data: data instanceof Uint8Array ? data : new Uint8Array(data || []) }; }
export function vSet(items) {
  const list = [];
  for (const item of (items || [])) {
    if (!list.some(x => valuesEqual(x, item))) {
      list.push(item);
    }
  }
  return { t: 'set', items: list, id: freshId() };
}
export function vDuration(secs) { return { t: 'duration', v: checkFiniteFloat(secs).v }; }
export function vSequence(pipeline) { return { t: 'sequence', source: pipeline.source, ops: pipeline.ops || [], id: freshId() }; }
export function vTask(id) { return { t: 'task', id: id !== undefined ? id : freshId() }; }
export function vGroup(id) { return { t: 'group', id: id !== undefined ? id : freshId() }; }
export function vRange(start, end) { return { t: 'range', start, end }; }
export function vType(name) { return { t: 'type', name }; }
export function vFail(msg, payload) { return { t: 'fail', msg: String(msg), payload: payload !== undefined ? payload : vNone() }; }
export function vUnset() { return { t: 'unset' }; }
function vStruct(type, fields, fixed, constructing) {
  return { t: 'struct', type, fields: fields || [], fixed: new Set(fixed || []), constructing: !!constructing, id: freshId() };
}

export function isFailure(v) { return v && v.t === 'fail'; }
export function isUnset(v) { return v && v.t === 'unset'; }

export function typeName(v) {
  switch (v.t) {
    case 'none': return 'none';
    case 'bool': return 'Bool';
    case 'int': return 'Int';
    case 'float': return 'Float';
    case 'str': return 'String';
    case 'list': return 'List';
    case 'dict': return 'Dict';
    case 'set': return 'Set';
    case 'duration': return 'Duration';
    case 'sequence': return 'Sequence';
    case 'task': return 'Task';
    case 'group': return 'Group';
    case 'struct': return 'struct';
    case 'func': case 'closure': case 'native': case 'bound': return 'Function';
    case 'byte': return 'Byte';
    case 'bytes': return 'Bytes';
    case 'type': return 'Type';
    case 'range': return 'Range';
    case 'fail': return 'Failure';
    default: return '<omitted argument>';
  }
}

function structDisplayName(v) {
  if (v && v.t === 'struct') return v.type;
  return typeName(v);
}

export function display(v) {
  switch (v.t) {
    case 'none': return 'none';
    case 'bool': return v.v ? 'true' : 'false';
    case 'int': return String(v.v);
    case 'float': return floatText(v.v);
    case 'byte': return String(v.v);
    case 'str': return v.v;
    case 'list': return `[${v.items.map(display).join(', ')}]`;
    case 'dict': return `#{${v.entries.map(([k, val]) => `${display(k)}: ${display(val)}`).join(', ')}}`;
    case 'set': return `{${v.items.map(display).join(', ')}}`;
    case 'sequence': return '<sequence>';
    case 'task': return `<task #${v.id}>`;
    case 'group': return `<group #${v.id}>`;
    case 'duration': return Number.isInteger(v.v) ? (Object.is(v.v, -0) ? '-0s' : `${v.v}s`) : `${v.v}s`;
    case 'struct': return `${v.type}{${v.fields.map(([k, val]) => `${k}: ${display(val)}`).join(', ')}}`;
    case 'bytes': return '<bytes>';
    case 'type': return v.name;
    case 'range': return `${v.start}..${v.end}`;
    case 'fail': return `fail("${v.msg}")`;
    case 'func': case 'closure': case 'native': case 'bound': return '<function>';
    default: return '<omitted argument>';
  }
}


// Canonical float rendering shared by display and String(): integral floats
// print with one decimal, preserving negative zero like the VM reference.
function floatText(f) {
  if (Number.isInteger(f)) return Object.is(f, -0) ? '-0.0' : `${f}.0`;
  return String(f);
}
// ---- numeric guards ----
// Integers never hold negative zero: Rust i64 arithmetic cannot produce it,
// but JS doubles can (`-33 * 0 === -0`). Normalizing here keeps every Int
// producer (arithmetic, conversions, lengths, indexes) identical to the VM.
function normInt(n) {
  return Object.is(n, -0) ? 0 : n;
}
function checkSafeInt(n) {
  if (!Number.isInteger(n) || n < MIN_SAFE_INT || n > MAX_SAFE_INT) {
    fault('AIPO_RT_OVERFLOW', `${n} exceeds integer range ±(2^53 - 1)`);
  }
  return { t: 'int', v: normInt(n) };
}
function checkFiniteFloat(f) {
  if (typeof f !== 'number' || !Number.isFinite(f)) fault('AIPO_RT_NON_FINITE_FLOAT', 'non-finite float');
  return { t: 'float', v: f };
}
function widen(v) { return v.t === 'byte' ? { t: 'int', v: v.v } : v; }
function needsWiden(a, b) { return a.t === 'byte' || b.t === 'byte'; }
function toF64(v) {
  if (v.t === 'int' || v.t === 'byte') return v.v;
  if (v.t === 'float') return v.v;
  return NaN;
}

// ---- arithmetic (Failure propagates, left priority) ----
function arithPre(a, b) {
  if (isFailure(a)) return a;
  if (isFailure(b)) return b;
  return null;
}
export function valAdd(a, b) {
  const p = arithPre(a, b); if (p) return p;
  if (needsWiden(a, b)) return valAdd(widen(a), widen(b));
  if (a.t === 'int' && b.t === 'int') return checkSafeInt(a.v + b.v);
  if (a.t === 'float' && b.t === 'float') return checkFiniteFloat(a.v + b.v);
  if (a.t === 'int' && b.t === 'float') return checkFiniteFloat(a.v + b.v);
  if (a.t === 'float' && b.t === 'int') return checkFiniteFloat(a.v + b.v);
  if (a.t === 'duration' && b.t === 'duration') return vDuration(checkFiniteFloat(a.v + b.v).v);
  if (a.t === 'str' && b.t === 'str') return vStr(a.v + b.v);
  if (a.t === 'list' && b.t === 'list') return vList([...a.items, ...b.items]);
  return typeMismatch('Int, Float, String, List, or Duration', `${typeName(a)} and ${typeName(b)}`);
}
export function valSub(a, b) {
  const p = arithPre(a, b); if (p) return p;
  if (needsWiden(a, b)) return valSub(widen(a), widen(b));
  if (a.t === 'int' && b.t === 'int') return checkSafeInt(a.v - b.v);
  if (a.t === 'float' && b.t === 'float') return checkFiniteFloat(a.v - b.v);
  if (a.t === 'int' && b.t === 'float') return checkFiniteFloat(a.v - b.v);
  if (a.t === 'float' && b.t === 'int') return checkFiniteFloat(a.v - b.v);
  if (a.t === 'duration' && b.t === 'duration') return vDuration(checkFiniteFloat(a.v - b.v).v);
  return typeMismatch('Int, Float, or Duration', `${typeName(a)} and ${typeName(b)}`);
}
export function valMul(a, b) {
  const p = arithPre(a, b); if (p) return p;
  if (needsWiden(a, b)) return valMul(widen(a), widen(b));
  if (a.t === 'int' && b.t === 'int') return checkSafeInt(a.v * b.v);
  if (a.t === 'float' && b.t === 'float') return checkFiniteFloat(a.v * b.v);
  if (a.t === 'int' && b.t === 'float') return checkFiniteFloat(a.v * b.v);
  if (a.t === 'float' && b.t === 'int') return checkFiniteFloat(a.v * b.v);
  return typeMismatch('Int or Float', `${typeName(a)} and ${typeName(b)}`);
}
export function valDiv(a, b) {
  const p = arithPre(a, b); if (p) return p;
  const x = toF64(widen(a)), y = toF64(widen(b));
  if (Number.isNaN(x) || Number.isNaN(y)) return typeMismatch('Int or Float', `${typeName(a)} and ${typeName(b)}`);
  if (y === 0) fault('AIPO_RT_DIV_ZERO', 'division by zero');
  return checkFiniteFloat(x / y);
}
export function valIntDiv(a, b) {
  const p = arithPre(a, b); if (p) return p;
  const x = widen(a), y = widen(b);
  if (x.t !== 'int' || y.t !== 'int') return typeMismatch('Int', `${typeName(a)} and ${typeName(b)}`);
  if (y.v === 0) fault('AIPO_RT_DIV_ZERO', 'division by zero');
  return checkSafeInt(Math.trunc(x.v / y.v));
}
export function valMod(a, b) {
  const p = arithPre(a, b); if (p) return p;
  const x = widen(a), y = widen(b);
  if (x.t !== 'int' || y.t !== 'int') return typeMismatch('Int', `${typeName(a)} and ${typeName(b)}`);
  if (y.v === 0) fault('AIPO_RT_DIV_ZERO', 'division by zero');
  return checkSafeInt(x.v % y.v);
}
export function valNeg(a) {
  if (isFailure(a)) return a;
  const x = widen(a);
  if (x.t === 'int') return checkSafeInt(-x.v);
  if (x.t === 'float') return checkFiniteFloat(-x.v);
  if (x.t === 'duration') return vDuration(checkFiniteFloat(-x.v).v);
  return typeMismatch('Int, Float, or Duration', typeName(a));
}
export function valPos(a) {
  if (isFailure(a)) return a;
  const x = widen(a);
  if (x.t === 'int' || x.t === 'float') return a;
  return typeMismatch('Int or Float', typeName(a));
}
export function valNot(a) {
  if (isFailure(a)) return a;
  if (a.t !== 'bool') return typeMismatch('Bool', typeName(a));
  return vBool(!a.v);
}

// ---- equality & comparison ----
export function valEqual(a, b) {
  if (isFailure(a)) return a;
  if (isFailure(b)) return b;
  return vBool(valuesEqual(a, b));
}
export function valuesEqual(a, b) {
  if (a.t !== b.t) {
    if ((a.t === 'int' || a.t === 'float' || a.t === 'byte') && (b.t === 'int' || b.t === 'float' || b.t === 'byte')) {
      return toF64(a) === toF64(b);
    }
    return false;
  }
  switch (a.t) {
    case 'none': return true;
    case 'bool': return a.v === b.v;
    case 'int': case 'byte': case 'float': return a.v === b.v;
    case 'str': return a.v === b.v;
    case 'bytes': { if (a.data.length !== b.data.length) return false; for (let i = 0; i < a.data.length; i++) if (a.data[i] !== b.data[i]) return false; return true; }
    case 'type': return a.name === b.name;
    case 'range': return a.start === b.start && a.end === b.end;
    case 'set': { if (a.items.length !== b.items.length) return false; for (let i = 0; i < a.items.length; i++) if (!valuesEqual(a.items[i], b.items[i])) return false; return true; }
    case 'duration': return a.v === b.v;
    case 'task': return a.id === b.id;
    case 'group': return a.id === b.id;
    case 'sequence': return a === b;
    case 'list': { if (a.items.length !== b.items.length) return false; for (let i = 0; i < a.items.length; i++) if (!valuesEqual(a.items[i], b.items[i])) return false; return true; }
    case 'dict': { if (a.entries.length !== b.entries.length) return false; for (let i = 0; i < a.entries.length; i++) if (!valuesEqual(a.entries[i][0], b.entries[i][0]) || !valuesEqual(a.entries[i][1], b.entries[i][1])) return false; return true; }
    case 'struct': { if (a.type !== b.type || a.fields.length !== b.fields.length) return false; for (let i = 0; i < a.fields.length; i++) if (a.fields[i][0] !== b.fields[i][0] || !valuesEqual(a.fields[i][1], b.fields[i][1])) return false; return true; }
    case 'fail': return a.msg === b.msg;
    case 'func': return a.idx === b.idx;
    case 'closure': return a.idx === b.idx && a.cells === b.cells;
    case 'native': return a.name === b.name;
    case 'bound': return a.name === b.name && a.arity === b.arity && valuesEqual(a.recv, b.recv);
    default: return false;
  }
}
function cmpOrder(a, b) {
  const an = (a.t === 'int' || a.t === 'byte' || a.t === 'float') ? toF64(a) : null;
  const bn = (b.t === 'int' || b.t === 'byte' || b.t === 'float') ? toF64(b) : null;
  if (an !== null && bn !== null) return an < bn ? -1 : an > bn ? 1 : 0;
  if (a.t === 'str' && b.t === 'str') return a.v < b.v ? -1 : a.v > b.v ? 1 : 0;
  if (a.t === 'bool' && b.t === 'bool') return a.v === b.v ? 0 : !a.v && b.v ? -1 : 1;
  if (a.t === 'duration' && b.t === 'duration') return a.v < b.v ? -1 : a.v > b.v ? 1 : 0;
  return null;
}
export function valLess(a, b) {
  if (isFailure(a)) return a;
  if (isFailure(b)) return b;
  const c = cmpOrder(widen(a), widen(b));
  if (c === null) return typeMismatch('comparable Int, Float, String, or Duration', `${typeName(a)} and ${typeName(b)}`);
  return vBool(c < 0);
}
export function valLessEqual(a, b) {
  if (isFailure(a)) return a;
  if (isFailure(b)) return b;
  const c = cmpOrder(widen(a), widen(b));
  if (c === null) return typeMismatch('comparable Int, Float, String, or Duration', `${typeName(a)} and ${typeName(b)}`);
  return vBool(c <= 0);
}
export function valGreater(a, b) { return valLess(b, a); }
export function valGreaterEqual(a, b) { return valLessEqual(b, a); }
export function valNotEqual(a, b) {
  const e = valEqual(a, b);
  if (isFailure(e)) return e;
  return vBool(!e.v);
}

// ---- strings: all code-point based; NFC invariant at construction boundaries ----
function chars(s) { return Array.from(s); }
function nfc(s) { return s.normalize('NFC'); }
function reqStr(v, what) {
  if (isFailure(v)) return v;
  if (v.t !== 'str') typeMismatch('String', typeName(v));
  return null;
}
function sLen(s) { return chars(s.v).length; }
function sByteLen(s) { return new TextEncoder().encode(s.v).length; }
function strSlice(text, start, end) {
  const c = chars(text);
  const len = c.length;
  let a = start < 0 ? len + start : start;
  let b = end < 0 ? len + end : end;
  a = Math.max(0, Math.min(len, a));
  b = Math.max(0, Math.min(len, b));
  if (a >= b) return '';
  return c.slice(a, b).join('');
}
export function std_string_len(a) {
  if (isFailure(a)) return a;
  if (a.t !== 'str') return typeMismatch('String', typeName(a));
  return vInt(sLen(a));
}
export function std_string_byte_len(a) {
  if (isFailure(a)) return a;
  if (a.t !== 'str') return typeMismatch('String', typeName(a));
  return vInt(sByteLen(a));
}
export function std_string_contains(t, s) {
  if (isFailure(t)) return t;
  if (isFailure(s)) return s;
  reqStr(t); reqStr(s);
  return vBool(t.v.includes(s.v));
}
export function std_string_starts_with(t, p) {
  if (isFailure(t)) return t;
  if (isFailure(p)) return p;
  reqStr(t); reqStr(p);
  return vBool(t.v.startsWith(p.v));
}
export function std_string_ends_with(t, s) {
  if (isFailure(t)) return t;
  if (isFailure(s)) return s;
  reqStr(t); reqStr(s);
  return vBool(t.v.endsWith(s.v));
}
export function std_string_find(t, sub) {
  if (isFailure(t)) return t;
  if (isFailure(sub)) return sub;
  reqStr(t); reqStr(sub);
  if (sub.v === '') return vInt(0);
  const idx = t.v.indexOf(sub.v);
  if (idx < 0) return vNone();
  return vInt(chars(t.v.slice(0, idx)).length);
}
export function std_string_lower(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  return vStr(nfc(a.v.toLowerCase()));
}
export function std_string_upper(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  return vStr(nfc(a.v.toUpperCase()));
}
export function std_string_capitalize(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  const c = chars(a.v);
  let out = '';
  let done = false;
  for (const ch of c) {
    const up = ch.toUpperCase(), lo = ch.toLowerCase();
    const cased = up !== lo;
    if (!done && cased) { out += up; done = true; }
    else if (done && cased) { out += lo; }
    else out += ch;
  }
  return vStr(nfc(out));
}
export function std_string_reverse(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  let parts;
  if (typeof Intl !== 'undefined' && Intl.Segmenter) {
    const seg = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
    parts = [...seg.segment(a.v)].map(s => s.segment);
  } else {
    parts = chars(a.v);
  }
  return vStr(nfc(parts.reverse().join('')));
}
export function std_string_trim(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  return vStr(a.v.trim());
}
export function std_string_split(t, d) {
  if (isFailure(t)) return t;
  if (isFailure(d)) return d;
  reqStr(t); reqStr(d);
  if (d.v === '') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected non-empty String separator');
  const parts = t.v.split(d.v);
  return vList(parts.map(p => vStr(p)));
}
export function std_string_join(sep, list) {
  if (isFailure(sep)) return sep;
  if (isFailure(list)) return list;
  reqStr(sep);
  if (list.t !== 'list') return typeMismatch('List', typeName(list));
  if (list.items.length === 0) return vStr('');
  for (const el of list.items) {
    if (isFailure(el)) return el;
    if (el.t !== 'str') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected List of String (use String(value) to convert)');
  }
  return vStr(nfc(list.items.map(e => e.v).join(sep.v)));
}
export function std_string_replace(t, from, to) {
  if (isFailure(t)) return t;
  if (isFailure(from)) return from;
  if (isFailure(to)) return to;
  reqStr(t); reqStr(from); reqStr(to);
  if (from.v === '') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected non-empty String `from`');
  return vStr(nfc(t.v.split(from.v).join(to.v)));
}
export function std_string_slice(t, s, e) {
  if (isFailure(t)) return t;
  if (isFailure(s)) return s;
  if (isFailure(e)) return e;
  reqStr(t);
  const a = widen(s), b = widen(e);
  if (a.t !== 'int' || b.t !== 'int') return typeMismatch('Int', 'slice bounds');
  return vStr(strSlice(t.v, a.v, b.v));
}
export function std_string_format(tpl, vals) {
  if (isFailure(tpl)) return tpl;
  if (isFailure(vals)) return vals;
  reqStr(tpl);
  if (vals.t !== 'dict') return typeMismatch('Dict', typeName(vals));
  const src = tpl.v;
  let out = '';
  let i = 0;
  const cps = chars(src);
  const isName = nm => /^[A-Za-z_][A-Za-z0-9_]*$/.test(nm);
  while (i < cps.length) {
    const ch = cps[i];
    if (ch === '{') {
      if (cps[i + 1] === '{') { out += '{'; i += 2; continue; }
      let name = '';
      let closed = false;
      let j = i + 1;
      while (j < cps.length) {
        if (cps[j] === '}') { closed = true; break; }
        if (cps[j] === '{') return vFail("malformed format template: nested '{' is not allowed");
        name += cps[j];
        j++;
      }
      if (!closed) return vFail("malformed format template: unclosed '{'");
      if (!isName(name)) return vFail(`unsupported placeholder {${name}}: only simple names are allowed`);
      const found = dictGet(vals, { t: 'str', v: name });
      if (found === null) return vFail(`missing format value for placeholder {${name}}`);
      if (isFailure(found)) return found;
      out += display(found);
      i = j + 1;
    } else if (ch === '}') {
      if (cps[i + 1] === '}') { out += '}'; i += 2; continue; }
      return vFail("malformed format template: unmatched '}'");
    } else {
      out += ch; i++;
    }
  }
  return vStr(nfc(out));
}

export function std_string_graphemes(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  let parts;
  if (typeof Intl !== 'undefined' && Intl.Segmenter) {
    const seg = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
    parts = [...seg.segment(a.v)].map(s => s.segment);
  } else {
    parts = chars(a.v);
  }
  return vList(parts.map(p => vStr(p)));
}

export function std_string_words(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  let parts;
  if (typeof Intl !== 'undefined' && Intl.Segmenter) {
    const seg = new Intl.Segmenter(undefined, { granularity: 'word' });
    parts = [...seg.segment(a.v)].filter(s => s.isWordLike).map(s => s.segment);
  } else {
    parts = a.v.match(/\w+/g) || [];
  }
  return vList(parts.map(p => vStr(p)));
}

export function std_string_lines(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  const s = a.v;
  if (s.length === 0) return vList([]);
  const parts = s.split(/\r\n|\r|\n/);
  if (parts.length > 0 && parts[parts.length - 1] === '' && (s.endsWith('\n') || s.endsWith('\r'))) {
    parts.pop();
  }
  return vList(parts.map(p => vStr(p)));
}

export function std_string_casefold(a) {
  if (isFailure(a)) return a;
  reqStr(a);
  return vStr(nfc(a.v.toLowerCase()));
}

// ---- math ----
function mathNum(v) {
  if (isFailure(v)) return v;
  const x = widen(v);
  if (x.t === 'int' || x.t === 'float') return x;
  typeMismatch('Int or Float', typeName(v));
}
export function std_math_abs(x) {
  if (isFailure(x)) return x;
  const a = widen(x);
  if (a.t === 'int') {
    if (a.v === MIN_SAFE_INT) fault('AIPO_RT_OVERFLOW', 'abs overflow');
    return checkSafeInt(Math.abs(a.v));
  }
  if (a.t === 'float') return checkFiniteFloat(Math.abs(a.v));
  return typeMismatch('Int or Float', typeName(x));
}
function mathPromote(a, b) {
  const x = widen(a), y = widen(b);
  if ((x.t === 'int' || x.t === 'float') && (y.t === 'int' || y.t === 'float')) {
    if (x.t === 'float' || y.t === 'float') return [toF64(x), toF64(y), true];
    return [x.v, y.v, false];
  }
  return typeMismatch('Int or Float', `${typeName(a)} and ${typeName(b)}`);
}
export function std_math_min(a, b) {
  if (isFailure(a)) return a;
  if (isFailure(b)) return b;
  const [x, y, fl] = mathPromote(a, b);
  if (fl) return checkFiniteFloat(Math.min(x, y));
  return checkSafeInt(Math.min(x, y));
}
export function std_math_max(a, b) {
  if (isFailure(a)) return a;
  if (isFailure(b)) return b;
  const [x, y, fl] = mathPromote(a, b);
  if (fl) return checkFiniteFloat(Math.max(x, y));
  return checkSafeInt(Math.max(x, y));
}
function mathToInt(v, name) {
  const r = toF64(mathNum(v));
  if (!Number.isFinite(r)) fault('AIPO_RT_NON_FINITE_FLOAT', 'non-finite float');
  return checkSafeInt(Math[name](r));
}
export function std_math_floor(x) { if (isFailure(x)) return x; return mathToInt(x, 'floor'); }
export function std_math_ceil(x) { if (isFailure(x)) return x; return mathToInt(x, 'ceil'); }
export function std_math_truncate(x) { if (isFailure(x)) return x; return mathToInt(x, 'trunc'); }
export function std_math_round(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  if (!Number.isFinite(r)) fault('AIPO_RT_NON_FINITE_FLOAT', 'non-finite float');
  const q = r >= 0 ? Math.floor(r + 0.5) : Math.ceil(r - 0.5);
  return checkSafeInt(q);
}
export function std_math_sqrt(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  if (r < 0) return vFail('cannot compute square root of negative number');
  return checkFiniteFloat(Math.sqrt(r));
}
export function std_math_pow(b, e) {
  if (isFailure(b)) return b;
  if (isFailure(e)) return e;
  const bb = widen(b), ee = widen(e);
  if (bb.t === 'int' && ee.t === 'int' && ee.v >= 0) {
    let r = 1;
    for (let i = 0; i < ee.v; i++) {
      r *= bb.v;
      if (!Number.isSafeInteger(r) || r < MIN_SAFE_INT || r > MAX_SAFE_INT) {
        const f = Math.pow(bb.v, ee.v);
        return checkFiniteFloat(f);
      }
    }
    return checkSafeInt(r);
  }
  const f = Math.pow(toF64(bb), toF64(ee));
  return checkFiniteFloat(f);
}
export function std_math_clamp(v, mn, mx) {
  if (isFailure(v)) return v;
  if (isFailure(mn)) return mn;
  if (isFailure(mx)) return mx;
  const a = widen(v), lo = widen(mn), hi = widen(mx);
  const an = toF64(a), ln = toF64(lo), hn = toF64(hi);
  if (Number.isNaN(an) || Number.isNaN(ln) || Number.isNaN(hn)) return typeMismatch('Int or Float', 'clamp bounds');
  const fl = a.t === 'float' || lo.t === 'float' || hi.t === 'float';
  if (ln > hn) return vFail(`math.clamp bounds are inverted: ${display(mn)} > ${display(mx)}`);
  const c = Math.min(Math.max(an, ln), hn);
  return fl ? checkFiniteFloat(c) : checkSafeInt(Math.trunc(c));
}
export function std_math_sin(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  return checkFiniteFloat(Math.sin(r));
}
export function std_math_cos(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  return checkFiniteFloat(Math.cos(r));
}
export function std_math_tan(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  return checkFiniteFloat(Math.tan(r));
}
export function std_math_asin(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  if (r < -1.0 || r > 1.0) return vFail('math.asin domain error: argument must be between -1.0 and 1.0');
  return checkFiniteFloat(Math.asin(r));
}
export function std_math_acos(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  if (r < -1.0 || r > 1.0) return vFail('math.acos domain error: argument must be between -1.0 and 1.0');
  return checkFiniteFloat(Math.acos(r));
}
export function std_math_atan(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  return checkFiniteFloat(Math.atan(r));
}
export function std_math_atan2(y, x) {
  if (isFailure(y)) return y;
  if (isFailure(x)) return x;
  const yn = toF64(mathNum(y));
  const xn = toF64(mathNum(x));
  return checkFiniteFloat(Math.atan2(yn, xn));
}
export function std_math_hypot(x, y) {
  if (isFailure(x)) return x;
  if (isFailure(y)) return y;
  const xn = toF64(mathNum(x));
  const yn = toF64(mathNum(y));
  return checkFiniteFloat(Math.hypot(xn, yn));
}
export function std_math_log(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  if (r <= 0.0) return vFail('math.log domain error: argument must be positive');
  return checkFiniteFloat(Math.log(r));
}
export function std_math_log2(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  if (r <= 0.0) return vFail('math.log2 domain error: argument must be positive');
  return checkFiniteFloat(Math.log2(r));
}
export function std_math_log10(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  if (r <= 0.0) return vFail('math.log10 domain error: argument must be positive');
  return checkFiniteFloat(Math.log10(r));
}
export function std_math_exp(x) {
  if (isFailure(x)) return x;
  const r = toF64(mathNum(x));
  return checkFiniteFloat(Math.exp(r));
}
export function std_math_sign(x) {
  if (isFailure(x)) return x;
  const num = mathNum(x);
  const s = Math.sign(toF64(num));
  return num.t === 'int' ? checkSafeInt(s) : checkFiniteFloat(s);
}
export function std_math_rad(deg) {
  if (isFailure(deg)) return deg;
  const d = toF64(mathNum(deg));
  return checkFiniteFloat(d * (Math.PI / 180.0));
}
export function std_math_deg(rad) {
  if (isFailure(rad)) return rad;
  const r = toF64(mathNum(rad));
  return checkFiniteFloat(r * (180.0 / Math.PI));
}

// ---- prelude & conversions ----
export function std_len(v) {
  if (isFailure(v)) return v;
  if (v.t === 'str') return vInt(chars(v.v).length);
  if (v.t === 'list') return vInt(v.items.length);
  if (v.t === 'dict') return vInt(v.entries.length);
  if (v.t === 'bytes') return vInt(v.data.length);
  if (v.t === 'set') return vInt(v.items.length);
  return typeMismatch('String, List, Dict, Bytes, or Set', typeName(v));
}
export function std_copy(v) {
  if (isFailure(v)) return v;
  if (v.t === 'list') return vList([...v.items]);
  if (v.t === 'dict') return vDict(v.entries.map(([k, x]) => [k, x]));
  if (v.t === 'set') return vSet([...v.items]);
  if (v.t === 'struct') return vStruct(v.type, v.fields.map(([k, x]) => [k, x]), [...v.fixed], v.constructing);
  return v;
}
export function std_same(a, b) {
  if (isFailure(a) || isFailure(b)) return vBool(false);
  if ((a.t === 'list' || a.t === 'dict' || a.t === 'struct' || a.t === 'set') && a === b) return vBool(true);
  if (a.t === 'str' && b.t === 'str') return vBool(a.v === b.v);
  if ((a.t === 'list' || a.t === 'dict' || a.t === 'struct' || a.t === 'set') || (b.t === 'list' || b.t === 'dict' || b.t === 'struct' || b.t === 'set')) return vBool(false);
  return vBool(valuesEqual(a, b));
}
export function std_some(v) { return vBool(!(v.t === 'none')); }
export function std_fail(x) {
  if (isFailure(x)) return x;
  return vFail(display(x));
}
function dq(s) { return JSON.stringify(s); }
function parseIntWhole(s) {
  if (!/^[+-]?\d+$/.test(s)) return null;
  let n;
  try { n = BigInt(s); } catch { return null; }
  if (n < BigInt(Number.MIN_SAFE_INTEGER) - BigInt(1) || n > BigInt(Number.MAX_SAFE_INTEGER) + BigInt(1)) {
    // Outside i64-representable fast path: Rust parse::<i64> fails -> invalid text.
    if (n < BigInt('-9223372036854775808') || n > BigInt('9223372036854775807')) return null;
  }
  return Number(n);
}
export function convInt(v) {
  if (isFailure(v)) return v;
  const x = widen(v);
  if (x.t === 'int') return x;
  if (x.t === 'float') {
    if (!Number.isFinite(x.v)) return vFail('Int cannot represent a non-finite Float');
    const t = Math.trunc(x.v);
    if (t < MIN_SAFE_INT || t > MAX_SAFE_INT) return vFail(`Float ${String(x.v)} is outside the Int range`);
    return { t: 'int', v: t };
  }
  if (x.t === 'str') {
    const p = parseIntWhole(x.v);
    if (p === null || !Number.isSafeInteger(p)) return vFail(`invalid integer text: ${dq(x.v)}`);
    if (p < MIN_SAFE_INT || p > MAX_SAFE_INT) return vFail(`integer text out of range: ${dq(x.v)}`);
    return { t: 'int', v: normInt(p) };
  }
  return typeMismatch('String, Int, Float, or Byte', typeName(v));
}
function parseFloatWhole(s) {
  if (/^[+-]?(inf|infinity|nan)$/i.test(s)) return Number(s);
  if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(s)) return null;
  return Number(s);
}
export function convFloat(v) {
  if (isFailure(v)) return v;
  const x = widen(v);
  if (x.t === 'float') return x;
  if (x.t === 'int') return checkFiniteFloat(x.v);
  if (x.t === 'str') {
    const n = parseFloatWhole(x.v);
    if (n === null || Number.isNaN(n)) return vFail(`invalid float text: ${dq(x.v)}`);
    if (!Number.isFinite(n)) return vFail(`non-finite float text: ${dq(x.v)}`);
    return { t: 'float', v: n };
  }
  return typeMismatch('String, Int, Float, or Byte', typeName(v));
}
export function convByte(v) {
  if (isFailure(v)) return v;
  const x = widen(v);
  let n = null;
  if (x.t === 'byte') return x;
  if (x.t === 'int') n = x.v;
  else if (x.t === 'float') {
    if (!Number.isFinite(x.v) || !Number.isInteger(x.v)) return vFail(`Byte requires an integral value in 0..=255, got ${String(x.v)}`);
    n = x.v;
  } else if (x.t === 'str') {
    const p = parseIntWhole(x.v);
    if (p === null || !Number.isSafeInteger(p)) return vFail(`invalid Byte text: ${dq(x.v)}`);
    n = p;
  } else return typeMismatch('String, Int, Float, or Byte', typeName(v));
  if (n < 0 || n > 255) return vFail(`Byte value ${n} is outside the range 0..=255`);
  return { t: 'byte', v: normInt(n) };
}
export function convString(v) {
  if (isFailure(v)) return v;
  switch (v.t) {
    case 'str': return vStr(v.v);
    case 'int': case 'byte': return vStr(String(v.v));
    case 'float': return vStr(floatText(v.v));
    case 'bool': return vStr(v.v ? 'true' : 'false');
    case 'none': return vStr('none');
    case 'type': return vStr(v.name);
    default: return typeMismatch('String-convertible value', typeName(v));
  }
}
export function convBytes(v) {
  if (isFailure(v)) return v;
  if (v.t !== 'int') return typeMismatch('Int for Bytes(count)', typeName(v));
  if (v.v < 0 || v.v > BYTES_MAX_ALLOCATION) return vFail(`Bytes(${v.v}) is outside the constructible range 0..=${BYTES_MAX_ALLOCATION}`);
  return vBytes(new Uint8Array(v.v));
}
export function convSet(v) {
  if (v === undefined || v === null || (v && v.t === 'none')) return vSet([]);
  if (isFailure(v)) return v;
  if (v.t === 'list' || v.t === 'set') return vSet(v.items);
  return typeMismatch('List or Set', typeName(v));
}
export function convDuration(v) {
  if (isFailure(v)) return v;
  const x = widen(v);
  if (x.t === 'int') return vDuration(x.v);
  if (x.t === 'float') return vDuration(x.v);
  return typeMismatch('Int or Float', typeName(v));
}

// ---- indexing / slicing ----
function rangeLen(r) { return Math.max(0, r.end - r.start); }
function normalizeSlice(len, a, b) {
  let from = a < 0 ? len + a : a;
  let to = b < 0 ? len + b : b;
  from = Math.max(0, Math.min(len, from));
  to = Math.max(0, Math.min(len, to));
  if (from >= to) return null;
  return [from, to];
}
function resolveIndex(len, idx) {
  let i = idx < 0 ? len + idx : idx;
  return i;
}
export function valGetIndex(target, index, activeIterations) {
  if (isFailure(target)) return target;
  if (isFailure(index)) return index;
  if (target.t === 'list') {
    const xi = widen(index);
    if (xi.t === 'range') {
      const r = normalizeSlice(target.items.length, xi.start, xi.end);
      if (!r) return vList([]);
      return vList(target.items.slice(r[0], r[1]));
    }
    if (xi.t !== 'int') return typeMismatch('Int index', typeName(index));
    const i = resolveIndex(target.items.length, xi.v);
    if (i < 0 || i >= target.items.length) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${xi.v} out of range (len ${target.items.length})`);
    return target.items[i];
  }
  if (target.t === 'str') {
    const c = chars(target.v);
    const xi = widen(index);
    if (xi.t === 'range') {
      const r = normalizeSlice(c.length, xi.start, xi.end);
      if (!r) return vStr('');
      return vStr(c.slice(r[0], r[1]).join(''));
    }
    if (xi.t !== 'int') return typeMismatch('Int index', typeName(index));
    const i = resolveIndex(c.length, xi.v);
    if (i < 0 || i >= c.length) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${xi.v} out of range (len ${c.length})`);
    return vStr(c[i]);
  }
  if (target.t === 'bytes') {
    const xi = widen(index);
    if (xi.t === 'range') {
      const r = normalizeSlice(target.data.length, xi.start, xi.end);
      if (!r) return vBytes(new Uint8Array(0));
      return vBytes(target.data.slice(r[0], r[1]));
    }
    if (xi.t !== 'int') return typeMismatch('Int index', typeName(index));
    const i = resolveIndex(target.data.length, xi.v);
    if (i < 0 || i >= target.data.length) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${xi.v} out of range (len ${target.data.length})`);
    return { t: 'byte', v: target.data[i] };
  }
  if (target.t === 'range') {
    const len = rangeLen(target);
    const xi = widen(index);
    if (xi.t !== 'int') return typeMismatch('Int index', typeName(index));
    const i = resolveIndex(len, xi.v);
    if (i < 0 || i >= len) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${xi.v} out of range (len ${len})`);
    return vInt(target.start + i);
  }
  if (target.t === 'dict') {
    const found = dictGet(target, index);
    if (found === null) fault('AIPO_RT_KEY_NOT_FOUND', `key ${display(index)} not found`);
    return found;
  }
  if (target.t === 'sequence') {
    if (!target._cached) target._cached = evaluateSequence(null, target);
    const len = target._cached.length;
    const xi = widen(index);
    if (xi.t !== 'int') return typeMismatch('Int index', typeName(index));
    const i = resolveIndex(len, xi.v);
    if (i < 0 || i >= len) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${xi.v} out of range (len ${len})`);
    return target._cached[i];
  }
  return typeMismatch('indexable collection or string', typeName(target));
}
/**
 * One `each` binding: mode 0 is the dict key or natural element; 1 the key/index; 2 the value.
 *
 * A dict projects key/value at the ordinal position; every other iterable projects
 * its element for modes 0 and 2 and the positional Int for mode 1.
 */
export function iterAt(m, coll, ordinal, mode) {
  if (coll.t === 'sequence') {
    if (!coll._cached) coll._cached = evaluateSequence(m, coll);
    const len = coll._cached.length;
    const i = resolveIndex(len, ordinal);
    if (i < 0 || i >= len) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${ordinal} out of range (len ${len})`);
    return mode === 1 ? vInt(i) : coll._cached[i];
  }
  if (coll.t === 'dict') {
    const len = coll.entries.length;
    const i = resolveIndex(len, ordinal);
    if (i < 0 || i >= len) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${ordinal} out of range (len ${len})`);
    return mode === 2 ? coll.entries[i][1] : coll.entries[i][0];
  }
  if (mode === 1) {
    const lenV = valLen(coll);
    const len = lenV.t === 'int' ? lenV.v : 0;
    const i = resolveIndex(len, ordinal);
    if (i < 0 || i >= len) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${ordinal} out of range (len ${len})`);
    return vInt(i);
  }
  return valGetIndex(coll, vInt(ordinal), m.active);
}
export function valSetIndex(target, index, value) {
  if (isFailure(target)) return target;
  if (isFailure(index)) return index;
  if (isFailure(value)) return value;
  if (target.t === 'list') {
    const xi = widen(index);
    if (xi.t !== 'int') return typeMismatch('Int index', typeName(index));
    const i = resolveIndex(target.items.length, xi.v);
    if (i < 0 || i >= target.items.length) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${xi.v} out of range (len ${target.items.length})`);
    target.items[i] = value;
    return vNone();
  }
  if (target.t === 'dict') {
    dictUpsert(target, index, value);
    return vNone();
  }
  if (target.t === 'bytes') {
    const xi = widen(index);
    if (xi.t !== 'int') return typeMismatch('Int index', typeName(index));
    const i = resolveIndex(target.data.length, xi.v);
    if (i < 0 || i >= target.data.length) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${xi.v} out of range (len ${target.data.length})`);
    const xv = widen(value);
    if (xv.t !== 'int') return typeMismatch('Byte or Int value', typeName(value));
    // Out-of-range ints are a type problem at the boundary, matching the VM's fault
    // instead of silently continuing with a dropped recoverable Failure.
    if (xv.v < 0 || xv.v > 255) return typeMismatch('Byte or Int in 0..=255', String(xv.v));
    target.data[i] = xv.v;
    return vNone();
  }
  return typeMismatch('mutable collection', typeName(target));
}
export function valLen(v) {
  if (isFailure(v)) return v;
  if (v.t === 'str') return vInt(chars(v.v).length);
  if (v.t === 'list') return vInt(v.items.length);
  if (v.t === 'dict') return vInt(v.entries.length);
  if (v.t === 'bytes') return vInt(v.data.length);
  if (v.t === 'range') return vInt(rangeLen(v));
  if (v.t === 'set') return vInt(v.items.length);
  if (v.t === 'sequence') {
    if (!v._cached) v._cached = evaluateSequence(null, v);
    return vInt(v._cached.length);
  }
  return typeMismatch('String, List, Dict, Bytes, Range, or Set', typeName(v));
}

// ---- List / Dict / Set natives (receiver-first) ----
const MUTATING = new Set(['add', 'insert', 'remove', 'remove_at', 'remove_last', 'clear']);
export function checkMutationAllowed(name, recv, active) {
  if (!MUTATING.has(name)) return;
  if (recv && (recv.t === 'list' || recv.t === 'dict' || recv.t === 'set') && active.includes(recv.id)) {
    fault('AIPO_RT_MUTATION_DURING_ITERATION', 'mutation during iteration');
  }
}
function reqList(v) { if (v.t !== 'list') return typeMismatch('List', typeName(v)); }
function reqDict(v) { if (v.t !== 'dict') return typeMismatch('Dict', typeName(v)); }
function intIndex(v) {
  const x = widen(v);
  if (x.t !== 'int') return typeMismatch('Int index', typeName(v));
  return x.v;
}
export const listNatives = {
  add(r, a) { reqList(r); r.items.push(a[0]); return vNone(); },
  insert(r, a) { reqList(r); const i0 = intIndex(a[0]); const idx = i0 < 0 ? r.items.length + i0 : i0; if (idx < 0 || idx > r.items.length) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${i0} out of range (len ${r.items.length})`); r.items.splice(idx, 0, a[1]); return vNone(); },
  remove(r, a) { reqList(r); const at = r.items.findIndex(x => valuesEqual(x, a[0])); if (at >= 0) r.items.splice(at, 1); return vBool(at >= 0); },
  remove_at(r, a) { reqList(r); if (r.items.length === 0) fault('AIPO_RT_INDEX_OUT_OF_RANGE', 'index 0 out of range (len 0)'); const i0 = intIndex(a[0]); const idx = i0 < 0 ? r.items.length + i0 : i0; if (idx < 0 || idx >= r.items.length) fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${i0} out of range (len ${r.items.length})`); return r.items.splice(idx, 1)[0]; },
  remove_last(r) { reqList(r); if (r.items.length === 0) fault('AIPO_RT_INDEX_OUT_OF_RANGE', 'index -1 out of range (len 0)'); return r.items.pop(); },
  clear(r) { reqList(r); r.items.length = 0; return vNone(); },
  contains(r, a) { reqList(r); return vBool(r.items.some(x => valuesEqual(x, a[0]))); },
  find(r, a) { reqList(r); const at = r.items.findIndex(x => valuesEqual(x, a[0])); return at < 0 ? vNone() : vInt(at); },
  count(r, a) { reqList(r); return vInt(r.items.filter(x => valuesEqual(x, a[0])).length); },
  first(r) { reqList(r); if (r.items.length === 0) fault('AIPO_RT_INDEX_OUT_OF_RANGE', 'index 0 out of range (len 0)'); return r.items[0]; },
  first_or(r, a) { reqList(r); return r.items.length > 0 ? r.items[0] : a[0]; },
  last(r) { reqList(r); if (r.items.length === 0) fault('AIPO_RT_INDEX_OUT_OF_RANGE', 'index -1 out of range (len 0)'); return r.items[r.items.length - 1]; },
  last_or(r, a) { reqList(r); return r.items.length > 0 ? r.items[r.items.length - 1] : a[0]; },
  find_index(r, a) { return listNatives.find(r, a); },
  is_empty(r) { reqList(r); return vBool(r.items.length === 0); },
  len(r) { reqList(r); return vInt(r.items.length); },
  reverse(r) { reqList(r); return vList([...r.items].reverse()); },
  sort(r) { reqList(r); const cp = [...r.items]; cp.sort(compareValues); return vList(cp); },
  take(r, a) {
    reqList(r);
    if (isFailure(a[0])) return a[0];
    const n = widen(a[0]);
    if (n.t !== 'int') return typeMismatch('Int', typeName(a[0]));
    const count = Math.max(0, n.v);
    return vList(r.items.slice(0, count));
  },
  skip(r, a) {
    reqList(r);
    if (isFailure(a[0])) return a[0];
    const n = widen(a[0]);
    if (n.t !== 'int') return typeMismatch('Int', typeName(a[0]));
    const count = Math.max(0, n.v);
    return vList(r.items.slice(count));
  },
  distinct(r) {
    reqList(r);
    const unique = [];
    for (const item of r.items) {
      if (!unique.some(seen => valuesEqual(seen, item))) {
        unique.push(item);
      }
    }
    return vList(unique);
  },
  zip(r, a) {
    reqList(r);
    if (isFailure(a[0])) return a[0];
    if (a[0].t !== 'list') return typeMismatch('List', typeName(a[0]));
    const other = a[0];
    const len = Math.min(r.items.length, other.items.length);
    const paired = [];
    for (let i = 0; i < len; i++) {
      paired.push(vList([r.items[i], other.items[i]]));
    }
    return vList(paired);
  },
  chain(r, a) {
    reqList(r);
    if (isFailure(a[0])) return a[0];
    if (a[0].t !== 'list') return typeMismatch('List', typeName(a[0]));
    return vList(r.items.concat(a[0].items));
  },
  chunk(r, a) {
    reqList(r);
    if (isFailure(a[0])) return a[0];
    const n = widen(a[0]);
    if (n.t !== 'int') return typeMismatch('Int', typeName(a[0]));
    if (n.v <= 0) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: positive Int for list.chunk size, got ${n.v}`);
    const size = n.v;
    const chunks = [];
    for (let i = 0; i < r.items.length; i += size) {
      chunks.push(vList(r.items.slice(i, i + size)));
    }
    return vList(chunks);
  },
  window(r, a) {
    reqList(r);
    if (isFailure(a[0])) return a[0];
    const n = widen(a[0]);
    if (n.t !== 'int') return typeMismatch('Int', typeName(a[0]));
    if (n.v <= 0) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: positive Int for list.window size, got ${n.v}`);
    const size = n.v;
    if (size > r.items.length) return vList([]);
    const windows = [];
    for (let i = 0; i <= r.items.length - size; i++) {
      windows.push(vList(r.items.slice(i, i + size)));
    }
    return vList(windows);
  },
  enumerate(r) {
    reqList(r);
    const indexed = [];
    for (let i = 0; i < r.items.length; i++) {
      indexed.push(vList([vInt(i), r.items[i]]));
    }
    return vList(indexed);
  },
  lazy(r) { reqList(r); return vSequence({ source: { type: 'list', items: [...r.items] }, ops: [] }); },
};
export const dictNatives = {
  has(r, a) { reqDict(r); return vBool(dictGet(r, a[0]) !== null); },
  get(r, a) { reqDict(r); const f = dictGet(r, a[0]); return f === null ? vNone() : f; },
  keys(r) { reqDict(r); return vList(r.entries.map(([k]) => k)); },
  values(r) { reqDict(r); return vList(r.entries.map(([, v]) => v)); },
  entries(r) { reqDict(r); return vList(r.entries.map(([k, v]) => vList([k, v]))); },
  remove(r, a) { reqDict(r); return vBool(dictRemove(r, a[0])); },
  clear(r) { reqDict(r); dictClear(r); return vNone(); },
  is_empty(r) { reqDict(r); return vBool(r.entries.length === 0); },
  len(r) { reqDict(r); return vInt(r.entries.length); },
  lazy(r) { reqDict(r); return vSequence({ source: { type: 'dict', items: r.entries.map(([, v]) => v) }, ops: [] }); },
};
function reqSet(v) { if (v.t !== 'set') return typeMismatch('Set', typeName(v)); }
export const setNatives = {
  has(r, a) { reqSet(r); return vBool(r.items.some(x => valuesEqual(x, a[0]))); },
  add(r, a) {
    reqSet(r);
    if (!r.items.some(x => valuesEqual(x, a[0]))) {
      r.items.push(a[0]);
    }
    return vNone();
  },
  remove(r, a) {
    reqSet(r);
    const at = r.items.findIndex(x => valuesEqual(x, a[0]));
    if (at >= 0) r.items.splice(at, 1);
    return vBool(at >= 0);
  },
  clear(r) { reqSet(r); r.items.length = 0; return vNone(); },
  is_empty(r) { reqSet(r); return vBool(r.items.length === 0); },
  len(r) { reqSet(r); return vInt(r.items.length); },
  to_list(r) { reqSet(r); return vList([...r.items]); },
  lazy(r) { reqSet(r); return vSequence({ source: { type: 'set', items: [...r.items] }, ops: [] }); },
};
function reqBytes(v) { if (v.t !== 'bytes') return typeMismatch('Bytes', typeName(v)); }
function bytesView(r, offset, size) {
  reqBytes(r);
  const idx = intIndex(offset);
  if (idx < 0 || idx + size > r.data.length) {
    fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index ${idx} out of range (len ${r.data.length})`);
  }
  return { view: new DataView(r.data.buffer, r.data.byteOffset, r.data.byteLength), idx };
}
export const bytesNatives = {
  read_i8(r, a) { const { view, idx } = bytesView(r, a[0], 1); return vInt(view.getInt8(idx)); },
  read_u8(r, a) { const { view, idx } = bytesView(r, a[0], 1); return vInt(view.getUint8(idx)); },
  read_i16(r, a) { const { view, idx } = bytesView(r, a[0], 2); return vInt(view.getInt16(idx, true)); },
  read_u16(r, a) { const { view, idx } = bytesView(r, a[0], 2); return vInt(view.getUint16(idx, true)); },
  read_i32(r, a) { const { view, idx } = bytesView(r, a[0], 4); return vInt(view.getInt32(idx, true)); },
  read_u32(r, a) { const { view, idx } = bytesView(r, a[0], 4); return vInt(view.getUint32(idx, true)); },
  read_i64(r, a) {
    const { view, idx } = bytesView(r, a[0], 8);
    const bi = view.getBigInt64(idx, true);
    if (bi < BigInt(MIN_SAFE_INT) || bi > BigInt(MAX_SAFE_INT)) {
      return vFail(`integer ${bi} outside safe range`);
    }
    return vInt(Number(bi));
  },
  read_u64(r, a) {
    const { view, idx } = bytesView(r, a[0], 8);
    const bu = view.getBigUint64(idx, true);
    if (bu > BigInt(MAX_SAFE_INT)) {
      return vFail(`unsigned integer ${bu} outside safe range`);
    }
    return vInt(Number(bu));
  },
  read_f32(r, a) { const { view, idx } = bytesView(r, a[0], 4); return checkFiniteFloat(view.getFloat32(idx, true)); },
  read_f64(r, a) { const { view, idx } = bytesView(r, a[0], 8); return checkFiniteFloat(view.getFloat64(idx, true)); },
  write_i8(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < -128 || val.v > 127) return vFail(`value ${val.v} out of range for i8`);
    const { view, idx } = bytesView(r, a[0], 1);
    view.setInt8(idx, val.v);
    return vNone();
  },
  write_u8(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < 0 || val.v > 255) return vFail(`value ${val.v} out of range for u8`);
    const { view, idx } = bytesView(r, a[0], 1);
    view.setUint8(idx, val.v);
    return vNone();
  },
  write_i16(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < -32768 || val.v > 32767) return vFail(`value ${val.v} out of range for i16`);
    const { view, idx } = bytesView(r, a[0], 2);
    view.setInt16(idx, val.v, true);
    return vNone();
  },
  write_u16(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < 0 || val.v > 65535) return vFail(`value ${val.v} out of range for u16`);
    const { view, idx } = bytesView(r, a[0], 2);
    view.setUint16(idx, val.v, true);
    return vNone();
  },
  write_i32(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < -2147483648 || val.v > 2147483647) return vFail(`value ${val.v} out of range for i32`);
    const { view, idx } = bytesView(r, a[0], 4);
    view.setInt32(idx, val.v, true);
    return vNone();
  },
  write_u32(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < 0 || val.v > 4294967295) return vFail(`value ${val.v} out of range for u32`);
    const { view, idx } = bytesView(r, a[0], 4);
    view.setUint32(idx, val.v, true);
    return vNone();
  },
  write_i64(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    const { view, idx } = bytesView(r, a[0], 8);
    view.setBigInt64(idx, BigInt(val.v), true);
    return vNone();
  },
  write_u64(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < 0) return vFail(`value ${val.v} out of range for u64`);
    const { view, idx } = bytesView(r, a[0], 8);
    view.setBigUint64(idx, BigInt(val.v), true);
    return vNone();
  },
  write_f32(r, a) {
    const f = toF64(widen(a[1]));
    if (Number.isNaN(f) || !Number.isFinite(f)) return typeMismatch('Float or Int', typeName(a[1]));
    const { view, idx } = bytesView(r, a[0], 4);
    view.setFloat32(idx, f, true);
    return vNone();
  },
  write_f64(r, a) {
    const f = toF64(widen(a[1]));
    if (Number.isNaN(f) || !Number.isFinite(f)) return typeMismatch('Float or Int', typeName(a[1]));
    const { view, idx } = bytesView(r, a[0], 8);
    view.setFloat64(idx, f, true);
    return vNone();
  },
  read_i16_le(r, a) { return bytesNatives.read_i16(r, a); },
  read_i16_be(r, a) { const { view, idx } = bytesView(r, a[0], 2); return vInt(view.getInt16(idx, false)); },
  read_u16_le(r, a) { return bytesNatives.read_u16(r, a); },
  read_u16_be(r, a) { const { view, idx } = bytesView(r, a[0], 2); return vInt(view.getUint16(idx, false)); },
  read_i32_le(r, a) { return bytesNatives.read_i32(r, a); },
  read_i32_be(r, a) { const { view, idx } = bytesView(r, a[0], 4); return vInt(view.getInt32(idx, false)); },
  read_u32_le(r, a) { return bytesNatives.read_u32(r, a); },
  read_u32_be(r, a) { const { view, idx } = bytesView(r, a[0], 4); return vInt(view.getUint32(idx, false)); },
  read_i64_le(r, a) { return bytesNatives.read_i64(r, a); },
  read_i64_be(r, a) {
    const { view, idx } = bytesView(r, a[0], 8);
    const bi = view.getBigInt64(idx, false);
    if (bi < BigInt(MIN_SAFE_INT) || bi > BigInt(MAX_SAFE_INT)) {
      return vFail(`integer ${bi} outside safe range`);
    }
    return vInt(Number(bi));
  },
  read_u64_le(r, a) { return bytesNatives.read_u64(r, a); },
  read_u64_be(r, a) {
    const { view, idx } = bytesView(r, a[0], 8);
    const bu = view.getBigUint64(idx, false);
    if (bu > BigInt(MAX_SAFE_INT)) {
      return vFail(`unsigned integer ${bu} outside safe range`);
    }
    return vInt(Number(bu));
  },
  read_f32_le(r, a) { return bytesNatives.read_f32(r, a); },
  read_f32_be(r, a) { const { view, idx } = bytesView(r, a[0], 4); return checkFiniteFloat(view.getFloat32(idx, false)); },
  read_f64_le(r, a) { return bytesNatives.read_f64(r, a); },
  read_f64_be(r, a) { const { view, idx } = bytesView(r, a[0], 8); return checkFiniteFloat(view.getFloat64(idx, false)); },

  write_i16_le(r, a) { return bytesNatives.write_i16(r, a); },
  write_i16_be(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < -32768 || val.v > 32767) return vFail(`value ${val.v} out of range for i16`);
    const { view, idx } = bytesView(r, a[0], 2);
    view.setInt16(idx, val.v, false);
    return vNone();
  },
  write_u16_le(r, a) { return bytesNatives.write_u16(r, a); },
  write_u16_be(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < 0 || val.v > 65535) return vFail(`value ${val.v} out of range for u16`);
    const { view, idx } = bytesView(r, a[0], 2);
    view.setUint16(idx, val.v, false);
    return vNone();
  },
  write_i32_le(r, a) { return bytesNatives.write_i32(r, a); },
  write_i32_be(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < -2147483648 || val.v > 2147483647) return vFail(`value ${val.v} out of range for i32`);
    const { view, idx } = bytesView(r, a[0], 4);
    view.setInt32(idx, val.v, false);
    return vNone();
  },
  write_u32_le(r, a) { return bytesNatives.write_u32(r, a); },
  write_u32_be(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < 0 || val.v > 4294967295) return vFail(`value ${val.v} out of range for u32`);
    const { view, idx } = bytesView(r, a[0], 4);
    view.setUint32(idx, val.v, false);
    return vNone();
  },
  write_i64_le(r, a) { return bytesNatives.write_i64(r, a); },
  write_i64_be(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    const { view, idx } = bytesView(r, a[0], 8);
    view.setBigInt64(idx, BigInt(val.v), false);
    return vNone();
  },
  write_u64_le(r, a) { return bytesNatives.write_u64(r, a); },
  write_u64_be(r, a) {
    const val = widen(a[1]);
    if (val.t !== 'int') return typeMismatch('Int', typeName(a[1]));
    if (val.v < 0) return vFail(`value ${val.v} must be non-negative for u64`);
    const { view, idx } = bytesView(r, a[0], 8);
    view.setBigUint64(idx, BigInt(val.v), false);
    return vNone();
  },
  write_f32_le(r, a) { return bytesNatives.write_f32(r, a); },
  write_f32_be(r, a) {
    const f = toF64(widen(a[1]));
    if (Number.isNaN(f) || !Number.isFinite(f)) return typeMismatch('Float or Int', typeName(a[1]));
    const { view, idx } = bytesView(r, a[0], 4);
    view.setFloat32(idx, f, false);
    return vNone();
  },
  write_f64_le(r, a) { return bytesNatives.write_f64(r, a); },
  write_f64_be(r, a) {
    const f = toF64(widen(a[1]));
    if (Number.isNaN(f) || !Number.isFinite(f)) return typeMismatch('Float or Int', typeName(a[1]));
    const { view, idx } = bytesView(r, a[0], 8);
    view.setFloat64(idx, f, false);
    return vNone();
  },
  slice(r, a) {
    const s = widen(a[0]);
    const e = widen(a[1]);
    if (s.t !== 'int' || e.t !== 'int') return typeMismatch('Int', s.t !== 'int' ? typeName(a[0]) : typeName(a[1]));
    const len = r.data.byteLength;
    const start = s.v < 0 ? Math.max(0, len + s.v) : Math.min(len, s.v);
    const end = e.v < 0 ? Math.max(0, len + e.v) : Math.min(len, e.v);
    if (start >= end) return vBytes(new Uint8Array(0));
    return vBytes(r.data.slice(start, end));
  },
  decode(r) {
    reqBytes(r);
    try {
      const s = new TextDecoder('utf-8', { fatal: true }).decode(r.data);
      return vStr(s);
    } catch {
      return vFail('invalid UTF-8 bytes');
    }
  },
};
function numKey(v) {
  if (v.t === 'int' || v.t === 'byte') return v.v;
  if (v.t === 'float') return v.v;
  return null;
}
function compareValues(a, b) {
  const an = numKey(a), bn = numKey(b);
  if (an !== null && bn !== null) return an - bn;
  if (a.t === 'str' && b.t === 'str') return a.v < b.v ? -1 : a.v > b.v ? 1 : 0;
  if (a.t === 'bool' && b.t === 'bool') return a.v === b.v ? 0 : !a.v ? -1 : 1;
  throw new AipoFault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected comparable sort values');
}

// ---- structs ----
export function structGetField(obj, field) {
  if (isFailure(obj)) {
    if (field === 'message') return vStr(obj.msg);
    if (field === 'payload') return obj.payload !== undefined ? obj.payload : vNone();
    return obj;
  }
  if (obj.t !== 'struct') return typeMismatch('struct instance', typeName(obj));
  const f = obj.fields.find(([k]) => k === field);
  if (!f) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${obj.type} has no field '${field}'`);
  return f[1];
}
export function structSetField(obj, field, value, journal) {
  if (isFailure(obj)) return obj;
  if (isFailure(value)) return value;
  if (obj.t !== 'struct') return typeMismatch('struct instance', typeName(obj));
  const at = obj.fields.findIndex(([k]) => k === field);
  if (at < 0) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${obj.type} has no field '${field}'`);
  if (!obj.constructing && obj.fixed.has(field)) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: fixed field '${obj.type}.${field}' cannot be reassigned`);
  if (journal && !obj.constructing && hasInvariant(obj.type)) {
    const seen = journal.some(e => e.inst === obj && e.field === field);
    if (!seen) journal.push({ inst: obj, field, prev: obj.fields[at][1] });
  }
  obj.fields[at][1] = value;
  return vNone();
}
const structDefs = new Map();
export function registerStructDef(name, fields) { structDefs.set(name, fields); }
export function getStructDef(name) { return structDefs.get(name); }
function hasInvariant(type) {
  return typeof __invariantEntries !== 'undefined' && __invariantEntries && __invariantEntries.has(type);
}
// __invariantEntries is installed per run (see interpreter part 4).
export let __invariantEntries = null;
export function setInvariantEntries(m) { __invariantEntries = m; }

// ---- machine ----
function constToValue(c) {
  if (typeof c === 'string') {
    if (c === 'None') return vNone();
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: bad constant ${c}`);
  }
  if (c && typeof c === 'object') {
    if ('None' in c) return vNone();
    if ('Bool' in c) return vBool(c.Bool);
    if ('Int' in c) return checkSafeInt(c.Int);
    if ('Float' in c) return checkFiniteFloat(c.Float);
    if ('String' in c) return vStr(c.String);
  }
  fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: bad constant');
}

function prepareFunction(fn) {
  fn.params = fn.params || [];
  fn.locals = fn.locals || [];
  fn.upvalues = fn.upvalues || [];
  fn.localNames = [...fn.params, ...fn.locals];
  fn.localSet = new Set(fn.localNames);
  fn.localIndex = new Map(fn.localNames.map((name, index) => [name, index]));
  fn.upvalueIndex = new Map(fn.upvalues.map((name, index) => [name, index]));
  for (const inst of fn.code) {
    if (inst.op === 'Constant') {
      inst.const = undefined;
    }
    if (inst.op === 'Load' || inst.op === 'Store') {
      const localSlot = fn.localIndex.get(inst.name);
      if (localSlot !== undefined) {
        inst.local = true;
        inst.slot = localSlot;
      } else {
        const upvalueSlot = fn.upvalueIndex.get(inst.name);
        if (upvalueSlot !== undefined) {
          inst.upvalue = true;
          inst.slot = upvalueSlot;
        }
      }
    } else if (inst.op === 'GetUpvalue' || inst.op === 'SetUpvalue') {
      const slot = fn.upvalueIndex.get(inst.name);
      if (slot !== undefined) {
        inst.upvalue = true;
        inst.slot = slot;
      }
    }
  }
}

function makeMachine(module) {
  const funcIndex = new Map();
  for (const fn of module.functions) prepareFunction(fn);
  prepareFunction(module.top);
  module.functions.forEach((f, i) => funcIndex.set(f.name, i));
  const invEntries = new Map();
  module.functions.forEach((f, i) => {
    const dot = f.name.lastIndexOf('.');
    if (dot > 0 && f.name.slice(dot + 1) === 'invariant') invEntries.set(f.name.slice(0, dot), i);
  });
  setInvariantEntries(invEntries);
  (module.structs || []).forEach(s => registerStructDef(s.name, s.fields.map(([n, fx]) => [n, !!fx])));
  const structMethods = new Map();
  module.functions.forEach((f, i) => {
    const dot = f.name.indexOf('.');
    if (dot > 0) {
      const tn = f.name.slice(0, dot), m = f.name.slice(dot + 1);
      if (m !== 'invariant') structMethods.set(`${tn}.${m}`, { idx: i, total: f.params.length });
    }
  });
  return {
    module, funcIndex, structMethods,
    stack: [], frames: [], handlers: [], journal: [], active: [],
    globals: makeGlobals(), halted: null, done: false, result: null,
    tasks: new Map(),
    runQueue: [],
    waiters: new Map(),
    joins: new Map(),
    groups: new Map(),
    current: 0,
    nextTask: 1,
    nextJoin: 1,
    nextGroup: 1,
    tick: 0,
    mainOutcome: null,
    invokeDepth: 0,
  };
}

// ---- time module: the clock is a host capability, not a language primitive ----
// With no clock installed, `time.now`/`time.monotonic` fault with AIPO_RT_CAPABILITY_DENIED
// rather than returning a faked or defaulted reading, which is the same observable behaviour
// the Rust VM has. The emitted entry installs the system clock — the CLI profile grants the
// same capability for `aipo run` — and a host that needs a deterministic run installs its own
// source on `globalThis.__aipoClock` first (a preload module is enough), so a replay reads a
// fixed clock on both backends.
function hostClock() {
  const clock = globalThis.__aipoClock;
  return clock && typeof clock === 'object' ? clock : null;
}

/** Installs the system clock unless the host already decided.
 *
 * The host's decision is whatever it put on `globalThis.__aipoClock` before this entry
 * evaluated: an object supplies the readings, `null` denies the capability. `undefined` means
 * no decision was made, so the entry installs the system clock. */
export function installDefaultClock() {
  if (globalThis.__aipoClock !== undefined) return;
  const origin = Date.now();
  globalThis.__aipoClock = {
    wallSeconds: () => Date.now() / 1000,
    monotonicSeconds: () => (Date.now() - origin) / 1000,
  };
}

/** Denies the `clock` capability, removing any installed source. */
export function revokeClock() {
  globalThis.__aipoClock = null;
}

function clockReading(capability, operation, read) {
  const clock = hostClock();
  if (!clock) {
    fault('AIPO_RT_CAPABILITY_DENIED',
      `${operation} requires the \`${capability}\` capability, which this host did not grant`);
  }
  return vDuration(read(clock));
}

function makeGlobals() {
  const g = new Map();
  g.set('none', vNone());
  g.set('true', vBool(true));
  g.set('false', vBool(false));
  const nat = (name, arity, fn) => ({ t: 'native', name, arity, fn });
  g.set('len', nat('len', 1, a => std_len(a[0])));
  g.set('copy', nat('copy', 1, a => std_copy(a[0])));
  g.set('same', nat('same', 2, a => std_same(a[0], a[1])));
  g.set('some', nat('some', 1, a => std_some(a[0])));
  g.set('fail', nat('fail', 1, a => std_fail(a[0])));
  g.set('Int', vType('Int'));
  g.set('Float', vType('Float'));
  g.set('Byte', vType('Byte'));
  g.set('String', vType('String'));
  g.set('Bool', vType('Bool'));
  g.set('List', vType('List'));
  g.set('Dict', vType('Dict'));
  g.set('Bytes', vType('Bytes'));
  g.set('Set', vType('Set'));
  g.set('Duration', vType('Duration'));
  g.set('Group', vType('Group'));
  // task module
  const taskEntries = [
    ['spawn', nat('task.spawn', 2, () => vNone())],
    ['sleep', nat('task.sleep', 1, () => vNone())],
    ['all', nat('task.all', 1, () => vNone())],
    ['race', nat('task.race', 1, () => vNone())],
    ['timeout', nat('task.timeout', 2, () => vNone())],
    ['cancel', nat('task.cancel', 1, () => vNone())],
    ['group', nat('task.group', 0, () => vNone())],
  ];
  g.set('task', vDict(taskEntries.map(([k, v]) => [vStr(k), v])));
  // math module
  const mathEntries = [
    ['abs', nat('math.abs', 1, a => std_math_abs(a[0]))],
    ['min', nat('math.min', 2, a => std_math_min(a[0], a[1]))],
    ['max', nat('math.max', 2, a => std_math_max(a[0], a[1]))],
    ['floor', nat('math.floor', 1, a => std_math_floor(a[0]))],
    ['ceil', nat('math.ceil', 1, a => std_math_ceil(a[0]))],
    ['round', nat('math.round', 1, a => std_math_round(a[0]))],
    ['truncate', nat('math.truncate', 1, a => std_math_truncate(a[0]))],
    ['sqrt', nat('math.sqrt', 1, a => std_math_sqrt(a[0]))],
    ['pow', nat('math.pow', 2, a => std_math_pow(a[0], a[1]))],
    ['clamp', nat('math.clamp', 3, a => std_math_clamp(a[0], a[1], a[2]))],
    ['sin', nat('math.sin', 1, a => std_math_sin(a[0]))],
    ['cos', nat('math.cos', 1, a => std_math_cos(a[0]))],
    ['tan', nat('math.tan', 1, a => std_math_tan(a[0]))],
    ['asin', nat('math.asin', 1, a => std_math_asin(a[0]))],
    ['acos', nat('math.acos', 1, a => std_math_acos(a[0]))],
    ['atan', nat('math.atan', 1, a => std_math_atan(a[0]))],
    ['atan2', nat('math.atan2', 2, a => std_math_atan2(a[0], a[1]))],
    ['hypot', nat('math.hypot', 2, a => std_math_hypot(a[0], a[1]))],
    ['log', nat('math.log', 1, a => std_math_log(a[0]))],
    ['log2', nat('math.log2', 1, a => std_math_log2(a[0]))],
    ['log10', nat('math.log10', 1, a => std_math_log10(a[0]))],
    ['exp', nat('math.exp', 1, a => std_math_exp(a[0]))],
    ['sign', nat('math.sign', 1, a => std_math_sign(a[0]))],
    ['rad', nat('math.rad', 1, a => std_math_rad(a[0]))],
    ['deg', nat('math.deg', 1, a => std_math_deg(a[0]))],
    ['pi', checkFiniteFloat(Math.PI)],
    ['e', checkFiniteFloat(Math.E)],
    ['tau', checkFiniteFloat(2 * Math.PI)],
  ];
  g.set('math', vDict(mathEntries.map(([k, v]) => [vStr(k), v])));
  // string module
  const sm = (n, arity, fn) => [vStr(n), nat(`string.${n}`, arity, fn)];
  g.set('string', vDict([
    sm('len', 1, a => std_string_len(a[0])),
    sm('byte_len', 1, a => std_string_byte_len(a[0])),
    sm('contains', 2, a => std_string_contains(a[0], a[1])),
    sm('starts_with', 2, a => std_string_starts_with(a[0], a[1])),
    sm('ends_with', 2, a => std_string_ends_with(a[0], a[1])),
    sm('find', 2, a => std_string_find(a[0], a[1])),
    sm('lower', 1, a => std_string_lower(a[0])),
    sm('upper', 1, a => std_string_upper(a[0])),
    sm('capitalize', 1, a => std_string_capitalize(a[0])),
    sm('reverse', 1, a => std_string_reverse(a[0])),
    sm('trim', 1, a => std_string_trim(a[0])),
    sm('split', 2, a => std_string_split(a[0], a[1])),
    sm('join', 2, a => std_string_join(a[0], a[1])),
    sm('replace', 3, a => std_string_replace(a[0], a[1], a[2])),
    sm('slice', 3, a => std_string_slice(a[0], a[1], a[2])),
    sm('format', 2, a => std_string_format(a[0], a[1])),
    sm('graphemes', 1, a => std_string_graphemes(a[0])),
    sm('words', 1, a => std_string_words(a[0])),
    sm('lines', 1, a => std_string_lines(a[0])),
    sm('casefold', 1, a => std_string_casefold(a[0])),
  ]));
  // io module
  const ioText = v => (v.t === 'str' ? v.v : display(v));
  g.set('io', vDict([
    [vStr('print'), nat('io.print', 1, a => { emitText(ioText(a[0])); return vNone(); })],
    [vStr('println'), nat('io.println', 1, a => { emitText(ioText(a[0])); emitText('\n'); return vNone(); })],
  ]));
  // time module (capability-gated clock + pure types)
  g.set('time', vDict([
    [vStr('now'), nat('time.now', 0, () => clockReading('clock.wall', 'time.now', c => c.wallSeconds()))],
    [vStr('monotonic'), nat('time.monotonic', 0, () => clockReading('clock.monotonic', 'time.monotonic', c => c.monotonicSeconds()))],
    [vStr('date'), nat('time.date', 3, a => std_time_date(a[0], a[1], a[2]))],
    [vStr('time_of_day'), nat('time.time_of_day', -1, a => std_time_time_of_day(a))],
    [vStr('date_time'), nat('time.date_time', -1, a => std_time_date_time(a))],
    [vStr('parse_date'), nat('time.parse_date', 1, a => std_time_parse_date(a[0]))],
    [vStr('parse_time'), nat('time.parse_time', 1, a => std_time_parse_time(a[0]))],
    [vStr('parse_iso'), nat('time.parse_iso', 1, a => std_time_parse_iso(a[0]))],
    [vStr('parse_datetime'), nat('time.parse_datetime', 1, a => std_time_parse_iso(a[0]))],
    [vStr('duration'), nat('time.duration', 1, a => std_time_duration(a[0]))],
  ]));
  // random module (deterministic PRNG)
  const defaultRng = createRngInstance(0);
  g.set('random', vDict([
    [vStr('create'), nat('random.create', 1, a => createRngInstance(a[0] ? widen(a[0]).v : 0))],
    [vStr('seed'), nat('random.seed', 1, a => {
      const s = widen(a[0]);
      if (s.t !== 'int') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected Int seed');
      const seedEntry = defaultRng.fields.find(([k]) => k === 'seed');
      const stepEntry = defaultRng.fields.find(([k]) => k === 'step');
      if (seedEntry) seedEntry[1] = vInt(s.v);
      if (stepEntry) stepEntry[1] = vInt(0);
      return vNone();
    })],
    [vStr('int'), nat('random.int', 2, a => rngMethodInt(defaultRng, a[0], a[1]))],
    [vStr('float'), nat('random.float', 0, () => rngMethodFloat(defaultRng))],
    [vStr('bool'), nat('random.bool', 0, () => rngMethodBool(defaultRng))],
    [vStr('choice'), nat('random.choice', 1, a => rngMethodChoice(defaultRng, a[0]))],
    [vStr('shuffle'), nat('random.shuffle', 1, a => rngMethodShuffle(defaultRng, a[0]))],
  ]));
  // json module
  g.set('json', vDict([
    [vStr('parse'), nat('json.parse', 1, a => std_json_parse(a[0]))],
    [vStr('stringify'), nat('json.stringify', -1, a => std_json_stringify(a[0], a[1]))],
  ]));
  // encoding module
  g.set('encoding', vDict([
    [vStr('base64_encode'), nat('encoding.base64_encode', 1, a => std_encoding_base64_encode(a[0]))],
    [vStr('base64_decode'), nat('encoding.base64_decode', 1, a => std_encoding_base64_decode(a[0]))],
    [vStr('base64url_encode'), nat('encoding.base64url_encode', 1, a => std_encoding_base64url_encode(a[0]))],
    [vStr('base64url_decode'), nat('encoding.base64url_decode', 1, a => std_encoding_base64url_decode(a[0]))],
    [vStr('hex_encode'), nat('encoding.hex_encode', 1, a => std_encoding_hex_encode(a[0]))],
    [vStr('hex_decode'), nat('encoding.hex_decode', 1, a => std_encoding_hex_decode(a[0]))],
    [vStr('utf8_encode'), nat('encoding.utf8_encode', 1, a => std_encoding_utf8_encode(a[0]))],
    [vStr('utf8_decode'), nat('encoding.utf8_decode', 1, a => std_encoding_utf8_decode(a[0]))],
  ]));
  // binary module
  g.set('binary', vDict([
    [vStr('read_i8'), nat('binary.read_i8', 2, a => bytesNatives.read_i8(a[0], [a[1]]))],
    [vStr('read_u8'), nat('binary.read_u8', 2, a => bytesNatives.read_u8(a[0], [a[1]]))],
    [vStr('read_i16_le'), nat('binary.read_i16_le', 2, a => bytesNatives.read_i16_le(a[0], [a[1]]))],
    [vStr('read_i16_be'), nat('binary.read_i16_be', 2, a => bytesNatives.read_i16_be(a[0], [a[1]]))],
    [vStr('read_u16_le'), nat('binary.read_u16_le', 2, a => bytesNatives.read_u16_le(a[0], [a[1]]))],
    [vStr('read_u16_be'), nat('binary.read_u16_be', 2, a => bytesNatives.read_u16_be(a[0], [a[1]]))],
    [vStr('read_i32_le'), nat('binary.read_i32_le', 2, a => bytesNatives.read_i32_le(a[0], [a[1]]))],
    [vStr('read_i32_be'), nat('binary.read_i32_be', 2, a => bytesNatives.read_i32_be(a[0], [a[1]]))],
    [vStr('read_u32_le'), nat('binary.read_u32_le', 2, a => bytesNatives.read_u32_le(a[0], [a[1]]))],
    [vStr('read_u32_be'), nat('binary.read_u32_be', 2, a => bytesNatives.read_u32_be(a[0], [a[1]]))],
    [vStr('read_i64_le'), nat('binary.read_i64_le', 2, a => bytesNatives.read_i64_le(a[0], [a[1]]))],
    [vStr('read_i64_be'), nat('binary.read_i64_be', 2, a => bytesNatives.read_i64_be(a[0], [a[1]]))],
    [vStr('read_u64_le'), nat('binary.read_u64_le', 2, a => bytesNatives.read_u64_le(a[0], [a[1]]))],
    [vStr('read_u64_be'), nat('binary.read_u64_be', 2, a => bytesNatives.read_u64_be(a[0], [a[1]]))],
    [vStr('read_f32_le'), nat('binary.read_f32_le', 2, a => bytesNatives.read_f32_le(a[0], [a[1]]))],
    [vStr('read_f32_be'), nat('binary.read_f32_be', 2, a => bytesNatives.read_f32_be(a[0], [a[1]]))],
    [vStr('read_f64_le'), nat('binary.read_f64_le', 2, a => bytesNatives.read_f64_le(a[0], [a[1]]))],
    [vStr('read_f64_be'), nat('binary.read_f64_be', 2, a => bytesNatives.read_f64_be(a[0], [a[1]]))],
    [vStr('write_i8'), nat('binary.write_i8', 3, a => bytesNatives.write_i8(a[0], [a[1], a[2]]))],
    [vStr('write_u8'), nat('binary.write_u8', 3, a => bytesNatives.write_u8(a[0], [a[1], a[2]]))],
    [vStr('write_i16_le'), nat('binary.write_i16_le', 3, a => bytesNatives.write_i16_le(a[0], [a[1], a[2]]))],
    [vStr('write_i16_be'), nat('binary.write_i16_be', 3, a => bytesNatives.write_i16_be(a[0], [a[1], a[2]]))],
    [vStr('write_u16_le'), nat('binary.write_u16_le', 3, a => bytesNatives.write_u16_le(a[0], [a[1], a[2]]))],
    [vStr('write_u16_be'), nat('binary.write_u16_be', 3, a => bytesNatives.write_u16_be(a[0], [a[1], a[2]]))],
    [vStr('write_i32_le'), nat('binary.write_i32_le', 3, a => bytesNatives.write_i32_le(a[0], [a[1], a[2]]))],
    [vStr('write_i32_be'), nat('binary.write_i32_be', 3, a => bytesNatives.write_i32_be(a[0], [a[1], a[2]]))],
    [vStr('write_u32_le'), nat('binary.write_u32_le', 3, a => bytesNatives.write_u32_le(a[0], [a[1], a[2]]))],
    [vStr('write_u32_be'), nat('binary.write_u32_be', 3, a => bytesNatives.write_u32_be(a[0], [a[1], a[2]]))],
    [vStr('write_i64_le'), nat('binary.write_i64_le', 3, a => bytesNatives.write_i64_le(a[0], [a[1], a[2]]))],
    [vStr('write_i64_be'), nat('binary.write_i64_be', 3, a => bytesNatives.write_i64_be(a[0], [a[1], a[2]]))],
    [vStr('write_u64_le'), nat('binary.write_u64_le', 3, a => bytesNatives.write_u64_le(a[0], [a[1], a[2]]))],
    [vStr('write_u64_be'), nat('binary.write_u64_be', 3, a => bytesNatives.write_u64_be(a[0], [a[1], a[2]]))],
    [vStr('write_f32_le'), nat('binary.write_f32_le', 3, a => bytesNatives.write_f32_le(a[0], [a[1], a[2]]))],
    [vStr('write_f32_be'), nat('binary.write_f32_be', 3, a => bytesNatives.write_f32_be(a[0], [a[1], a[2]]))],
    [vStr('write_f64_le'), nat('binary.write_f64_le', 3, a => bytesNatives.write_f64_le(a[0], [a[1], a[2]]))],
    [vStr('write_f64_be'), nat('binary.write_f64_be', 3, a => bytesNatives.write_f64_be(a[0], [a[1], a[2]]))],
    [vStr('read_varint'), nat('binary.read_varint', 2, a => std_binary_read_varint(a[0], a[1]))],
    [vStr('write_varint'), nat('binary.write_varint', 3, a => std_binary_write_varint(a[0], a[1], a[2]))],
    [vStr('slice'), nat('binary.slice', 3, a => bytesNatives.slice(a[0], [a[1], a[2]]))],
  ]));
  // path module
  g.set('path', vDict([
    [vStr('join'), nat('path.join', -1, a => std_path_join(a))],
    [vStr('normalize'), nat('path.normalize', 1, a => std_path_normalize(a[0]))],
    [vStr('is_absolute'), nat('path.is_absolute', 1, a => std_path_is_absolute(a[0]))],
    [vStr('basename'), nat('path.basename', -1, a => std_path_basename(a[0], a[1]))],
    [vStr('dirname'), nat('path.dirname', 1, a => std_path_dirname(a[0]))],
    [vStr('ext'), nat('path.ext', 1, a => std_path_ext(a[0]))],
  ]));
  // url module
  g.set('url', vDict([
    [vStr('parse'), nat('url.parse', 1, a => std_url_parse(a[0]))],
  ]));
  // regex module
  g.set('regex', vDict([
    [vStr('compile'), nat('regex.compile', 1, a => std_regex_compile(a[0]))],
    [vStr('is_match'), nat('regex.is_match', 2, a => std_regex_is_match(a[0], a[1]))],
    [vStr('replace'), nat('regex.replace', 3, a => std_regex_replace(a[0], a[1], a[2]))],
  ]));
  // expect and testing modules
  const expectDict = vDict([
    [vStr('equal'), nat('expect.equal', 2, a => std_expect_equal(a[0], a[1]))],
    [vStr('not_equal'), nat('expect.not_equal', 2, a => std_expect_not_equal(a[0], a[1]))],
    [vStr('true'), nat('expect.true', 1, a => std_expect_true(a[0]))],
    [vStr('false'), nat('expect.false', 1, a => std_expect_false(a[0]))],
    [vStr('none'), nat('expect.none', 1, a => std_expect_none(a[0]))],
    [vStr('some'), nat('expect.some', 1, a => std_expect_some(a[0]))],
    [vStr('failure'), nat('expect.failure', 1, a => std_expect_failure(a[0]))],
    [vStr('contains'), nat('expect.contains', 2, a => std_expect_contains(a[0], a[1]))],
    [vStr('approx'), nat('expect.approx', -1, a => std_expect_approx(a))],
  ]);
  g.set('expect', expectDict);
  g.set('test', nat('test', 2, () => vNone()));
  g.set('testing', vDict([
    [vStr('expect'), expectDict],
    [vStr('test'), nat('testing.test', 2, () => vNone())],
    [vStr('equal'), nat('testing.equal', 2, a => std_expect_equal(a[0], a[1]))],
    [vStr('not_equal'), nat('testing.not_equal', 2, a => std_expect_not_equal(a[0], a[1]))],
    [vStr('true'), nat('testing.true', 1, a => std_expect_true(a[0]))],
    [vStr('false'), nat('testing.false', 1, a => std_expect_false(a[0]))],
    [vStr('none'), nat('testing.none', 1, a => std_expect_none(a[0]))],
    [vStr('some'), nat('testing.some', 1, a => std_expect_some(a[0]))],
    [vStr('failure'), nat('testing.failure', 1, a => std_expect_failure(a[0]))],
    [vStr('contains'), nat('testing.contains', 2, a => std_expect_contains(a[0], a[1]))],
    [vStr('approx'), nat('testing.approx', -1, a => std_expect_approx(a))],
  ]));
  // log module
  g.set('log', vDict([
    [vStr('trace'), nat('log.trace', -1, a => std_log_emit('TRACE', a))],
    [vStr('debug'), nat('log.debug', -1, a => std_log_emit('DEBUG', a))],
    [vStr('info'), nat('log.info', -1, a => std_log_emit('INFO', a))],
    [vStr('warning'), nat('log.warning', -1, a => std_log_emit('WARNING', a))],
    [vStr('error'), nat('log.error', -1, a => std_log_emit('ERROR', a))],
  ]));
  return g;
}

export function std_path_is_absolute(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  const p = arg.v;
  if (p.startsWith('/') || p.startsWith('\\')) return vBool(true);
  if (/^[A-Za-z]:[/\\]/.test(p)) return vBool(true);
  return vBool(false);
}

function normalizePathStr(path) {
  if (!path) return '.';
  const isAbs = path.startsWith('/') || path.startsWith('\\');
  const hasTrailing = path.endsWith('/') || path.endsWith('\\');

  let prefix = '';
  let rest = path;
  if (/^[A-Za-z]:/.test(path)) {
    prefix = path.slice(0, 2);
    rest = path.slice(2);
  }

  const rawSegments = rest.split(/[/\\]/).filter(s => s !== '' && s !== '.');
  const stack = [];
  for (const seg of rawSegments) {
    if (seg === '..') {
      if (stack.length > 0 && stack[stack.length - 1] !== '..') {
        stack.pop();
        continue;
      }
      if (!isAbs) {
        stack.push('..');
      }
    } else {
      stack.push(seg);
    }
  }

  let out = '';
  if (prefix) {
    out = prefix + '/';
  } else if (isAbs) {
    out = '/';
  }

  if (stack.length === 0) {
    if (!out) return '.';
  } else {
    out += stack.join('/');
    if (hasTrailing && !out.endsWith('/')) {
      out += '/';
    }
  }

  return out;
}

export function std_path_normalize(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  return vStr(normalizePathStr(arg.v));
}

export function std_path_join(args) {
  const parts = [];
  for (const arg of args) {
    if (isFailure(arg)) return arg;
    if (arg.t === 'list') {
      for (const item of arg.items) {
        if (isFailure(item)) return item;
        if (item.t !== 'str') return typeMismatch('String', typeName(item));
        parts.push(item.v);
      }
    } else if (arg.t === 'str') {
      parts.push(arg.v);
    } else {
      return typeMismatch('String or List of Strings', typeName(arg));
    }
  }

  if (parts.length === 0) return vStr('.');

  let joined = '';
  for (const part of parts) {
    const isAbs = part.startsWith('/') || part.startsWith('\\') || /^[A-Za-z]:[/\\]/.test(part);
    if (isAbs) {
      joined = part;
    } else if (!joined || joined === '.') {
      joined = part;
    } else {
      if (!joined.endsWith('/') && !joined.endsWith('\\')) {
        joined += '/';
      }
      joined += part;
    }
  }

  return vStr(normalizePathStr(joined));
}

export function std_path_basename(arg, extArg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  let ext = '';
  if (extArg !== undefined && extArg.t !== 'none') {
    if (extArg.t !== 'str') return typeMismatch('String', typeName(extArg));
    ext = extArg.v;
  }
  const clean = arg.v.replace(/[/\\]+$/, '');
  const at = Math.max(clean.lastIndexOf('/'), clean.lastIndexOf('\\'));
  const base = at >= 0 ? clean.slice(at + 1) : clean;
  if (ext && base.endsWith(ext) && base.length > ext.length) {
    return vStr(base.slice(0, base.length - ext.length));
  }
  return vStr(base);
}

export function std_path_dirname(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  const p = arg.v;
  const clean = p.replace(/[/\\]+$/, '');
  const at = Math.max(clean.lastIndexOf('/'), clean.lastIndexOf('\\'));
  if (at === 0) return vStr('/');
  if (at > 0) return vStr(clean.slice(0, at));
  const isAbs = p.startsWith('/') || p.startsWith('\\') || /^[A-Za-z]:[/\\]/.test(p);
  return vStr(isAbs ? '/' : '.');
}

export function std_path_ext(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  const clean = arg.v.replace(/[/\\]+$/, '');
  const at = Math.max(clean.lastIndexOf('/'), clean.lastIndexOf('\\'));
  const base = at >= 0 ? clean.slice(at + 1) : clean;
  const dot = base.lastIndexOf('.');
  if (dot > 0) {
    return vStr(base.slice(dot));
  }
  return vStr('');
}

export function std_url_parse(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  try {
    const u = new URL(arg.v);
    return vDict([
      [vStr('href'), vStr(u.href)],
      [vStr('origin'), vStr(u.origin)],
      [vStr('protocol'), vStr(u.protocol)],
      [vStr('username'), vStr(u.username)],
      [vStr('password'), vStr(u.password)],
      [vStr('host'), vStr(u.host)],
      [vStr('hostname'), vStr(u.hostname)],
      [vStr('port'), vStr(u.port)],
      [vStr('pathname'), vStr(u.pathname)],
      [vStr('search'), vStr(u.search)],
      [vStr('hash'), vStr(u.hash)],
    ]);
  } catch (e) {
    return vFail(`invalid URL: ${e.message}`);
  }
}

export function std_regex_compile(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  try {
    new RegExp(arg.v);
    return vStruct('Pattern', [['pattern', arg]]);
  } catch (e) {
    return vFail(`invalid regex pattern: ${e.message}`);
  }
}

export function std_regex_is_match(pat, text) {
  if (isFailure(pat)) return pat;
  if (isFailure(text)) return text;
  if (pat.t !== 'str') return typeMismatch('String', typeName(pat));
  if (text.t !== 'str') return typeMismatch('String', typeName(text));
  try {
    const re = new RegExp(pat.v);
    return vBool(re.test(text.v));
  } catch (e) {
    return vFail(`invalid regex pattern: ${e.message}`);
  }
}

export function std_regex_replace(pat, text, repl) {
  if (isFailure(pat)) return pat;
  if (isFailure(text)) return text;
  if (isFailure(repl)) return repl;
  if (pat.t !== 'str') return typeMismatch('String', typeName(pat));
  if (text.t !== 'str') return typeMismatch('String', typeName(text));
  if (repl.t !== 'str') return typeMismatch('String', typeName(repl));
  try {
    const re = new RegExp(pat.v, 'g');
    return vStr(text.v.replace(re, repl.v));
  } catch (e) {
    return vFail(`invalid regex pattern: ${e.message}`);
  }
}

function getPatternStr(obj) {
  if (isFailure(obj)) return obj;
  if (obj.t !== 'struct' || obj.type !== 'Pattern') return typeMismatch('Pattern struct instance', typeName(obj));
  const f = obj.fields.find(([k]) => k === 'pattern');
  if (!f || f[1].t !== 'str') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: Pattern has no String pattern field');
  return f[1].v;
}

export function patternMethodIsMatch(recv, text) {
  if (isFailure(recv)) return recv;
  if (isFailure(text)) return text;
  if (text.t !== 'str') return typeMismatch('String', typeName(text));
  const pat = getPatternStr(recv);
  if (typeof pat !== 'string') return pat;
  try {
    const re = new RegExp(pat);
    return vBool(re.test(text.v));
  } catch (e) {
    return vFail(`regex error: ${e.message}`);
  }
}

export function patternMethodFind(recv, text) {
  if (isFailure(recv)) return recv;
  if (isFailure(text)) return text;
  if (text.t !== 'str') return typeMismatch('String', typeName(text));
  const pat = getPatternStr(recv);
  if (typeof pat !== 'string') return pat;
  try {
    const m = text.v.match(new RegExp(pat));
    return m ? vStr(m[0]) : vNone();
  } catch (e) {
    return vFail(`regex error: ${e.message}`);
  }
}

export function patternMethodFindAll(recv, text) {
  if (isFailure(recv)) return recv;
  if (isFailure(text)) return text;
  if (text.t !== 'str') return typeMismatch('String', typeName(text));
  const pat = getPatternStr(recv);
  if (typeof pat !== 'string') return pat;
  try {
    const m = [...text.v.matchAll(new RegExp(pat, 'g'))];
    return vList(m.map(x => vStr(x[0])));
  } catch (e) {
    return vFail(`regex error: ${e.message}`);
  }
}

export function patternMethodReplace(recv, text, repl) {
  if (isFailure(recv)) return recv;
  if (isFailure(text)) return text;
  if (isFailure(repl)) return repl;
  if (text.t !== 'str') return typeMismatch('String', typeName(text));
  if (repl.t !== 'str') return typeMismatch('String', typeName(repl));
  const pat = getPatternStr(recv);
  if (typeof pat !== 'string') return pat;
  try {
    const res = text.v.replace(new RegExp(pat, 'g'), repl.v);
    return vStr(res);
  } catch (e) {
    return vFail(`regex error: ${e.message}`);
  }
}

export function patternMethodSplit(recv, text) {
  if (isFailure(recv)) return recv;
  if (isFailure(text)) return text;
  if (text.t !== 'str') return typeMismatch('String', typeName(text));
  const pat = getPatternStr(recv);
  if (typeof pat !== 'string') return pat;
  try {
    const parts = text.v.split(new RegExp(pat));
    return vList(parts.map(vStr));
  } catch (e) {
    return vFail(`regex error: ${e.message}`);
  }
}

function extractDataBytes(arg, op) {
  if (isFailure(arg)) return arg;
  if (arg.t === 'bytes') return arg.data;
  if (arg.t === 'str') return new TextEncoder().encode(arg.v);
  return typeMismatch('Bytes or String', typeName(arg));
}

export function std_encoding_base64_encode(arg) {
  const bytes = extractDataBytes(arg, 'encoding.base64_encode');
  if (isFailure(bytes)) return bytes;
  if (typeof Buffer !== 'undefined') {
    return vStr(Buffer.from(bytes).toString('base64'));
  }
  let binary = '';
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
  return vStr(btoa(binary));
}

export function std_encoding_base64_decode(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  const text = arg.v.trim();
  if (text === '') return vBytes(new Uint8Array(0));
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(text) || text.length % 4 !== 0) {
    return vFail('invalid base64 string');
  }
  try {
    if (typeof Buffer !== 'undefined') {
      return vBytes(new Uint8Array(Buffer.from(text, 'base64')));
    }
    const bin = atob(text);
    const u8 = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
    return vBytes(u8);
  } catch (e) {
    return vFail(e.message);
  }
}

export function std_encoding_base64url_encode(arg) {
  const bytes = extractDataBytes(arg, 'encoding.base64url_encode');
  if (isFailure(bytes)) return bytes;
  let b64;
  if (typeof Buffer !== 'undefined') {
    b64 = Buffer.from(bytes).toString('base64');
  } else {
    let binary = '';
    for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
    b64 = btoa(binary);
  }
  return vStr(b64.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''));
}

export function std_encoding_base64url_decode(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  let text = arg.v.trim().replace(/-/g, '+').replace(/_/g, '/');
  while (text.length % 4 !== 0) {
    text += '=';
  }
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(text)) {
    return vFail('invalid base64url string');
  }
  try {
    if (typeof Buffer !== 'undefined') {
      return vBytes(new Uint8Array(Buffer.from(text, 'base64')));
    }
    const bin = atob(text);
    const u8 = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
    return vBytes(u8);
  } catch (e) {
    return vFail(e.message);
  }
}

export function std_encoding_hex_encode(arg) {
  const bytes = extractDataBytes(arg, 'encoding.hex_encode');
  if (isFailure(bytes)) return bytes;
  let out = '';
  for (let i = 0; i < bytes.length; i++) {
    out += bytes[i].toString(16).padStart(2, '0');
  }
  return vStr(out);
}

export function std_encoding_hex_decode(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  const text = arg.v.trim();
  if (text.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(text)) {
    return vFail('invalid hex string');
  }
  const u8 = new Uint8Array(text.length / 2);
  for (let i = 0; i < text.length; i += 2) {
    u8[i / 2] = parseInt(text.slice(i, i + 2), 16);
  }
  return vBytes(u8);
}

export function std_encoding_utf8_encode(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'str') return typeMismatch('String', typeName(arg));
  return vBytes(new TextEncoder().encode(arg.v));
}

export function std_encoding_utf8_decode(arg) {
  if (isFailure(arg)) return arg;
  if (arg.t !== 'bytes') return typeMismatch('Bytes', typeName(arg));
  try {
    const text = new TextDecoder('utf-8', { fatal: true }).decode(arg.data);
    return vStr(text.normalize('NFC'));
  } catch (_) {
    return vFail('bytes are not valid UTF-8');
  }
}

export function std_json_parse(textVal) {
  if (isFailure(textVal)) return textVal;
  if (textVal.t !== 'str') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected String text, got ${typeName(textVal)}`);
  try {
    return parseJsonStrict(textVal.v);
  } catch (e) {
    return vFail(`json.parse error: ${e.message}`);
  }
}

function parseJsonStrict(str) {
  let pos = 0;
  function skipWhitespace() {
    while (pos < str.length && /\s/.test(str[pos])) pos++;
  }
  function parseValue() {
    skipWhitespace();
    if (pos >= str.length) throw new Error('unexpected end of JSON input');
    const ch = str[pos];
    if (ch === 'n') {
      if (str.slice(pos, pos + 4) === 'null') { pos += 4; return vNone(); }
      throw new Error(`unexpected token '${str.slice(pos, pos + 4)}'`);
    }
    if (ch === 't') {
      if (str.slice(pos, pos + 4) === 'true') { pos += 4; return vBool(true); }
      throw new Error(`unexpected token '${str.slice(pos, pos + 4)}'`);
    }
    if (ch === 'f') {
      if (str.slice(pos, pos + 5) === 'false') { pos += 5; return vBool(false); }
      throw new Error(`unexpected token '${str.slice(pos, pos + 5)}'`);
    }
    if (ch === '"') return parseString();
    if (ch === '[') return parseArray();
    if (ch === '{') return parseObject();
    if (ch === '-' || (ch >= '0' && ch <= '9')) return parseNumber();
    throw new Error(`unexpected character '${ch}' at position ${pos}`);
  }
  function parseString() {
    const start = pos;
    pos++;
    while (pos < str.length) {
      if (str[pos] === '\\') {
        pos += 2;
      } else if (str[pos] === '"') {
        pos++;
        const slice = str.slice(start, pos);
        return vStr(JSON.parse(slice));
      } else {
        pos++;
      }
    }
    throw new Error('unterminated string in JSON');
  }
  function parseNumber() {
    const start = pos;
    if (str[pos] === '-') pos++;
    while (pos < str.length && /[0-9.eE+-]/.test(str[pos])) pos++;
    const slice = str.slice(start, pos);
    const num = Number(slice);
    if (!Number.isFinite(num)) throw new Error(`invalid number '${slice}' in JSON`);
    if (Number.isInteger(num) && num >= MIN_SAFE_INT && num <= MAX_SAFE_INT && !slice.includes('.') && !slice.includes('e') && !slice.includes('E')) {
      return vInt(num);
    }
    return vFloat(num);
  }
  function parseArray() {
    pos++;
    skipWhitespace();
    const items = [];
    if (pos < str.length && str[pos] === ']') { pos++; return vList(items); }
    while (pos < str.length) {
      items.push(parseValue());
      skipWhitespace();
      if (pos >= str.length) throw new Error('unclosed array in JSON');
      if (str[pos] === ',') {
        pos++;
        skipWhitespace();
      } else if (str[pos] === ']') {
        pos++;
        return vList(items);
      } else {
        throw new Error(`expected ',' or ']' in JSON array, got '${str[pos]}'`);
      }
    }
    throw new Error('unclosed array in JSON');
  }
  function parseObject() {
    pos++;
    skipWhitespace();
    const entries = [];
    const seen = new Set();
    if (pos < str.length && str[pos] === '}') { pos++; return vDict(entries); }
    while (pos < str.length) {
      skipWhitespace();
      if (pos >= str.length || str[pos] !== '"') throw new Error(`expected string key in JSON object, got '${str[pos]}'`);
      const keyVal = parseString();
      const key = keyVal.v;
      if (seen.has(key)) throw new Error(`duplicate object key: '${key}'`);
      seen.add(key);
      skipWhitespace();
      if (pos >= str.length || str[pos] !== ':') throw new Error(`expected ':' after key in JSON object, got '${str[pos]}'`);
      pos++;
      const val = parseValue();
      entries.push([keyVal, val]);
      skipWhitespace();
      if (pos >= str.length) throw new Error('unclosed object in JSON');
      if (str[pos] === ',') {
        pos++;
        skipWhitespace();
      } else if (str[pos] === '}') {
        pos++;
        return vDict(entries);
      } else {
        throw new Error(`expected ',' or '}' in JSON object, got '${str[pos]}'`);
      }
    }
    throw new Error('unclosed object in JSON');
  }
  const result = parseValue();
  skipWhitespace();
  if (pos < str.length) throw new Error(`extra data after JSON input at position ${pos}`);
  return result;
}

export function std_json_stringify(val, prettyVal) {
  if (isFailure(val)) return val;
  const pretty = prettyVal && prettyVal.t === 'bool' ? prettyVal.v : false;
  const active = new Set();
  try {
    return vStr(stringifyJson(val, pretty, 0, active));
  } catch (e) {
    return vFail(`json.stringify error: ${e.message}`);
  }
}

function stringifyJson(v, pretty, depth, active) {
  if (isFailure(v)) return vFail(v.msg);
  switch (v.t) {
    case 'none': return 'null';
    case 'bool': return v.v ? 'true' : 'false';
    case 'int': return String(v.v);
    case 'float': {
      if (!Number.isFinite(v.v)) throw new Error('non-finite float cannot be serialized to JSON');
      return Number.isInteger(v.v) ? `${v.v}.0` : String(v.v);
    }
    case 'str': return JSON.stringify(v.v.normalize('NFC'));
    case 'list': {
      if (active.has(v.id)) throw new Error('cyclic value cannot be serialized');
      active.add(v.id);
      try {
        if (v.items.length === 0) return '[]';
        if (pretty) {
          const ind = '  '.repeat(depth + 1);
          const inner = v.items.map(it => ind + stringifyJson(it, pretty, depth + 1, active)).join(',\n');
          return `[\n${inner}\n${'  '.repeat(depth)}]`;
        }
        return `[${v.items.map(it => stringifyJson(it, pretty, depth + 1, active)).join(', ')}]`;
      } finally {
        active.delete(v.id);
      }
    }
    case 'dict': {
      if (active.has(v.id)) throw new Error('cyclic value cannot be serialized');
      active.add(v.id);
      try {
        if (v.entries.length === 0) return '{}';
        if (pretty) {
          const ind = '  '.repeat(depth + 1);
          const inner = v.entries.map(([k, val]) => {
            if (k.t !== 'str') throw new Error(`object keys must be strings, got ${typeName(k)}`);
            return `${ind}${JSON.stringify(k.v.normalize('NFC'))}: ${stringifyJson(val, pretty, depth + 1, active)}`;
          }).join(',\n');
          return `{\n${inner}\n${'  '.repeat(depth)}}`;
        }
        const inner = v.entries.map(([k, val]) => {
          if (k.t !== 'str') throw new Error(`object keys must be strings, got ${typeName(k)}`);
          return `${JSON.stringify(k.v.normalize('NFC'))}: ${stringifyJson(val, pretty, depth + 1, active)}`;
        }).join(', ');
        return `{${inner}}`;
      } finally {
        active.delete(v.id);
      }
    }
    case 'struct': {
      if (v.fields.length === 0) return '{}';
      if (pretty) {
        const ind = '  '.repeat(depth + 1);
        const inner = v.fields.map(([k, val]) => {
          return `${ind}${JSON.stringify(k)}: ${stringifyJson(val, pretty, depth + 1, active)}`;
        }).join(',\n');
        return `{\n${inner}\n${'  '.repeat(depth)}}`;
      }
      const inner = v.fields.map(([k, val]) => {
        return `${JSON.stringify(k)}: ${stringifyJson(val, pretty, depth + 1, active)}`;
      }).join(', ');
      return `{${inner}}`;
    }
    default:
      throw new Error(`${typeName(v)} is not serializable to JSON`);
  }
}

export function std_binary_read_varint(bytesVal, offsetVal) {
  if (bytesVal.t !== 'bytes') typeMismatch('Bytes', typeName(bytesVal));
  const off = widen(offsetVal);
  if (off.t !== 'int') typeMismatch('Int', typeName(offsetVal));
  const b = bytesVal.data;
  const len = b.byteLength;
  const at = off.v < 0 ? len + off.v : off.v;
  if (at < 0 || at >= len) {
    fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index out of range: ${off.v} for length ${len}`);
  }
  let result = 0n;
  let shift = 0n;
  let bytesRead = 0;
  while (at + bytesRead < len) {
    const byte = b[at + bytesRead];
    bytesRead++;
    const payload = BigInt(byte & 0x7F);
    if (shift >= 64n || (shift === 63n && payload > 1n)) {
      return vFail('varint overflow: exceeds 64-bit integer');
    }
    result |= (payload << shift);
    if ((byte & 0x80) === 0) {
      if (result > 9007199254740991n) {
        return vFail(`varint value ${result} exceeds safe integer range`);
      }
      return vList([vInt(Number(result)), vInt(bytesRead)]);
    }
    shift += 7n;
  }
  return vFail('unexpected end of bytes while reading varint');
}

export function std_binary_write_varint(bytesVal, offsetVal, val) {
  if (bytesVal.t !== 'bytes') typeMismatch('Bytes', typeName(bytesVal));
  const off = widen(offsetVal);
  if (off.t !== 'int') typeMismatch('Int', typeName(offsetVal));
  const v = widen(val);
  if (v.t !== 'int') typeMismatch('non-negative Int for binary.write_varint', typeName(val));
  if (v.v < 0) return vFail(`varint value ${v.v} must be non-negative`);
  const b = bytesVal.data;
  const len = b.byteLength;
  const at = off.v < 0 ? len + off.v : off.v;
  if (at < 0 || at > len) {
    fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index out of range: ${off.v} for length ${len}`);
  }
  let cur = BigInt(v.v);
  const encoded = [];
  while (true) {
    const byte = Number(cur & 0x7Fn);
    cur >>= 7n;
    if (cur === 0n) {
      encoded.push(byte);
      break;
    }
    encoded.push(byte | 0x80);
  }
  if (at + encoded.length > len) {
    fault('AIPO_RT_INDEX_OUT_OF_RANGE', `index out of range: ${off.v} for length ${len}`);
  }
  for (let i = 0; i < encoded.length; i++) {
    b[at + i] = encoded[i];
  }
  return vInt(encoded.length);
}

function isLeapYear(year) {
  return (year % 4 === 0 && year % 100 !== 0) || (year % 400 === 0);
}

function daysInMonth(year, month) {
  switch (month) {
    case 1: case 3: case 5: case 7: case 8: case 10: case 12: return 31;
    case 4: case 6: case 9: case 11: return 30;
    case 2: return isLeapYear(year) ? 29 : 28;
    default: return 0;
  }
}

function daysSinceEpoch(year, month, day) {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor((y >= 0 ? y : y - 399) / 400);
  const yoe = y - era * 400;
  const m = month <= 2 ? month + 12 : month;
  const doy = Math.floor((153 * (m - 3) + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468;
}

export function vDate(year, month, day) {
  return vStruct('Date', [
    ['year', vInt(year)],
    ['month', vInt(month)],
    ['day', vInt(day)],
  ]);
}

export function vTimeOfDay(hour, minute, second, millisecond) {
  return vStruct('TimeOfDay', [
    ['hour', vInt(hour)],
    ['minute', vInt(minute)],
    ['second', vInt(second)],
    ['millisecond', vInt(millisecond)],
  ]);
}

export function vDateTime(year, month, day, hour, minute, second, millisecond, offsetMinutes) {
  return vStruct('DateTime', [
    ['year', vInt(year)],
    ['month', vInt(month)],
    ['day', vInt(day)],
    ['hour', vInt(hour)],
    ['minute', vInt(minute)],
    ['second', vInt(second)],
    ['millisecond', vInt(millisecond)],
    ['offset_minutes', vInt(offsetMinutes)],
  ]);
}

function getStructField(inst, field) {
  const f = inst.fields.find(([k]) => k === field);
  return f ? f[1] : null;
}

export function std_date_to_iso(receiver) {
  if (receiver.t !== 'struct' || receiver.type !== 'Date') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: Date struct receiver, got ${typeName(receiver)}`);
  }
  const y = getStructField(receiver, 'year')?.v ?? 0;
  const m = getStructField(receiver, 'month')?.v ?? 0;
  const d = getStructField(receiver, 'day')?.v ?? 0;
  const ys = String(y).padStart(4, '0');
  const ms = String(m).padStart(2, '0');
  const ds = String(d).padStart(2, '0');
  return vStr(`${ys}-${ms}-${ds}`);
}

export function std_time_to_iso(receiver) {
  if (receiver.t !== 'struct' || receiver.type !== 'TimeOfDay') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: TimeOfDay struct receiver, got ${typeName(receiver)}`);
  }
  const h = String(getStructField(receiver, 'hour')?.v ?? 0).padStart(2, '0');
  const min = String(getStructField(receiver, 'minute')?.v ?? 0).padStart(2, '0');
  const s = String(getStructField(receiver, 'second')?.v ?? 0).padStart(2, '0');
  const msVal = getStructField(receiver, 'millisecond')?.v ?? 0;
  if (msVal > 0) {
    const ms = String(msVal).padStart(3, '0');
    return vStr(`${h}:${min}:${s}.${ms}`);
  }
  return vStr(`${h}:${min}:${s}`);
}

export function std_datetime_to_iso(receiver) {
  if (receiver.t !== 'struct' || receiver.type !== 'DateTime') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: DateTime struct receiver, got ${typeName(receiver)}`);
  }
  const y = String(getStructField(receiver, 'year')?.v ?? 0).padStart(4, '0');
  const m = String(getStructField(receiver, 'month')?.v ?? 0).padStart(2, '0');
  const d = String(getStructField(receiver, 'day')?.v ?? 0).padStart(2, '0');
  const h = String(getStructField(receiver, 'hour')?.v ?? 0).padStart(2, '0');
  const min = String(getStructField(receiver, 'minute')?.v ?? 0).padStart(2, '0');
  const s = String(getStructField(receiver, 'second')?.v ?? 0).padStart(2, '0');
  const msVal = getStructField(receiver, 'millisecond')?.v ?? 0;
  const offVal = getStructField(receiver, 'offset_minutes')?.v ?? 0;

  const timeStr = msVal > 0 ? `${h}:${min}:${s}.${String(msVal).padStart(3, '0')}` : `${h}:${min}:${s}`;
  let offStr = 'Z';
  if (offVal !== 0) {
    const sign = offVal >= 0 ? '+' : '-';
    const absOff = Math.abs(offVal);
    const offH = String(Math.floor(absOff / 60)).padStart(2, '0');
    const offM = String(absOff % 60).padStart(2, '0');
    offStr = `${sign}${offH}:${offM}`;
  }
  return vStr(`${y}-${m}-${d}T${timeStr}${offStr}`);
}

export function std_datetime_date(receiver) {
  if (receiver.t !== 'struct' || receiver.type !== 'DateTime') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: DateTime struct receiver, got ${typeName(receiver)}`);
  }
  const y = getStructField(receiver, 'year')?.v ?? 0;
  const m = getStructField(receiver, 'month')?.v ?? 0;
  const d = getStructField(receiver, 'day')?.v ?? 0;
  return vDate(y, m, d);
}

export function std_datetime_time(receiver) {
  if (receiver.t !== 'struct' || receiver.type !== 'DateTime') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: DateTime struct receiver, got ${typeName(receiver)}`);
  }
  const h = getStructField(receiver, 'hour')?.v ?? 0;
  const min = getStructField(receiver, 'minute')?.v ?? 0;
  const s = getStructField(receiver, 'second')?.v ?? 0;
  const ms = getStructField(receiver, 'millisecond')?.v ?? 0;
  return vTimeOfDay(h, min, s, ms);
}

export function std_datetime_epoch_seconds(receiver) {
  if (receiver.t !== 'struct' || receiver.type !== 'DateTime') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: DateTime struct receiver, got ${typeName(receiver)}`);
  }
  const y = getStructField(receiver, 'year')?.v ?? 0;
  const m = getStructField(receiver, 'month')?.v ?? 0;
  const d = getStructField(receiver, 'day')?.v ?? 0;
  const h = getStructField(receiver, 'hour')?.v ?? 0;
  const min = getStructField(receiver, 'minute')?.v ?? 0;
  const s = getStructField(receiver, 'second')?.v ?? 0;
  const ms = getStructField(receiver, 'millisecond')?.v ?? 0;
  const off = getStructField(receiver, 'offset_minutes')?.v ?? 0;

  const days = daysSinceEpoch(y, m, d);
  const totalSec = days * 86400 + h * 3600 + min * 60 + s - (off * 60);
  const total = totalSec + ms / 1000.0;
  return checkFiniteFloat(total);
}

export function std_time_date(yVal, mVal, dVal) {
  const y = widen(yVal), m = widen(mVal), d = widen(dVal);
  if (y.t !== 'int' || m.t !== 'int' || d.t !== 'int') {
    fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected Int for year, month, day');
  }
  if (y.v < 1 || y.v > 9999) return vFail(`year ${y.v} out of valid range 1..=9999`);
  if (m.v < 1 || m.v > 12) return vFail(`month ${m.v} out of valid range 1..=12`);
  const maxD = daysInMonth(y.v, m.v);
  if (d.v < 1 || d.v > maxD) {
    return vFail(`day ${d.v} out of valid range 1..=${maxD} for month ${m.v} in year ${y.v}`);
  }
  return vDate(y.v, m.v, d.v);
}

export function std_time_time_of_day(args) {
  if (args.length < 2 || args.length > 4) {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: 2 to 4 arguments for time.time_of_day, got ${args.length}`);
  }
  const h = widen(args[0]), min = widen(args[1]);
  const s = args.length >= 3 ? widen(args[2]) : vInt(0);
  const ms = args.length >= 4 ? widen(args[3]) : vInt(0);
  if (h.t !== 'int' || min.t !== 'int' || s.t !== 'int' || ms.t !== 'int') {
    fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected Int arguments for time.time_of_day');
  }
  if (h.v < 0 || h.v > 23) return vFail(`hour ${h.v} out of range 0..=23`);
  if (min.v < 0 || min.v > 59) return vFail(`minute ${min.v} out of range 0..=59`);
  if (s.v < 0 || s.v > 59) return vFail(`second ${s.v} out of range 0..=59`);
  if (ms.v < 0 || ms.v > 999) return vFail(`millisecond ${ms.v} out of range 0..=999`);
  return vTimeOfDay(h.v, min.v, s.v, ms.v);
}

export function std_time_date_time(args) {
  if (args.length === 2 || args.length === 3) {
    const dObj = args[0], tObj = args[1];
    if (dObj.t !== 'struct' || dObj.type !== 'Date') {
      fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: Date struct as first argument, got ${typeName(dObj)}`);
    }
    if (tObj.t !== 'struct' || tObj.type !== 'TimeOfDay') {
      fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: TimeOfDay struct as second argument, got ${typeName(tObj)}`);
    }
    const off = args.length === 3 ? widen(args[2]) : vInt(0);
    if (off.t !== 'int') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: Int for offset_minutes');
    if (off.v < -840 || off.v > 840) return vFail(`offset_minutes ${off.v} out of range -840..=840`);
    const y = getStructField(dObj, 'year')?.v ?? 0;
    const m = getStructField(dObj, 'month')?.v ?? 0;
    const d = getStructField(dObj, 'day')?.v ?? 0;
    const h = getStructField(tObj, 'hour')?.v ?? 0;
    const min = getStructField(tObj, 'minute')?.v ?? 0;
    const s = getStructField(tObj, 'second')?.v ?? 0;
    const ms = getStructField(tObj, 'millisecond')?.v ?? 0;
    return vDateTime(y, m, d, h, min, s, ms, off.v);
  } else if (args.length >= 4 && args.length <= 8) {
    const y = widen(args[0]), m = widen(args[1]), d = widen(args[2]), h = widen(args[3]);
    const min = args.length >= 5 ? widen(args[4]) : vInt(0);
    const s = args.length >= 6 ? widen(args[5]) : vInt(0);
    const ms = args.length >= 7 ? widen(args[6]) : vInt(0);
    const off = args.length >= 8 ? widen(args[7]) : vInt(0);
    if ([y, m, d, h, min, s, ms, off].some(x => x.t !== 'int')) {
      fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: Int for date_time arguments');
    }
    const dateCheck = std_time_date(y, m, d);
    if (isFailure(dateCheck)) return dateCheck;
    const timeCheck = std_time_time_of_day([h, min, s, ms]);
    if (isFailure(timeCheck)) return timeCheck;
    if (off.v < -840 || off.v > 840) return vFail(`offset_minutes ${off.v} out of range -840..=840`);
    return vDateTime(y.v, m.v, d.v, h.v, min.v, s.v, ms.v, off.v);
  } else {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: 2, 3 or 4..=8 arguments for time.date_time, got ${args.length}`);
  }
}

export function std_time_duration(arg) {
  const w = widen(arg);
  if (w.t === 'duration') return w;
  if (w.t === 'float') return vDuration(w.v);
  if (w.t === 'int') return vDuration(w.v);
  fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: Float or Int for time.duration, got ${typeName(arg)}`);
}

export function std_time_parse_date(arg) {
  if (arg.t !== 'str') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: String for time.parse_date, got ${typeName(arg)}`);
  const text = arg.v.trim();
  const parts = text.split('-');
  if (parts.length !== 3) return vFail(`invalid date format "${text}", expected YYYY-MM-DD`);
  const y = Number(parts[0]), m = Number(parts[1]), d = Number(parts[2]);
  if (!Number.isInteger(y)) return vFail(`invalid year in date: "${parts[0]}"`);
  if (!Number.isInteger(m)) return vFail(`invalid month in date: "${parts[1]}"`);
  if (!Number.isInteger(d)) return vFail(`invalid day in date: "${parts[2]}"`);
  return std_time_date(vInt(y), vInt(m), vInt(d));
}

export function std_time_parse_time(arg) {
  if (arg.t !== 'str') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: String for time.parse_time, got ${typeName(arg)}`);
  const text = arg.v.trim();
  const parts = text.split(':');
  if (parts.length < 2 || parts.length > 3) {
    return vFail(`invalid time format "${text}", expected HH:MM[:SS[.sss]]`);
  }
  const h = Number(parts[0]), min = Number(parts[1]);
  if (!Number.isInteger(h)) return vFail(`invalid hour in time: "${parts[0]}"`);
  if (!Number.isInteger(min)) return vFail(`invalid minute in time: "${parts[1]}"`);
  let s = 0, ms = 0;
  if (parts.length === 3) {
    const secStr = parts[2];
    const dot = secStr.indexOf('.');
    if (dot >= 0) {
      const secP = Number(secStr.slice(0, dot));
      if (!Number.isInteger(secP)) return vFail(`invalid second: "${secStr.slice(0, dot)}"`);
      s = secP;
      let msText = secStr.slice(dot + 1);
      if (msText.length > 3) msText = msText.slice(0, 3);
      while (msText.length < 3) msText += '0';
      ms = Number(msText) || 0;
    } else {
      const secP = Number(secStr);
      if (!Number.isInteger(secP)) return vFail(`invalid second: "${secStr}"`);
      s = secP;
    }
  }
  return std_time_time_of_day([vInt(h), vInt(min), vInt(s), vInt(ms)]);
}

function parseOffsetMinutes(s, sign) {
  const colon = s.indexOf(':');
  if (colon >= 0) {
    const h = Number(s.slice(0, colon)) || 0;
    const m = Number(s.slice(colon + 1)) || 0;
    return sign * (h * 60 + m);
  } else if (s.length === 4) {
    const h = Number(s.slice(0, 2)) || 0;
    const m = Number(s.slice(2)) || 0;
    return sign * (h * 60 + m);
  } else if (s.length === 2) {
    const h = Number(s) || 0;
    return sign * (h * 60);
  }
  return 0;
}

export function std_time_parse_iso(arg) {
  if (arg.t !== 'str') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: String for time.parse_iso, got ${typeName(arg)}`);
  const text = arg.v.trim();
  let sepIdx = text.indexOf('T');
  if (sepIdx < 0) sepIdx = text.indexOf('t');
  if (sepIdx < 0) sepIdx = text.indexOf(' ');
  if (sepIdx < 0) {
    return vFail(`invalid ISO 8601 string "${text}": missing 'T' separator`);
  }
  const dateStr = text.slice(0, sepIdx);
  const timeAndOffset = text.slice(sepIdx + 1);

  const dVal = std_time_parse_date(vStr(dateStr));
  if (isFailure(dVal)) return dVal;

  let timeStr = timeAndOffset;
  let offsetMinutes = 0;
  if (timeAndOffset.endsWith('Z') || timeAndOffset.endsWith('z')) {
    timeStr = timeAndOffset.slice(0, -1);
    offsetMinutes = 0;
  } else {
    const plus = timeAndOffset.lastIndexOf('+');
    const minus = timeAndOffset.lastIndexOf('-');
    if (plus >= 0) {
      timeStr = timeAndOffset.slice(0, plus);
      offsetMinutes = parseOffsetMinutes(timeAndOffset.slice(plus + 1), 1);
    } else if (minus >= 0) {
      timeStr = timeAndOffset.slice(0, minus);
      offsetMinutes = parseOffsetMinutes(timeAndOffset.slice(minus + 1), -1);
    }
  }

  const tVal = std_time_parse_time(vStr(timeStr));
  if (isFailure(tVal)) return tVal;

  const y = getStructField(dVal, 'year')?.v ?? 0;
  const m = getStructField(dVal, 'month')?.v ?? 0;
  const d = getStructField(dVal, 'day')?.v ?? 0;
  const h = getStructField(tVal, 'hour')?.v ?? 0;
  const min = getStructField(tVal, 'minute')?.v ?? 0;
  const s = getStructField(tVal, 'second')?.v ?? 0;
  const ms = getStructField(tVal, 'millisecond')?.v ?? 0;

  return vDateTime(y, m, d, h, min, s, ms, offsetMinutes);
}

export function std_expect_equal(actual, expected) {
  if (valuesEqual(actual, expected)) {
    return vNone();
  }
  return vFail(`expect.equal failed: expected ${display(expected)}, got ${display(actual)}`);
}

export function std_expect_not_equal(actual, expected) {
  if (!valuesEqual(actual, expected)) {
    return vNone();
  }
  return vFail(`expect.not_equal failed: expected values to differ, both are ${display(actual)}`);
}

export function std_expect_true(val) {
  if (val.t === 'bool' && val.v === true) {
    return vNone();
  }
  return vFail(`expect.true failed: expected true, got ${display(val)}`);
}

export function std_expect_false(val) {
  if (val.t === 'bool' && val.v === false) {
    return vNone();
  }
  return vFail(`expect.false failed: expected false, got ${display(val)}`);
}

export function std_expect_none(val) {
  if (val.t === 'none') {
    return vNone();
  }
  return vFail(`expect.none failed: expected none, got ${display(val)}`);
}

export function std_expect_some(val) {
  if (val.t !== 'none') {
    return vNone();
  }
  return vFail('expect.some failed: expected a value, got none');
}

export function std_expect_failure(val) {
  if (isFailure(val)) {
    return vNone();
  }
  return vFail(`expect.failure failed: expected Failure, got ${display(val)}`);
}

export function std_expect_contains(coll, elem) {
  let found = false;
  switch (coll.t) {
    case 'str': {
      if (elem.t !== 'str') typeMismatch('String substring', typeName(elem));
      found = coll.v.includes(elem.v);
      break;
    }
    case 'list': {
      found = coll.items.some(x => valuesEqual(x, elem));
      break;
    }
    case 'dict': {
      found = dictGet(coll, elem) !== null;
      break;
    }
    case 'set': {
      found = coll.items.some(x => valuesEqual(x, elem));
      break;
    }
    case 'bytes': {
      const e = widen(elem);
      if (e.t !== 'int' || e.v < 0 || e.v > 255) {
        typeMismatch('Byte or Int in 0..=255', typeName(elem));
      }
      found = coll.data.includes(e.v);
      break;
    }
    default:
      typeMismatch('String, List, Dict, Set, or Bytes for expect.contains', typeName(coll));
  }
  if (found) {
    return vNone();
  }
  return vFail(`expect.contains failed: collection does not contain ${display(elem)}`);
}

export function std_expect_approx(args) {
  if (args.length < 2 || args.length > 3) {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: 2 or 3 arguments for expect.approx, got ${args.length}`);
  }
  const toNum = (v, name) => {
    const w = widen(v);
    if (w.t === 'int') return w.v;
    if (w.t === 'float') return w.v;
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: Float or Int for ${name}, got ${typeName(v)}`);
  };
  const act = toNum(args[0], 'actual');
  const exp = toNum(args[1], 'expected');
  const tol = args.length === 3 ? toNum(args[2], 'tolerance') : 0.0001;
  const diff = Math.abs(act - exp);
  if (diff <= tol) {
    return vNone();
  }
  return vFail(`expect.approx failed: difference ${diff} exceeds tolerance ${tol}`);
}

export function std_log_emit(level, args) {
  if (args.length === 0 || args.length > 2) {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: 1 or 2 arguments (message, [fields]) for log, got ${args.length}`);
  }
  const msg = args[0].t === 'str' ? args[0].v : display(args[0]);
  let line;
  if (args.length === 2) {
    const fieldsStr = display(args[1]);
    line = `[${level}] ${msg} ${fieldsStr}`;
  } else {
    line = `[${level}] ${msg}`;
  }
  emitText(line + '\n');
  return vNone();
}

const MASK64 = 0xFFFFFFFFFFFFFFFFn;
const SPLITMIX_INC = 0x9e3779b97f4a7c15n;
const SPLITMIX_MUL1 = 0xbf58476d1ce4e5b9n;
const SPLITMIX_MUL2 = 0x94d049bb133111ebn;

export function splitmix64Step(seed, step) {
  const s = (BigInt(seed) + BigInt(step) * SPLITMIX_INC) & MASK64;
  let z = s;
  z = ((z ^ (z >> 30n)) * SPLITMIX_MUL1) & MASK64;
  z = ((z ^ (z >> 27n)) * SPLITMIX_MUL2) & MASK64;
  return (z ^ (z >> 31n)) & MASK64;
}

export function createRngInstance(seed) {
  const s = (seed !== undefined && seed !== null) ? Number(widen(seed).v !== undefined ? widen(seed).v : seed) : 0;
  return vStruct('Rng', [
    ['seed', vInt(s)],
    ['step', vInt(0)],
  ]);
}

function rngStepWord(recv) {
  if (recv.t !== 'struct' || recv.type !== 'Rng') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Rng struct receiver, got ${typeName(recv)}`);
  }
  const seedEntry = recv.fields.find(([k]) => k === 'seed');
  const stepEntry = recv.fields.find(([k]) => k === 'step');
  const seed = seedEntry ? seedEntry[1].v : 0;
  const step = stepEntry ? stepEntry[1].v : 0;
  const word = splitmix64Step(seed, step);
  if (stepEntry) {
    stepEntry[1] = vInt((step + 1) & 0x1FFFFFFFFFFFFF);
  }
  return word;
}

function rngMethodInt(recv, minVal, maxVal) {
  const mn = widen(minVal), mx = widen(maxVal);
  if (mn.t !== 'int' || mx.t !== 'int') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected Int for min and max');
  if (mn.v > mx.v) return vFail('random.int: min must be <= max');
  if (mn.v === mx.v) return mn;
  const word = rngStepWord(recv);
  const span = BigInt(mx.v - mn.v + 1);
  const n = Number(word % span);
  return vInt(mn.v + n);
}

function rngMethodFloat(recv) {
  const word = rngStepWord(recv);
  const bits = word >> 11n;
  const f = Number(bits) * (1 / 9007199254740992);
  return checkFiniteFloat(f);
}

function rngMethodBool(recv) {
  const word = rngStepWord(recv);
  return vBool((word & 1n) === 1n);
}

function rngMethodChoice(recv, listVal) {
  if (listVal.t !== 'list') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected List');
  if (listVal.items.length === 0) return vFail('random.choice: cannot choose from empty list');
  const word = rngStepWord(recv);
  const idx = Number(word % BigInt(listVal.items.length));
  return listVal.items[idx];
}

function rngMethodShuffle(recv, listVal) {
  if (listVal.t !== 'list') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected List');
  const items = [...listVal.items];
  for (let i = items.length - 1; i > 0; i--) {
    const word = rngStepWord(recv);
    const j = Number(word % BigInt(i + 1));
    const tmp = items[i];
    items[i] = items[j];
    items[j] = tmp;
  }
  return vList(items);
}

function mPush(m, v) {
  if (m.stack.length >= 1024) fault('AIPO_RT_OVERFLOW', `operand stack exceeded depth limit 1024 (${m.frames.length} active call frames; deep or unbounded recursion is the usual cause)`);
  m.stack.push(v);
}
function mPop(m) {
  if (m.stack.length === 0) fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: stack underflow');
  return m.stack.pop();
}
function mPeek(m) {
  if (m.stack.length === 0) fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: stack underflow');
  return m.stack[m.stack.length - 1];
}
function curFrame(m) { return m.frames.length ? m.frames[m.frames.length - 1] : null; }

function bindMethod(m, recv, name) {
  if (recv.t === 'group') {
    if (name === 'spawn') return { t: 'bound', name, arity: 2, recv, kind: 'group' };
    if (name === 'wait') return { t: 'bound', name, arity: 0, recv, kind: 'group' };
    return null;
  }
  const tn = recv.t === 'struct' ? recv.type : typeName(recv);
  const key = `${tn}.${name}`;
  const table = {
    'Rng.int': [2, a => rngMethodInt(a[0], a[1], a[2])],
    'Rng.float': [0, a => rngMethodFloat(a[0])],
    'Rng.bool': [0, a => rngMethodBool(a[0])],
    'Rng.choice': [1, a => rngMethodChoice(a[0], a[1])],
    'Rng.shuffle': [1, a => rngMethodShuffle(a[0], a[1])],
    'Pattern.is_match': [1, a => patternMethodIsMatch(a[0], a[1])],
    'Pattern.find': [1, a => patternMethodFind(a[0], a[1])],
    'Pattern.find_all': [1, a => patternMethodFindAll(a[0], a[1])],
    'Pattern.replace': [2, a => patternMethodReplace(a[0], a[1], a[2])],
    'Pattern.split': [1, a => patternMethodSplit(a[0], a[1])],
    'String.len': [0, a => std_string_len(a[0])],
    'String.byte_len': [0, a => std_string_byte_len(a[0])],
    'String.contains': [1, a => std_string_contains(a[0], a[1])],
    'String.starts_with': [1, a => std_string_starts_with(a[0], a[1])],
    'String.ends_with': [1, a => std_string_ends_with(a[0], a[1])],
    'String.find': [1, a => std_string_find(a[0], a[1])],
    'String.lower': [0, a => std_string_lower(a[0])],
    'String.upper': [0, a => std_string_upper(a[0])],
    'String.capitalize': [0, a => std_string_capitalize(a[0])],
    'String.reverse': [0, a => std_string_reverse(a[0])],
    'String.trim': [0, a => std_string_trim(a[0])],
    'String.split': [1, a => std_string_split(a[0], a[1])],
    'String.join': [1, a => std_string_join(a[0], a[1])],
    'String.replace': [2, a => std_string_replace(a[0], a[1], a[2])],
    'String.slice': [2, a => std_string_slice(a[0], a[1], a[2])],
    'String.format': [1, a => std_string_format(a[0], a[1])],
    'String.graphemes': [0, a => std_string_graphemes(a[0])],
    'String.words': [0, a => std_string_words(a[0])],
    'String.lines': [0, a => std_string_lines(a[0])],
    'String.casefold': [0, a => std_string_casefold(a[0])],
    'String.encode': [0, a => { reqStr(a[0]); return vBytes(new TextEncoder().encode(a[0].v)); }],
    'String.encode_utf8': [0, a => { reqStr(a[0]); return vBytes(new TextEncoder().encode(a[0].v)); }],
    'Duration.total_seconds': [0, a => { if (a[0].t !== 'duration') return typeMismatch('Duration', typeName(a[0])); return checkFiniteFloat(a[0].v); }],
    'List.add': [1, a => listNatives.add(a[0], a.slice(1))],
    'List.insert': [2, a => listNatives.insert(a[0], a.slice(1))],
    'List.remove': [1, a => listNatives.remove(a[0], a.slice(1))],
    'List.remove_at': [1, a => listNatives.remove_at(a[0], a.slice(1))],
    'List.remove_last': [0, a => listNatives.remove_last(a[0], a.slice(1))],
    'List.clear': [0, a => listNatives.clear(a[0], a.slice(1))],
    'List.contains': [1, a => listNatives.contains(a[0], a.slice(1))],
    'List.find': [1, a => listNatives.find(a[0], a.slice(1))],
    'List.find_index': [1, a => listNatives.find_index(a[0], a.slice(1))],
    'List.count': [1, a => listNatives.count(a[0], a.slice(1))],
    'List.first': [0, a => listNatives.first(a[0], a.slice(1))],
    'List.first_or': [1, a => listNatives.first_or(a[0], a.slice(1))],
    'List.last': [0, a => listNatives.last(a[0], a.slice(1))],
    'List.last_or': [1, a => listNatives.last_or(a[0], a.slice(1))],
    'List.is_empty': [0, a => listNatives.is_empty(a[0], a.slice(1))],
    'List.reverse': [0, a => listNatives.reverse(a[0], a.slice(1))],
    'List.sort': [0, a => listNatives.sort(a[0], a.slice(1))],
    'List.len': [0, a => listNatives.len(a[0], a.slice(1))],
    'List.take': [1, a => listNatives.take(a[0], a.slice(1))],
    'List.skip': [1, a => listNatives.skip(a[0], a.slice(1))],
    'List.distinct': [0, a => listNatives.distinct(a[0], a.slice(1))],
    'List.zip': [1, a => listNatives.zip(a[0], a.slice(1))],
    'List.chain': [1, a => listNatives.chain(a[0], a.slice(1))],
    'List.chunk': [1, a => listNatives.chunk(a[0], a.slice(1))],
    'List.window': [1, a => listNatives.window(a[0], a.slice(1))],
    'List.enumerate': [0, a => listNatives.enumerate(a[0], a.slice(1))],
    'List.lazy': [0, a => listNatives.lazy(a[0])],
    'Dict.has': [1, a => dictNatives.has(a[0], a.slice(1))],
    'Dict.get': [1, a => dictNatives.get(a[0], a.slice(1))],
    'Dict.keys': [0, a => dictNatives.keys(a[0], a.slice(1))],
    'Dict.values': [0, a => dictNatives.values(a[0], a.slice(1))],
    'Dict.entries': [0, a => dictNatives.entries(a[0], a.slice(1))],
    'Dict.remove': [1, a => dictNatives.remove(a[0], a.slice(1))],
    'Dict.clear': [0, a => dictNatives.clear(a[0], a.slice(1))],
    'Dict.is_empty': [0, a => dictNatives.is_empty(a[0], a.slice(1))],
    'Dict.len': [0, a => dictNatives.len(a[0], a.slice(1))],
    'Dict.lazy': [0, a => dictNatives.lazy(a[0])],
    'Set.has': [1, a => setNatives.has(a[0], a.slice(1))],
    'Set.add': [1, a => setNatives.add(a[0], a.slice(1))],
    'Set.remove': [1, a => setNatives.remove(a[0], a.slice(1))],
    'Set.clear': [0, a => setNatives.clear(a[0])],
    'Set.is_empty': [0, a => setNatives.is_empty(a[0])],
    'Set.len': [0, a => setNatives.len(a[0])],
    'Set.to_list': [0, a => setNatives.to_list(a[0])],
    'Set.lazy': [0, a => setNatives.lazy(a[0])],
    'Bytes.len': [0, a => vInt(a[0].data.byteLength)],
    'Bytes.read_i8': [1, a => bytesNatives.read_i8(a[0], a.slice(1))],
    'Bytes.read_u8': [1, a => bytesNatives.read_u8(a[0], a.slice(1))],
    'Bytes.read_i16': [1, a => bytesNatives.read_i16(a[0], a.slice(1))],
    'Bytes.read_u16': [1, a => bytesNatives.read_u16(a[0], a.slice(1))],
    'Bytes.read_i32': [1, a => bytesNatives.read_i32(a[0], a.slice(1))],
    'Bytes.read_u32': [1, a => bytesNatives.read_u32(a[0], a.slice(1))],
    'Bytes.read_i64': [1, a => bytesNatives.read_i64(a[0], a.slice(1))],
    'Bytes.read_u64': [1, a => bytesNatives.read_u64(a[0], a.slice(1))],
    'Bytes.read_f32': [1, a => bytesNatives.read_f32(a[0], a.slice(1))],
    'Bytes.read_f64': [1, a => bytesNatives.read_f64(a[0], a.slice(1))],
    'Bytes.write_i8': [2, a => bytesNatives.write_i8(a[0], a.slice(1))],
    'Bytes.write_u8': [2, a => bytesNatives.write_u8(a[0], a.slice(1))],
    'Bytes.write_i16': [2, a => bytesNatives.write_i16(a[0], a.slice(1))],
    'Bytes.write_u16': [2, a => bytesNatives.write_u16(a[0], a.slice(1))],
    'Bytes.write_i32': [2, a => bytesNatives.write_i32(a[0], a.slice(1))],
    'Bytes.write_u32': [2, a => bytesNatives.write_u32(a[0], a.slice(1))],
    'Bytes.write_i64': [2, a => bytesNatives.write_i64(a[0], a.slice(1))],
    'Bytes.write_u64': [2, a => bytesNatives.write_u64(a[0], a.slice(1))],
    'Bytes.write_f32': [2, a => bytesNatives.write_f32(a[0], a.slice(1))],
    'Bytes.write_f64': [2, a => bytesNatives.write_f64(a[0], a.slice(1))],
    'Bytes.decode': [0, a => bytesNatives.decode(a[0])],
    'Bytes.decode_utf8': [0, a => bytesNatives.decode(a[0])],
    'Bytes.slice': [2, a => bytesNatives.slice(a[0], a.slice(1))],
    'Bytes.read_i16_le': [1, a => bytesNatives.read_i16_le(a[0], a.slice(1))],
    'Bytes.read_i16_be': [1, a => bytesNatives.read_i16_be(a[0], a.slice(1))],
    'Bytes.read_u16_le': [1, a => bytesNatives.read_u16_le(a[0], a.slice(1))],
    'Bytes.read_u16_be': [1, a => bytesNatives.read_u16_be(a[0], a.slice(1))],
    'Bytes.read_i32_le': [1, a => bytesNatives.read_i32_le(a[0], a.slice(1))],
    'Bytes.read_i32_be': [1, a => bytesNatives.read_i32_be(a[0], a.slice(1))],
    'Bytes.read_u32_le': [1, a => bytesNatives.read_u32_le(a[0], a.slice(1))],
    'Bytes.read_u32_be': [1, a => bytesNatives.read_u32_be(a[0], a.slice(1))],
    'Bytes.read_i64_le': [1, a => bytesNatives.read_i64_le(a[0], a.slice(1))],
    'Bytes.read_i64_be': [1, a => bytesNatives.read_i64_be(a[0], a.slice(1))],
    'Bytes.read_u64_le': [1, a => bytesNatives.read_u64_le(a[0], a.slice(1))],
    'Bytes.read_u64_be': [1, a => bytesNatives.read_u64_be(a[0], a.slice(1))],
    'Bytes.read_f32_le': [1, a => bytesNatives.read_f32_le(a[0], a.slice(1))],
    'Bytes.read_f32_be': [1, a => bytesNatives.read_f32_be(a[0], a.slice(1))],
    'Bytes.read_f64_le': [1, a => bytesNatives.read_f64_le(a[0], a.slice(1))],
    'Bytes.read_f64_be': [1, a => bytesNatives.read_f64_be(a[0], a.slice(1))],
    'Bytes.write_i16_le': [2, a => bytesNatives.write_i16_le(a[0], a.slice(1))],
    'Bytes.write_i16_be': [2, a => bytesNatives.write_i16_be(a[0], a.slice(1))],
    'Bytes.write_u16_le': [2, a => bytesNatives.write_u16_le(a[0], a.slice(1))],
    'Bytes.write_u16_be': [2, a => bytesNatives.write_u16_be(a[0], a.slice(1))],
    'Bytes.write_i32_le': [2, a => bytesNatives.write_i32_le(a[0], a.slice(1))],
    'Bytes.write_i32_be': [2, a => bytesNatives.write_i32_be(a[0], a.slice(1))],
    'Bytes.write_u32_le': [2, a => bytesNatives.write_u32_le(a[0], a.slice(1))],
    'Bytes.write_u32_be': [2, a => bytesNatives.write_u32_be(a[0], a.slice(1))],
    'Bytes.write_i64_le': [2, a => bytesNatives.write_i64_le(a[0], a.slice(1))],
    'Bytes.write_i64_be': [2, a => bytesNatives.write_i64_be(a[0], a.slice(1))],
    'Bytes.write_u64_le': [2, a => bytesNatives.write_u64_le(a[0], a.slice(1))],
    'Bytes.write_u64_be': [2, a => bytesNatives.write_u64_be(a[0], a.slice(1))],
    'Bytes.write_f32_le': [2, a => bytesNatives.write_f32_le(a[0], a.slice(1))],
    'Bytes.write_f32_be': [2, a => bytesNatives.write_f32_be(a[0], a.slice(1))],
    'Bytes.write_f64_le': [2, a => bytesNatives.write_f64_le(a[0], a.slice(1))],
    'Bytes.write_f64_be': [2, a => bytesNatives.write_f64_be(a[0], a.slice(1))],
    'Duration.total_milliseconds': [0, a => { if (a[0].t !== 'duration') return typeMismatch('Duration', typeName(a[0])); return checkFiniteFloat(a[0].v * 1000); }],
    'Duration.to_string': [0, a => { if (a[0].t !== 'duration') return typeMismatch('Duration', typeName(a[0])); return vStr(`${a[0].v}s`); }],
    'Date.to_iso': [0, a => std_date_to_iso(a[0])],
    'Date.to_string': [0, a => std_date_to_iso(a[0])],
    'TimeOfDay.to_iso': [0, a => std_time_to_iso(a[0])],
    'TimeOfDay.to_string': [0, a => std_time_to_iso(a[0])],
    'DateTime.to_iso': [0, a => std_datetime_to_iso(a[0])],
    'DateTime.to_string': [0, a => std_datetime_to_iso(a[0])],
    'DateTime.date': [0, a => std_datetime_date(a[0])],
    'DateTime.time': [0, a => std_datetime_time(a[0])],
    'DateTime.epoch_seconds': [0, a => std_datetime_epoch_seconds(a[0])],
  };
  if (table[key]) {
    const [arity, fn] = table[key];
    return { t: 'bound', name, arity, recv, kind: 'native', fn };
  }
  if ((name === 'filter' || name === 'transform' || name === 'map' || name === 'sort_by' || name === 'any' || name === 'all' || name === 'flat_map') &&
      (recv.t === 'list' || recv.t === 'dict' || recv.t === 'set' || recv.t === 'str' || recv.t === 'range')) {
    return { t: 'bound', name, arity: 1, recv, kind: 'higher' };
  }
  if (name === 'reduce' &&
      (recv.t === 'list' || recv.t === 'dict' || recv.t === 'set' || recv.t === 'str' || recv.t === 'range')) {
    return { t: 'bound', name, arity: 2, recv, kind: 'higher' };
  }
  if (recv.t === 'sequence') {
    if (name === 'map' || name === 'transform') {
      return { t: 'bound', name, arity: 1, recv, kind: 'native', fn: ([, fn]) => vSequence({ source: recv.source, ops: [...recv.ops, { op: 'map', fn }] }) };
    }
    if (name === 'filter') {
      return { t: 'bound', name, arity: 1, recv, kind: 'native', fn: ([, fn]) => vSequence({ source: recv.source, ops: [...recv.ops, { op: 'filter', fn }] }) };
    }
    if (name === 'flat_map') {
      return { t: 'bound', name, arity: 1, recv, kind: 'native', fn: ([, fn]) => vSequence({ source: recv.source, ops: [...recv.ops, { op: 'flat_map', fn }] }) };
    }
    if (name === 'take') {
      return { t: 'bound', name, arity: 1, recv, kind: 'native', fn: ([, n]) => vSequence({ source: recv.source, ops: [...recv.ops, { op: 'take', n }] }) };
    }
    if (name === 'skip') {
      return { t: 'bound', name, arity: 1, recv, kind: 'native', fn: ([, n]) => vSequence({ source: recv.source, ops: [...recv.ops, { op: 'skip', n }] }) };
    }
    if (name === 'distinct') {
      return { t: 'bound', name, arity: 0, recv, kind: 'native', fn: () => vSequence({ source: recv.source, ops: [...recv.ops, { op: 'distinct' }] }) };
    }
    if (name === 'enumerate') {
      return { t: 'bound', name, arity: 0, recv, kind: 'native', fn: () => vSequence({ source: recv.source, ops: [...recv.ops, { op: 'enumerate' }] }) };
    }
    if (name === 'collect') {
      return { t: 'bound', name, arity: 0, recv, kind: 'native', fn: ([r]) => vList(evaluateSequence(m, r)) };
    }
    if (name === 'count') {
      return { t: 'bound', name, arity: 0, recv, kind: 'native', fn: ([r]) => vInt(evaluateSequence(m, r).length) };
    }
    if (name === 'find' || name === 'any' || name === 'all') {
      return { t: 'bound', name, arity: 1, recv, kind: 'higher' };
    }
    if (name === 'reduce') {
      return { t: 'bound', name, arity: 2, recv, kind: 'higher' };
    }
  }
  return null;
}

function doGetField(m, target, field) {
  if (isFailure(target)) {
    if (field === 'message') return vStr(target.msg);
    if (field === 'payload') return target.payload !== undefined ? target.payload : vNone();
    return target;
  }
  if (target.t === 'struct') {
    const f = target.fields.find(([k]) => k === field);
    if (f) return f[1];
    const sm = m.structMethods.get(`${target.type}.${field}`);
    if (sm) {
      return { t: 'bound', name: `${target.type}.${field}`, arity: Math.max(0, m.module.functions[sm.idx].params.length - 1), recv: target, kind: 'ufunc', idx: sm.idx, total: sm.total };
    }
    const b = bindMethod(m, target, field);
    if (b) return b;
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${target.type} has no field '${field}'`);
  }
  if (target.t === 'dict') {
    const found = dictGet(target, { t: 'str', v: field });
    if (found !== null) return found;
    const b = bindMethod(m, target, field);
    if (b) return b;
    fault('AIPO_RT_KEY_NOT_FOUND', `key ${field} not found`);
  }
  const b = bindMethod(m, target, field);
  if (b) return b;
  return typeMismatch('struct, collection, or type with that member', `${typeName(target)}.${field}`);
}

function convertViaType(tag, args) {
  if (tag === 'Set' && args.length === 0) return vSet([]);
  if (args.length !== 1) return typeMismatch('1 argument', `${args.length} arguments`);
  const a = args[0];
  switch (tag) {
    case 'Int': return convInt(a);
    case 'Float': return convFloat(a);
    case 'Byte': return convByte(a);
    case 'String': return convString(a);
    case 'Bytes': return convBytes(a);
    case 'Set': return convSet(a);
    case 'Duration': return convDuration(a);
    default: fault('AIPO_RT_NOT_CALLABLE', `${tag} (no conversion form in V1)`);
  }
}

function checkArity(got, want, what) {
  if (want === -1 || want === undefined) return;
  if (got !== want) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected ${want} arguments${what ? ` for ${what}` : ''}, got ${got}`);
}

function beginCall(m, argc) {
  if (m.stack.length < argc + 1) fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: stack underflow');
  const calleeIdx = m.stack.length - 1 - argc;
  const callee = m.stack[calleeIdx];
  if (isFailure(callee)) {
    const f = m.stack.splice(calleeIdx);
    mPush(m, f[0]);
    return;
  }
  const isFailureInspector = callee.t === 'native' && (callee.name === 'expect.failure' || callee.name === 'testing.failure');
  if (!isFailureInspector) {
    for (let i = 0; i < argc; i++) {
      if (isFailure(m.stack[calleeIdx + 1 + i])) {
        const f = m.stack[calleeIdx + 1 + i];
        m.stack.length = calleeIdx;
        mPush(m, f);
        return;
      }
    }
  }
  const args = m.stack.slice(calleeIdx + 1, calleeIdx + 1 + argc);
  const callFn = (idx, vars, cells) => {
    const fn = m.module.functions[idx];
    checkArity(argc, fn.params.length);
    const v = new Array(fn.localNames.length);
    fn.params.forEach((_, i) => { v[i] = args[i]; });
    m.frames.push({ fn, ip: 0, vars: v, cells, base: calleeIdx, journalStart: m.journal.length });
  };
  const isAsyncCallee = (idx) => {
    const fn = m.module.functions[idx];
    return !!(fn && fn.async);
  };
  if (callee.t === 'func' || callee.t === 'closure') {
    // Canon: calling an `async fn` never runs the body inline — the call spawns
    // the task eagerly and its result is the `Task` handle, awaited explicitly.
    if (isAsyncCallee(callee.idx)) {
      asyncCall(m, callee, args, calleeIdx);
      return;
    }
    callFn(callee.idx, {}, callee.t === 'closure' ? callee.cells : null);
    m.stack.length = calleeIdx + 1 + argc;
  } else if (callee.t === 'native') {
    checkArity(argc, callee.arity);
    if (callee.name && callee.name.startsWith('task.')) {
      taskCall(m, callee.name, calleeIdx, args);
      return;
    }
    if (callee.name === 'test' || callee.name === 'testing.test') {
      testCall(m, calleeIdx, args);
      return;
    }
    const r = callee.fn(args);
    m.stack.length = calleeIdx;
    mPush(m, r);
  } else if (callee.t === 'type') {
    const r = convertViaType(callee.name, args);
    m.stack.length = calleeIdx;
    mPush(m, r);
  } else if (callee.t === 'bound') {
    checkArity(argc, callee.arity);
    if (callee.kind === 'group') {
      groupMethod(m, callee.name, callee.recv, args, calleeIdx);
      return;
    }
    if (callee.kind === 'native') {
      checkMutationAllowed(callee.name, callee.recv, m.active);
      const r = callee.fn([callee.recv, ...args]);
      m.stack.length = calleeIdx;
      mPush(m, r);
    } else if (callee.kind === 'ufunc') {
      if (argc + 1 !== callee.total) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected ${callee.total - 1} arguments for ${callee.name}, got ${argc}`);
      if (isAsyncCallee(callee.idx)) {
        // An `async fn` method is an `async fn`: the receiver is argument 0 of the
        // spawned task body, matching the VM's method call protocol.
        asyncCall(m, { t: 'func', idx: callee.idx, arity: callee.total }, [callee.recv, ...args], calleeIdx);
        return;
      }
      m.stack[calleeIdx] = callee.recv;
      const fn = m.module.functions[callee.idx];
      const v = new Array(fn.localNames.length);
      v[0] = callee.recv;
      args.forEach((a, i) => { v[i + 1] = a; });
      m.frames.push({ fn, ip: 0, vars: v, cells: null, base: calleeIdx, journalStart: m.journal.length });
      m.stack.length = calleeIdx + 1 + argc;
    } else if (callee.kind === 'higher') {
      m.stack.length = calleeIdx;
      const r = higherOrder(m, callee.name, callee.recv, args);
      mPush(m, r);
    }
  } else {
    fault('AIPO_RT_NOT_CALLABLE', `${typeName(callee)} is not callable`);
  }
}

function evaluateSequence(m, seq) {
  let items = [...seq.source.items];
  for (const op of seq.ops) {
    if (op.op === 'map') {
      const out = [];
      for (const it of items) {
        if (m) {
          try { out.push(invokeSame(m, op.fn, [it])); }
          catch (e) { if (e instanceof AipoFault) throw e; out.push(vFail(e && e.uncaught ? e.uncaught : String(e))); }
        } else {
          out.push(it);
        }
      }
      items = out;
    } else if (op.op === 'filter') {
      const out = [];
      for (const it of items) {
        if (m) {
          let r;
          try { r = invokeSame(m, op.fn, [it]); }
          catch (e) { if (e instanceof AipoFault) throw e; throw new AipoFault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${e && e.uncaught ? e.uncaught : 'failure in predicate'}`); }
          if (r.t !== 'bool') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Bool predicate result, got ${typeName(r)}`);
          if (r.v) out.push(it);
        } else {
          out.push(it);
        }
      }
      items = out;
    } else if (op.op === 'flat_map') {
      const out = [];
      for (const it of items) {
        if (m) {
          let r;
          try { r = invokeSame(m, op.fn, [it]); }
          catch (e) { if (e instanceof AipoFault) throw e; out.push(vFail(e && e.uncaught ? e.uncaught : String(e))); continue; }
          if (r.t === 'list') {
            for (const sub of r.items) out.push(sub);
          } else {
            out.push(r);
          }
        } else {
          out.push(it);
        }
      }
      items = out;
    } else if (op.op === 'take') {
      const n = op.n.t === 'int' ? Math.max(0, Number(op.n.v)) : 0;
      items = items.slice(0, n);
    } else if (op.op === 'skip') {
      const n = op.n.t === 'int' ? Math.max(0, Number(op.n.v)) : 0;
      items = items.slice(n);
    } else if (op.op === 'distinct') {
      const seen = new Set();
      const out = [];
      for (const it of items) {
        const k = display(it);
        if (!seen.has(k)) {
          seen.add(k);
          out.push(it);
        }
      }
      items = out;
    } else if (op.op === 'enumerate') {
      items = items.map((it, i) => vList([vInt(i), it]));
    }
  }
  return items;
}

function iterableItems(m, recv) {
  if (recv.t === 'sequence') return evaluateSequence(m, recv);
  if (recv.t === 'list') return [...recv.items];
  if (recv.t === 'dict') return recv.entries.map(([, v]) => v);
  if (recv.t === 'set') return [...recv.items];
  if (recv.t === 'str') return chars(recv.v).map(c => vStr(c));
  if (recv.t === 'range') {
    const out = [];
    for (let i = recv.start; i < recv.end; i++) out.push(vInt(i));
    return out;
  }
  return typeMismatch('iterable List, Dict, Set, String, or Range', typeName(recv));
}

function invokeSame(m, callee, args) {
  const stackBase = m.stack.length;
  const frameBase = m.frames.length;
  const handlerBase = m.handlers.length;
  const journalBase = m.journal.length;
  mPush(m, callee);
  args.forEach(a => mPush(m, a));
  beginCall(m, args.length);
  const called = m.frames.length > frameBase;
  if (!called) {
    const r = mPop(m);
    m.stack.length = stackBase;
    m.handlers.length = handlerBase;
    m.journal.length = journalBase;
    if (isFailure(r)) throw { uncaught: r.msg };
    return r;
  }
  m.invokeDepth++;
  try {
    while (m.frames.length > frameBase) {
      stepFn(m);
      if (m.done) break;
    }
  } finally {
    m.invokeDepth--;
  }
  if (m.done) {
    const r = m.result;
    m.done = false;
    m.stack.length = stackBase;
    m.frames.length = frameBase;
    m.handlers.length = handlerBase;
    m.journal.length = journalBase;
    if (isFailure(r)) throw { uncaught: r.msg };
    return r;
  }
  const r = mPop(m);
  m.stack.length = stackBase;
  m.handlers.length = handlerBase;
  m.journal.length = journalBase;
  if (isFailure(r)) throw { uncaught: r.msg };
  return r;
}

function higherOrder(m, name, recv, args) {
  const items = iterableItems(m, recv);
  const guardId = (recv.t === 'list' || recv.t === 'dict' || recv.t === 'set') ? recv.id : null;
  if (guardId !== null) m.active.push(guardId);
  try {
    if (name === 'find') {
      const callable = args[0] !== undefined ? args[0] : vNone();
      for (const it of items) {
        let r;
        try { r = invokeSame(m, callable, [it]); }
        catch (e) { if (e instanceof AipoFault) throw e; throw new AipoFault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${e && e.uncaught ? e.uncaught : 'failure in predicate'}`); }
        if (r.t !== 'bool') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Bool predicate result, got ${typeName(r)}`);
        if (r.v) return it;
      }
      return vNone();
    }
    if (name === 'filter') {
      const callable = args[0] !== undefined ? args[0] : vNone();
      const kept = [];
      for (const it of items) {
        let r;
        try { r = invokeSame(m, callable, [it]); }
        catch (e) { if (e instanceof AipoFault) throw e; throw new AipoFault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${e && e.uncaught ? e.uncaught : 'failure in predicate'}`); }
        if (r.t !== 'bool') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Bool predicate result, got ${typeName(r)}`);
        if (r.v) kept.push(it);
      }
      return vList(kept);
    }
    if (name === 'transform' || name === 'map') {
      const callable = args[0] !== undefined ? args[0] : vNone();
      const out = [];
      for (const it of items) {
        try { out.push(invokeSame(m, callable, [it])); }
        catch (e) { if (e instanceof AipoFault) throw e; out.push(vFail(e.uncaught)); }
      }
      return vList(out);
    }
    if (name === 'any') {
      const callable = args[0] !== undefined ? args[0] : vNone();
      for (const it of items) {
        let r;
        try { r = invokeSame(m, callable, [it]); }
        catch (e) { if (e instanceof AipoFault) throw e; throw new AipoFault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${e && e.uncaught ? e.uncaught : 'failure in predicate'}`); }
        if (r.t !== 'bool') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Bool predicate result, got ${typeName(r)}`);
        if (r.v) return vBool(true);
      }
      return vBool(false);
    }
    if (name === 'all') {
      const callable = args[0] !== undefined ? args[0] : vNone();
      for (const it of items) {
        let r;
        try { r = invokeSame(m, callable, [it]); }
        catch (e) { if (e instanceof AipoFault) throw e; throw new AipoFault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${e && e.uncaught ? e.uncaught : 'failure in predicate'}`); }
        if (r.t !== 'bool') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Bool predicate result, got ${typeName(r)}`);
        if (!r.v) return vBool(false);
      }
      return vBool(true);
    }
    if (name === 'flat_map') {
      const callable = args[0] !== undefined ? args[0] : vNone();
      const flat = [];
      for (const it of items) {
        let sub;
        try { sub = invokeSame(m, callable, [it]); }
        catch (e) { if (e instanceof AipoFault) throw e; throw new AipoFault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${e && e.uncaught ? e.uncaught : 'failure in flat_map'}`); }
        if (sub.t !== 'list') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected List result from flat_map callback, got ${typeName(sub)}`);
        flat.push(...sub.items);
      }
      return vList(flat);
    }
    if (name === 'reduce') {
      if (args.length < 2) {
        fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected 2 arguments for reduce: initial, callable, got ${args.length} arguments`);
      }
      let acc = args[0];
      const callable = args[1];
      if (isFailure(acc)) return acc;
      for (const it of items) {
        try {
          acc = invokeSame(m, callable, [acc, it]);
        } catch (e) {
          if (e instanceof AipoFault) throw e;
          acc = vFail(e && e.uncaught ? e.uncaught : 'failure in reduce');
        }
        if (isFailure(acc)) break;
      }
      return acc;
    }
    if (name === 'sort_by') {
      const callable = args[0] !== undefined ? args[0] : vNone();
      const keyed = [];
      for (const it of items) {
        let k;
        try { k = invokeSame(m, callable, [it]); }
        catch (e) { if (e instanceof AipoFault) throw e; throw new AipoFault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: failure in sort key'); }
        keyed.push([k, it]);
      }
      keyed.sort((a, b) => sortKeyCompare(a[0], b[0]));
      return vList(keyed.map(([, it]) => it));
    }
    return typeMismatch('collection method', name);
  } finally {
    if (guardId !== null) {
      const at = m.active.lastIndexOf(guardId);
      if (at >= 0) m.active.splice(at, 1);
    }
  }
}

function sortKeyCompare(a, b) {
  const an = numKey(a), bn = numKey(b);
  if (an !== null && bn !== null) return an - bn;
  if (an !== null || bn !== null) throw new AipoFault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected comparable sort keys');
  if (a.t === 'str' && b.t === 'str') return a.v < b.v ? -1 : a.v > b.v ? 1 : 0;
  if (a.t === 'bool' && b.t === 'bool') return a.v === b.v ? 0 : !a.v ? -1 : 1;
  throw new AipoFault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: expected comparable sort keys');
}

function tagMatches(tag, v) {
  switch (tag) {
    case 'none': return v.t === 'none';
    case 'Bool': return v.t === 'bool';
    case 'Int': return v.t === 'int';
    case 'Float': return v.t === 'float';
    case 'Byte': return v.t === 'byte';
    case 'String': return v.t === 'str';
    case 'List': return v.t === 'list';
    case 'Dict': return v.t === 'dict';
    case 'Bytes': return v.t === 'bytes';
    case 'Range': return v.t === 'range';
    case 'Set': return v.t === 'set';
    case 'Duration': return v.t === 'duration';
    case 'Sequence': return v.t === 'sequence';
    case 'Task': return v.t === 'task';
    case 'Group': return v.t === 'group';
    default:
      if (v.t === 'struct') {
        return v.type === tag || v.type.startsWith(tag + '.');
      }
      return false;
  }
}

function operationArity(m, value, name) {
  if (value.t === 'struct') {
    const sm = m.structMethods.get(`${value.type}.${name}`);
    if (sm) return Math.max(0, m.module.functions[sm.idx].params.length - 1);
  }
  const b = bindMethod(m, value, name);
  return b ? b.arity : null;
}

function assertContract(m, type, nullable, pos, ops, value) {
  if (isUnset(value) || isFailure(value)) return;
  if (value.t === 'none') {
    if (!nullable) fault('AIPO_RT_TYPE_MISMATCH', `contract violation at ${pos}: expected ${type}, got none`);
    return;
  }
  if (type === 'Function') {
    if (!(value.t === 'func' || value.t === 'closure' || value.t === 'native' || value.t === 'bound')) {
      fault('AIPO_RT_TYPE_MISMATCH', `contract violation at ${pos}: expected Function, got ${typeName(value)}`);
    }
  } else if (['none', 'Bool', 'Int', 'Float', 'Byte', 'String', 'List', 'Dict', 'Bytes', 'Range', 'Set', 'Duration', 'Sequence', 'Task'].includes(type)) {
    if (!tagMatches(type, value)) {
      fault('AIPO_RT_TYPE_MISMATCH', `contract violation at ${pos}: expected ${nullable ? `${type}?` : type}, got ${structDisplayName(value)}`);
    }
  } else if (getStructDef(type)) {
    if (!(value.t === 'struct' && value.type === type)) {
      fault('AIPO_RT_TYPE_MISMATCH', `contract violation at ${pos}: expected ${nullable ? `${type}?` : type}, got ${structDisplayName(value)}`);
    }
  }
  if (ops && ops.length && value.t !== 'none') {
    for (const [opName, opArity] of ops) {
      const found = operationArity(m, value, opName);
      if (found === null) {
        fault('AIPO_RT_TYPE_MISMATCH', `contract violation at ${pos}: expected ${type}.${opName}/${opArity}, got ${structDisplayName(value)} without '${opName}'`);
      } else if (found !== opArity) {
        fault('AIPO_RT_TYPE_MISMATCH', `contract violation at ${pos}: expected ${type}.${opName}/${opArity}, got ${structDisplayName(value)}.${opName}/${found}`);
      }
    }
  }
}

function handleFailure(m, failVal) {
  if (m.handlers.length) {
    const h = m.handlers.pop();
    m.frames.length = h.frameDepth;
    m.stack.length = h.stackDepth;
    // Match the Rust VM exactly: recovery releases every provisional mutation journaled
    // by frames that did not survive, including entries the surviving frame journaled
    // before the handler was registered. Keeping them would roll back mutations that
    // canon already committed past the handler boundary.
    const fr = curFrame(m);
    m.journal.length = fr ? fr.journalStart : 0;
    mPush(m, failVal);
    if (fr) fr.ip = h.ip;
    return;
  }
  if (m.frames.length) {
    const fr = m.frames.pop();
    m.stack.length = fr.base;
    m.journal.length = fr.journalStart;
    if (m.frames.length === 0 && m.current !== null && m.current !== 0) {
      m.stack.length = 0;
      completeCurrent(m, { t: 'failed', v: failVal });
      return;
    }
    mPush(m, failVal);
    return;
  }
  if (m.current !== null && m.current !== 0) {
    m.stack.length = 0;
    completeCurrent(m, { t: 'failed', v: failVal });
    return;
  }
  m.halted = failVal;
  m.done = true;
  completeCurrent(m, { t: 'failed', v: failVal });
}

function checkMutations(m) {
  const fr = curFrame(m);
  const start = fr ? fr.journalStart : 0;
  if (m.journal.length <= start) return;
  const involved = [];
  const seen = new Set();
  for (let i = start; i < m.journal.length; i++) {
    const e = m.journal[i];
    if (!seen.has(e.inst.id)) { seen.add(e.inst.id); involved.push(e.inst); }
  }
  let failedType = null;
  for (const inst of involved) {
    const idx = __invariantEntries ? __invariantEntries.get(inst.type) : undefined;
    if (idx === undefined) continue;
    let r;
    try {
      r = invokeSame(m, { t: 'func', idx, arity: 1 }, [inst]);
    } catch (e) {
      if (e instanceof AipoFault) throw e;
      failedType = inst.type;
      break;
    }
    if (!(r.t === 'bool' && r.v === true)) { failedType = inst.type; break; }
  }
  if (failedType) {
    for (let i = m.journal.length - 1; i >= start; i--) {
      const e = m.journal[i];
      const at = e.inst.fields.findIndex(([k]) => k === e.field);
      if (at >= 0) e.inst.fields[at][1] = e.prev;
    }
    m.journal.length = start;
    handleFailure(m, vFail(`invariant() of ${failedType} did not hold after mutation`));
  } else {
    m.journal.length = start;
  }
}

// ---- cooperative scheduler & async combinators ----
const SUSPENDED = Symbol('suspended');

function suspendCurrent(m, status) {
  const id = m.current !== null ? m.current : 0;
  const existing = m.tasks.get(id);
  if (existing && existing.status && existing.status.t === 'cancelled') {
    fault('AIPO_RT_CANCELLED', `task ${id} was cancelled`);
  }
  const sleepingUntil = status.t === 'sleeping' ? status.until : null;
  const state = {
    status,
    stack: m.stack,
    frames: m.frames,
    handlers: m.handlers,
    journal: m.journal,
    active: m.active,
    result: null,
    sleepingUntil,
  };
  m.tasks.set(id, state);
  m.stack = [];
  m.frames = [];
  m.handlers = [];
  m.journal = [];
  m.active = [];
  if (status.t === 'sleeping' && !m.runQueue.includes(id)) {
    m.runQueue.push(id);
  }
  m.current = null;
  throw SUSPENDED;
}

function suspendAtCall(m, status) {
  const fr = curFrame(m);
  if (fr) {
    fr.ip--;
  }
  suspendCurrent(m, status);
}

function loadTask(m, id) {
  const st = m.tasks.get(id) || {
    status: { t: 'pending' },
    stack: [], frames: [], handlers: [], journal: [], active: [],
    result: null, sleepingUntil: null,
  };
  m.tasks.delete(id);
  m.stack = st.stack;
  m.frames = st.frames;
  m.handlers = st.handlers;
  m.journal = st.journal;
  m.active = st.active;
  st.stack = [];
  st.frames = [];
  st.handlers = [];
  st.journal = [];
  st.active = [];
  st.status = { t: 'running' };
  m.tasks.set(id, st);
  m.current = id;
}

function completeCurrent(m, outcome) {
  const id = m.current !== null ? m.current : 0;
  m.current = null;
  const existing = m.tasks.get(id);
  if (existing && existing.status && existing.status.t === 'cancelled') {
    outcome = { t: 'cancelled' };
  }
  if (id === 0) {
    m.mainOutcome = outcome;
    m.tasks.delete(0);
    return true;
  }
  finishTask(m, id, outcome);
  return false;
}

function finishTask(m, id, outcome) {
  const st = m.tasks.get(id);
  if (st) {
    if (outcome.t === 'cancelled') {
      st.status = { t: 'cancelled' };
    } else if (outcome.t === 'ready') {
      st.status = { t: 'ready' };
      st.result = outcome.v;
    } else if (outcome.t === 'failed') {
      st.status = { t: 'failed' };
      st.result = outcome.v;
    }
  }
  const ws = m.waiters.get(id);
  if (ws) {
    m.waiters.delete(id);
    for (const waiter of ws) {
      wakeWaiter(m, waiter);
    }
  }
  const affected = [];
  for (const [joinId, join] of m.joins) {
    if (join.members.includes(id)) {
      affected.push(joinId);
    }
  }
  for (const joinId of affected) {
    const join = m.joins.get(joinId);
    if (join && !join.completed.includes(id)) {
      join.completed.push(id);
    }
    pollJoin(m, joinId);
  }
}

function wakeWaiter(m, waiter) {
  const st = m.tasks.get(waiter);
  if (st) {
    if (st.status.t === 'blocked') {
      st.status = { t: 'pending' };
    } else {
      return;
    }
  } else {
    return;
  }
  if (!m.runQueue.includes(waiter)) {
    m.runQueue.push(waiter);
  }
}

function selectNext(m) {
  if (m.runQueue.length === 0 && !m.tasks.has(0)) {
    return;
  }
  while (true) {
    pollTimeouts(m);
    m.runQueue = m.runQueue.filter(id => {
      const st = m.tasks.get(id);
      if (!st || !st.status) return false;
      const t = st.status.t;
      return t === 'pending' || t === 'blocked' || t === 'running' || t === 'sleeping';
    });
    let pick = null;
    for (const id of m.runQueue) {
      const st = m.tasks.get(id);
      if (!st || !st.status) continue;
      const t = st.status.t;
      if (t === 'pending' || t === 'blocked' || t === 'running') {
        pick = id;
        break;
      }
      if (t === 'sleeping' && st.status.until <= m.tick) {
        pick = id;
        break;
      }
    }
    if (pick !== null) {
      const idx = m.runQueue.indexOf(pick);
      if (idx >= 0) m.runQueue.splice(idx, 1);
      const st = m.tasks.get(pick);
      if (st && st.status && st.status.t === 'cancelled') {
        continue;
      }
      loadTask(m, pick);
      return;
    }
    let nextWake = null;
    for (const st of m.tasks.values()) {
      if (st.status && st.status.t === 'sleeping' && st.status.until > m.tick) {
        if (nextWake === null || st.status.until < nextWake) {
          nextWake = st.status.until;
        }
      }
    }
    let nextTimeout = null;
    for (const join of m.joins.values()) {
      if (!join.done && join.deadline !== null && join.deadline > m.tick) {
        if (nextTimeout === null || join.deadline < nextTimeout) {
          nextTimeout = join.deadline;
        }
      }
    }
    let targetTick = null;
    if (nextWake !== null && nextTimeout !== null) {
      targetTick = Math.min(nextWake, nextTimeout);
    } else if (nextWake !== null) {
      targetTick = nextWake;
    } else if (nextTimeout !== null) {
      targetTick = nextTimeout;
    }
    if (targetTick !== null) {
      m.tick = targetTick;
    } else {
      let live = false;
      for (const st of m.tasks.values()) {
        if (st.status) {
          const t = st.status.t;
          if (t === 'pending' || t === 'sleeping' || t === 'blocked' || t === 'running') {
            live = true;
            break;
          }
        }
      }
      if (live) {
        fault('AIPO_RT_TYPE_MISMATCH', 'scheduler deadlock: live tasks with nothing runnable');
      }
      return;
    }
  }
}

function pollTimeouts(m) {
  const expired = [];
  for (const [joinId, join] of m.joins) {
    if (!join.done && join.kind === 'timeout' && join.deadline !== null && m.tick > join.deadline) {
      expired.push(joinId);
    }
  }
  for (const joinId of expired) {
    const join = m.joins.get(joinId);
    if (join) {
      join.done = true;
      join.outcome = vFail('timeout');
      const member = join.members[0];
      if (member !== undefined) {
        finishTask(m, member, { t: 'cancelled' });
      }
      wakeJoin(m, joinId);
    }
  }
}

function pollJoin(m, joinId) {
  const join = m.joins.get(joinId);
  if (!join || join.done) return;

  const terminal = id => {
    const st = m.tasks.get(id);
    if (!st || !st.status) return false;
    const t = st.status.t;
    return t === 'ready' || t === 'failed' || t === 'cancelled';
  };

  let resolution = { t: 'pending' };

  if (join.kind === 'all') {
    const anyCancelled = join.members.some(id => {
      const r = taskResult(m, id);
      return r && r.t === 'cancelled';
    });
    if (anyCancelled) {
      resolution = { t: 'fault', code: 'AIPO_RT_CANCELLED', message: 'a joined task was cancelled' };
    } else if (join.members.every(terminal)) {
      const order = completionOrder(m, joinId);
      let firstFailure = null;
      for (const member of order) {
        const r = taskResult(m, member);
        if (r && r.t === 'failed') {
          firstFailure = r.v;
          break;
        }
      }
      if (firstFailure) {
        resolution = { t: 'value', v: firstFailure };
      } else {
        const values = join.order.map(member => {
          const r = taskResult(m, member);
          return r ? r.v : vNone();
        });
        resolution = { t: 'value', v: vList(values) };
      }
    }
  } else if (join.kind === 'race') {
    const order = completionOrder(m, joinId);
    let winner = null;
    for (const member of order) {
      const r = taskResult(m, member);
      if (r) {
        winner = r;
        break;
      }
    }
    if (winner) {
      if (winner.t === 'cancelled') {
        resolution = { t: 'fault', code: 'AIPO_RT_CANCELLED', message: 'the winning task was cancelled' };
      } else {
        resolution = { t: 'value', v: winner.v };
      }
    }
  } else if (join.kind === 'timeout') {
    const member = join.members[0];
    const r = member !== undefined ? taskResult(m, member) : null;
    if (r) {
      if (r.t === 'cancelled') {
        resolution = { t: 'fault', code: 'AIPO_RT_CANCELLED', message: 'the timed task was cancelled' };
      } else {
        resolution = { t: 'value', v: r.v };
      }
    }
  } else if (join.kind === 'group_wait') {
    const allTerminal = join.members.every(terminal);
    const anyCancelled = join.members.some(id => {
      const r = taskResult(m, id);
      return r && r.t === 'cancelled';
    });
    if (anyCancelled) {
      resolution = { t: 'fault', code: 'AIPO_RT_CANCELLED', message: 'a group member was cancelled' };
    } else if (allTerminal) {
      const order = completionOrder(m, joinId);
      const values = order.map(member => {
        const r = taskResult(m, member);
        return r ? r.v : vNone();
      });
      resolution = { t: 'value', v: vList(values) };
    }
  }

  if (resolution.t === 'value') {
    join.done = true;
    join.outcome = resolution.v;
    wakeJoin(m, joinId);
  } else if (resolution.t === 'fault') {
    join.done = true;
    join.fault = { code: resolution.code, message: resolution.message };
    wakeJoin(m, joinId);
  }
}

function wakeJoin(m, joinId) {
  const join = m.joins.get(joinId);
  if (!join) return;
  for (const waiter of join.waiters) {
    wakeWaiter(m, waiter);
  }
}

function completionOrder(m, joinId) {
  const join = m.joins.get(joinId);
  if (!join) return [];
  const ordered = join.completed.filter(id => join.members.includes(id));
  const missing = join.members.filter(id => !ordered.includes(id));
  return [...missing, ...ordered];
}

function taskResult(m, id) {
  const st = m.tasks.get(id);
  if (!st) return null;
  if (st.status.t === 'ready') return { t: 'ready', v: st.result };
  if (st.status.t === 'failed') return { t: 'failed', v: st.result };
  if (st.status.t === 'cancelled') return { t: 'cancelled' };
  return null;
}

function blocksOn(m, from, target) {
  let cursor = from;
  let visited = 0;
  const max = m.tasks.size + 1;
  while (visited <= max) {
    visited++;
    const st = m.tasks.get(cursor);
    if (st && st.status && st.status.t === 'blocked' && st.status.target.t === 'task') {
      const next = st.status.target.id;
      if (next === target) return true;
      cursor = next;
    } else {
      return false;
    }
  }
  return false;
}

function awaitChain(m, me, id) {
  const chain = [me, id];
  let cursor = id;
  let visited = 0;
  const max = m.tasks.size;
  while (visited <= max) {
    visited++;
    const st = m.tasks.get(cursor);
    if (st && st.status && st.status.t === 'blocked' && st.status.target.t === 'task') {
      const next = st.status.target.id;
      chain.push(next);
      if (next === me) break;
      cursor = next;
    } else {
      break;
    }
  }
  return chain;
}

function spawnTask(m, callee, args, group) {
  if (callee.t !== 'func' && callee.t !== 'closure') {
    fault('AIPO_RT_NOT_CALLABLE', `${typeName(callee)} is not callable`);
  }
  const fn = m.module.functions[callee.idx];
  checkArity(args.length, fn.params.length);
  const id = m.nextTask++;
  const v = new Array(fn.localNames.length);
  fn.params.forEach((_, i) => { v[i] = args[i]; });
  const frame = { fn, ip: 0, vars: v, cells: callee.cells || null, base: 0, journalStart: 0 };
  const state = {
    status: { t: 'pending' },
    stack: [callee, ...args],
    frames: [frame],
    handlers: [],
    journal: [],
    active: [],
    result: null,
    sleepingUntil: null,
  };
  m.tasks.set(id, state);
  m.runQueue.push(id);
  if (group !== null && group !== undefined) {
    const grp = m.groups.get(group);
    if (grp) grp.members.push(id);
    for (const [, join] of m.joins) {
      if (!join.done && join.kind === 'group_wait' && join.group === group) {
        if (!join.members.includes(id)) {
          join.members.push(id);
          join.order.push(id);
        }
      }
    }
  }
  return id;
}

function resolveCall(m, calleeIdx, value) {
  m.stack.length = calleeIdx;
  mPush(m, value);
}

// Spawns `callee(args)` as a task and leaves its `Task` handle as the call result.
function asyncCall(m, callee, args, calleeIdx) {
  const id = spawnTask(m, callee, args, null);
  resolveCall(m, calleeIdx, vTask(id));
}

let testMode = 'disabled';
let testTarget = '';
let testDiscovered = [];
let testRan = false;

export function setTestMode(mode, target = '') {
  testMode = mode;
  testTarget = target;
  testDiscovered = [];
  testRan = false;
}

export function getTestDiscovered() {
  return testDiscovered;
}

export function didTestRun() {
  return testRan;
}

function testCall(m, calleeIdx, args) {
  if (args.length !== 2) {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected 2 arguments for test, got ${args.length}`);
  }
  const nameVal = args[0];
  if (nameVal.t !== 'str') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected String test name, got ${typeName(nameVal)}`);
  }
  const name = nameVal.v;
  const cb = args[1];
  if (cb.t !== 'func' && cb.t !== 'closure' && cb.t !== 'native' && cb.t !== 'bound') {
    fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected callable test body, got ${typeName(cb)}`);
  }

  if (testMode === 'discover') {
    testDiscovered.push(name);
    resolveCall(m, calleeIdx, vNone());
    return;
  }
  if (testMode === 'execute') {
    if (name === testTarget) {
      testRan = true;
      m.stack.length = calleeIdx;
      mPush(m, cb);
      beginCall(m, 0, calleeIdx);
    } else {
      resolveCall(m, calleeIdx, vNone());
    }
    return;
  }
  // Disabled (normal run): invoke inline
  m.stack.length = calleeIdx;
  mPush(m, cb);
  beginCall(m, 0, calleeIdx);
}

function taskCall(m, name, calleeIdx, args) {
  for (const arg of args) {
    if (isFailure(arg)) {
      resolveCall(m, calleeIdx, arg);
      return;
    }
  }
  switch (name) {
    case 'task.spawn': {
      const items = taskListArg(args[1], 'task.spawn(f, args)');
      const id = spawnTask(m, args[0], items, null);
      resolveCall(m, calleeIdx, vTask(id));
      break;
    }
    case 'task.sleep':
      doSleep(m, calleeIdx, args[0]);
      break;
    case 'task.all': {
      const members = taskMembers(args[0], 'task.all');
      if (members.failure) {
        resolveCall(m, calleeIdx, members.failure);
      } else {
        doJoin(m, 'all', members.tasks, null, calleeIdx, 'task.all');
      }
      break;
    }
    case 'task.race': {
      const members = taskMembers(args[0], 'task.race');
      if (members.failure) {
        resolveCall(m, calleeIdx, members.failure);
      } else {
        doJoin(m, 'race', members.tasks, null, calleeIdx, 'task.race');
      }
      break;
    }
    case 'task.timeout': {
      if (args[0].t !== 'task') typeMismatch('Task for task.timeout', typeName(args[0]));
      const ticks = sleepTicks(args[1], 'task.timeout');
      if (ticks.failure) {
        resolveCall(m, calleeIdx, ticks.failure);
      } else {
        const deadline = m.tick + ticks.ticks;
        doJoin(m, 'timeout', [args[0].id], deadline, calleeIdx, 'task.timeout');
      }
      break;
    }
    case 'task.cancel': {
      if (args[0].t !== 'task') typeMismatch('Task for task.cancel', typeName(args[0]));
      const id = args[0].id;
      const state = m.tasks.get(id);
      if (!state) typeMismatch('live task for task.cancel', `unknown task ${id}`);
      if (state.status.t !== 'ready' && state.status.t !== 'failed' && state.status.t !== 'cancelled') {
        finishTask(m, id, { t: 'cancelled' });
      }
      resolveCall(m, calleeIdx, vNone());
      break;
    }
    case 'task.group': {
      const id = m.nextGroup++;
      m.groups.set(id, { members: [] });
      resolveCall(m, calleeIdx, vGroup(id));
      break;
    }
    default:
      fault('AIPO_RT_TYPE_MISMATCH', `unknown task function ${name}`);
  }
}

function doSleep(m, calleeIdx, arg) {
  if (m.invokeDepth > 0) {
    fault('AIPO_RT_AWAIT_IN_CALLBACK', 'sleep is not allowed inside a synchronous host callback');
  }
  const me = m.current !== null ? m.current : 0;
  const meState = m.tasks.get(me);
  if (meState && meState.sleepingUntil !== null && meState.sleepingUntil !== undefined) {
    const until = meState.sleepingUntil;
    meState.sleepingUntil = null;
    if (until <= m.tick) {
      resolveCall(m, calleeIdx, vNone());
      return;
    }
    meState.sleepingUntil = until;
    suspendAtCall(m, { t: 'sleeping', until });
    return;
  }
  const ticks = sleepTicks(arg, 'task.sleep');
  if (ticks.failure) {
    resolveCall(m, calleeIdx, ticks.failure);
    return;
  }
  const until = m.tick + ticks.ticks;
  suspendAtCall(m, { t: 'sleeping', until });
}

function doJoin(m, kind, members, deadline, calleeIdx, op) {
  if (m.invokeDepth > 0) {
    fault('AIPO_RT_AWAIT_IN_CALLBACK', `${op} is not allowed inside a synchronous host callback`);
  }
  if (members.length === 0) {
    let empty = vNone();
    if (kind === 'race') empty = vFail('race of no tasks');
    else if (kind === 'all' || kind === 'group_wait') empty = vList([]);
    else if (kind === 'timeout') empty = vFail('timeout of no task');
    resolveCall(m, calleeIdx, empty);
    return;
  }
  const me = m.current !== null ? m.current : 0;
  let joinId = joinFor(m, kind, members);
  if (joinId === null) joinId = pendingJoin(m, kind);
  if (joinId === null) {
    joinId = m.nextJoin++;
    m.joins.set(joinId, {
      kind,
      order: [...members],
      members: [...members],
      completed: [],
      waiters: [me],
      deadline,
      group: null,
      done: false,
      outcome: null,
      fault: null,
    });
  }
  pollJoin(m, joinId);
  const join = m.joins.get(joinId);
  if (join.done) {
    const { fault: f, outcome } = join;
    m.joins.delete(joinId);
    if (f) fault(f.code, f.message);
    resolveCall(m, calleeIdx, outcome !== null ? outcome : vNone());
    return;
  }
  suspendAtCall(m, { t: 'blocked', target: { t: 'join', id: joinId } });
}

function pendingJoin(m, kind) {
  const me = m.current !== null ? m.current : 0;
  for (const [id, join] of m.joins) {
    if (join.kind === kind && join.waiters.includes(me)) {
      return id;
    }
  }
  return null;
}

function joinFor(m, kind, members) {
  const me = m.current !== null ? m.current : 0;
  for (const [id, join] of m.joins) {
    if (join.kind === kind && join.waiters.includes(me) &&
        join.members.length === members.length &&
        join.members.every((v, i) => v === members[i])) {
      return id;
    }
  }
  return null;
}

function groupMethod(m, name, recv, args, calleeIdx) {
  if (recv.t !== 'group') typeMismatch('Group receiver', typeName(recv));
  const groupId = recv.id;
  if (!m.groups.has(groupId)) typeMismatch('live group', `unknown group ${groupId}`);
  for (const arg of args) {
    if (isFailure(arg)) {
      resolveCall(m, calleeIdx, arg);
      return;
    }
  }
  if (name === 'spawn') {
    const items = taskListArg(args[1], 'group.spawn(f, args)');
    const id = spawnTask(m, args[0], items, groupId);
    resolveCall(m, calleeIdx, vTask(id));
  } else if (name === 'wait') {
    doGroupWait(m, groupId, calleeIdx);
  } else {
    typeMismatch('spawn or wait', `unknown group method ${name}`);
  }
}

function doGroupWait(m, groupId, calleeIdx) {
  if (m.invokeDepth > 0) {
    fault('AIPO_RT_AWAIT_IN_CALLBACK', 'group.wait is not allowed inside a synchronous host callback');
  }
  const me = m.current !== null ? m.current : 0;
  let joinId = null;
  for (const [id, join] of m.joins) {
    if (join.kind === 'group_wait' && join.group === groupId && join.waiters.includes(me)) {
      joinId = id;
      break;
    }
  }
  if (joinId === null) {
    const group = m.groups.get(groupId);
    const members = group ? [...group.members] : [];
    const allTerminal = members.every(id => {
      const r = taskResult(m, id);
      return r && (r.t === 'ready' || r.t === 'failed');
    });
    if (allTerminal && members.length > 0) {
      const anyCancelled = members.some(id => {
        const r = taskResult(m, id);
        return r && r.t === 'cancelled';
      });
      if (anyCancelled) {
        fault('AIPO_RT_CANCELLED', 'a group member was cancelled');
      }
      const values = members.map(id => {
        const r = taskResult(m, id);
        return r ? r.v : vNone();
      });
      resolveCall(m, calleeIdx, vList(values));
      return;
    }
    joinId = m.nextJoin++;
    m.joins.set(joinId, {
      kind: 'group_wait',
      order: [...members],
      members: [...members],
      completed: [],
      waiters: [me],
      deadline: null,
      group: groupId,
      done: false,
      outcome: null,
      fault: null,
    });
  }
  pollJoin(m, joinId);
  const join = m.joins.get(joinId);
  if (join.done) {
    if (join.fault) {
      fault(join.fault.code, join.fault.message);
    }
    const outcome = join.outcome !== null ? join.outcome : vNone();
    resolveCall(m, calleeIdx, outcome);
    return;
  }
  suspendAtCall(m, { t: 'blocked', target: { t: 'join', id: joinId } });
}

function sleepTicks(arg, op) {
  if (arg.t === 'int') {
    if (arg.v >= 0) return { ticks: arg.v };
    return { failure: vFail(`${op} amount must be >= 0`) };
  }
  if (arg.t === 'byte') {
    return { ticks: arg.v };
  }
  if (arg.t === 'duration') {
    if (Number.isFinite(arg.v) && arg.v >= 0) {
      return { ticks: Math.trunc(arg.v * 1000) };
    }
    return { failure: vFail(`${op} duration must be finite and >= 0`) };
  }
  typeMismatch('Int ticks or Duration', typeName(arg));
}

function taskListArg(arg, op) {
  if (arg.t === 'list') {
    return [...arg.items];
  }
  typeMismatch(`argument list for ${op}`, typeName(arg));
}

function taskMembers(arg, op) {
  if (arg.t !== 'list') {
    typeMismatch(`task list for ${op}`, typeName(arg));
  }
  const tasks = [];
  for (const item of arg.items) {
    if (item.t === 'task') {
      tasks.push(item.id);
    } else if (isFailure(item)) {
      return { failure: item };
    } else {
      typeMismatch(`Task members for ${op}`, typeName(item));
    }
  }
  return { tasks };
}

// ---- instruction dispatch ----
function binOp(m, op, a, b) {
  switch (op) {
    case 'Add': return valAdd(a, b);
    case 'Sub': return valSub(a, b);
    case 'Mul': return valMul(a, b);
    case 'Div': return valDiv(a, b);
    case 'IntDiv': return valIntDiv(a, b);
    case 'Mod': return valMod(a, b);
    case 'Equal': return valEqual(a, b);
    case 'NotEqual': return valNotEqual(a, b);
    case 'Less': return valLess(a, b);
    case 'LessEqual': return valLessEqual(a, b);
    case 'Greater': return valGreater(a, b);
    case 'GreaterEqual': return valGreaterEqual(a, b);
    case 'Is': {
      if (isFailure(a)) return a;
      if (isFailure(b)) return b;
      if (b.t !== 'type') return typeMismatch('type value on the right of is', typeName(b));
      return vBool(tagMatches(b.name, a));
    }
    case 'And': {
      if (isFailure(a)) return a;
      if (isFailure(b)) return b;
      if (a.t !== 'bool' || b.t !== 'bool') return typeMismatch('Bool', `${typeName(a)} and ${typeName(b)}`);
      return vBool(a.v && b.v);
    }
    case 'Or': {
      if (isFailure(a)) return a;
      if (isFailure(b)) return b;
      if (a.t !== 'bool' || b.t !== 'bool') return typeMismatch('Bool', `${typeName(a)} and ${typeName(b)}`);
      return vBool(a.v || b.v);
    }
    case 'OrElse': {
      if (isFailure(a)) return b;
      return a;
    }
    case 'Pipeline': {
      mPush(m, b);
      mPush(m, a);
      beginCall(m, 1);
      return null;
    }
    case 'Range': {
      if (isFailure(a)) return a;
      if (isFailure(b)) return b;
      const x = widen(a), y = widen(b);
      if (x.t !== 'int' || y.t !== 'int') return typeMismatch('Int range bounds', `${typeName(a)} and ${typeName(b)}`);
      return vRange(x.v, y.v);
    }
    default: fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: unknown operator ${op}`);
  }
}

function stepFn(m) {
  const fr = curFrame(m);
  if (!fr) {
    const r = m.stack.length ? m.stack[m.stack.length - 1] : vNone();
    m.stack.length = 0;
    completeCurrent(m, isFailure(r) ? { t: 'failed', v: r } : { t: 'ready', v: r });
    return;
  }
  const fn = fr.fn;
  if (fr.ip >= fn.code.length) {
    doReturn(m, vNone());
    return;
  }
  const inst = fn.code[fr.ip++];
  const op = inst.op;
  switch (op) {
    case 'Constant': {
      if (inst.const === undefined) inst.const = constToValue(inst.value);
      mPush(m, inst.const);
      break;
    }
    case 'Load': {
      if (inst.local) {
        const value = fr.vars[inst.slot];
        mPush(m, value === undefined ? vUnset() : value);
      } else {
        const n = inst.name;
        if (inst.upvalue) {
          if (fr.cells && Object.hasOwn(fr.cells, n)) mPush(m, fr.cells[n].v);
          else fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: stack underflow');
        } else if (m.globals.has(n)) mPush(m, m.globals.get(n));
        else fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: undefined global '${n}'`);
      }
      break;
    }
    case 'Store': {
      const n = inst.name;
      const v = mPop(m);
      if (inst.local) fr.vars[inst.slot] = v;
      else if (inst.upvalue) {
        if (!fr.cells || !Object.hasOwn(fr.cells, n)) fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: stack underflow');
        fr.cells[n].v = v;
      } else m.globals.set(n, v);
      break;
    }
    case 'GetUpvalue': {
      const n = fn.upvalues[inst.slot];
      if (!fr.cells || !Object.hasOwn(fr.cells, n)) fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: stack underflow');
      mPush(m, fr.cells[n].v);
      break;
    }
    case 'SetUpvalue': {
      const n = fn.upvalues[inst.slot];
      if (!fr.cells || !Object.hasOwn(fr.cells, n)) fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: stack underflow');
      fr.cells[n].v = mPop(m);
      break;
    }
    case 'Binary': {
      const b = mPop(m);
      const a = mPop(m);
      const r = binOp(m, inst.op2, a, b);
      if (r !== null && r !== undefined) mPush(m, r);
      break;
    }
    case 'Unary': {
      const a = mPop(m);
      if (inst.op2 === 'Neg') mPush(m, valNeg(a));
      else if (inst.op2 === 'Pos') mPush(m, valPos(a));
      else if (inst.op2 === 'Not') mPush(m, valNot(a));
      else fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: unknown unary ${inst.op2}`);
      break;
    }
    case 'Call': beginCall(m, inst.argc); break;
    case 'Await': {
      const target = mPeek(m);
      if (isFailure(target)) break;
      if (target.t !== 'task') typeMismatch('Task to await', typeName(target));
      const id = target.id;
      if (m.invokeDepth > 0) fault('AIPO_RT_AWAIT_IN_CALLBACK', 'await is not allowed inside a synchronous host callback');
      const res = taskResult(m, id);
      if (res) {
        if (res.t === 'cancelled') {
          fault('AIPO_RT_CANCELLED', `task ${id} was cancelled`);
        }
        mPop(m);
        mPush(m, res.v);
        break;
      }
      if (!m.tasks.has(id)) {
        typeMismatch('live task to await', `unknown task ${id}`);
      }
      const me = m.current !== null ? m.current : 0;
      if (me === id || blocksOn(m, id, me)) {
        const chain = awaitChain(m, me, id);
        fault('AIPO_RT_AWAIT_CYCLE', `await cycle detected: ${chain.join(' -> ')}`);
      }
      let ws = m.waiters.get(id);
      if (!ws) { ws = []; m.waiters.set(id, ws); }
      if (!ws.includes(me)) ws.push(me);
      const qIdx = m.runQueue.indexOf(id);
      if (qIdx >= 0) m.runQueue.splice(qIdx, 1);
      m.runQueue.unshift(id);
      fr.ip--;
      suspendCurrent(m, { t: 'blocked', target: { t: 'task', id } });
      break;
    }
    case 'Return': {
      const v = inst.has ? mPop(m) : vNone();
      doReturn(m, v);
      break;
    }
    case 'Jump': fr.ip = inst.t; break;
    case 'JumpIfFalse': {
      const c = mPop(m);
      if (c.t !== 'bool') fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: expected Bool, got ${typeName(c)}`);
      if (!c.v) fr.ip = inst.t;
      break;
    }
    case 'JumpIfSetLocal': {
      const v = fr.vars[inst.slot] === undefined ? vUnset() : fr.vars[inst.slot];
      if (v.t !== 'unset') fr.ip = inst.t;
      break;
    }
    case 'PushHandler': {
      m.handlers.push({ ip: inst.t, fn, stackDepth: m.stack.length, frameDepth: m.frames.length });
      break;
    }
    case 'PopHandler': m.handlers.pop(); break;
    case 'Pop': mPop(m); break;
    case 'Dup': mPush(m, mPeek(m)); break;
    case 'CloneStruct': {
      // Functional update (`base with { ... }`). The value is replaced in place so the
      // copy — not the base — receives the following SetField overrides.
      const v = mPeek(m);
      if (isFailure(v)) { mPop(m); handleFailure(m, v); break; }
      if (v.t !== 'struct') return typeMismatch('struct instance', typeName(v));
      const i = m.stack.length - 1;
      m.stack[i] = vStruct(v.type, v.fields.map((f) => [f[0], f[1]]), new Set(v.fixed), false);
      break;
    }
    case 'IsVariant': {
      const v = mPop(m);
      const matches = v && v.t === 'struct' && (v.type === inst.variant || v.type.endsWith('.' + inst.variant));
      mPush(m, vBool(Boolean(matches)));
      break;
    }
    case 'GetField': {
      const t = mPop(m);
      mPush(m, doGetField(m, t, inst.f));
      break;
    }
    case 'SetField': {
      const nv = mPop(m);
      const t = mPop(m);
      if (isFailure(t)) { handleFailure(m, t); break; }
      if (isFailure(nv)) { handleFailure(m, nv); break; }
      if (t.t !== 'struct') return typeMismatch('struct instance', typeName(t));
      const at = t.fields.findIndex(([k]) => k === inst.f);
      if (at < 0) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: ${t.type} has no field '${inst.f}'`);
      if (!t.constructing && t.fixed.has(inst.f)) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: fixed field '${t.type}.${inst.f}' cannot be reassigned`);
      if (!t.constructing && __invariantEntries && __invariantEntries.has(t.type)) {
        if (!m.journal.some(e => e.inst === t && e.field === inst.f)) {
          m.journal.push({ inst: t, field: inst.f, prev: t.fields[at][1] });
        }
      }
      t.fields[at][1] = nv;
      break;
    }
    case 'GetIndex': {
      const ix = mPop(m);
      const t = mPop(m);
      mPush(m, valGetIndex(t, ix, m.active));
      break;
    }
    case 'SetIndex': {
      const v = mPop(m);
      const ix = mPop(m);
      const t = mPop(m);
      if (isFailure(t)) { handleFailure(m, t); break; }
      if (isFailure(ix)) { handleFailure(m, ix); break; }
      if (isFailure(v)) { handleFailure(m, v); break; }
      valSetIndex(t, ix, v);
      break;
    }
    case 'BuildList': {
      const n = inst.n;
      const items = [];
      for (let i = 0; i < n; i++) items.unshift(mPop(m));
      mPush(m, vList(items));
      break;
    }
    case 'BuildDict': {
      const n = inst.n;
      const entries = [];
      for (let i = 0; i < n; i++) {
        const v = mPop(m);
        const k = mPop(m);
        entries.unshift([k, v]);
      }
      mPush(m, vDict(entries));
      break;
    }
    case 'BuildStruct': {
      const n = inst.n;
      const vals = [];
      for (let i = 0; i < n; i++) vals.unshift(mPop(m));
      const def = getStructDef(inst.type);
      const fields = def
        ? def.slice(0, n).map(([fname], i) => [fname, vals[i] !== undefined ? vals[i] : vNone()])
        : vals.map((v, i) => [`field_${i}`, v]);
      const fixed = inst.defer ? [] : (def ? def.filter(([, fx]) => fx).map(([fname]) => fname) : []);
      mPush(m, vStruct(inst.type, fields, fixed, !!inst.defer));
      break;
    }
    case 'SealStruct': {
      const v = mPop(m);
      if (v.t !== 'struct') return typeMismatch('struct instance', typeName(v));
      const def = getStructDef(v.type);
      v.fixed = new Set(def ? def.filter(([, fx]) => fx).map(([fname]) => fname) : []);
      v.constructing = false;
      mPush(m, v);
      break;
    }
    case 'AssertInvariant': {
      const v = mPop(m);
      if (!(v.t === 'bool' && v.v === true)) {
        fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: invariant() of ${inst.type} did not hold after construction`);
      }
      break;
    }
    case 'AssertContract': {
      const v = mPeek(m);
      assertContract(m, inst.type, !!inst.null, inst.pos, inst.ops || [], v);
      break;
    }
    case 'CheckMutations': checkMutations(m); break;
    case 'PropagateFailure': {
      // An empty operand stack means this boundary produced no value to check,
      // exactly like the VM's `stack.last()` guard.
      if (m.stack.length === 0) break;
      const v = mPeek(m);
      if (isFailure(v)) {
        mPop(m);
        handleFailure(m, v);
      }
      break;
    }
    case 'Fail': {
      const v = mPop(m);
      if (isFailure(v)) {
        handleFailure(m, v);
      } else {
        const msg = display(v);
        const payload = v.t === 'str' ? vNone() : v;
        handleFailure(m, vFail(msg, payload));
      }
      break;
    }
    case 'MakeFunction': {
      const idx = m.funcIndex.get(inst.name);
      if (idx === undefined) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: unknown function '${inst.name}'`);
      mPush(m, { t: 'func', idx, arity: m.module.functions[idx].params.length });
      break;
    }
    case 'MakeClosure': {
      const idx = m.funcIndex.get(inst.name);
      if (idx === undefined) fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: unknown function '${inst.name}'`);
      const cells = {};
      for (const cap of inst.ups || []) {
        if (inst.self !== undefined && inst.self !== null && cap === inst.self) {
          cells[cap] = { v: vUnset() };
        } else if (fn.localIndex.has(cap)) {
          const value = fr.vars[fn.localIndex.get(cap)];
          cells[cap] = { v: value === undefined ? vUnset() : value };
        } else if (fr.cells && Object.hasOwn(fr.cells, cap)) {
          cells[cap] = fr.cells[cap];
        } else if (m.globals.has(cap)) {
          cells[cap] = { v: m.globals.get(cap) };
        } else {
          fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: undefined global '${cap}'`);
        }
      }
      mPush(m, { t: 'closure', idx, arity: m.module.functions[idx].params.length, cells });
      break;
    }
    case 'FillSelfCapture': {
      const c = mPop(m);
      if (c.t !== 'closure') return typeMismatch('Function', typeName(c));
      const cell = c.cells[inst.name];
      if (!cell || cell.v.t !== 'unset') fault('AIPO_RT_TYPE_MISMATCH', 'type mismatch: corrupted closure (missing self placeholder)');
      cell.v = c;
      mPush(m, c);
      break;
    }
    case 'Range': {
      const b = mPop(m);
      const a = mPop(m);
      if (isFailure(a)) { mPush(m, a); break; }
      if (isFailure(b)) { mPush(m, b); break; }
      const x = widen(a), y = widen(b);
      if (x.t !== 'int' || y.t !== 'int') return typeMismatch('Int range bounds', `${typeName(a)} and ${typeName(b)}`);
      mPush(m, vRange(x.v, y.v));
      break;
    }
    case 'Len': {
      const v = mPop(m);
      mPush(m, valLen(v));
      break;
    }
    case 'TypeIs': {
      const tag = mPop(m);
      const v = mPop(m);
      if (isFailure(v)) { mPush(m, v); break; }
      if (tag.t === 'struct') {
        mPush(m, vBool(v.t === 'struct' && v.type === tag.type));
        break;
      }
      if (tag.t !== 'type') return typeMismatch('type value on the right of is', typeName(tag));
      mPush(m, vBool(tagMatches(tag.name, v)));
      break;
    }
    case 'TypeIsNullable': {
      const tag = mPop(m);
      const v = mPop(m);
      if (isFailure(v)) { mPush(m, v); break; }
      if (v.t === 'none') {
        mPush(m, vBool(true));
        break;
      }
      if (tag.t === 'struct') {
        mPush(m, vBool(v.t === 'struct' && v.type === tag.type));
        break;
      }
      if (tag.t !== 'type') return typeMismatch('type value on the right of is', typeName(tag));
      mPush(m, vBool(tagMatches(tag.name, v)));
      break;
    }
    case 'IterGuard': {
      const v = mPop(m);
      // Always push so `IterGuardEnd` stays balanced for unguarded values (ranges,
      // strings); `null` never matches a real collection id.
      m.active.push((v.t === 'list' || v.t === 'dict' || v.t === 'bytes' || v.t === 'set') ? v.id : null);
      break;
    }
    case 'IterGuardEnd': m.active.pop(); break;
    case 'IterAt': {
      const ix = mPop(m);
      const coll = mPop(m);
      if (ix.t !== 'int') return typeMismatch('Int iteration index', typeName(ix));
      mPush(m, iterAt(m, coll, ix.v, inst.mode));
      break;
    }
    case 'PushUnset': mPush(m, vUnset()); break;
    default: fault('AIPO_RT_TYPE_MISMATCH', `type mismatch: unknown instruction ${op}`);
  }
}

function doReturn(m, v) {
  if (isFailure(v)) {
    const fr0 = curFrame(m);
    if (fr0) {
      m.frames.pop();
      m.stack.length = fr0.base;
      m.journal.length = fr0.journalStart;
      if (m.frames.length === 0 && m.current !== null && m.current !== 0) {
        m.stack.length = 0;
        completeCurrent(m, { t: 'failed', v });
        return;
      }
    }
    handleFailure(m, v);
    return;
  }
  const fr = m.frames.pop();
  if (!fr) {
    m.stack.length = 0;
    completeCurrent(m, isFailure(v) ? { t: 'failed', v } : { t: 'ready', v });
    return;
  }
  m.journal.length = fr.journalStart;
  if (m.frames.length === 0) {
    m.stack.length = 0;
    completeCurrent(m, isFailure(v) ? { t: 'failed', v } : { t: 'ready', v });
    return;
  }
  m.stack.length = fr.base;
  mPush(m, v);
}

// ---- entry point ----
export function runModule(module) {
  const m = makeMachine(module);
  for (const f of module.functions) {
    m.globals.set(f.name, { t: 'func', idx: m.funcIndex.get(f.name), arity: f.params.length });
  }
  if (module.structs) {
    for (const s of module.structs) {
      if (s.name.includes('.')) {
        const [enumName] = s.name.split('.');
        if (!m.globals.has(enumName)) {
          m.globals.set(enumName, { t: 'type', name: enumName });
        }
      } else if (!m.globals.has(s.name)) {
        m.globals.set(s.name, { t: 'type', name: s.name });
      }
    }
  }
  const top = module.top;
  m.frames.push({ fn: top, ip: 0, vars: new Array(top.localNames.length), cells: null, base: 0, journalStart: 0 });
  try {
    while (m.mainOutcome === null) {
      if (m.current === null && m.frames.length === 0) {
        selectNext(m);
        if (m.mainOutcome !== null) break;
      }
      try {
        stepFn(m);
      } catch (e) {
        if (e === SUSPENDED) {
          continue;
        }
        if (m.current !== null && m.current !== 0) {
          // The machine was mid-task. Discard that task's frames before settling it:
          // an abandoned frame would otherwise keep running as the entry script.
          m.frames.length = 0;
          m.stack.length = 0;
          m.handlers.length = 0;
          m.journal.length = 0;
          m.active.length = 0;
          const state = m.tasks.get(m.current);
          if (state && state.status && state.status.t === 'cancelled') {
            // Cancellation is a task state, not a recoverable failure: the task ends
            // cancelled and whoever awaits it faults with `AIPO_RT_CANCELLED`.
            completeCurrent(m, { t: 'cancelled' });
          } else {
            // Every other fault is non-recoverable by canon (ADP-006 F): it keeps its
            // code and aborts the program, wherever it was raised.
            throw e;
          }
        } else {
          throw e;
        }
      }
    }
  } catch (e) {
    if (e && e.uncaught !== undefined) {
      printFault('AIPO_RT_FAILURE_UNCAUGHT', `uncaught failure: ${e.uncaught}`);
      return 1;
    }
    if (e instanceof AipoFault) {
      printFault(e.code, e.message.replace(/^\[[^\]]+\] /, ''));
      return 1;
    }
    throw e;
  }
  if (m.mainOutcome) {
    if (m.mainOutcome.t === 'cancelled') {
      printFault('AIPO_RT_CANCELLED', 'entry script was cancelled');
      return 1;
    }
    if (m.mainOutcome.t === 'failed') {
      printFault('AIPO_RT_FAILURE_UNCAUGHT', `uncaught failure: ${m.mainOutcome.v.msg}`);
      return 1;
    }
    m.result = m.mainOutcome.v;
  }
  if (m.halted) {
    printFault('AIPO_RT_FAILURE_UNCAUGHT', `uncaught failure: ${m.halted.msg}`);
    return 1;
  }
  const r = m.result;
  if (r && isFailure(r)) {
    printFault('AIPO_RT_FAILURE_UNCAUGHT', `uncaught failure: ${r.msg}`);
    return 1;
  }
  return 0;
}

function printFault(code, message) {
  const text = `error: [${code}] ${message}\n`;
  if (typeof process !== 'undefined' && process.stderr && process.stderr.write) {
    process.stderr.write(text);
  } else {
    emitText(text);
  }
}
