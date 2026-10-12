// Shim self-test: unit properties of the pure value/stdlib layer.
// Run with `node selftest.mjs` (requires Node >= 20). Machine-level behavior
// (frames, handlers, journals) is covered by the VM<->JS differential suite.
import assert from 'node:assert/strict';
import * as R from './aipo-runtime.js';

function throwsCode(fn, code) {
  try {
    fn();
  } catch (e) {
    assert.equal(e.code, code, `expected ${code}, got ${e.code}: ${e.message}`);
    return;
  }
  assert.fail(`expected fault ${code}, but nothing threw`);
}

// --- numerics: range, finiteness, div-zero ---
assert.equal(R.vInt(9007199254740991).v, 9007199254740991);
throwsCode(() => R.vInt(9007199254740992), 'AIPO_RT_OVERFLOW');
throwsCode(() => R.valAdd(R.vInt(9007199254740991), R.vInt(1)), 'AIPO_RT_OVERFLOW');
throwsCode(() => R.valDiv(R.vInt(1), R.vInt(0)), 'AIPO_RT_DIV_ZERO');
throwsCode(() => R.valIntDiv(R.vInt(1), R.vInt(0)), 'AIPO_RT_DIV_ZERO');
assert.equal(R.valDiv(R.vInt(1), R.vInt(2)).v, 0.5);
assert.equal(R.display(R.valDiv(R.vInt(2), R.vInt(1))), '2.0');
assert.equal(R.valIntDiv(R.vInt(7), R.vInt(2)).v, 3);
assert.equal(R.valMod(R.vInt(7), R.vInt(3)).v, 1);
// Byte widens: Byte(200) + Byte(100) exceeds Byte but fits Int.
assert.equal(R.valAdd(R.vByte(200), R.vByte(100)).v, 300);

// --- strings: code points, NFC, tolerant slices ---
assert.equal(R.std_string_len(R.vStr('é')).v, 1);
assert.equal(R.std_string_byte_len(R.vStr('é')).v, 2);
assert.equal(R.vStr('é').v, 'é', 'decomposed input normalizes at construction');
assert.equal(R.valAdd(R.vStr('e'), R.vStr('́')).v, 'é', 'concat normalizes');
assert.equal(R.std_string_reverse(R.vStr('́e')).v, 'é');
assert.equal(R.std_string_slice(R.vStr('hello'), R.vInt(-10), R.vInt(99)).v, 'hello');
assert.equal(R.std_string_slice(R.vStr('hello'), R.vInt(3), R.vInt(1)).v, '');
throwsCode(() => R.std_string_split(R.vStr('a'), R.vStr('')), 'AIPO_RT_TYPE_MISMATCH');
throwsCode(() => R.std_string_join(R.vStr(','), R.vList([R.vInt(1)])), 'AIPO_RT_TYPE_MISMATCH');
assert.equal(R.std_string_find(R.vStr('abc'), R.vStr('')).v, 0);
assert.ok(R.std_string_find(R.vStr('abc'), R.vStr('z')).t === 'none');
assert.equal(R.std_string_capitalize(R.vStr('123 abc')).v, '123 Abc');

// --- format: recoverable, never a fault ---
assert.equal(R.std_string_format(R.vStr('hi {x}'), R.vDict([])).msg, 'missing format value for placeholder {x}');
assert.equal(R.std_string_format(R.vStr('hi {0}'), R.vDict([])).msg, 'unsupported placeholder {0}: only simple names are allowed');
assert.equal(R.std_string_format(R.vStr('hi {x'), R.vDict([])).msg, "malformed format template: unclosed '{'");

// --- math: Failure channel ---
assert.equal(R.std_math_sqrt(R.vInt(-1)).msg, 'cannot compute square root of negative number');
assert.ok(R.std_math_clamp(R.vInt(5), R.vInt(3), R.vInt(0)).msg.startsWith('math.clamp bounds are inverted'));
assert.equal(R.std_math_round(R.vFloat(2.5)).v, 3);
assert.equal(R.std_math_round(R.vFloat(-2.5)).v, -3);

