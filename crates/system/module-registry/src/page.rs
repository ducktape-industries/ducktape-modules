//! Bounded, resumable query pages over the kernel's `Scan`.

use borsh::{BorshDeserialize, BorshSerialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
