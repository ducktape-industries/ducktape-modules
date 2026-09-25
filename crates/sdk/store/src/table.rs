//! Typed tables over a context: `const` descriptors whose methods take the
//! context. `const ACCOUNTS: Map<AccountNumber, Account> = Map::new("a");`

use std::marker::PhantomData;

use abi::{Refusal, Scan};
use borsh::{BorshDeserialize, BorshSerialize};
use guest::{ExecCtx, QueryCtx, corrupt};

use crate::key::KeyCodec;
use crate::page::{Listing, Page, PageReply};

pub struct Map<K, V> {
    prefix: &'static str,
    _types: PhantomData<fn() -> (K, V)>,
}

impl<K: KeyCodec, V: BorshSerialize + BorshDeserialize> Map<K, V> {
    pub const fn new(prefix: &'static str) -> Self {
        Map {
            prefix,
            _types: PhantomData,
        }
    }

    /// The table's prefix followed by `head`: a full key, or the leading
    /// elements of a tuple key.
    pub fn key<H: KeyCodec>(&self, head: &H) -> Vec<u8> {
        let mut bytes = self.prefix.as_bytes().to_vec();
        head.encode_key(&mut bytes);
        bytes
    }

    /// Every key whose leading elements are `head` (a tuple key's prefix).
    pub fn prefix_of<H: KeyCodec>(&self, head: &H) -> Scan {
        Scan::prefix(self.key(head))
    }

    /// Every key whose leading elements sort before `head`.
    pub fn below<H: KeyCodec>(&self, head: &H) -> Scan {
        let mut hi = self.prefix.as_bytes().to_vec();
        head.encode_key(&mut hi);
        Scan::range(self.prefix.as_bytes().to_vec(), Some(hi))
    }

    pub fn get(&self, ctx: &QueryCtx, key: &K) -> Result<Option<V>, Refusal> {
        let bytes = self.key(key);
        ctx.get(&bytes)
            .map(|value| decode_value(self.prefix, &bytes, &value))
            .transpose()
    }

    pub fn has(&self, ctx: &QueryCtx, key: &K) -> bool {
        ctx.get(self.key(key)).is_some()
    }

    pub fn put(&self, ctx: &ExecCtx, key: &K, value: &V) {
        ctx.set(self.key(key), abi::encode(value));
    }

    pub fn remove(&self, ctx: &ExecCtx, key: &K) {
        ctx.delete(self.key(key));
    }

    /// The rows a scan admits, keys decoded back. The scan comes from
    /// `prefix_of`, `below`, or `Page::scan` over `self.prefix()`.
    pub fn scan(&self, ctx: &QueryCtx, scan: Scan) -> Result<Vec<(K, V)>, Refusal> {
        ctx.scan(scan)
            .into_iter()
            .map(|entry| {
                let key = decode_key(self.prefix, &entry.key)?;
                let value = decode_value(self.prefix, &entry.key, &entry.value)?;
                Ok((key, value))
            })
            .collect()
    }

    pub fn all(&self, ctx: &QueryCtx) -> Result<Vec<(K, V)>, Refusal> {
        self.scan(ctx, Scan::prefix(self.prefix))
    }

    /// One page of the table in key order, resumable through `PageReply::next`.
    pub fn range(
        &self,
        ctx: &QueryCtx,
        page: &Page,
        height: u64,
    ) -> Result<PageReply<(K, V)>, Refusal> {
        self.range_of(ctx, &(), page, height)
    }

    /// One page of the keys whose leading elements are `head`.
    pub fn range_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        page: &Page,
        height: u64,
    ) -> Result<PageReply<(K, V)>, Refusal> {
        let listing = page.listing(self.key(head), height)?;
        self.page_of(ctx, head, &listing)
    }

    /// One page of the keys whose leading elements are `head`, over a
    /// listing the program opened itself (one whose cursors are bound to
    /// more than the prefix: the whole query, a height).
    pub fn page_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        listing: &Listing,
    ) -> Result<PageReply<(K, V)>, Refusal> {
        let rows = ctx
            .scan(listing.scan_ahead(&self.key(head)))
            .into_iter()
            .map(|entry| {
                let key = decode_key(self.prefix, &entry.key)?;
                let value = decode_value(self.prefix, &entry.key, &entry.value)?;
                Ok((entry.key, (key, value)))
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        Ok(listing.reply(rows))
    }

    pub fn prefix(&self) -> &'static str {
        self.prefix
    }
}

