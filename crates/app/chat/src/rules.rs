//! The checks an op passes before it writes. Each refuses with the reason a
//! view can act on (`store::refuse`); none writes.
use abi::Refusal;
use store::{Reads, invalid, unauthorized, wrong_state};

use crate::state::MEMBERS;
use crate::{ChannelRow, MAX_EMOJI_BYTES, MAX_ID_BYTES, MAX_NAME_BYTES, MsgRow, Party, dm_peers};

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
pub(crate) fn namespace(id: &str, party: &Party) -> Result<(), Refusal> {
    let Some(prefix) = crate::program_of(id) else {
        return Ok(());
    };
    let allowed = match party {
        Party::Module(program) => prefix == program,
        Party::System => true,
        Party::Account(_) | Party::Key(_) => false,
    };
    if !allowed {
        return Err(unauthorized("colon ids belong to their program namespace"));
    }
    Ok(())
}

/// A plain channel id: an [`id`] in the actor's [`namespace`], and never a
/// dm id, which opens only through `CreateDmChannel` with both peers
/// seated (a plain create would let anyone own the room first).
pub(crate) fn channel_id(channel_id: &str, party: &Party) -> Result<(), Refusal> {
    id("channel_id", channel_id)?;
    namespace(channel_id, party)?;
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

/// `party` may write in `channel`: it is not archived, and posting is open,
/// or `party` owns it or is a member.
pub(crate) fn writable(
    store: &impl Reads,
    channel: &ChannelRow,
    party: &Party,
) -> Result<(), Refusal> {
    if channel.archived {
        return Err(wrong_state(format!("{} is archived", channel.id)));
    }
    let seated = MEMBERS.has(store, &(channel.id.clone(), party.clone()));
    if !channel.admits(party, seated) {
        return Err(unauthorized(format!(
            "{} is not a member of {}",
            handle(party),
            channel.id
        )));
    }
    Ok(())
}

pub(crate) fn owned(channel: &ChannelRow, party: &Party) -> Result<(), Refusal> {
    if channel.owner != *party {
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
pub(crate) fn editable(row: &MsgRow, party: &Party) -> Result<(), Refusal> {
    if row.author != *party {
        return Err(unauthorized("only the author edits"));
    }
    if row.deleted {
        return Err(wrong_state("the message is deleted"));
    }
    Ok(())
}

/// How a refusal sentence names a party.
fn handle(party: &Party) -> String {
    match party {
        Party::Account(account) => format!("acct:{account}"),
        Party::Key(key) => format!("user:{}", abi::hex(key)),
        Party::Module(module) => format!("module:{module}"),
        Party::System => "system".to_string(),
    }
}
