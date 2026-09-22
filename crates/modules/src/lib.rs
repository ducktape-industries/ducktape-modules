pub mod identity;
pub mod module_registry;
pub mod valset;

use borsh::{BorshDeserialize, BorshSerialize};

pub type AccountNumber = u64;

pub const AUTHORITY: &str = "governance";

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Page {
    pub after: Option<Vec<u8>>,
    pub limit: Option<u64>,
}

impl Page {
    pub const MAX_LIMIT: u64 = 100;

    pub fn limit(&self) -> u64 {
        self.limit
            .unwrap_or(Self::MAX_LIMIT)
            .clamp(1, Self::MAX_LIMIT)
    }

    pub fn scan(&self, prefix: &[u8]) -> abi::Scan {
        let mut scan = abi::Scan::prefix(prefix);
        if let Some(after) = &self.after {
            let start = scan.lo.clone();
            scan = scan.after(after);
            scan.lo = scan.lo.max(start);
        }
        scan.limit = Some(self.limit());
        scan
    }
}

/// A bounded query result. Resume with `next` as `Page::after` on the same query.
/// Height identifies the answering state; pagination does not pin a snapshot.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PageReply<T> {
    pub height: u64,
    pub items: Vec<T>,
    pub next: Option<Vec<u8>>,
}

impl Page {
    /// Consume ordered rows, retaining one lookahead to detect the final page.
    pub fn reply<T>(
        &self,
        height: u64,
        rows: impl IntoIterator<Item = (Vec<u8>, T)>,
    ) -> PageReply<T> {
        let mut rows = rows
            .into_iter()
            .filter(|(key, _)| self.after.as_ref().is_none_or(|after| key > after));
        let mut items = Vec::new();
        let mut last = None;
        for (key, item) in rows.by_ref().take(self.limit() as usize) {
            last = Some(key);
            items.push(item);
        }
        let next = rows.next().and(last);
        PageReply {
            height,
            items,
            next,
        }
    }

    pub fn scan_ahead(&self, prefix: &[u8]) -> abi::Scan {
        self.scan(prefix).limit(self.limit() + 1)
    }
}

pub mod program {
    use abi::{Env, Origin, ProgramId, Refusal, reason};

    pub fn external(env: &Env) -> Result<Vec<u8>, Refusal> {
        match &env.origin {
            Origin::External(signer) => Ok(signer.clone()),
            other => Err(Refusal::new(
                reason::UNAUTHORIZED,
                format!("only a signed frame may do this, not {other:?}"),
            )),
        }
    }

    pub fn program(env: &Env) -> Result<ProgramId, Refusal> {
        match &env.origin {
            Origin::Program(program) => Ok(program.clone()),
            other => Err(Refusal::new(
                reason::UNAUTHORIZED,
                format!("only a program may do this, not {other:?}"),
            )),
        }
    }

    pub fn from(env: &Env, program: &str) -> Result<(), Refusal> {
        let sent_by_program = matches!(&env.origin, Origin::Program(sender) if sender == program);
        let sent_by_system = env.origin == Origin::System;
        let authorized = sent_by_program || sent_by_system;
        if !authorized {
            return Err(Refusal::new(
                reason::UNAUTHORIZED,
                format!("only {program} may do this, not {:?}", env.origin),
            ));
        }
        Ok(())
    }

    pub fn u64_key(prefix: &str, number: u64) -> Vec<u8> {
        let mut key = prefix.as_bytes().to_vec();
        key.extend_from_slice(&number.to_be_bytes());
        key
    }

    pub fn bytes_key(prefix: &str, bytes: &[u8]) -> Vec<u8> {
        let mut key = prefix.as_bytes().to_vec();
        key.extend_from_slice(bytes);
        key
    }

    pub fn not_found(what: impl Into<String>) -> Refusal {
        Refusal::new(reason::NOT_FOUND, what)
    }

    pub fn invalid(sentence: impl Into<String>) -> Refusal {
        Refusal::new(reason::INVALID_INPUT, sentence)
    }

