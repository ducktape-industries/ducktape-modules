//! The replay fixtures: `replies.bin` is every captured reply back to back,
//! `replies.idx` one line per shape — `name offset len request-hex sha256`,
//! the request being the borsh `Query` or `Op` that produced the bytes.
//! Shared by `#[path]` between forge's generator and forge-view's tests.
#![allow(dead_code)]
use std::path::Path;

pub struct Fixture {
    pub name: String,
    pub offset: usize,
    pub len: usize,
    pub request: Vec<u8>,
    pub sha256: String,
}

pub fn index(dir: &Path) -> Vec<Fixture> {
    let text = std::fs::read_to_string(dir.join("replies.idx")).expect("replies.idx");
    text.lines()
        .map(|line| {
            let mut f = line.split(' ');
            let mut next = || f.next().expect("five fields per line");
            Fixture {
                name: next().to_owned(),
                offset: next().parse().expect("offset"),
                len: next().parse().expect("len"),
                request: unhex(next()),
                sha256: next().to_owned(),
            }
        })
        .collect()
}

/// One committed fixture, exactly as the program answered it.
pub fn bytes(dir: &Path, name: &str) -> Vec<u8> {
    let all = std::fs::read(dir.join("replies.bin")).expect("replies.bin");
    let entry = index(dir)
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no fixture named {name}"));
    all[entry.offset..entry.offset + entry.len].to_vec()
}

pub fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "a borsh hex has whole bytes");
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
        .collect()
}
