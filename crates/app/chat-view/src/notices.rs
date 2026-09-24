//! What chat hands the host's notification centre: a message that mentions
//! the reader, or one in a direct room she is in, landing in a room she is
//! not looking at. The host decides whether it becomes a banner. The tab
//! badge counts such messages in rooms still unread.
use std::collections::BTreeMap;

use ducktape_view_guest::Context;
use ducktape_view_guest::doors::{Badge, NotifyPost, Post};

use crate::chat::{Block, ChannelInfo, Mark, MsgRow, Party};
use crate::client::{self, NameDirectory};
use crate::{Chat, ChatApi, ChatViewQuery, ChatViewReply};

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
        self.channels_arrived(channels);
        if let (Some(before), Some(me)) = (before, self.my_account()) {
            let viewing = self.viewing();
            let moved: Vec<(String, u64, u64)> = self
                .channels
                .ready()
                .into_iter()
                .flatten()
                .filter(|info| Some(info.channel.id.as_str()) != viewing.as_deref())
                .filter_map(|info| {
                    let was = before.get(&info.channel.id).copied().unwrap_or(0);
                    (info.head_seq > was).then(|| (info.channel.id.clone(), was, info.head_seq))
                })
                .collect();
            for (channel, was, head) in moved {
                self.read_news(channel, was, head, me, cx);
            }
        }
        self.settle_badge(cx);
    }

    /// The room on screen, if the reader can see it.
    fn viewing(&self) -> Option<String> {
        self.room
            .as_ref()
            .filter(|_| self.reads.visible)
            .map(|room| room.id.clone())
    }

    /// Reads the messages past `was` in `channel` and posts the ones meant
    /// for the reader.
    fn read_news(&mut self, channel: String, was: u64, head: u64, me: u64, cx: &mut Context<Self>) {
        let fresh = (head - was).min(MAX_NEW);
        let viewer = self.viewer();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let Ok(ChatViewReply::Messages(rows)) = host
                .ask::<ducktape_view_guest::doors::Query<ChatApi>>(ChatViewQuery::MessagesAround {
                    channel_id: channel.clone(),
                    seq: head,
                    viewer_handles: viewer,
                    page: ::chat::Page {
                        after: None,
                        limit: Some(fresh * 2),
                    },
                })
                .await
            else {
                return;
            };
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                let empty = NameDirectory::default();
                let names = chat.names.ready().unwrap_or(&empty);
                let name = chat
                    .info(&channel)
                    .map(|info| info.channel.name.clone())
                    .unwrap_or_default();
                let posts: Vec<Post> = rows
                    .iter()
                    .filter(|row| row.seq > was && row.seq <= head)
                    .filter_map(|row| notice(row, me, &name, &chat.session.chain, names))
                    .collect();
                if posts.is_empty() || chat.viewing().as_deref() == Some(channel.as_str()) {
                    return;
                }
                *chat.attention.entry(channel).or_default() += posts.len() as i64;
                for post in posts {
                    cx.host().notify::<NotifyPost>(post);
                }
                chat.settle_badge(cx);
            });
        })
        .detach();
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
            cx.host().notify::<Badge>(count);
        }
    }
}

/// The notice `row` makes for account `me`, if it is meant for her: it
/// mentions her, or it is in a direct room she is in. Her own never is.
fn notice(row: &MsgRow, me: u64, name: &str, chain: &str, names: &NameDirectory) -> Option<Post> {
    if row.deleted || row.author == format!("acct:{me}") {
        return None;
    }
    let direct = client::dm_peer_of(me, &row.channel_id).is_some();
    if !direct && !mentions(&row.blocks, me) {
        return None;
    }
    let sender = client::author_display(&row.author, names);
    Some(Post {
        title: match direct {
            true => sender.clone(),
            false => format!("{sender} mentioned you"),
        },
        body: client::message_body(&row.blocks, names),
        tag: match direct {
            true => format!("@{sender}"),
            false => format!("#{name}"),
        },
        link: crate::chat::channel_link(chain, &row.channel_id, Some(row.seq)),
    })
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
    use crate::chat::Span;

    fn row(channel: &str, author: &str, blocks: Vec<Block>) -> MsgRow {
        MsgRow {
            channel_id: channel.into(),
            seq: 7,
            author: author.into(),
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
        let names = NameDirectory::default();
        let chain = "testnet-0a1b2c3d";
        let me = Mark::Mention(Party::Account(3));
        let mention = row("design", "acct:5", said("@me", vec![me.clone()]));
        let post = notice(&mention, 3, "design", chain, &names).unwrap();
        assert_eq!(post.title, "account 5 mentioned you");
        assert_eq!(post.tag, "#design");
        assert_eq!(post.link, "duck://testnet-0a1b2c3d/chat/design/7");

        let other = Mark::Mention(Party::Account(4));
        assert!(
            notice(
                &row("design", "acct:5", said("@x", vec![other])),
                3,
                "design",
                chain,
                &names
            )
            .is_none()
        );
        assert!(
            notice(
                &row("design", "acct:5", said("hi", vec![])),
                3,
                "design",
                chain,
                &names
            )
            .is_none()
        );
        assert!(
            notice(
                &row("design", "acct:3", said("@me", vec![me])),
                3,
                "design",
                chain,
                &names
            )
            .is_none()
        );

        let direct = notice(
            &row("dm-3-5", "acct:5", said("hi", vec![])),
            3,
            "",
            chain,
            &names,
        )
        .unwrap();
        assert_eq!(
            (direct.title.as_str(), direct.body.as_str()),
            ("account 5", "hi")
        );
        assert!(
            notice(
                &row("dm-4-5", "acct:5", said("hi", vec![])),
                3,
                "",
                chain,
                &names
            )
            .is_none()
        );
    }
}
