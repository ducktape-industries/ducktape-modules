// Runs the `call` crate's self-test table inside a wasm32-unknown-unknown
// module under node's WebAssembly — no WASI, no imports object, one fresh
// instance per case so a trap cannot poison the next case's heap.
//
//   cargo rustc -p call --lib --features selftest --crate-type cdylib \
//     --target wasm32-unknown-unknown --release
//   node crates/modules/apps/call/wasm-selftest.js <path/to/call.wasm>
'use strict';
const fs = require('fs');
const module_ = new WebAssembly.Module(fs.readFileSync(process.argv[2]));
const imports = WebAssembly.Module.imports(module_);
console.log(`imports: ${imports.length === 0 ? 'none' : JSON.stringify(imports)}`);
if (imports.length !== 0) process.exit(2);
const count = new WebAssembly.Instance(module_, {}).exports.selftest_count();
let failed = 0;
for (let i = 0; i < count; i++) {
  const e = new WebAssembly.Instance(module_, {}).exports;
  const name = Buffer.from(e.memory.buffer, e.selftest_name_ptr(i), e.selftest_name_len(i)).toString();
  try {
    e.selftest_run(i);
    console.log(`ok   ${name}`);
  } catch (err) {
    failed++;
    console.log(`FAIL ${name}: ${err.message}`);
  }
}
console.log(`${count - failed} passed, ${failed} failed, ${count} total (wasm32-unknown-unknown under node ${process.version})`);
process.exit(failed ? 1 : 0);
