//! A page's cursor belongs to one query and one snapshot; sparse scans may return empty pages.
use crate::contract::{Bounds, Cursor, Page, Query};
use crate::refuse::{invalid, stale};
use crate::sandbox::Sandbox;
use abi::{Entry, Refusal, Scan};
use borsh::BorshSerialize;

pub struct Paging {
    height: u64,
    scope: Vec<u8>,
    after: Option<Vec<u8>>,
    pub limit: usize,
}
impl Paging {
    pub fn for_query(height: u64, bounds: &Bounds, query: &Query) -> Result<Option<Self>, Refusal> {
        let mut scope = query.clone();
        let (cursor, limit) = match &mut scope {
            Query::Repos { cursor, limit }
            | Query::Repo { cursor, limit, .. }
            | Query::Refs { cursor, limit, .. }
            | Query::Log { cursor, limit, .. }
            | Query::Tree { cursor, limit, .. }
            | Query::Diff { cursor, limit, .. }
            | Query::Compare { cursor, limit, .. }
            | Query::Changes { cursor, limit, .. }
            | Query::Change { cursor, limit, .. }
            | Query::Judgment { cursor, limit, .. } => (cursor.take(), std::mem::take(limit)),
            _ => return Ok(None),
        };
        Self::new(height, bounds, &scope, cursor, limit).map(Some)
    }

    pub fn new(
        height: u64,
        bounds: &Bounds,
        scope: &impl BorshSerialize,
        cursor: Option<Cursor>,
        limit: u32,
    ) -> Result<Self, Refusal> {
        if limit == 0 || limit > bounds.page_size {
            return Err(invalid("limit must be 1..=Bounds.page_size"));
        }
        let scope = abi::encode(scope);
        if let Some(c) = &cursor {
            if c.scope != scope {
                return Err(invalid("cursor belongs to another query"));
            }
            if c.height != height {
                return Err(stale("cursor height changed; restart the listing"));
            }
        }
        Ok(Self {
            height,
            scope,
            after: cursor.map(|c| c.after),
            limit: limit as usize,
        })
    }
    pub fn next(&self, after: Vec<u8>) -> Cursor {
        Cursor {
            height: self.height,
            scope: self.scope.clone(),
            after,
        }
    }
    pub fn entries<S: Sandbox>(&self, sandbox: &S, prefix: &[u8]) -> Result<Page<Entry>, Refusal> {
        let mut scan = Scan::prefix(prefix).limit(self.limit as u64 + 1);
        if let Some(after) = &self.after {
            if !after.starts_with(prefix) {
                return Err(invalid("cursor outside this listing"));
            }
            scan = scan.after(after);
        }
        let mut items = sandbox.scan(scan);
        let more = items.len() > self.limit;
        items.truncate(self.limit);
        let next = if more {
            items.last().map(|e| self.next(e.key.clone()))
        } else {
            None
        };
        Ok(Page { items, next })
    }
    pub fn slice<T: Clone>(&self, values: &[T]) -> Result<Page<T>, Refusal> {
        let start = match &self.after {
            None => 0,
            Some(bytes) => {
                let n: u64 = abi::decode(bytes).map_err(|_| invalid("invalid page offset"))?;
                usize::try_from(n).map_err(|_| invalid("page offset is too large"))?
            }
        };
        if start > values.len() {
            return Err(invalid("cursor past the end of this listing"));
        }
        let end = start.saturating_add(self.limit).min(values.len());
        Ok(Page {
            items: values[start..end].to_vec(),
            next: (end < values.len()).then(|| self.next(abi::encode(&(end as u64)))),
        })
    }
}
