# View packaging

```sh
cargo run -p view-pack -- PROGRAM.wasm VIEW.wasm OUTPUT.wasm
cargo run -p view-pack -- --strip PROGRAM.wasm OUTPUT.wasm
```

Inputs must be valid core WebAssembly modules. The output may be the program
input path. Embedding removes all existing `ducktape.view` custom sections,
preserves every other program byte, and appends one custom section (section ID
0, name `ducktape.view`, payload equal to the complete view module). Repeating
the operation produces identical bytes. Strip removes only these sections.

The nested view retains its `ducktape.view.manifest` section. The app extracts
the view from the program code blob and passes that view to
`view_wire::manifest::read_manifest`; no separate view file is needed at runtime.

`make wasm-modules` builds the system programs, app programs, and views, then
packs them under `$CARGO_TARGET_DIR/pack/` (default `target/pack/`):
`chat.wasm` embeds `chat_view.wasm`, `forge.wasm` embeds `forge_view.wasm`
and `module_registry.wasm` embeds `settings_view.wasm`. Those are build
outputs, never committed: genesis and qa consume them from there. A founding
file must name the packaged program artifact as its code; the kernel stores
those complete bytes in its code blob.

`make wasm-reproducible` builds them twice, the second time from a fresh
target directory, and requires identical sha256 for every artifact and no
absolute path inside any of them.
The integration test builds Chat and its view, invokes the CLI, and verifies
the app's section-reader contract and actual manifest parser. Its nested Cargo
build logs are saved under `target/pack/test-{program,view}.log`.

At base `31d862b`, Chat emits preferred size `1180x760`, while the host manifest
parser requires `1180,760`. The integration test deliberately fails at manifest
acceptance until the view owner fixes that value; embedding does not rewrite
view metadata. Blob extraction and stripping are checked before that assertion.
