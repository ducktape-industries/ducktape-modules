//! What chat hands the host's notification centre: a message that mentions
//! the reader, or one in a direct room she is in, landing in a room she is
//! not looking at. The host decides whether it becomes a banner. The tab
//! badge counts such messages in rooms still unread.
use std::collections::BTreeMap;

use chat::{Block, ChannelInfo, Mark, MsgRow, Party, Query, Reply};
use ducktape_view_guest::Context;
use ducktape_view_guest::methods::{HostBadge, NotifyPost, NotifySeen, Post};

use crate::api::{Ask, ChatApi};
use crate::message::message_body;
use crate::names::dm_peer_of;
use crate::watch::log;
use crate::{Chat, links};
use chat::view::Names;

/// The most new messages read out of one room per change.
const MAX_NEW: u64 = 20;

impl Chat {
    /// The channel list landed: the rooms, then a notice for what moved in
    /// a room the reader is not looking at, and the badge.
    pub(crate) fn channels_landed(&mut self, channels: Vec<ChannelInfo>, cx: &mut Context<Self>) {
        // before the first list there is nothing to compare against: what
        // was already there is history, not news
        let before: Option<BTreeMap<String, u64>> = self.channels.ready().map(|list| {
            list.iter()
                .map(|info| (info.channel.id.clone(), info.head_seq))
                .collect()
        });
        self.channels_arrived(channels, cx);
        if let Some(me) = self.my_account() {
            // the count is not kept, the read cursors are: the first list
            // a view that started over (a reload carries its state, not the
            // count) sees counts again what is meant for her in each room
            // still unread, and announces only what moved since `before`
            let recount = !std::mem::replace(&mut self.recounted, true);
            let viewing = self.viewing();
            let news: Vec<(String, u64, u64, u64)> = self
                .channels
                .ready()
                .into_iter()
                .flatten()
                .filter(|info| Some(info.channel.id.as_str()) != viewing.as_deref())
                .filter_map(|info| {
                    let (id, head) = (&info.channel.id, info.head_seq);
                    let seen = before
                        .as_ref()
                        .map_or(head, |before| before.get(id).copied().unwrap_or(0));
                    let cursor = match recount {
                        true => self.reads.cursors.get(id).copied().unwrap_or(head),
                        false => head,
                    };
                    let from = seen.min(cursor);
                    (head > from).then(|| (id.clone(), from, seen, head))
                })
                .collect();
            for (channel, from, seen, head) in news {
                self.read_news(channel, from, seen, head, me, cx);
            }
        }
        self.settle_badge(cx);
        self.save_reads(cx);
    }

    /// The room on screen, if the reader can see it.
    pub(crate) fn viewing(&self) -> Option<String> {
        self.room
            .as_ref()
            .filter(|_| self.reads.visible)
            .map(|room| room.id.clone())
    }

    /// Reads the messages past `was` in `channel` and counts the ones meant
    /// for the reader, posting those past `seen`.
    fn read_news(
        &mut self,
        channel: String,
        was: u64,
        seen: u64,
        head: u64,
        me: u64,
        cx: &mut Context<Self>,
    ) {
        let fresh = (head - was).min(MAX_NEW);
        let viewer = self.viewer();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let asked = host
                .ask::<Ask<ChatApi>>(Query::MessagesAround {
                    channel_id: channel.clone(),
                    seq: head,
                    viewer,
                    page: chat::Page {
                        after: None,
                        limit: Some(fresh * 2),
                    },
                })
                .await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                let rows = match asked {
                    Ok(Reply::Messages(rows)) => rows,
                    Ok(_) => return log(cx, "news", &ducktape_view_guest::host::wrong_reply()),
                    Err(refusal) => return log(cx, "news", &refusal),
                };
                let empty = Names::default();
                let names = chat.names.ready().unwrap_or(&empty);
                let name = chat
                    .info(&channel)
                    .map(|info| info.channel.name.clone())
                    .unwrap_or_default();
                let posts: Vec<(u64, Post)> = rows
                    .iter()
                    .filter(|row| row.seq > was && row.seq <= head)
                    .filter_map(|row| {
                        notice(row, me, &name, &chat.session.chain, names)
                            .map(|post| (row.seq, post))
                    })
                    .collect();
                if posts.is_empty() || chat.viewing().as_deref() == Some(channel.as_str()) {
                    return;
                }
                *chat.attention.entry(channel).or_default() += posts.len() as i64;
                for (_, post) in posts.into_iter().filter(|(seq, _)| *seq > seen) {
                    cx.host().notify::<NotifyPost>(post);
                }
                chat.settle_badge(cx);
            });
        })
        .detach();
    }

    /// `room` is read: the host's rows for it, under the tag its notices
    /// carry, are read too.
    pub(crate) fn read_notices(&self, room: &str, cx: &mut Context<Self>) {
        let (Some(me), Some(info)) = (self.my_account(), self.info(room)) else {
            return;
        };
        let empty = Names::default();
        let names = self.names.ready().unwrap_or(&empty);
        cx.host()
            .notify::<NotifySeen>(tag(me, room, &info.channel.name, names));
    }

    /// The tab badge: messages meant for the reader in rooms still unread.
    /// A room read, or on screen, drops out.
    pub(crate) fn settle_badge(&mut self, cx: &mut Context<Self>) {
        let viewing = self.viewing();
        let read: Vec<String> = self
            .attention
            .keys()
            .filter(|id| {
                Some(id.as_str()) == viewing.as_deref()
                    || self.info(id).is_none_or(|info| !self.unread(info))
            })
            .cloned()
            .collect();
        for id in read {
            self.attention.remove(&id);
        }
        let count: i64 = self.attention.values().sum();
        if self.badge != Some(count) {
            self.badge = Some(count);
            cx.host().notify::<HostBadge>(count);
        }
    }
}

