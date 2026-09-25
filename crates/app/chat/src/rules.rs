//! The checks an op passes before it writes. Each refuses with the reason a
//! view can act on (`guest::refuse`); none writes.
use guest::{QueryCtx, Refusal, invalid, unauthorized, wrong_state};

use crate::state::MEMBERS;
use crate::{
    ChannelRow, MAX_EMOJI_BYTES, MAX_ID_BYTES, MAX_NAME_BYTES, MsgRow, Principal, dm_peers,
};

/// A channel or message id: 1..=64 bytes, no `/` (a room link's separator).
pub(crate) fn id(what: &str, id: &str) -> Result<(), Refusal> {
    if id.is_empty() || id.len() > MAX_ID_BYTES || id.contains('/') {
        return Err(invalid(format!(
            "{what} is 1..={MAX_ID_BYTES} bytes without '/'"
        )));
    }
    Ok(())
}

/// An id with a `:` belongs to the program its prefix names: `forge:web:3`
/// is forge's alone. The system may use any.
pub(crate) fn namespace(id: &str, principal: &Principal) -> Result<(), Refusal> {
    let Some(prefix) = crate::program_of(id) else {
        return Ok(());
    };
    let allowed = match principal {
        Principal::Module(program) => prefix == program,
        Principal::System => true,
        Principal::Account(_) => false,
    };
    if !allowed {
        return Err(unauthorized("colon ids belong to their program namespace"));
    }
    Ok(())
}

/// A plain channel id: an [`id`] in the actor's [`namespace`], and never a
/// dm id, which opens only through `CreateDmChannel` with both peers
/// seated (a plain create would let anyone own the room first).
pub(crate) fn channel_id(channel_id: &str, principal: &Principal) -> Result<(), Refusal> {
    id("channel_id", channel_id)?;
    namespace(channel_id, principal)?;
    if dm_peers(channel_id).is_some() {
        return Err(unauthorized("dm ids open only through CreateDmChannel"));
    }
    Ok(())
}

pub(crate) fn name(name: &str) -> Result<(), Refusal> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(invalid(format!("a name is 1..={MAX_NAME_BYTES} bytes")));
    }
    Ok(())
}

pub(crate) fn emoji(emoji: &str) -> Result<(), Refusal> {
    if emoji.is_empty() || emoji.len() > MAX_EMOJI_BYTES || emoji.contains('/') {
        return Err(invalid("not an emoji"));
    }
    Ok(())
}

/// `principal` may write in `channel`: it is not archived, and posting is open,
/// or `principal` owns it or is a member.
pub(crate) fn writable(
    ctx: &QueryCtx,
    channel: &ChannelRow,
    principal: &Principal,
) -> Result<(), Refusal> {
    if channel.archived {
        return Err(wrong_state(format!("{} is archived", channel.id)));
    }
    let seated = MEMBERS.has(ctx, &(channel.id.clone(), principal.clone()));
    if !channel.admits(principal, seated) {
        return Err(unauthorized(format!(
            "{} is not a member of {}",
            handle(principal),
            channel.id
        )));
    }
    Ok(())
}

pub(crate) fn owned(channel: &ChannelRow, principal: &Principal) -> Result<(), Refusal> {
    if channel.owner != *principal {
        return Err(unauthorized(format!(
            "only the owner of {} may",
            channel.id
        )));
    }
    Ok(())
}

/// A dm belongs to its two peers alike: no one seats a third, removes
/// either, renames or archives it, the peer who opened it included.
pub(crate) fn not_dm(channel: &ChannelRow) -> Result<(), Refusal> {
    if dm_peers(&channel.id).is_some() {
        return Err(unauthorized(format!(
            "{} is a dm; neither peer reshapes it",
            channel.id
        )));
    }
    Ok(())
}

/// A message only its author edits, and only while it stands.
pub(crate) fn editable(row: &MsgRow, principal: &Principal) -> Result<(), Refusal> {
    if row.author != *principal {
        return Err(unauthorized("only the author edits"));
    }
    if row.deleted {
        return Err(wrong_state("the message is deleted"));
    }
    Ok(())
}

/// How a refusal sentence names a principal.
fn handle(principal: &Principal) -> String {
    match principal {
        Principal::Account(account) => format!("acct:{account}"),
        Principal::Module(module) => format!("module:{module}"),
        Principal::System => "system".to_string(),
    }
}
