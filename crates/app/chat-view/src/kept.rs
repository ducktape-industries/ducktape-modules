//! What chat keeps on this device between runs (`store`): the reader's read
//! cursors, so the tab badge and the unread dots come back right after a
//! relaunch, and their frequent reactions. Per network by the host, per
//! reader by the key.
use std::collections::BTreeMap;

use ducktape_view_guest::Context;
use ducktape_view_guest::store;

use crate::Chat;

const EMOJI: &str = "emoji";
/// How many times the kept cursors are asked for before chat gives up on
/// them.
const READ_ATTEMPTS: u32 = 3;

impl Chat {
    /// The key the reader's cursors are kept under; none with no key seated.
    fn reads_key(&self) -> Option<String> {
        (!self.session.signer.is_empty()).then(|| format!("reads/{}", self.session.signer))
    }

    /// The reader's cursors and reactions off the device. Until they land,
    /// nothing is written back: a room first seen now is read to its head,
    /// and that must not overwrite what was kept.
    pub(crate) fn load_kept(&mut self, cx: &mut Context<Self>) {
        self.reads.kept = None;
        let Some(key) = self.reads_key() else {
            return;
        };
        let emoji = store::get::<Vec<String>>(&cx.host(), EMOJI);
        cx.spawn(async move |this, cx| {
            let emoji = emoji.await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                let emoji = emoji.unwrap_or_else(|refusal| {
                    cx.host()
                        .log_refused("chat", "the kept reactions", &refusal);
                    None
                });
                for kept in emoji.unwrap_or_default() {
                    if !chat.recent_emoji.contains(&kept) {
                        chat.recent_emoji.push(kept);
                    }
                }
                chat.recent_emoji.truncate(crate::emoji::RECENT);
            });
        })
        .detach();
        self.load_reads(key, 1, cx);
    }

    /// The kept cursors, asked again on a refusal. After [`READ_ATTEMPTS`]
    /// the reader starts from what this session sees, so their reads still
    /// save; what the device held for rooms not seen yet is lost.
    fn load_reads(&mut self, key: String, attempt: u32, cx: &mut Context<Self>) {
        let reads = store::get::<BTreeMap<String, u64>>(&cx.host(), &key);
        cx.spawn(async move |this, cx| {
            let reads = reads.await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                match reads {
                    Ok(reads) => chat.reads_landed(key, reads.unwrap_or_default(), cx),
                    // the reader changed meanwhile: their own load is under way
                    Err(_) if chat.reads_key().as_ref() != Some(&key) => {}
                    Err(refusal) if attempt < READ_ATTEMPTS => {
                        cx.host()
                            .log(format!("read cursors not loaded, asking again: {refusal}"));
                        chat.load_reads(key, attempt + 1, cx);
                    }
                    Err(refusal) => {
                        cx.host().log(format!(
                            "read cursors not loaded, starting from this session: {refusal}"
                        ));
                        chat.reads_landed(key, BTreeMap::new(), cx);
                        // nothing of this session is on the device yet
                        chat.reads.written.clear();
                        chat.save_reads(cx);
                    }
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