    pub fn already_exists(sentence: impl Into<String>) -> Refusal {
        Refusal::new(reason::ALREADY_EXISTS, sentence)
    }

    pub fn wrong_state(sentence: impl Into<String>) -> Refusal {
        Refusal::new(reason::WRONG_STATE, sentence)
    }

    pub fn unauthorized(sentence: impl Into<String>) -> Refusal {
        Refusal::new(reason::UNAUTHORIZED, sentence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_contracts_are_prefixes_of_the_program_contracts() {
        assert_eq!(
            abi::encode(&abi::valset::Query::Validators),
            abi::encode(&valset::Query::Validators)
        );
        assert_eq!(
            abi::encode(&abi::valset::Query::Members),
            abi::encode(&valset::Query::Members)
        );
        let member = abi::valset::Member {
            key: vec![1],
            address: "a".into(),
        };
        assert_eq!(
            abi::encode(&abi::valset::Reply::Validators(vec![vec![1]])),
            abi::encode(&valset::Reply::Validators(vec![vec![1]]))
        );
        assert_eq!(
            abi::encode(&abi::valset::Reply::Members(vec![member.clone()])),
            abi::encode(&valset::Reply::Members(vec![member]))
        );
        assert_eq!(
            abi::encode(&abi::module_registry::Query::At(9)),
            abi::encode(&module_registry::Query::At(9))
        );
        let entry = abi::module_registry::Entry {
            program: "p".into(),
            code: abi::BlobId::Sha256([1; 32]),
            params: vec![2],
        };
        assert_eq!(
            abi::encode(&abi::module_registry::Reply::Programs(vec![entry.clone()])),
            abi::encode(&module_registry::Reply::Programs(vec![entry]))
        );
    }

    #[test]
    fn a_page_scans_after_its_cursor() {
        let page = Page {
            after: Some(b"p/3".to_vec()),
            limit: Some(2),
        };
        let scan = page.scan(b"p/");
        assert!(!scan.admits(b"p/3"));
        assert!(scan.admits(b"p/4"));
        assert_eq!(scan.limit, Some(2));
        assert_eq!(Page::default().scan(b"p/").limit, Some(Page::MAX_LIMIT));
    }
    #[test]
    fn pages_are_bounded_round_trip_and_resume_without_duplicates() {
        let rows: Vec<_> = (0..105u8).map(|n| (vec![n], n)).collect();
        for limit in [None, Some(u64::MAX), Some(100), Some(0), Some(1)] {
            let page = Page { after: None, limit };
            let encoded = abi::encode(&page);
            let page: Page = abi::decode(&encoded).unwrap();
            let first = page.reply(7, rows.clone());
            assert_eq!(first.items.len(), page.limit() as usize);
            assert_eq!(first.height, 7);
            let decoded: PageReply<u8> = abi::decode(&abi::encode(&first)).unwrap();
            assert_eq!(decoded, first);
            let mut all = first.items;
            let mut next = first.next;
            while let Some(after) = next {
                let reply = Page {
                    after: Some(after),
                    limit,
                }
                .reply(7, rows.clone());
                assert!(!reply.items.is_empty());
                all.extend(reply.items);
                next = reply.next;
            }
            assert_eq!(all, (0..105u8).collect::<Vec<_>>());
        }
        let page = Page {
            after: Some(vec![255]),
            limit: None,
        };
        assert_eq!(page.reply(9, rows).items, Vec::<u8>::new());
        assert_eq!(page.reply::<u8>(9, []).next, None);
    }

    #[test]
    fn a_cursor_cannot_escape_its_prefix() {
        let page = Page {
            after: Some(b"a/".to_vec()),
            limit: None,
        };
        let scan = page.scan(b"c/1/");
        assert!(!scan.admits(b"b/2"));
        assert!(scan.admits(b"c/1/2"));
        assert!(!scan.admits(b"c/2/2"));
    }
}
