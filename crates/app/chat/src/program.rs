//! The module: the signer resolved to its principal, then every op and
//! every query, each handed to its function in `ops.rs` or `queries.rs`.

use guest::{ExecCtx, Module, QueryCtx, Refusal};

use crate::ops::{
    create_channel, delete, edit, join_huddle, leave_huddle, open_dm, post, react, rename,
    set_archived, set_membership,
};
use crate::queries::{
    around, attention, by_id, channel, channels, roots, search, seen_by, tagged, thread,
};
use crate::state::MEMBERS;
use crate::{Op, PostPolicy, Query, Reply};

pub struct Chat;

impl Module for Chat {
    type Op = Op;
    type Query = Query;
    type Response = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Refusal> {
        let sender = identity::principal_of(ctx, &ctx.env().origin)?;
        match op {
            Op::CreateChannel {
                channel_id,
                name,
                post_policy,
            } => create_channel(ctx, &sender, channel_id, name, post_policy, false),
            Op::CreateVoiceChannel { channel_id, name } => {
                create_channel(ctx, &sender, channel_id, name, PostPolicy::Open, true)
            }
            Op::CreateDmChannel { counterpart, name } => open_dm(ctx, &sender, counterpart, name),
            Op::RenameChannel { channel_id, name } => rename(ctx, &sender, &channel_id, name),
            Op::SetChannelArchived {
                channel_id,
                archived,
            } => set_archived(ctx, &sender, &channel_id, archived),
            Op::PostMessage {
                channel_id,
                message_id,
                blocks,
                thread,
            } => post(ctx, &sender, channel_id, message_id, blocks, thread),
            Op::EditMessage {
                channel_id,
                seq,
                blocks,
                base_rev,
            } => edit(ctx, &sender, &channel_id, seq, blocks, base_rev),
            Op::DeleteMessage { channel_id, seq } => delete(ctx, &sender, &channel_id, seq),
            Op::AddReaction {
                channel_id,
                seq,
                emoji,
            } => react(ctx, &sender, &channel_id, seq, &emoji, true),
            Op::RemoveReaction {
                channel_id,
                seq,
                emoji,
            } => react(ctx, &sender, &channel_id, seq, &emoji, false),
            Op::SetMembership {
                channel_id,
                principal,
                member,
            } => set_membership(ctx, &sender, &channel_id, principal, member),
            Op::JoinHuddle {
                channel_id,
                node,
                node_proof,
            } => join_huddle(ctx, &sender, &channel_id, &node, &node_proof),
            Op::LeaveHuddle { channel_id } => leave_huddle(ctx, &sender, &channel_id),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Refusal> {
        let height = ctx.env().height;
        let (reply, viewer) = match query {
            Query::Channels { page } => (Reply::Channels(channels(ctx, &page, height)?), vec![]),
            Query::Channel { channel_id } => (Reply::Channel(channel(ctx, &channel_id)?), vec![]),
            Query::MessageById { message_id } => (Reply::Message(by_id(ctx, &message_id)?), vec![]),
            Query::ThreadAttention { channel_id, author } => (
                Reply::Attention(attention(ctx, &channel_id, author)?),
                vec![],
            ),
            Query::Roots {
                channel_id,
                viewer,
                page,
            } => (Reply::Roots(roots(ctx, channel_id, &page, height)?), viewer),
            Query::MessagesAround {
                channel_id,
                seq,
                viewer,
                page,
            } => (
                Reply::Messages(around(ctx, channel_id, seq, &page)?),
                viewer,
            ),
            Query::Thread {
                channel_id,
                root_seq,
                viewer,
                page,
            } => (thread(ctx, channel_id, root_seq, &page, height)?, viewer),
            Query::Members { channel_id, page } => {
                let members = MEMBERS.range_of(ctx, &channel_id, &page, height)?;
                (Reply::Members(members.map(|(_, member)| member)), vec![])
            }
            Query::Search {
                text,
                viewer,
                channel_id,
                page,
            } => (Reply::Hits(search(ctx, &text, channel_id, &page)?), viewer),
            Query::TagSearch {
                tag,
                viewer,
                channel_id,
                page,
            } => (
                Reply::TagHits(tagged(ctx, &tag, channel_id, &page, height)?),
                viewer,
            ),
            Query::Accounts { page } => {
                (Reply::Accounts(crate::origin::accounts(ctx, page)?), vec![])
            }
        };
        seen_by(ctx, reply, viewer)
    }
}

#[cfg(feature = "program")]
guest::export!(Chat);
