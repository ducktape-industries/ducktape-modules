use std::borrow::Cow;

use wasm_encoder::{CustomSection, Encode, Section};
use wasmparser::{Encoding, Parser, Payload, Validator};

pub const VIEW_SECTION: &str = "ducktape.view";

fn validate(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(
        Parser::new(0).parse_all(bytes).next(),
        Some(Ok(Payload::Version {
            encoding: Encoding::Module,
            ..
        }))
    ) {
        return Err("expected a core WebAssembly module".into());
    }
    Validator::new().validate_all(bytes)?;
    Ok(())
}

/// Remove every embedded view, preserving all other bytes, including section headers.
pub fn strip(program: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    validate(program)?;
    let mut output = Vec::with_capacity(program.len());
    let mut keep = 0;
    let mut section_start = 8;
    for payload in Parser::new(0).parse_all(program) {
        let payload = payload?;
        if let Some((_, range)) = payload.as_section() {
            if matches!(&payload, Payload::CustomSection(s) if s.name() == VIEW_SECTION) {
                output.extend_from_slice(&program[keep..section_start]);
                keep = range.end;
            }
            section_start = range.end;
        }
    }
    output.extend_from_slice(&program[keep..]);
    Ok(output)
}

/// The payload is the complete view module, without framing or transformation.
pub fn embed(program: &[u8], view: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    validate(view)?;
    let mut output = strip(program)?;
    let section = CustomSection {
        name: Cow::Borrowed(VIEW_SECTION),
        data: Cow::Borrowed(view),
    };
    output.push(section.id());
    section.encode(&mut output);
    Ok(output)
}