// --- conversions: exact messages ---
assert.equal(R.convInt(R.vStr('not a number')).msg, 'invalid integer text: "not a number"');
assert.equal(R.convFloat(R.vStr('xx')).msg, 'invalid float text: "xx"');
assert.equal(R.convByte(R.vInt(256)).msg, 'Byte value 256 is outside the range 0..=255');
assert.equal(R.convBytes(R.vInt(-1)).msg, 'Bytes(-1) is outside the constructible range 0..=67108864');

// --- equality: numeric cross-type, structural collections ---
assert.ok(R.valuesEqual(R.vInt(1), R.vFloat(1.0)));
assert.ok(R.valuesEqual(R.vByte(2), R.vInt(2)));
assert.ok(!R.valuesEqual(R.vInt(1), R.vStr('1')));
assert.ok(R.valuesEqual(R.vList([R.vInt(1)]), R.vList([R.vInt(1)])));
assert.ok(!R.valuesEqual(R.vList([R.vInt(1)]), R.vList([R.vInt(2)])));

// --- dict: insertion order preserved ---
const d = R.vDict([[R.vStr('b'), R.vInt(2)], [R.vStr('a'), R.vInt(1)]]);
assert.deepEqual(R.dictNatives.keys(d, []).items.map(k => k.v), ['b', 'a']);
assert.equal(R.iterAt(null, d, 1, 0).v, 'a');
assert.equal(R.iterAt(null, d, 1, 1).v, 'a');
assert.equal(R.iterAt(null, d, 1, 2).v, 1);
assert.equal(R.dictNatives.get(d, [R.vStr('a')]).v, 1);
assert.ok(R.dictNatives.get(d, [R.vStr('zzz')]).t === 'none');

// --- list: negative indices, tolerant ranges via valGetIndex ---
const l = R.vList([R.vInt(1), R.vInt(2), R.vInt(3)]);
assert.equal(R.valGetIndex(l, R.vInt(-1), []).v, 3);
throwsCode(() => R.valGetIndex(l, R.vInt(9), []), 'AIPO_RT_INDEX_OUT_OF_RANGE');
assert.equal(R.valGetIndex(l, R.vRange(1, 99), []).items.length, 2);
assert.equal(R.valLen(R.vRange(3, 3)).v, 0);

// --- Failure propagation vs faults ---
const f = R.vFail('x');
assert.ok(R.valAdd(f, R.vInt(1)) === f);
assert.ok(R.valNot(f) === f);
assert.equal(R.structGetField(f, 'message').v, 'x');
console.log('shim selftest: all assertions passed');
// --- negative zero preserves VM display ---
assert.equal(R.display({ t: 'float', v: -0 }), '-0.0');
assert.equal(R.display({ t: 'float', v: 0 }), '0.0');
console.log('shim selftest: negative-zero assertions passed');
// --- integers never hold negative zero (Rust i64 has none) ---
const negMul = R.valMul(R.vInt(-33), R.vInt(0));
assert.equal(negMul.t, 'int');
assert.ok(Object.is(negMul.v, 0) && !Object.is(negMul.v, -0));
assert.equal(R.display(negMul), '0');
console.log('shim selftest: int-negzero assertions passed');

// --- Wave 3: Set ---
const s = R.vSet([R.vInt(1), R.vInt(2), R.vInt(1), R.vInt(3)]);
assert.equal(s.items.length, 3);
assert.equal(R.setNatives.has(s, [R.vInt(2)]).v, true);
assert.equal(R.setNatives.has(s, [R.vInt(99)]).v, false);
R.setNatives.add(s, [R.vInt(4)]);
assert.equal(s.items.length, 4);
assert.equal(R.setNatives.remove(s, [R.vInt(2)]).v, true);
assert.equal(R.setNatives.remove(s, [R.vInt(2)]).v, false);
assert.equal(R.setNatives.len(s).v, 3);
assert.equal(R.valLen(s).v, 3);
assert.equal(R.std_len(s).v, 3);
const sCopy = R.std_copy(s);
assert.ok(R.valuesEqual(s, sCopy));
assert.equal(R.std_same(s, sCopy).v, false);
assert.equal(R.std_same(s, s).v, true);
assert.equal(R.display(s), '{1, 3, 4}');

