//! Bounded, resumable query pages over the kernel's `Scan`: the one paging
//! vocabulary every program's queries speak.

use abi::Refusal;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::refuse::invalid;

#[derive(
    Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize,
)]
pub struct Page {
    pub after: Option<Vec<u8>>,
    pub limit: Option<u64>,
}

impl Page {
    pub const MAX_LIMIT: u64 = 256;

    pub const fn first(limit: u64) -> Page {
        Page {
            after: None,
            limit: Some(limit),
        }
    }

    pub fn limit(&self) -> u64 {
        self.limit
            .unwrap_or(Self::MAX_LIMIT)
            .clamp(1, Self::MAX_LIMIT)
    }

    /// The same page, its limit capped at `max` (a program's own bound).
    pub fn bounded(&self, max: u64) -> Page {
        Page {
            after: self.after.clone(),
            limit: Some(self.limit().min(max.max(1))),
        }
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

    pub fn scan_ahead(&self, prefix: &[u8]) -> abi::Scan {
        self.scan(prefix).limit(self.limit() + 1)
    }

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

    /// A page of an in-memory list: the cursor is the big-endian offset of
    /// the next item.
    pub fn slice<T: Clone>(&self, height: u64, values: &[T]) -> Result<PageReply<T>, Refusal> {
        let start = match &self.after {
            None => 0,
            Some(bytes) => <[u8; 8]>::try_from(bytes.as_slice())
                .map(|b| u64::from_be_bytes(b) as usize)
                .map_err(|_| invalid("a list cursor is an 8-byte offset"))?,
        };
        if start > values.len() {
            return Err(invalid("cursor past the end of this listing"));
        }
        let end = start.saturating_add(self.limit() as usize).min(values.len());
        Ok(PageReply {
            height,
            items: values[start..end].to_vec(),
            next: (end < values.len()).then(|| (end as u64).to_be_bytes().to_vec()),
        })
    }
}

/// A bounded query result. Resume with `next` as `Page::after` on the same query.
/// Height identifies the answering state; pagination does not pin a snapshot.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct PageReply<T> {
    pub height: u64,
    pub items: Vec<T>,
    pub next: Option<Vec<u8>>,
}

impl<T> PageReply<T> {
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> PageReply<U> {
        PageReply {
            height: self.height,
            items: self.items.into_iter().map(f).collect(),
            next: self.next,
        }
    }

    pub fn try_map<U>(self, f: impl FnMut(T) -> Result<U, Refusal>) -> Result<PageReply<U>, Refusal> {
        Ok(PageReply {
            height: self.height,
            items: self.items.into_iter().map(f).collect::<Result<_, _>>()?,
            next: self.next,
        })
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
        assert_eq!(Page::first(500).bounded(10).limit(), 10);
    }

    #[test]
    fn pages_are_bounded_round_trip_and_resume_without_duplicates() {
        let rows: Vec<_> = (0..=255u8).map(|n| (vec![n], n)).chain([(vec![255, 0], 0u8)]).collect();
        let total = rows.len();
        for limit in [None, Some(u64::MAX), Some(100), Some(0), Some(1)] {
            let page = Page { after: None, limit };
            let encoded = abi::encode(&page);
            let page: Page = abi::decode(&encoded).unwrap();
            let first = page.reply(7, rows.clone());
            assert_eq!(first.items.len(), (page.limit() as usize).min(total));
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
            assert_eq!(all.len(), total);
        }
        let page = Page {
            after: Some(vec![255, 255]),
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

    #[test]
    fn a_list_pages_by_offset() {
        let values: Vec<u32> = (0..5).collect();
        let first = Page::first(2).slice(1, &values).unwrap();
        assert_eq!(first.items, [0, 1]);
        let second = Page {
            after: first.next.clone(),
            limit: Some(2),
        }
        .slice(1, &values)
        .unwrap();
        assert_eq!(second.items, [2, 3]);
        let last = Page {
            after: second.next,
            limit: Some(2),
        }
        .slice(1, &values)
        .unwrap();
        assert_eq!((last.items, last.next), (vec![4], None));
        assert!(
            Page {
                after: Some(vec![1]),
                limit: None
            }
            .slice(1, &values)
            .is_err()
        );
    }
}
