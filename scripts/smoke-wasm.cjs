// Exercise the generated JavaScript wrapper and the actual WASM module.
const assert = require('node:assert/strict');
const { AppCore } = require('../dist/wasm-node/app_core.js');
const encoder = new TextEncoder();
const decoder = new TextDecoder();
const encode = (value) => encoder.encode(JSON.stringify(value));
const decode = (bytes) => JSON.parse(decoder.decode(bytes));

const core = new AppCore();
const other = new AppCore();
try {
  assert.equal(core.protocol_version(), 1);
  assert.deepEqual(decode(core.view()).bootstrap, { status: 'idle' });
  assert.throws(() => core.process_event(encoder.encode('invalid JSON')));
  const effects = decode(core.process_event(encode({ type: 'start' })));
  assert.deepEqual(decode(core.view()).bootstrap, { status: 'loading' });
  const request = effects.find(({ effect }) => effect.type === 'host_info');
  const render = effects.find(({ effect }) => effect.type === 'render');
  assert.ok(request);
  assert.ok(render);
  const success = encode({ Ok: { name: 'WASM host', version: '1.0' } });
  assert.throws(() => core.handle_response(render.id, success));
  assert.throws(() => core.handle_response(request.id, encode({})));
  const renders = decode(core.handle_response(request.id, success));
  assert.deepEqual(renders.map(({ effect }) => effect.type), ['render']);
  assert.deepEqual(decode(core.view()), {
    initialized: true,
    bootstrap: { status: 'ready', value: { name: 'WASM host', version: '1.0' } },
  });
  assert.throws(() => core.handle_response(request.id, success));
  assert.deepEqual(decode(other.view()), { initialized: false, bootstrap: { status: 'idle' } });
  console.log('WASM bindings: event -> effect -> result -> typed view PASS');
} finally {
  core.free();
  other.free();
}
