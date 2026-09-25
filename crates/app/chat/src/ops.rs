//! [`execute`]: one function per [`Op`]. Each checks first (`rules`), then
//! writes, so a refused op leaves the store as it found it.
use abi::Refusal;
use store::{Writes, already_exists, capacity, invalid, unauthorized, wrong_state};

use crate::rules;
use crate::state::{
    ANSWERED, CHANNELS, HEADS, MEMBERS, MESSAGE_IDS, MESSAGES, REACTIONS, REPLIES, ROOTS, channel,
    fits, message, newest_first, replace_message, toggle,
};
use crate::{
    AccountNumber, Block, ChannelRow, Frame, HUDDLE_NODE_KEY_BYTES, HuddleEntry,
    MAX_HUDDLE_MEMBERS, MAX_REACTION_EMOJIS, MAX_REVISIONS, MAX_THREAD_REPLIES, MemberRow, MsgRow,
    Op, Party, PostPolicy, Reaction, dm_channel_id, hex, plain_text, tags,
};

pub fn execute(store: &mut impl Writes, frame: &Frame, op: Op) -> Result<(), Refusal> {
    match op {
        Op::CreateChannel {
            channel_id,
            name,
            post_policy,
        } => create_channel(store, frame, channel_id, name, post_policy, false),
        Op::CreateVoiceChannel { channel_id, name } => {
            create_channel(store, frame, channel_id, name, PostPolicy::Open, true)
        }
        Op::CreateDmChannel { counterpart, name } => open_dm(store, frame, counterpart, name),
        Op::RenameChannel { channel_id, name } => rename(store, frame, &channel_id, name),
        Op::SetChannelArchived {
            channel_id,
            archived,
        } => set_archived(store, frame, &channel_id, archived),
        Op::PostMessage {
            channel_id,
            message_id,
            blocks,
            thread,
        } => post(store, frame, channel_id, message_id, blocks, thread),
        Op::EditMessage {
            channel_id,
            seq,
            blocks,
            base_rev,
        } => edit(store, frame, &channel_id, seq, blocks, base_rev),
        Op::DeleteMessage { channel_id, seq } => delete(store, frame, &channel_id, seq),
        Op::AddReaction {
            channel_id,
            seq,
            emoji,
        } => react(store, frame, &channel_id, seq, &emoji, true),
        Op::RemoveReaction {
            channel_id,
            seq,
            emoji,
        } => react(store, frame, &channel_id, seq, &emoji, false),
        Op::SetMembership {
            channel_id,
            party,
            member,
        } => set_membership(store, frame, &channel_id, party, member),
        Op::JoinHuddle {
            channel_id, node, ..
        } => join_huddle(store, frame, &channel_id, &node),
        Op::LeaveHuddle { channel_id } => leave_huddle(store, frame, &channel_id),
    }
}

// ── channels ────────────────────────────────────────────────────────────────

fn create_channel(
    store: &mut impl Writes,
    frame: &Frame,
    id: String,
    name: String,
    post_policy: PostPolicy,
    voice: bool,
) -> Result<(), Refusal> {
    rules::channel_id(&id, &frame.party)?;
    rules::name(&name)?;
    if CHANNELS.has(store, &id) {
        return Err(already_exists(format!("channel {id} exists")));
    }
    let channel = ChannelRow {
        id: id.clone(),
        name,
        created_at: frame.time,
        post_policy,
        owner: frame.party.clone(),
        archived: false,
        huddle: Vec::new(),
        voice,
    };
    CHANNELS.put(store, &id, &channel);
    Ok(())
}

/// The members-only room of the actor's account and `counterpart`, both
/// seated. Opening it again changes nothing.
fn open_dm(
    store: &mut impl Writes,
    frame: &Frame,
    counterpart: AccountNumber,
    name: String,
) -> Result<(), Refusal> {
    let Party::Account(me) = frame.party else {
        return Err(unauthorized("only an account opens a dm"));
    };
    if me == counterpart {
        return Err(invalid("a dm needs two accounts"));
    }
    let id = dm_channel_id(me, counterpart);
    if CHANNELS.has(store, &id) {
        return Ok(());
    }
    rules::name(&name)?;
    let channel = ChannelRow {
        id: id.clone(),
        name,
        created_at: frame.time,
        post_policy: PostPolicy::MembersOnly,
        owner: frame.party.clone(),
        archived: false,
        huddle: Vec::new(),
        voice: false,
    };
    CHANNELS.put(store, &id, &channel);
    for peer in [me, counterpart] {
        seat(store, frame, &id, Party::Account(peer));
    }
    Ok(())
}

fn rename(store: &mut impl Writes, frame: &Frame, id: &str, name: String) -> Result<(), Refusal> {
    rules::name(&name)?;
    let mut channel = channel(store, id)?;
    rules::not_dm(&channel)?;
    rules::owned(&channel, &frame.party)?;
    channel.name = name;
    CHANNELS.put(store, &channel.id, &channel);
    Ok(())
}

