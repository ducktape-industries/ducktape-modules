//! Message search: `#tag` pages through the tag index, anything else is a
//! full-text search the program caps.
use ducktape_view_guest::Context;

use crate::queries::search_hits;
use crate::{Chat, Hits, Search};

impl Chat {
    pub(crate) fn search_submit(&mut self, cx: &mut Context<Self>) {
        let query = self.search.draft.trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.search.query = query;
        self.search_now(cx);
    }

    pub(crate) fn search_now(&mut self, cx: &mut Context<Self>) {
        let (text, viewer) = (self.search.query.clone(), self.viewer());
        let host = cx.host();
        self.search.hits = cx.load(
            async move {
                let (rows, capped, next_after) =
                    search_hits(host, text, None, viewer, None).await?;
                Ok(Hits {
                    rows,
                    capped,
                    has_more: next_after.is_some(),
                    next_after,
                })
            },
            |chat| &mut chat.search.hits,
        );
    }

    pub(crate) fn search_more(&mut self, cx: &mut Context<Self>) {
        let Some(after) = self.search.hits.ready().and_then(|h| h.next_after.clone()) else {
            return;
        };
        if self.search.more_loading {
            return;
        }
        self.search.more_loading = true;
        let (text, viewer) = (self.search.query.clone(), self.viewer());
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = search_hits(host, text, None, viewer, Some(after)).await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                chat.search.more_loading = false;
                let (rows, _, next_after) = match result {
                    Ok(page) => page,
                    Err(refusal) => {
                        chat.notice = format!("Couldn’t search further: {}", refusal.sentence);
                        return;
                    }
                };
                if let Some(hits) = chat.search.hits.ready_mut() {
                    for row in rows {
                        if !hits
                            .rows
                            .iter()
                            .any(|h| h.channel_id == row.channel_id && h.seq == row.seq)
                        {
                            hits.rows.push(row);
                        }
                    }
                    hits.has_more = next_after.is_some();
                    hits.next_after = next_after;
                }
            });
        })
        .detach();
    }

    pub(crate) fn search_clear(&mut self) {
        self.search = Search::default();
    }

    /// A hit opens its room around the message.
    pub(crate) fn open_hit(
        &mut self,
        channel_id: String,
        seq: u64,
        window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        self.search_clear();
        self.create = None;
        self.open_at(channel_id, seq, window, cx);
    }
}