/// The notice `row` makes for account `me`, if it is meant for her: it
/// mentions her, or it is in a direct room she is in. Her own never is.
fn notice(row: &MsgRow, me: u64, name: &str, chain: &str, names: &Names) -> Option<Post> {
    if row.deleted || row.author == Party::Account(me) {
        return None;
    }
    let direct = dm_peer_of(me, &row.channel_id).is_some();
    if !direct && !mentions(&row.blocks, me) {
        return None;
    }
    let sender = names.author(&row.author);
    Some(Post {
        title: match direct {
            true => sender,
            false => format!("{sender} mentioned you"),
        },
        body: message_body(&row.blocks, names),
        tag: tag(me, &row.channel_id, name, names),
        // a notice without a link is one the centre cannot open, not a bad one
        link: links::channel_link(chain, &row.channel_id, Some(row.seq)).unwrap_or_default(),
    })
}

/// The tag a room's notices go under: `@peer` for a direct room, the
/// channel's `#name` otherwise.
fn tag(me: u64, room: &str, name: &str, names: &Names) -> String {
    match dm_peer_of(me, room) {
        Some(peer) => format!("@{}", names.author(&Party::Account(peer))),
        None => format!("#{name}"),
    }
}

fn mentions(blocks: &[Block], me: u64) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(spans) | Block::Quote(spans) => spans
            .iter()
            .any(|span| span.marks.contains(&Mark::Mention(Party::Account(me)))),
        Block::Code { .. } | Block::Divider => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat::Span;

    fn row(channel: &str, author: u64, blocks: Vec<Block>) -> MsgRow {
        MsgRow {
            channel_id: channel.into(),
            seq: 7,
            author: Party::Account(author),
            blocks,
            ..MsgRow::default()
        }
    }

    fn said(text: &str, marks: Vec<Mark>) -> Vec<Block> {
        vec![Block::Paragraph(vec![Span {
            text: text.into(),
            marks,
        }])]
    }

    /// A mention of me, or any word in my direct room, from someone else:
    /// a notice. A mention of another, my own words, or a plain message in
    /// a channel: none.
    #[test]
    fn only_a_mention_or_a_direct_message_to_me_is_a_notice() {
        let names = Names::default();
        let chain = "testnet-0a1b2c3d";
        let me = Mark::Mention(Party::Account(3));
        let mention = row("design", 5, said("@me", vec![me.clone()]));
        let post = notice(&mention, 3, "design", chain, &names).unwrap();
        assert_eq!(post.title, "account 5 mentioned you");
        assert_eq!(post.tag, "#design");
        assert_eq!(post.link, "duck://testnet-0a1b2c3d/chat/design/7");

        let other = Mark::Mention(Party::Account(4));
        assert!(
            notice(
                &row("design", 5, said("@x", vec![other])),
                3,
                "design",
                chain,
                &names
            )
            .is_none()
        );
        assert!(
            notice(
                &row("design", 5, said("hi", vec![])),
                3,
                "design",
                chain,
                &names
            )
            .is_none()
        );
        assert!(
            notice(
                &row("design", 3, said("@me", vec![me])),
                3,
                "design",
                chain,
                &names
            )
            .is_none()
        );

        let direct = notice(&row("dm-3-5", 5, said("hi", vec![])), 3, "", chain, &names).unwrap();
        assert_eq!(
            (direct.title.as_str(), direct.body.as_str()),
            ("account 5", "hi")
        );
        assert!(notice(&row("dm-4-5", 5, said("hi", vec![])), 3, "", chain, &names).is_none());
    }
}