fn set_archived(
    store: &mut impl Writes,
    frame: &Frame,
    id: &str,
    archived: bool,
) -> Result<(), Refusal> {
    let mut channel = channel(store, id)?;
    rules::not_dm(&channel)?;
    rules::owned(&channel, &frame.party)?;
    channel.archived = archived;
    CHANNELS.put(store, &channel.id, &channel);
    Ok(())
}

fn set_membership(
    store: &mut impl Writes,
    frame: &Frame,
    id: &str,
    party: Party,
    member: bool,
) -> Result<(), Refusal> {
    let channel = channel(store, id)?;
    rules::not_dm(&channel)?;
    rules::owned(&channel, &frame.party)?;
    if member {
        seat(store, frame, id, party);
    } else {
        MEMBERS.remove(store, &(id.to_owned(), party));
    }
    Ok(())
}

fn seat(store: &mut impl Writes, frame: &Frame, id: &str, party: Party) {
    let row = MemberRow {
        party: party.clone(),
        height: frame.height,
        time: frame.time,
    };
    MEMBERS.put(store, &(id.to_owned(), party), &row);
}

// ── messages ────────────────────────────────────────────────────────────────

fn post(
    store: &mut impl Writes,
    frame: &Frame,
    channel_id: String,
    message_id: String,
    blocks: Vec<Block>,
    thread: Option<u64>,
) -> Result<(), Refusal> {
    rules::id("message_id", &message_id)?;
    rules::namespace(&message_id, &frame.party)?;
    rules::writable(store, &channel(store, &channel_id)?, &frame.party)?;
    if MESSAGE_IDS.has(store, &message_id) {
        return Err(already_exists(format!("message {message_id} exists")));
    }
    let seq = HEADS.get(store, &channel_id)?.unwrap_or(0) + 1;
    let row = MsgRow {
        channel_id: channel_id.clone(),
        seq,
        message_id: message_id.clone(),
        height: frame.height,
        time: frame.time,
        text: plain_text(&blocks),
        tags: tags(&blocks),
        blocks,
        thread,
        ..MsgRow::by(frame.party.clone())
    };
    fits(&row)?;
    match thread {
        Some(root) => answer(store, &channel_id, root, seq)?,
        None => ROOTS.insert(store, &(channel_id.clone(), newest_first(seq))),
    }
    replace_message(store, None, &row);
    MESSAGE_IDS.put(store, &message_id, &(channel_id.clone(), seq));
    HEADS.put(store, &channel_id, &seq);
    Ok(())
}

/// Reply `seq` joins the thread under `root`: the root counts it, and its
/// author's [`ANSWERED`] entry moves to this reply.
fn answer(store: &mut impl Writes, channel_id: &str, root: u64, seq: u64) -> Result<(), Refusal> {
    let mut row = message(store, channel_id, root)?;
    if row.thread.is_some() {
        return Err(invalid("a reply cannot be a thread root"));
    }
    if row.reply_count >= MAX_THREAD_REPLIES {
        return Err(capacity("this thread is full"));
    }
    forget_answer(store, &row);
    let channel_id = channel_id.to_owned();
    let answered = (channel_id.clone(), row.author.clone(), newest_first(seq));
    ANSWERED.put(store, &answered, &root);
    REPLIES.insert(store, &(channel_id.clone(), root, seq));
    row.reply_count += 1;
    row.last_reply_seq = Some(seq);
    MESSAGES.put(store, &(channel_id, root), &row);
    Ok(())
}

/// Drops the root's [`ANSWERED`] entry, if a reply made one.
fn forget_answer(store: &mut impl Writes, root: &MsgRow) {
    if let Some(last) = root.last_reply_seq {
        let key = (
            root.channel_id.clone(),
            root.author.clone(),
            newest_first(last),
        );
        ANSWERED.remove(store, &key);
    }
}

fn edit(
    store: &mut impl Writes,
    frame: &Frame,
    channel_id: &str,
    seq: u64,
    blocks: Vec<Block>,
    base_rev: Option<u32>,
) -> Result<(), Refusal> {
    rules::writable(store, &channel(store, channel_id)?, &frame.party)?;
    let old = message(store, channel_id, seq)?;
    rules::editable(&old, &frame.party)?;
    if old.rev >= MAX_REVISIONS {
        return Err(capacity("the message has no revisions left"));
    }
    let row = MsgRow {
        text: plain_text(&blocks),
        tags: tags(&blocks),
        blocks,
        rev: old.rev + 1,
        edited: true,
        edited_at: Some(frame.time),
        base_rev,
        ..old.clone()
    };
    fits(&row)?;
    replace_message(store, Some(&old), &row);
    Ok(())
}

