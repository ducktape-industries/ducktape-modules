use std::{fs, path::Path, process::Command};
use wasm_encoder::{CustomSection, Module};
use wasmparser::{Parser, Payload};

// Contract copied from app/app/src/backend/views.rs::view_section.
fn host_view(bytes: &[u8]) -> Option<Vec<u8>> {
    for payload in Parser::new(0).parse_all(bytes) {
        if let Ok(Payload::CustomSection(section)) = payload
            && section.name() == "ducktape.view"
        {
            return Some(section.data().to_vec());
        }
    }
    None
}

#[test]
fn replacement_and_strip_preserve_program_bytes() {
    let mut program = Module::new();
    program.section(&CustomSection {
        name: "unrelated".into(),
        data: b"keep me".as_slice().into(),
    });
    let program = program.finish();
    let view = Module::new().finish();
    let packed = view_pack::embed(&program, &view).unwrap();
    assert_eq!(host_view(&packed).unwrap(), view);
    assert_eq!(view_pack::embed(&packed, &view).unwrap(), packed);
    assert_eq!(view_pack::strip(&packed).unwrap(), program);
    let replaced = view_pack::embed(&packed, &program).unwrap();
    assert_eq!(host_view(&replaced).unwrap(), program);
    let mut duplicate = packed.clone();
    duplicate.extend_from_slice(&packed[program.len()..]);
    assert_eq!(view_pack::embed(&duplicate, &view).unwrap(), packed);
    assert_eq!(view_pack::strip(&program).unwrap(), program);
    assert!(view_pack::embed(b"broken", &view).is_err());
    assert!(view_pack::embed(&program, b"broken").is_err());
    assert!(view_pack::embed(&program, b"\0asm\x0d\0\x01\0").is_err());
}

fn build(root: &Path, target: &Path, log: &Path, args: &[&str]) {
    let output = fs::File::create(log).unwrap();
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .env("RUSTC_WRAPPER", "")
        .args(args)
        .stdout(output.try_clone().unwrap())
        .stderr(output)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "build exit {status}; log: {}",
        log.display()
    );
}

#[test]
fn built_chat_blob_satisfies_host_reader_and_manifest_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let logs = root.join("target/pack");
    fs::create_dir_all(&logs).unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    build(
        &root,
        &target,
        &logs.join("test-program.log"),
        &[
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            "crates/app/chat-program/Cargo.toml",
        ],
    );
    build(
        &root,
        &target,
        &logs.join("test-view.log"),
        &[
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            "chat-view",
        ],
    );
    let release = target.join("wasm32-unknown-unknown/release");
    let program = fs::read(release.join("chat_program.wasm")).unwrap();
    let view = fs::read(release.join("chat_view.wasm")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let blob = dir.path().join("chat.wasm");
    let status = Command::new(env!("CARGO_BIN_EXE_view-pack"))
        .arg(release.join("chat_program.wasm"))
        .arg(release.join("chat_view.wasm"))
        .arg(&blob)
        .status()
        .unwrap();
    assert!(status.success(), "packer exit {status}");
    let packed = fs::read(&blob).unwrap();
    let extracted = host_view(&packed).unwrap();
    assert_eq!(extracted, view);
    assert_eq!(view_pack::strip(&packed).unwrap(), program);
    assert_eq!(view_pack::embed(&packed, &view).unwrap(), packed);
    let stripped = dir.path().join("stripped.wasm");
    let status = Command::new(env!("CARGO_BIN_EXE_view-pack"))
        .arg("--strip")
        .arg(&blob)
        .arg(&stripped)
        .status()
        .unwrap();
    assert!(status.success(), "strip exit {status}");
    assert_eq!(fs::read(stripped).unwrap(), program);
    let manifest_bytes = Parser::new(0)
        .parse_all(&extracted)
        .find_map(|payload| match payload.unwrap() {
            Payload::CustomSection(section) if section.name() == "ducktape.view.manifest" => {
                Some(section.data().to_vec())
            }
            _ => None,
        })
        .expect("the view must retain its manifest section");
    println!(
        "host contract extracted {} exact view bytes; manifest: {:?}",
        extracted.len(),
        String::from_utf8_lossy(&manifest_bytes)
    );
    let manifest = view_wire::manifest::read_manifest(&extracted)
        .expect("host rejected the built view manifest (preferred size must be comma-separated)");
    assert_eq!(manifest.name, "Chat");
    assert_eq!(manifest.wire_epoch, view_wire::WIRE_EPOCH);
    println!(
        "host contract read {} view bytes; manifest: {} / epoch {}",
        extracted.len(),
        manifest.name,
        manifest.wire_epoch
    );
}
