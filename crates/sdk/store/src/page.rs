//! Bounded, resumable query pages over the kernel's `Scan`: the one paging
//! vocabulary every program's queries speak.

use abi::Refusal;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use guest::{invalid, stale};

/// What `Page::after` and `PageReply::next` carry, opaque to clients: the
/// listing the cursor belongs to (`scope`), the height that answered it, and
/// the raw position to resume after.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Cursor {
    pub height: u64,
    pub scope: Vec<u8>,
    pub after: Vec<u8>,
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    Serialize,
    Deserialize,
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

    /// The scope a listing binds its cursors to: the borsh of the query's
    /// identifying arguments (a prefix, a channel, a repo and ref).
    pub fn scope_of(args: &impl BorshSerialize) -> Vec<u8> {
        abi::encode(args)
    }

    /// The cursor `after` carries, refused when it belongs to another
    /// listing than `scope`.
    pub fn open(&self, scope: &[u8]) -> Result<Option<Cursor>, Refusal> {
        let Some(bytes) = &self.after else {
            return Ok(None);
        };
        let cursor: Cursor =
            abi::decode(bytes).map_err(|_| invalid("a page cursor does not decode"))?;
        if cursor.scope != scope {
            return Err(stale("the cursor belongs to another listing"));
        }
        Ok(Some(cursor))
    }

    /// This page over one listing: its cursor opened against `scope`, and
    /// every `next` it answers bound to `scope` and `height`.
    pub fn listing(&self, scope: Vec<u8>, height: u64) -> Result<Listing, Refusal> {
        let cursor = self.open(&scope)?;
        Ok(Listing {
            limit: self.limit(),
            cursor_height: cursor.as_ref().map(|c| c.height),
            after: cursor.map(|c| c.after),
            scope,
            height,
        })
    }
}

/// A `Page` opened over one listing (see `Page::listing`).
#[derive(Debug)]
pub struct Listing {
    limit: u64,
    after: Option<Vec<u8>>,
    scope: Vec<u8>,
    height: u64,
    /// The height that answered the cursor, for a program whose listings
    /// are rewritten (forge refuses a cursor from another height).
    pub cursor_height: Option<u64>,
}

impl Listing {
    pub fn limit(&self) -> u64 {
        self.limit
    }

    fn next(&self, after: Vec<u8>) -> Vec<u8> {
        abi::encode(&Cursor {
            height: self.height,
            scope: self.scope.clone(),
            after,
        })
    }

    pub fn scan(&self, prefix: &[u8]) -> abi::Scan {
        let mut scan = abi::Scan::prefix(prefix);
        if let Some(after) = &self.after {
            let start = scan.lo.clone();
            scan = scan.after(after);
            scan.lo = scan.lo.max(start);
        }
        scan.limit = Some(self.limit);
        scan
    }

    pub fn scan_ahead(&self, prefix: &[u8]) -> abi::Scan {
        self.scan(prefix).limit(self.limit + 1)
    }

    /// Consume ordered rows, retaining one lookahead to detect the final page.
    pub fn reply<T>(&self, rows: impl IntoIterator<Item = (Vec<u8>, T)>) -> PageReply<T> {
        let mut rows = rows
            .into_iter()
            .filter(|(key, _)| self.after.as_ref().is_none_or(|after| key > after));
        let mut items = Vec::new();
        let mut last = None;
        for (key, item) in rows.by_ref().take(self.limit as usize) {
            last = Some(key);
            items.push(item);
        }
        let next = rows.next().and(last).map(|key| self.next(key));
        PageReply {
            height: self.height,
            items,
            next,
        }
    }