// --- Wave 3: Duration ---
const dur1 = R.vDuration(2.5);
const dur2 = R.vDuration(1.5);
const durSum = R.valAdd(dur1, dur2);
assert.equal(durSum.t, 'duration');
assert.equal(durSum.v, 4.0);
assert.equal(R.valSub(dur1, dur2).v, 1.0);
assert.equal(R.valNeg(dur1).v, -2.5);
assert.equal(R.valLess(dur2, dur1).v, true);
assert.equal(R.valLessEqual(dur1, dur1).v, true);
assert.equal(R.display(R.vDuration(2.0)), '2s');
assert.equal(R.display(dur1), '2.5s');

// --- Wave 3: Bytes packing ---
const buf = R.vBytes(new Uint8Array(8));
R.bytesNatives.write_i16(buf, [R.vInt(0), R.vInt(-500)]);
R.bytesNatives.write_u16(buf, [R.vInt(2), R.vInt(40000)]);
assert.equal(R.bytesNatives.read_i16(buf, [R.vInt(0)]).v, -500);
assert.equal(R.bytesNatives.read_u16(buf, [R.vInt(2)]).v, 40000);
throwsCode(() => R.bytesNatives.read_i32(buf, [R.vInt(6)]), 'AIPO_RT_INDEX_OUT_OF_RANGE');
assert.ok(R.isFailure(R.bytesNatives.write_i8(buf, [R.vInt(0), R.vInt(300)])));
R.valSetIndex(buf, R.vInt(4), R.vInt(42));
assert.equal(R.valGetIndex(buf, R.vInt(4), []).v, 42);

// --- Wave 3: Bytes decode & String encode ---
const encoded = R.vBytes(new TextEncoder().encode('Hello Aipo'));
const decoded = R.bytesNatives.decode(encoded);
assert.equal(decoded.v, 'Hello Aipo');
const invalidUtf8 = R.vBytes(new Uint8Array([0xFF, 0xFE]));
assert.ok(R.isFailure(R.bytesNatives.decode(invalidUtf8)));

// --- Wave 3: Sequence .lazy() ---
const listLazy = R.listNatives.lazy(l);
assert.equal(listLazy.t, 'sequence');
assert.equal(R.display(listLazy), '<sequence>');

// --- Wave 3: Task & Group values ---
const t = R.vTask(42);
assert.equal(t.t, 'task');
assert.equal(R.display(t), '<task #42>');

const g = R.vGroup(99);
assert.equal(g.t, 'group');
assert.equal(R.display(g), '<group #99>');
assert.equal(R.typeName(g), 'Group');
assert.ok(R.valuesEqual(g, R.vGroup(99)));
assert.ok(!R.valuesEqual(g, R.vGroup(100)));

// --- Wave 3: runModule async scheduler execution ---
const asyncMod = {
  version: R.RUNTIME_VERSION,
  functions: [
    {
      name: 'worker',
      params: [],
      locals: [],
      upvalues: [],
      code: [
        { op: 'Constant', value: { Int: 42 } },
        { op: 'Return', has: true },
      ],
    },
  ],
  top: {
    name: 'main',
    params: [],
    locals: [],
    upvalues: [],
    code: [
      { op: 'Load', name: 'task' },
      { op: 'GetField', f: 'spawn' },
      { op: 'MakeFunction', name: 'worker' },
      { op: 'BuildList', n: 0 },
      { op: 'Call', argc: 2 },
      { op: 'Await' },
      { op: 'Return', has: true },
    ],
  },
  structs: [],
};

assert.equal(R.runModule(asyncMod), 0);

console.log('shim selftest: Wave 3 assertions passed');

