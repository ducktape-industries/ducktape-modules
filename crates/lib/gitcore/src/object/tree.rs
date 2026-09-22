// Tree objects: entry modes, git's entry ordering, parse and serialize.

use crate::error::{Error, Result};
use crate::oid::{Hash, Oid};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    Regular,
    Executable,
    Symlink,
    Directory,
    Gitlink,
}

impl Mode {
    pub fn parse(token: &[u8]) -> Result<Mode> {
        match token {
            b"100644" => Ok(Mode::Regular),
            b"100755" => Ok(Mode::Executable),
            b"120000" => Ok(Mode::Symlink),
            b"40000" => Ok(Mode::Directory),
            b"160000" => Ok(Mode::Gitlink),
            _ => Err(Error::BadMode),
        }
    }

    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Mode::Regular => b"100644",
            Mode::Executable => b"100755",
            Mode::Symlink => b"120000",
            Mode::Directory => b"40000",
            Mode::Gitlink => b"160000",
        }
    }

    pub fn is_directory(self) -> bool {
        self == Mode::Directory
    }

    pub fn is_file(self) -> bool {
        matches!(self, Mode::Regular | Mode::Executable)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: Mode,
    pub name: Vec<u8>,
    pub id: Oid,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Tree {
    pub entries: Vec<TreeEntry>,
}

pub(crate) fn entry_order(a: &TreeEntry, b: &TreeEntry) -> Ordering {
    let common = a.name.len().min(b.name.len());
    let prefix = a.name[..common].cmp(&b.name[..common]);
    let prefix_differs = prefix != Ordering::Equal;
    if prefix_differs {
        return prefix;
    }
    tail_byte(a, common).cmp(&tail_byte(b, common))
}

fn tail_byte(entry: &TreeEntry, at: usize) -> Option<u8> {
    let after_name = entry.mode.is_directory().then_some(b'/');
    entry.name.get(at).copied().or(after_name)
}

impl Tree {
    pub fn parse(bytes: &[u8], hash: Hash) -> Result<Tree> {
        let mut entries = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            let Some(space) = rest.iter().position(|byte| *byte == b' ') else {
                return Err(Error::Truncated);
            };
            let mode = Mode::parse(&rest[..space])?;
            rest = &rest[space + 1..];
            let Some(nul) = rest.iter().position(|byte| *byte == 0) else {
                return Err(Error::Truncated);
            };
            let name = rest[..nul].to_vec();
            rest = &rest[nul + 1..];
            let has_id = rest.len() >= hash.size();
            if !has_id {
                return Err(Error::Truncated);
            }
            let id = Oid::from_bytes(hash, &rest[..hash.size()])?;
            rest = &rest[hash.size()..];
            let entry = TreeEntry { mode, name, id };
            let in_order = entries
                .last()
                .is_none_or(|previous| entry_order(previous, &entry) == Ordering::Less);
            if !in_order {
                return Err(Error::UnsortedTree);
            }
            entries.push(entry);
        }
        Ok(Tree { entries })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut sorted: Vec<&TreeEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| entry_order(a, b));
        let mut out = Vec::new();
        for entry in sorted {
            out.extend_from_slice(entry.mode.as_bytes());
            out.push(b' ');
            out.extend_from_slice(&entry.name);
            out.push(0);
            out.extend_from_slice(entry.id.as_bytes());
        }
        out
    }

    pub fn find(&self, name: &[u8]) -> Option<&TreeEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }
}