/// The author or the channel's owner deletes. What stays is a tombstone
/// holding the message's place in its timeline or thread; its body, its
/// postings and its reactions go.
fn delete(
    store: &mut impl Writes,
    frame: &Frame,
    channel_id: &str,
    seq: u64,
) -> Result<(), Refusal> {
    let channel = channel(store, channel_id)?;
    let old = message(store, channel_id, seq)?;
    if old.author != frame.party && channel.owner != frame.party {
        return Err(unauthorized("only the author or the owner deletes"));
    }
    if old.deleted {
        return Ok(());
    }
    forget_answer(store, &old);
    let reacted = REACTIONS.scan(store, REACTIONS.prefix_of(&(channel_id.to_owned(), seq)))?;
    for key in reacted {
        REACTIONS.remove(store, &key);
    }
    let tombstone = MsgRow {
        blocks: Vec::new(),
        text: String::new(),
        tags: Vec::new(),
        reactions: Vec::new(),
        deleted: true,
        ..old.clone()
    };
    replace_message(store, Some(&old), &tombstone);
    Ok(())
}

// ── reactions ───────────────────────────────────────────────────────────────

/// Adds or removes the actor's `emoji` on a message. Choosing what is
/// already chosen, or dropping what is not, changes nothing.
fn react(
    store: &mut impl Writes,
    frame: &Frame,
    channel_id: &str,
    seq: u64,
    emoji: &str,
    on: bool,
) -> Result<(), Refusal> {
    rules::emoji(emoji)?;
    rules::writable(store, &channel(store, channel_id)?, &frame.party)?;
    let mut row = message(store, channel_id, seq)?;
    if row.deleted {
        return Err(wrong_state("the message is deleted"));
    }
    let key = (
        channel_id.to_owned(),
        seq,
        emoji.to_owned(),
        frame.party.clone(),
    );
    if REACTIONS.has(store, &key) == on {
        return Ok(());
    }
    if on {
        count_in(&mut row.reactions, emoji)?;
    } else {
        count_out(&mut row.reactions, emoji);
    }
    fits(&row)?;
    toggle(store, &REACTIONS, &key, on);
    MESSAGES.put(store, &(key.0, seq), &row);
    Ok(())
}

/// One more `emoji`: its count grows, or it joins the list in emoji order.
fn count_in(reactions: &mut Vec<Reaction>, emoji: &str) -> Result<(), Refusal> {
    if let Some(reaction) = reactions.iter_mut().find(|r| r.emoji == emoji) {
        reaction.count += 1;
        return Ok(());
    }
    if reactions.len() >= MAX_REACTION_EMOJIS {
        return Err(capacity("no room for another emoji"));
    }
    reactions.push(Reaction {
        emoji: emoji.to_owned(),
        count: 1,
        reacted_by_me: false,
    });
    reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
    Ok(())
}

/// One fewer `emoji`: at zero it leaves the list.
fn count_out(reactions: &mut Vec<Reaction>, emoji: &str) {
    if let Some(at) = reactions.iter().position(|r| r.emoji == emoji) {
        reactions[at].count -= 1;
        if reactions[at].count == 0 {
            reactions.remove(at);
        }
    }
}

// ── huddles ─────────────────────────────────────────────────────────────────

/// Seats the actor's node in the channel's huddle, or moves her seat to a
/// new node. The node's consent was checked by the program (`program.rs`).
fn join_huddle(
    store: &mut impl Writes,
    frame: &Frame,
    channel_id: &str,
    node: &[u8],
) -> Result<(), Refusal> {
    if !frame.party.is_person() {
        return Err(unauthorized("only people join a huddle"));
    }
    if node.len() != HUDDLE_NODE_KEY_BYTES {
        return Err(invalid(format!(
            "a node key is {HUDDLE_NODE_KEY_BYTES} bytes"
        )));
    }
    let mut channel = channel(store, channel_id)?;
    rules::writable(store, &channel, &frame.party)?;
    let seat = HuddleEntry {
        party: frame.party.clone(),
        node: hex(node),
        joined_at: frame.time,
    };
    match channel.huddle.iter().position(|e| e.party == frame.party) {
        Some(at) => channel.huddle[at] = seat,
        None if channel.huddle.len() >= MAX_HUDDLE_MEMBERS => {
            return Err(capacity("the huddle is full"));
        }
        None => channel.huddle.push(seat),
    }
    CHANNELS.put(store, &channel.id, &channel);
    Ok(())
}

fn leave_huddle(store: &mut impl Writes, frame: &Frame, channel_id: &str) -> Result<(), Refusal> {
    let mut channel = channel(store, channel_id)?;
    channel.huddle.retain(|e| e.party != frame.party);
    CHANNELS.put(store, &channel.id, &channel);
    Ok(())
}
