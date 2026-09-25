//! What the network calls a party, as the composer offers and inserts it.
//! The roster itself is [`chat::view::Names`], shared with every view that
//! names chat parties.
use chat::view::Names;
use chat::Party;

/// Autocomplete candidates: every named account, then the room's
/// account members, labelled without the `@`.
pub fn mention_choices(names: &Names, members: &[Party]) -> Vec<MentionChoice> {
    let accounts = names.numbers().map(Party::Account);
    let members = members.iter().filter(|party| party.is_person());
    let mut choices: Vec<MentionChoice> = Vec::new();
    for party in accounts.chain(members.cloned()) {
        if !choices.iter().any(|choice| choice.party == party) {
            choices.push(MentionChoice {
                label: names.mention(&party)[1..].to_string(),
                party,
            });
        }
    }
    choices.sort_by_key(|choice| choice.label.to_lowercase());
    choices
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionChoice {
    pub label: String,
    pub party: Party,
}

/// The canonical token the composer inserts: `<@account>`.
pub fn mention_token(party: &Party) -> String {
    match party {
        Party::Account(account) => format!("<@{account}>"),
        Party::Module(_) | Party::System => String::new(),
    }
}

/// The other account of a dm room `mine` is in.
pub fn dm_peer_of(mine: u64, channel_id: &str) -> Option<u64> {
    let (a, b) = chat::dm_peers(channel_id)?;
    (mine == a).then_some(b).or((mine == b).then_some(a))
}