pub struct Set<K> {
    map: Map<K, ()>,
}

impl<K: KeyCodec> Set<K> {
    pub const fn new(prefix: &'static str) -> Self {
        Set {
            map: Map::new(prefix),
        }
    }

    pub fn key<H: KeyCodec>(&self, head: &H) -> Vec<u8> {
        self.map.key(head)
    }

    pub fn prefix_of<H: KeyCodec>(&self, head: &H) -> Scan {
        self.map.prefix_of(head)
    }

    pub fn has(&self, ctx: &QueryCtx, key: &K) -> bool {
        self.map.has(ctx, key)
    }

    pub fn insert(&self, ctx: &ExecCtx, key: &K) {
        self.map.put(ctx, key, &());
    }

    pub fn remove(&self, ctx: &ExecCtx, key: &K) {
        self.map.remove(ctx, key);
    }

    pub fn scan(&self, ctx: &QueryCtx, scan: Scan) -> Result<Vec<K>, Refusal> {
        Ok(self
            .map
            .scan(ctx, scan)?
            .into_iter()
            .map(|(k, ())| k)
            .collect())
    }

    pub fn all(&self, ctx: &QueryCtx) -> Result<Vec<K>, Refusal> {
        self.scan(ctx, Scan::prefix(self.map.prefix))
    }

    pub fn range(&self, ctx: &QueryCtx, page: &Page, height: u64) -> Result<PageReply<K>, Refusal> {
        Ok(self.map.range(ctx, page, height)?.map(|(k, ())| k))
    }

    pub fn range_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        page: &Page,
        height: u64,
    ) -> Result<PageReply<K>, Refusal> {
        Ok(self.map.range_of(ctx, head, page, height)?.map(|(k, ())| k))
    }

    pub fn page_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        listing: &Listing,
    ) -> Result<PageReply<K>, Refusal> {
        Ok(self.map.page_of(ctx, head, listing)?.map(|(k, ())| k))
    }

    pub fn prefix(&self) -> &'static str {
        self.map.prefix
    }
}

pub struct Item<T> {
    key: &'static str,
    _type: PhantomData<fn() -> T>,
}

impl<T: BorshSerialize + BorshDeserialize> Item<T> {
    pub const fn new(key: &'static str) -> Self {
        Item {
            key,
            _type: PhantomData,
        }
    }

    pub fn get(&self, ctx: &QueryCtx) -> Result<Option<T>, Refusal> {
        ctx.get(self.key)
            .map(|value| decode_value(self.key, b"", &value))
            .transpose()
    }

    pub fn put(&self, ctx: &ExecCtx, value: &T) {
        ctx.set(self.key.as_bytes().to_vec(), abi::encode(value));
    }

    /// Reads the value (or its default), lets `change` alter it, stores and returns it.
    pub fn update(&self, ctx: &ExecCtx, change: impl FnOnce(&mut T)) -> Result<T, Refusal>
    where
        T: Default,
    {
        let mut value = self.get(ctx)?.unwrap_or_default();
        change(&mut value);
        self.put(ctx, &value);
        Ok(value)
    }
}

fn decode_key<K: KeyCodec>(table: &str, raw: &[u8]) -> Result<K, Refusal> {
    let mut rest = raw.get(table.len()..).unwrap_or_default();
    match K::decode_key(&mut rest) {
        Some(key) if rest.is_empty() => Ok(key),
        _ => Err(corrupt(table, raw, "the key does not decode")),
    }
}

fn decode_value<V: BorshDeserialize>(table: &str, key: &[u8], value: &[u8]) -> Result<V, Refusal> {
    borsh::from_slice(value).map_err(|fault| corrupt(table, key, fault))
}

#[cfg(test)]
mod tests {
    use super::*;
    use abi::{Cause, Env, Origin};
    use guest::MockHost;

    /// A context over a fresh host.
    fn exec() -> ExecCtx {
        MockHost::default().exec(Env {
            network: vec![],
            height: 0,
            time: 0,
            me: "test".into(),
            origin: Origin::System,
            cause: Cause::Direct,
        })
    }

    const NUMBERS: Map<u64, String> = Map::new("n/");
    const PAIRS: Map<(u64, String), u8> = Map::new("p/");
    const SEEN: Set<Vec<u8>> = Set::new("s/");
    const NEXT: Item<u64> = Item::new("next");

