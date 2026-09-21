// The forge program's wire: its founding bounds, its ops, its queries and their replies. Shared with every client of the program.

use abi::HashKind;
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Bounds {
    pub max_objects: u64,
    pub max_delta_depth: u64,
    pub max_object_size: u64,
    pub push_walk: u64,
    pub fetch_walk: u64,
    pub merge_cost: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Settings {
    pub head: Vec<u8>,
    pub allow_force: bool,
    pub allow_delete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            head: b"refs/heads/main".to_vec(),
            allow_force: false,
            allow_delete: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Repo {
    pub hash: HashKind,
    pub owner: Vec<u8>,
    pub settings: Settings,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Create {
        repo: String,
        hash: HashKind,
    },
    Configure {
        repo: String,
        settings: Settings,
    },
    Grant {
        repo: String,
        key: Vec<u8>,
    },
    Revoke {
        repo: String,
        key: Vec<u8>,
    },
    Push {
        repo: String,
        request: Vec<u8>,
    },
    Merge {
        repo: String,
        into: Vec<u8>,
        from: Vec<u8>,
        message: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Service {
    ReceivePack,
    UploadPack,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    Repos,
    Refs { repo: String },
    Advertise { repo: String, service: Service },
    Upload { repo: String, request: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RepoInfo {
    pub name: String,
    pub repo: Repo,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RefInfo {
    pub name: Vec<u8>,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Repos(Vec<RepoInfo>),
    Refs(Vec<RefInfo>),
}

pub fn valid_repo_name(name: &str) -> bool {
    let not_empty = !name.is_empty();
    let plain_ascii = name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    let not_hidden = !name.starts_with('.');
    let not_git_suffixed = !name.ends_with(".git");
    not_empty && plain_ascii && not_hidden && not_git_suffixed
}
