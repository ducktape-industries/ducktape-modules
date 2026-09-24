//! What chat keeps on this device between runs (`store`): the reader's read
//! cursors, so the tab badge and the unread dots come back right after a
//! relaunch, and her frequent reactions. Per network by the host, per
//! reader by the key.
use std::collections::BTreeMap;

use ducktape_view_guest::Context;
use ducktape_view_guest::store;

use crate::Chat;

const EMOJI: &str = "emoji";

impl Chat {
    /// The key the reader's cursors are kept under; none with no key seated.
    fn reads_key(&self) -> Option<String> {
        (!self.session.account.is_empty()).then(|| format!("reads/{}", self.session.account))
    }

    /// The reader's cursors and reactions off the device. Until they land,
    /// nothing is written back: a room first seen now is read to its head,
    /// and that must not overwrite what was kept.
    pub(crate) fn load_kept(&mut self, cx: &mut Context<Self>) {
        self.reads.kept = None;
        let Some(key) = self.reads_key() else {
            return;
        };
        let reads = store::get::<BTreeMap<String, u64>>(&cx.host(), &key);
        let emoji = store::get::<Vec<String>>(&cx.host(), EMOJI);
        cx.spawn(async move |this, cx| {
            let (reads, emoji) = (reads.await, emoji.await);
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                for kept in emoji.ok().flatten().unwrap_or_default().into_iter() {
                    if !chat.recent_emoji.contains(&kept) {
                        chat.recent_emoji.push(kept);
                    }
                }
                chat.recent_emoji.truncate(crate::emoji::RECENT);
                match reads {
                    Ok(reads) => chat.reads_landed(key, reads.unwrap_or_default(), cx),
                    Err(refusal) => cx.host().log(format!("read cursors not loaded: {refusal}")),
                }
            });
        })
        .detach();
    }

    /// The kept cursors stand for every room but the one on screen, and the
    /// badge is counted again from them.
    fn reads_landed(&mut self, key: String, kept: BTreeMap<String, u64>, cx: &mut Context<Self>) {
        if self.reads_key().as_ref() != Some(&key) {
            return;
        }
        let viewing = self.viewing();
        for (room, seq) in kept {
            if Some(&room) != viewing.as_ref() {
                self.reads.cursors.insert(room, seq);
            }
        }
        self.reads.written = self.reads.cursors.clone();
        self.reads.kept = Some(key);
        self.attention.clear();
        self.recounted = false;
        if let Some(list) = self.channels.ready().cloned() {
            self.channels_landed(list, cx);
        }
    }

    /// The cursors onto the device when they moved.
    pub(crate) fn save_reads(&mut self, cx: &mut Context<Self>) {
        let key = self.reads_key();
        if key.is_none() || key != self.reads.kept || self.reads.written == self.reads.cursors {
            return;
        }
        store::set(
            &cx.host(),
            key.as_deref().unwrap_or_default(),
            Some(&self.reads.cursors),
        );
        self.reads.written = self.reads.cursors.clone();
    }

    pub(crate) fn save_emoji(&self, cx: &mut Context<Self>) {
        store::set(&cx.host(), EMOJI, Some(&self.recent_emoji));
    }
}