    #[test]
    fn tables_round_trip_scan_in_key_order_and_refuse_corrupt_rows() {
        let ctx = exec();
        NUMBERS.put(&ctx, &10, &"ten".into());
        NUMBERS.put(&ctx, &2, &"two".into());
        assert_eq!(NUMBERS.get(&ctx, &2).unwrap().as_deref(), Some("two"));
        assert!(NUMBERS.has(&ctx, &10) && !NUMBERS.has(&ctx, &3));
        let keys: Vec<u64> = NUMBERS
            .all(&ctx)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, [2, 10], "numeric, not lexical");
        NUMBERS.remove(&ctx, &2);
        assert_eq!(NUMBERS.all(&ctx).unwrap().len(), 1);

        PAIRS.put(&ctx, &(1, "b".into()), &1);
        PAIRS.put(&ctx, &(1, "a".into()), &2);
        PAIRS.put(&ctx, &(2, "a".into()), &3);
        let under_one = PAIRS.scan(&ctx, PAIRS.prefix_of(&1u64)).unwrap();
        assert_eq!(under_one.len(), 2);
        let paged = PAIRS.range_of(&ctx, &1u64, &Page::first(1), 0).unwrap();
        assert_eq!((paged.items.len(), paged.next.is_some()), (1, true));
        assert_eq!(under_one[0].0.1, "a");
        assert_eq!(PAIRS.scan(&ctx, PAIRS.below(&2u64)).unwrap().len(), 2);

        SEEN.insert(&ctx, &vec![7]);
        assert!(SEEN.has(&ctx, &vec![7]));
        assert_eq!(SEEN.all(&ctx).unwrap(), [vec![7]]);

        assert_eq!(NEXT.update(&ctx, |n| *n += 1).unwrap(), 1);
        assert_eq!(NEXT.get(&ctx).unwrap(), Some(1));

        ctx.set(b"n/short".to_vec(), abi::encode(&"x".to_string()));
        let refusal = NUMBERS.all(&ctx).unwrap_err();
        assert_eq!(refusal.reason, abi::reason::CORRUPT);
        assert!(refusal.sentence.starts_with("n/["), "{refusal}");
    }

    /// String heads list by name, not by length, and a whole-element prefix
    /// or `below` never reaches a longer name.
    #[test]
    fn string_keys_list_by_name_and_a_prefix_is_a_whole_element() {
        const NAMED: Map<(String, u64), ()> = Map::new("x/");
        let ctx = exec();
        for (name, n) in [
            ("general", 1),
            ("abc", 2),
            ("ab", 1),
            ("abc", 1),
            ("docs", 1),
            ("a\0b", 1),
        ] {
            NAMED.put(&ctx, &(name.into(), n), &());
        }
        let order = |scan| -> Vec<(String, u64)> {
            NAMED
                .scan(&ctx, scan)
                .unwrap()
                .into_iter()
                .map(|(k, ())| k)
                .collect()
        };
        let all = order(Scan::prefix(NAMED.prefix()));
        let names: Vec<&str> = all.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a\0b", "ab", "abc", "abc", "docs", "general"]);
        assert_eq!(all[2..4], [("abc".into(), 1), ("abc".into(), 2)]);
        assert_eq!(
            order(NAMED.prefix_of(&"ab".to_string())),
            [("ab".into(), 1)]
        );
        assert_eq!(order(NAMED.prefix_of(&"a".to_string())), []);
        assert_eq!(order(NAMED.below(&"abc".to_string())).len(), 2);
    }

    #[test]
    fn a_range_pages_with_lookahead() {
        let ctx = exec();
        for n in 0..5u64 {
            NUMBERS.put(&ctx, &n, &n.to_string());
        }
        let reply = NUMBERS.range(&ctx, &Page::first(2), 3).unwrap();
        assert_eq!((reply.items.len(), reply.height), (2, 3));
        let page = Page {
            after: reply.next,
            limit: Some(2),
        };
        let reply = NUMBERS.range(&ctx, &page, 3).unwrap();
        assert_eq!(reply.items[0].0, 2);
        assert!(reply.next.is_some());
        let page = Page {
            after: reply.next,
            limit: Some(2),
        };
        assert_eq!(NUMBERS.range(&ctx, &page, 3).unwrap().next, None);
    }
}