    /// A page of an in-memory list: the cursor's position is the big-endian
    /// offset of the next item.
    pub fn slice<T: Clone>(&self, values: &[T]) -> Result<PageReply<T>, Refusal> {
        let start = match &self.after {
            None => 0,
            Some(bytes) => <[u8; 8]>::try_from(bytes.as_slice())
                .map(|b| u64::from_be_bytes(b) as usize)
                .map_err(|_| invalid("a list cursor is an 8-byte offset"))?,
        };
        if start > values.len() {
            return Err(invalid("cursor past the end of this listing"));
        }
        let end = start.saturating_add(self.limit as usize).min(values.len());
        Ok(PageReply {
            height: self.height,
            items: values[start..end].to_vec(),
            next: (end < values.len()).then(|| self.next((end as u64).to_be_bytes().to_vec())),
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

    pub fn try_map<U>(
        self,
        f: impl FnMut(T) -> Result<U, Refusal>,
    ) -> Result<PageReply<U>, Refusal> {
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

    fn listing(page: &Page) -> Listing {
        page.listing(b"p/".to_vec(), 7).unwrap()
    }

    #[test]
    fn a_page_scans_after_its_cursor_and_a_cursor_stays_in_its_listing() {
        let first = listing(&Page::first(2)).reply([
            (b"p/3".to_vec(), 3u8),
            (b"p/4".to_vec(), 4),
            (b"p/5".to_vec(), 5),
        ]);
        let page = Page {
            after: first.next.clone(),
            limit: Some(2),
        };
        let scan = listing(&page).scan(b"p/");
        assert!(!scan.admits(b"p/4"));
        assert!(scan.admits(b"p/5"));
        assert_eq!(scan.limit, Some(2));
        assert_eq!(
            listing(&Page::default()).scan(b"p/").limit,
            Some(Page::MAX_LIMIT)
        );
        assert_eq!(Page::first(500).bounded(10).limit(), 10);
        let other = page.listing(b"q/".to_vec(), 7).unwrap_err();
        assert_eq!(other.reason, abi::reason::STALE);
        assert_eq!(listing(&page).cursor_height, Some(7));
        let garbage = Page {
            after: Some(vec![1]),
            limit: None,
        };
        assert_eq!(
            garbage.open(b"p/").unwrap_err().reason,
            abi::reason::INVALID_INPUT
        );
    }

    #[test]
    fn pages_are_bounded_round_trip_and_resume_without_duplicates() {
        let rows: Vec<_> = (0..=255u8)
            .map(|n| (vec![n], n))
            .chain([(vec![255, 0], 0u8)])
            .collect();
        let total = rows.len();
        for limit in [None, Some(u64::MAX), Some(100), Some(0), Some(1)] {
            let page = Page { after: None, limit };
            let encoded = abi::encode(&page);
            let page: Page = abi::decode(&encoded).unwrap();
            let first = listing(&page).reply(rows.clone());
            assert_eq!(first.items.len(), (page.limit() as usize).min(total));
            assert_eq!(first.height, 7);
            let decoded: PageReply<u8> = abi::decode(&abi::encode(&first)).unwrap();
            assert_eq!(decoded, first);
            let mut all = first.items;
            let mut next = first.next;
            while let Some(after) = next {
                let reply = listing(&Page {
                    after: Some(after),
                    limit,
                })
                .reply(rows.clone());
                assert!(!reply.items.is_empty());
                all.extend(reply.items);
                next = reply.next;
            }
            assert_eq!(all.len(), total);
        }
        assert_eq!(listing(&Page::default()).reply::<u8>([]).next, None);
    }

    #[test]
    fn a_cursor_cannot_escape_its_prefix() {
        let past = Page::default().listing(b"c/1/".to_vec(), 1).unwrap();
        let page = Page {
            after: Some(past.next(b"a/".to_vec())),
            limit: None,
        };
        let scan = page.listing(b"c/1/".to_vec(), 1).unwrap().scan(b"c/1/");
        assert!(!scan.admits(b"b/2"));
        assert!(scan.admits(b"c/1/2"));
        assert!(!scan.admits(b"c/2/2"));
    }

    #[test]
    fn a_list_pages_by_offset() {
        let values: Vec<u32> = (0..5).collect();
        let at = |after| {
            listing(&Page {
                after,
                limit: Some(2),
            })
        };
        let first = at(None).slice(&values).unwrap();
        assert_eq!(first.items, [0, 1]);
        let second = at(first.next).slice(&values).unwrap();
        assert_eq!(second.items, [2, 3]);
        let last = at(second.next).slice(&values).unwrap();
        assert_eq!((last.items, last.next), (vec![4], None));
        let bad = listing(&Page::default()).next(vec![1]);
        assert!(at(Some(bad)).slice(&values).is_err());
    }
}
