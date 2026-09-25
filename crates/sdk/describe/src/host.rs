//! The host half: a program's [`SECTION`](crate::SECTION) read out of its
//! code blob, compiled, and called on an op. The section is UNTRUSTED
//! input: anyone may publish a program. It is compiled from binary only,
//! must import nothing, runs with a fuel budget and a memory ceiling in a
//! fresh instance per call, and its answer is bounded and strictly
//! decoded. Every failure is `None`: the reader shows the op's bytes.
use wasmtime::{Engine, Instance, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::Description;

/// The most a section may carry: a describe module is kilobytes.
pub const MAX_SECTION: usize = 4 << 20;
/// Instructions one call may run: decoding a 16 MB push fits well inside.
pub const FUEL: u64 = 250_000_000;
/// The module's memory ceiling: the op, its decoded copy and the answer.
pub const MEMORY: usize = 64 << 20;
/// The longest answer read back.
pub const MAX_ANSWER: usize = 64 << 10;

/// The payload of the describe section of a core module, if it has one.
/// Walks the section headers only; a malformed module has none.
pub fn section(module: &[u8]) -> Option<&[u8]> {
    let mut rest = module.strip_prefix(b"\0asm\x01\0\0\0")?;
    while let Some((&id, after)) = rest.split_first() {
        let (size, after) = leb(after)?;
        let body = after.get(..size)?;
        rest = &after[size..];
        if id == 0 {
            let (len, name) = leb(body)?;
            if name.get(..len)? == crate::SECTION.as_bytes() {
                return Some(&name[len..]);
            }
        }
    }
    None
}

/// An unsigned LEB128 u32 and what follows it.
fn leb(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let mut value = 0u32;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7f).checked_shl(7 * index as u32)?;
        if byte & 0x80 == 0 {
            return Some((value as usize, &bytes[index + 1..]));
        }
    }
    None
}

/// The section compiled, if it is a module that imports nothing. `engine`
/// must consume fuel (`Config::consume_fuel`), or every [`run`] is `None`.
pub fn compile(engine: &Engine, section: &[u8]) -> Option<Module> {
    if section.len() > MAX_SECTION {
        return None;
    }
    let module = Module::from_binary(engine, section).ok()?;
    let pure = module.imports().len() == 0;
    pure.then_some(module)
}

/// The module's description of `op`, in an instance of its own.
pub fn run(engine: &Engine, module: &Module, op: &[u8]) -> Option<Description> {
    let limits = StoreLimitsBuilder::new()
        .memory_size(MEMORY)
        .memories(1)
        .instances(1)
        .tables(1)
        .table_elements(1 << 16)
        .trap_on_grow_failure(true)
        .build();
    let mut store: Store<StoreLimits> = Store::new(engine, limits);
    store.limiter(|limits| limits);
    store.set_fuel(FUEL).ok()?;
    let instance = Instance::new(&mut store, module, &[]).ok()?;
    let memory = instance.get_memory(&mut store, "memory")?;
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut store, "alloc")
        .ok()?;
    let describe = instance
        .get_typed_func::<(u32, u32), u64>(&mut store, "describe")
        .ok()?;
    let len = u32::try_from(op.len()).ok()?;
    let ptr = alloc.call(&mut store, len).ok()?;
    memory.write(&mut store, ptr as usize, op).ok()?;
    let packed = describe.call(&mut store, (ptr, len)).ok()?;
    let (ptr, len) = ((packed >> 32) as usize, packed as u32 as usize);
    if len == 0 || len > MAX_ANSWER {
        return None;
    }
    let answer = memory.data(&store).get(ptr..ptr.checked_add(len)?)?;
    Description::decode(answer)
}
