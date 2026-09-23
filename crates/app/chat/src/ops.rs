use super::*;

// ── execute ─────────────────────────────────────────────────────────────────

pub fn execute(store: &mut impl Writes, frame: &Frame, msg: ChatMsg) -> Result<(), Refusal> {
    let actor = party_handle(&frame.party);
    match msg {
        ChatMsg::CreateChannel {
            channel_id,
            name,
            post_policy,
        } => create_channel(store, frame, channel_id, name, post_policy, false),
        ChatMsg::CreateVoiceChannel { channel_id, name } => {
            create_channel(store, frame, channel_id, name, PostPolicy::Open, true)
        }
        ChatMsg::CreateDmChannel { counterpart, name } => {
            let Party::Account(me) = frame.party else {
                return Err(unauthorized("only an account opens a dm"));
            };
            if me == counterpart {
                return Err(invalid("a dm needs two accounts"));
            }
            let id = dm_channel_id(me, counterpart);
            if load::<ChannelRow>(store, &chan_key(&id))?.is_some() {
                return Ok(());
            }
            create_channel(
                store,
                frame,
                id.clone(),
                name,
                PostPolicy::MembersOnly,
                false,
            )?;
            for party in [Party::Account(me), Party::Account(counterpart)] {
                set_member(store, frame, &id, &party, true);
            }
            Ok(())
        }
        ChatMsg::RenameChannel { channel_id, name } => {
            checked_name(&name)?;
            let mut ch = channel(store, &channel_id)?;
            owned(&ch, &frame.party)?;
            ch.name = name;
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
        ChatMsg::SetChannelArchived {
            channel_id,
            archived,
        } => {
            let mut ch = channel(store, &channel_id)?;
            owned(&ch, &frame.party)?;
            ch.archived = archived;
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
        ChatMsg::PostMessage {
            channel_id,
            message_id,
            blocks,
            thread,
        } => {
            checked_id("message_id", &message_id)?;
            reserved_id(&message_id, &frame.party)?;
            let ch = channel(store, &channel_id)?;
            writable(store, &ch, &frame.party)?;
            if store.get(msgid_key(&message_id).as_bytes()).is_some() {
                return Err(already_exists(format!("message {message_id} exists")));
            }
            let seq = head_seq(store, &channel_id) + 1;
            if let Some(root_seq) = thread {
                let mut root = row(store, &channel_id, root_seq)?;
                if root.thread.is_some() {
                    return Err(invalid("a reply cannot be a thread root"));
                }
                if root.reply_count >= MAX_THREAD_REPLIES {
                    return Err(capacity("this thread is full"));
                }
                if let Some(last) = root.last_reply_seq {
                    store.delete(attention_key(&channel_id, &root.author, last).as_bytes());
                }
                store.set(
                    attention_key(&channel_id, &root.author, seq).into_bytes(),
                    abi::encode(&root_seq),
                );
                root.reply_count += 1;
                root.last_reply_seq = Some(seq);
                put_row(store, &root)?;
                mark(store, thread_key(&channel_id, root_seq, seq));
            } else {
                mark(store, root_key(&channel_id, seq));
            }
            let text = plain_text(&blocks);
            let row = MsgRow {
                channel_id: channel_id.clone(),
                seq,
                message_id: message_id.clone(),
                author: actor,
                height: frame.height,
                time: frame.time,
                tags: tags(&blocks),
                blocks,
                text,
                thread,
                ..MsgRow::default()
            };
            put_row(store, &row)?;
            index(store, &row, true);
            save(store, msgid_key(&message_id), &(&channel_id, seq));
            save(store, seq_key(&channel_id), &seq);
            Ok(())
        }
        ChatMsg::EditMessage {
            channel_id,
            seq,
            blocks,
            base_rev,
        } => {
            let ch = channel(store, &channel_id)?;
            writable(store, &ch, &frame.party)?;
            let mut row = row(store, &channel_id, seq)?;
            if row.author != actor {
                return Err(unauthorized("only the author edits"));
            }
            if row.deleted {
                return Err(wrong_state("the message is deleted"));
            }
            if row.rev >= MAX_REVISIONS {
                return Err(capacity("the message has no revisions left"));
            }
            index(store, &row, false);
            row.text = plain_text(&blocks);
            row.tags = tags(&blocks);
            row.blocks = blocks;
            row.rev += 1;
            row.edited = true;
            row.edited_at = Some(frame.time);
            row.base_rev = base_rev;
            put_row(store, &row)?;
            index(store, &row, true);
            Ok(())
        }
        ChatMsg::DeleteMessage { channel_id, seq } => {
            let ch = channel(store, &channel_id)?;
            let mut row = row(store, &channel_id, seq)?;
            if row.author != actor && ch.owner != actor {
                return Err(unauthorized("only the author or the owner deletes"));
            }
            if row.deleted {
                return Ok(());
            }
            index(store, &row, false);
            if let Some(last) = row.last_reply_seq {
                store.delete(attention_key(&channel_id, &row.author, last).as_bytes());
            }
            for entry in store.scan(Scan::prefix(react_key(&channel_id, seq, "", ""))) {
                store.delete(entry.key);
            }
            row = MsgRow {
                blocks: Vec::new(),
                text: String::new(),
                tags: Vec::new(),
                reactions: Vec::new(),
                deleted: true,
                ..row
            };
            put_row(store, &row)
        }
        ChatMsg::AddReaction {
            channel_id,
            seq,
            emoji,
        } => react(store, frame, &channel_id, seq, &emoji, true),
        ChatMsg::RemoveReaction {
            channel_id,
            seq,
            emoji,
        } => react(store, frame, &channel_id, seq, &emoji, false),
        ChatMsg::SetMembership {
            channel_id,
            party,
            member,
        } => {
            let ch = channel(store, &channel_id)?;
            owned(&ch, &frame.party)?;
            set_member(store, frame, &channel_id, &party, member);
            Ok(())
        }
        ChatMsg::JoinHuddle {
            channel_id, node, ..
        } => {
            if !frame.party.is_person() {
                return Err(unauthorized("only people join a huddle"));
            }
            if node.len() != HUDDLE_NODE_KEY_BYTES {
                return Err(invalid(format!(
                    "a node key is {HUDDLE_NODE_KEY_BYTES} bytes"
                )));
            }
            let mut ch = channel(store, &channel_id)?;
            writable(store, &ch, &frame.party)?;
            let entry = HuddleEntry {
                party: actor.clone(),
                node: hex(&node),
                joined_at: frame.time,
            };
            match ch.huddle.iter().position(|e| e.party == actor) {
                Some(seat) => ch.huddle[seat] = entry,
                None if ch.huddle.len() >= MAX_HUDDLE_MEMBERS => {
                    return Err(capacity("the huddle is full"));
                }
                None => ch.huddle.push(entry),
            }
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
        ChatMsg::LeaveHuddle { channel_id } => {
            let mut ch = channel(store, &channel_id)?;
            ch.huddle.retain(|e| e.party != actor);
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
    }
}

// A program owns only its exact prefix, including the colon boundary.
fn reserved_id(id: &str, party: &Party) -> Result<(), Refusal> {
    if let Some((prefix, _)) = id.split_once(':') {
        let allowed = match party {
            Party::Module(program) => prefix == program,
            Party::System => true,
            Party::Account(_) | Party::Key(_) => false,
        };
        if !allowed {
            return Err(unauthorized("colon ids belong to their program namespace"));
        }
    }
    Ok(())
}

fn create_channel(
    store: &mut impl Writes,
    frame: &Frame,
    id: String,
    name: String,
    post_policy: PostPolicy,
    voice: bool,
) -> Result<(), Refusal> {
    checked_id("channel_id", &id)?;
    reserved_id(&id, &frame.party)?;
    checked_name(&name)?;
    if load::<ChannelRow>(store, &chan_key(&id))?.is_some() {
        return Err(already_exists(format!("channel {id} exists")));
    }
    let ch = ChannelRow {
        id: id.clone(),
        name,
        created_at: frame.time,
        post_policy,
        owner: party_handle(&frame.party),
        archived: false,
        huddle: Vec::new(),
        voice,
    };
    save(store, chan_key(&id), &ch);
    Ok(())
}

fn set_member(store: &mut impl Writes, frame: &Frame, ch: &str, party: &Party, member: bool) {
    let key = member_key(ch, &party_handle(party));
    if member {
        let row = MemberRow {
            party: party_handle(party),
            height: frame.height,
            time: frame.time,
        };
        save(store, key, &row);
    } else {
        store.delete(key.as_bytes());
    }
}

fn react(
    store: &mut impl Writes,
    frame: &Frame,
    channel_id: &str,
    seq: u64,
    emoji: &str,
    on: bool,
) -> Result<(), Refusal> {
    if emoji.is_empty() || emoji.len() > MAX_EMOJI_BYTES || emoji.contains('/') {
        return Err(invalid("not an emoji"));
    }
    let ch = channel(store, channel_id)?;
    writable(store, &ch, &frame.party)?;
    let mut row = row(store, channel_id, seq)?;
    if row.deleted {
        return Err(wrong_state("the message is deleted"));
    }
    let key = react_key(channel_id, seq, emoji, &party_handle(&frame.party));
    if store.get(key.as_bytes()).is_some() == on {
        return Ok(());
    }
    let at = row.reactions.iter().position(|r| r.emoji == emoji);
    match (on, at) {
        (true, Some(i)) => row.reactions[i].count += 1,
        (true, None) => {
            if row.reactions.len() >= MAX_REACTION_EMOJIS {
                return Err(capacity("no room for another emoji"));
            }
            row.reactions.push(ReactionSummary {
                emoji: emoji.to_string(),
                count: 1,
                reacted_by_me: false,
            });
            row.reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
        }
        (false, Some(i)) => {
            row.reactions[i].count -= 1;
            if row.reactions[i].count == 0 {
                row.reactions.remove(i);
            }
        }
        (false, None) => return Ok(()),
    }
    if on {
        mark(store, key);
    } else {
        store.delete(key.as_bytes());
    }
    put_row(store, &row)
}
